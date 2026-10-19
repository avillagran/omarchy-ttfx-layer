//! The assembly renderer's borrowed grid becomes an owned, sparse GTK frame.
//! The pool has no byte length in the ABI: validity of nonzero handles and their
//! 32-byte preceding headers is guaranteed by the assembly producer, not by this
//! consumer. Dimensions, offsets and UTF-8 are checked before interpreting cells.
use std::ffi::c_void;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

const MAX_CELLS: usize = 1_000_000;
const MAX_POOL_OFFSET: usize = 0x00ff_ff00;

#[derive(Clone, Debug, PartialEq)]
pub struct DirectCell {
    pub x: usize,
    pub y: usize,
    pub ch: char,
    pub color: (u8, u8, u8),
    pub background: Option<(u8, u8, u8)>,
    /// Assembly style bits (reverse is already resolved into the two colors).
    pub attrs: u64,
}

/// Decode inside the emit callback only. Never keep a borrowed pool/grid slice.
///
/// # Safety
/// `grid` has `cols*rows` readable elements, and `pool` contains the initial
/// 32-byte header and every nonzero handle's visual and preceding 32-byte
/// metadata, all valid for this callback's lifetime.
pub unsafe fn decode_frame(
    cols: u64,
    rows: u64,
    grid: *const u32,
    pool: *const u8,
    cells: &mut Vec<DirectCell>,
) -> Result<(), &'static str> {
    let width = usize::try_from(cols).map_err(|_| "columns overflow")?;
    let height = usize::try_from(rows).map_err(|_| "rows overflow")?;
    let count = width.checked_mul(height).ok_or("grid size overflow")?;
    if count == 0 || count > MAX_CELLS {
        return Err("grid size outside supported range");
    }
    if grid.is_null() || pool.is_null() {
        return Err("null assembly frame pointer");
    }
    // visual_init interns the unstyled blank first at offset 32. Verify the
    // header once per frame before taking the hot path; on a 236x78 grid most
    // cells are this handle, so they need no UTF-8/color/attribute decoding.
    let blank = unsafe {
        let symbol = (pool as *const u64).read_unaligned();
        let fg = (pool.add(8) as *const u64).read_unaligned();
        let bg = (pool.add(16) as *const u64).read_unaligned();
        let attrs = (pool.add(24) as *const u64).read_unaligned();
        (symbol == (1u64 << 32) | b' ' as u64 && fg == u64::MAX && bg == u64::MAX && attrs == 0)
            .then_some((1u32 << 24) | 32)
    };
    cells.clear();
    // SAFETY: the assembly hook contract guarantees the grid length.
    let handles = unsafe { std::slice::from_raw_parts(grid, count) };
    for (index, &handle) in handles.iter().enumerate() {
        if handle == 0 || Some(handle) == blank {
            continue;
        }
        let offset = (handle & 0x00ff_ffff) as usize;
        let bytes = (handle >> 24) as usize;
        if !(1..=128).contains(&bytes)
            || !(32..MAX_POOL_OFFSET).contains(&offset)
            || offset
                .checked_add(bytes)
                .is_none_or(|end| end > MAX_POOL_OFFSET + 4096)
        {
            return Err("visual handle offset or length out of range");
        }
        // SAFETY: a valid handle is issued by visual_make, whose header starts
        // 32 bytes before the byte string. read_unaligned works for fake fixtures.
        let visual = unsafe { pool.add(offset) };
        let symbol = unsafe { (visual.sub(32) as *const u64).read_unaligned() };
        let foreground = unsafe { (visual.sub(24) as *const u64).read_unaligned() };
        let background = unsafe { (visual.sub(16) as *const u64).read_unaligned() };
        let attrs = unsafe { (visual.sub(8) as *const u64).read_unaligned() };
        let len = ((symbol >> 32) & 0xff) as usize;
        if !(1..=4).contains(&len) {
            return Err("visual symbol length out of range");
        }
        let packed = (symbol as u32).to_le_bytes();
        let text = std::str::from_utf8(&packed[..len]).map_err(|_| "visual symbol is not UTF-8")?;
        let mut chars = text.chars();
        let ch = chars.next().ok_or("empty visual symbol")?;
        if chars.next().is_some() {
            return Err("visual symbol contains multiple characters");
        }
        if attrs & !0x7f != 0 {
            return Err("unsupported visual attribute bits");
        }
        if attrs & 8 != 0 {
            return Err("blink requires GTK redraw scheduling");
        }
        // Default foreground is light gray, default background is black.
        // Reverse swaps even absent defaults, and is retained for diagnostics.
        let reversed = attrs & 16 != 0;
        let foreground_color = if foreground == u64::MAX {
            0xcccccc
        } else {
            foreground
        };
        let background_color = (background != u64::MAX).then_some(background);
        let color = if reversed {
            background_color.unwrap_or(0)
        } else {
            foreground_color
        };
        let bg = if reversed {
            Some(foreground_color)
        } else {
            background_color
        };
        if ch.is_control() {
            return Err("visual symbol is a control character");
        }
        if ch == ' ' && bg.is_none() && attrs & (4 | 64) == 0 {
            continue;
        }
        let rgb = |value: u64| ((value >> 16) as u8, (value >> 8) as u8, value as u8);
        cells.push(DirectCell {
            x: index % width,
            y: height - 1 - index / width,
            ch,
            color: rgb(color),
            background: bg.map(rgb),
            attrs,
        });
    }
    Ok(())
}

pub struct FrameSink {
    pub snapshot: Arc<Mutex<Vec<DirectCell>>>,
    pub generation: Arc<AtomicU64>,
    pub stop: Arc<AtomicBool>,
    pub dimensions: (usize, usize),
    /// Reusable decode storage; publication returns the previous slot buffer.
    pub decoded: Vec<DirectCell>,
    pub error: Option<&'static str>,
    /// Producer-thread-only mapper; construct and update inside the ASM worker.
    pub color_transform: Option<ttfx::utils::ansi::ColorTransform>,
}

impl FrameSink {
    pub fn new(
        snapshot: Arc<Mutex<Vec<DirectCell>>>,
        generation: Arc<AtomicU64>,
        stop: Arc<AtomicBool>,
        dimensions: (usize, usize),
    ) -> Self {
        Self {
            snapshot,
            generation,
            stop,
            dimensions,
            decoded: Vec::new(),
            error: None,
            color_transform: None,
        }
    }
}

/// The sole C ABI entrypoint. `ctx` is owned by the caller for the duration of
/// run_direct; no GTK object or assembly pointer crosses the worker boundary.
///
/// # Safety
/// Non-null `ctx` must point to an exclusively borrowed `FrameSink` for this
/// synchronous call. For matching dimensions, `grid` and `pool` must satisfy
/// `decode_frame`'s borrowed-frame safety contract.
pub unsafe extern "C" fn emit_frame(
    ctx: *mut c_void,
    cols: u64,
    rows: u64,
    grid: *const u32,
    pool: *const u8,
) -> u64 {
    if ctx.is_null() {
        return 1;
    }
    // SAFETY: run_direct invokes the callback synchronously with this unique
    // stack-owned context. Only the producer thread may mutate it.
    let sink = unsafe { &mut *(ctx as *mut FrameSink) };
    match std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        emit_frame_inner(sink, cols, rows, grid, pool)
    })) {
        Ok(result) => result,
        Err(_) => {
            sink.error = Some("assembly frame callback panicked");
            1
        }
    }
}

fn emit_frame_inner(
    sink: &mut FrameSink,
    cols: u64,
    rows: u64,
    grid: *const u32,
    pool: *const u8,
) -> u64 {
    if sink.stop.load(Ordering::Relaxed) || (cols as usize, rows as usize) != sink.dimensions {
        return 1;
    }
    // SAFETY: emit_frame's borrowed frame contract guarantees valid pointers.
    if let Err(error) = unsafe { decode_frame(cols, rows, grid, pool, &mut sink.decoded) } {
        sink.error = Some(error);
        return 1;
    }
    if let Some(map) = &sink.color_transform {
        for cell in &mut sink.decoded {
            let (r, g, b) = cell.color;
            cell.color = map(r, g, b);
            if let Some((r, g, b)) = cell.background {
                cell.background = Some(map(r, g, b));
            }
        }
    }
    match sink.snapshot.lock() {
        Ok(mut frame) => {
            // Shutdown holds this same lock before setting stop, so a decoded
            // frame cannot commit after its window has been destroyed.
            if sink.stop.load(Ordering::Acquire) {
                return 1;
            }
            // Transfer the fully decoded frame; the next decode clears and
            // reuses the previous slot without touching the consumer's frame.
            std::mem::swap(&mut *frame, &mut sink.decoded);
            let committed = sink.generation.fetch_add(1, Ordering::Release) + 1;
            crate::frame_cadence::record_produced(committed);
            crate::frame_wakeup::notify_current();
            0
        }
        Err(_) => {
            sink.error = Some("frame slot mutex poisoned");
            1
        }
    }
}

#[cfg(test)]
mod wakeup_tests {
    use super::*;

    #[test]
    fn committed_assembly_frame_wakes_ui_and_stopped_sink_cannot_publish() {
        let stop = Arc::new(AtomicBool::new(false));
        let wakeup = Arc::new(crate::frame_wakeup::FrameWakeup::new(stop.clone()).unwrap());
        crate::frame_wakeup::set_current(Some(wakeup.clone()));
        let mut sink = FrameSink::new(
            Arc::new(Mutex::new(Vec::new())),
            Arc::new(AtomicU64::new(0)),
            stop.clone(),
            (1, 1),
        );
        let grid = [0u32];
        let pool = [0u8; 32];
        assert_eq!(
            emit_frame_inner(&mut sink, 1, 1, grid.as_ptr(), pool.as_ptr()),
            0
        );
        assert_eq!(sink.generation.load(Ordering::Acquire), 1);
        assert_eq!(wakeup.drain().unwrap(), Some(1));
        {
            let _guard = sink.snapshot.lock().unwrap();
            stop.store(true, Ordering::Release);
            wakeup.disable();
        }
        assert_eq!(
            emit_frame_inner(&mut sink, 1, 1, grid.as_ptr(), pool.as_ptr()),
            1
        );
        assert_eq!(sink.generation.load(Ordering::Acquire), 1);
        assert_eq!(wakeup.drain().unwrap(), None);
        crate::frame_wakeup::set_current(None);
    }
}
