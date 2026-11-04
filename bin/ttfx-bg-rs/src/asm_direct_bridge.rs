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
#[allow(dead_code)] // The binary decodes through decode_frame_with.
pub unsafe fn decode_frame(
    cols: u64,
    rows: u64,
    grid: *const u32,
    pool: *const u8,
    cells: &mut Vec<DirectCell>,
) -> Result<(), &'static str> {
    unsafe { decode_frame_with(cols, rows, grid, pool, cells, &mut HandleMemo::default()) }
}

/// What one handle decodes to: `None` is a cell with nothing to draw.
type Decoded = Option<(char, (u8, u8, u8), Option<(u8, u8, u8)>, u64)>;

/// Handles repeat across a frame (a word in one color, a beam's glyphs), and
/// each first decode reads the pool's scattered headers. Remember this frame's
/// results per handle, direct-mapped. A pool is only valid for its callback,
/// so every frame starts a new epoch and nothing carries over.
pub struct HandleMemo {
    epoch: u32,
    slots: Vec<(u32, u32, Decoded)>,
}

const HANDLE_MEMO_BITS: u32 = 9;

impl Default for HandleMemo {
    fn default() -> Self {
        Self { epoch: 0, slots: vec![(0, 0, None); 1 << HANDLE_MEMO_BITS] }
    }
}

impl HandleMemo {
    fn next_frame(&mut self) {
        self.epoch = self.epoch.wrapping_add(1);
        if self.epoch == 0 {
            self.slots.fill((0, 0, None));
            self.epoch = 1;
        }
    }
}

/// [`decode_frame`] with a memo that lives across calls.
///
/// # Safety
/// As for [`decode_frame`].
pub unsafe fn decode_frame_with(
    cols: u64,
    rows: u64,
    grid: *const u32,
    pool: *const u8,
    cells: &mut Vec<DirectCell>,
    memo: &mut HandleMemo,
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
    memo.next_frame();
    // SAFETY: the assembly hook contract guarantees the grid length.
    let handles = unsafe { std::slice::from_raw_parts(grid, count) };
    for (index, &handle) in handles.iter().enumerate() {
        if handle == 0 || Some(handle) == blank {
            continue;
        }
        let slot = (handle.wrapping_mul(0x9e37_79b1) >> (32 - HANDLE_MEMO_BITS)) as usize;
        let (key, epoch, decoded) = memo.slots[slot];
        let decoded = if key == handle && epoch == memo.epoch {
            decoded
        } else {
            // SAFETY: as for decode_frame; the visual is read in this callback.
            let decoded = unsafe { decode_visual(handle, pool) }?;
            memo.slots[slot] = (handle, memo.epoch, decoded);
            decoded
        };
        if let Some((ch, color, background, attrs)) = decoded {
            cells.push(DirectCell {
                x: index % width,
                y: height - 1 - index / width,
                ch,
                color,
                background,
                attrs,
            });
        }
    }
    Ok(())
}

/// One pooled visual as a cell, or `None` when it draws nothing.
///
/// # Safety
/// `handle` is nonzero and `pool` satisfies [`decode_frame`]'s contract.
unsafe fn decode_visual(handle: u32, pool: *const u8) -> Result<Decoded, &'static str> {
    {
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
            return Ok(None);
        }
        let rgb = |value: u64| ((value >> 16) as u8, (value >> 8) as u8, value as u8);
        Ok(Some((ch, rgb(color), bg.map(rgb), attrs)))
    }
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
    /// Results of `color_transform`, which is a pure function of the color.
    color_memo: ColorMemo,
    handle_memo: HandleMemo,
}

/// A frame repeats a few hundred distinct colors over thousands of cells, and
/// the theme transform does HSV math per call: remember its results, like a
/// palette lookup table filled on demand. Exact: a slot answers only for the
/// color it was filled with, and only for the transform it holds alive.
#[derive(Default)]
struct ColorMemo {
    owner: Option<ttfx::utils::ansi::ColorTransform>,
    /// `FILLED | input << 24 | output`, direct-mapped by input color.
    slots: Vec<u64>,
}

const MEMO_BITS: u32 = 12;
const FILLED: u64 = 1 << 63;

impl ColorMemo {
    /// Forget every result unless they came from this very transform. Holding
    /// the owner keeps its allocation alive, so its address cannot be reused.
    fn use_transform(&mut self, transform: &ttfx::utils::ansi::ColorTransform) {
        if !self.owner.as_ref().is_some_and(|owner| std::rc::Rc::ptr_eq(owner, transform)) {
            self.owner = Some(transform.clone());
            self.slots.clear();
            self.slots.resize(1 << MEMO_BITS, 0);
        }
    }

    fn map(&mut self, transform: &ttfx::utils::ansi::ColorTransform, (r, g, b): (u8, u8, u8)) -> (u8, u8, u8) {
        let input = (r as u64) << 16 | (g as u64) << 8 | b as u64;
        let slot = ((input as u32).wrapping_mul(0x9e37_79b1) >> (32 - MEMO_BITS)) as usize;
        let entry = self.slots[slot];
        if entry & FILLED != 0 && (entry >> 24) & 0xff_ffff == input {
            return ((entry >> 16) as u8, (entry >> 8) as u8, entry as u8);
        }
        let (r, g, b) = transform(r, g, b);
        self.slots[slot] = FILLED | input << 24 | (r as u64) << 16 | (g as u64) << 8 | b as u64;
        (r, g, b)
    }
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
            color_memo: ColorMemo::default(),
            handle_memo: HandleMemo::default(),
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
    // Hold the effect while its monitor is hidden; it resumes where it was.
    crate::occlusion::wait_while_hidden(&sink.stop);
    // SAFETY: emit_frame's borrowed frame contract guarantees valid pointers.
    if let Err(error) = unsafe { decode_frame_with(cols, rows, grid, pool, &mut sink.decoded, &mut sink.handle_memo) } {
        sink.error = Some(error);
        return 1;
    }
    if let Some(map) = &sink.color_transform {
        let memo = &mut sink.color_memo;
        memo.use_transform(map);
        for cell in &mut sink.decoded {
            cell.color = memo.map(map, cell.color);
            if let Some(background) = cell.background {
                cell.background = Some(memo.map(map, background));
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
    fn color_memo_matches_the_transform_and_follows_a_replaced_one() {
        let calls = std::rc::Rc::new(std::cell::Cell::new(0));
        let counted = calls.clone();
        let first: ttfx::utils::ansi::ColorTransform = std::rc::Rc::new(move |r, g, b| {
            counted.set(counted.get() + 1);
            (g ^ 0x5a, b.wrapping_mul(3), r.wrapping_add(g))
        });
        let mut memo = ColorMemo::default();
        let mut seed = 0x1234_5678u32;
        let mut colors = Vec::new();
        for _ in 0..3000 {
            seed ^= seed << 13; seed ^= seed >> 17; seed ^= seed << 5;
            // Few distinct colors, as in a frame, plus colliding outliers.
            colors.push(if seed % 4 == 0 { (seed as u8, (seed >> 8) as u8, (seed >> 16) as u8) } else { ((seed % 7) as u8, 9, (seed % 5) as u8) });
        }
        for _ in 0..2 {
            memo.use_transform(&first);
            for &(r, g, b) in &colors {
                assert_eq!(memo.map(&first, (r, g, b)), (g ^ 0x5a, b.wrapping_mul(3), r.wrapping_add(g)));
            }
        }
        assert!(calls.get() < colors.len(), "repeated colors must come from the memo");
        let second: ttfx::utils::ansi::ColorTransform = std::rc::Rc::new(|r, g, b| (b, g, r));
        memo.use_transform(&second);
        for &(r, g, b) in &colors {
            assert_eq!(memo.map(&second, (r, g, b)), (b, g, r));
        }
        memo.use_transform(&first);
        assert_eq!(memo.map(&first, (1, 2, 3)), (2 ^ 0x5a, 9, 3));
    }

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
