//! The C ABI between Rust and asm/lib.asm (or asm/aarch64/lib.s). Field
//! offsets here are mirrored by the RQ_* constants in asm/ttfx.inc (and, through
//! the generated asm/aarch64/defs.inc, the aarch64 engine); change them together.

use std::cell::Cell;
use std::ffi::c_void;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Mutex, OnceLock};
use std::time::Instant;

use super::effects::{self, Words};
use super::{DirectFrameHook, Run};
use crate::engine::effect::RunOutcome;
use crate::engine::error::EngineError;
use crate::engine::terminal::{self, TerminalConfig};
use crate::utils::graphics::Color;
use crate::utils::rng::Rng;

#[repr(C)]
struct Request {
    input_ptr: *const u8,
    input_len: u64,
    effect: u64,
    effect_config: *const u64,
    tab_width: i64,
    frame_rate: i64,
    canvas_width: i64,
    canvas_height: i64,
    anchor_canvas: u64,
    anchor_text: u64,
    existing_colors: u64,
    background: u64,
    flags: u64,
    max_frames: u64,
    term_width: i64,
    term_height: i64,
    rng_state: [u64; 4],
    stop_check: extern "C" fn(*mut c_void) -> u64,
    stop_ctx: *mut c_void,
    line_lengths: *const i64,
    line_count: u64,
    error_ptr: *const u8,
    error_len: u64,
    error_kind: u64,
    audio_poll: extern "C" fn(*mut c_void) -> u64,
    audio_params: extern "C" fn(*mut c_void, *mut f32) -> u64,
    audio_ctx: *mut c_void,
    direct_emit: Option<unsafe extern "C" fn(*mut c_void, u64, u64, *const u32, *const u8) -> u64>,
    direct_ctx: *mut c_void,
}

/// audio_poll_default's frame counter, reset by offer() before each run.
static SCRIPT_FRAME: AtomicU64 = AtomicU64::new(0);

// Every linked tier owns process-global .tstate and mmap regions. Never enter
// any tier while another run may still be using a borrowed frame or pool.
static ENGINE_RUN_LOCK: Mutex<()> = Mutex::new(());
thread_local! {
    static ENGINE_RUN_ACTIVE: Cell<bool> = const { Cell::new(false) };
    #[cfg(any(target_arch = "x86_64", target_arch = "aarch64"))]
    static ACTIVE_AUDIO_STATS: Cell<Option<unsafe extern "C" fn(*mut u64)>> = const { Cell::new(None) };
}

struct ActiveRun;

impl Drop for ActiveRun {
    fn drop(&mut self) {
        ENGINE_RUN_ACTIVE.with(|active| active.set(false));
        #[cfg(any(target_arch = "x86_64", target_arch = "aarch64"))]
        ACTIVE_AUDIO_STATS.with(|active| active.set(None));
    }
}

fn with_engine_lock<T>(run: impl FnOnce() -> T) -> Result<T, &'static str> {
    if ENGINE_RUN_ACTIVE.with(Cell::get) {
        return Err("assembly engine callbacks cannot start another run");
    }
    let _guard = ENGINE_RUN_LOCK
        .lock()
        .map_err(|_| "assembly engine state lock poisoned")?;
    ENGINE_RUN_ACTIVE.with(|active| active.set(true));
    let _active = ActiveRun;
    Ok(run())
}

/// The default poll word's script, parsed once from TTFX_ASM_AUDIO_SCRIPT:
/// "vol,bass,beat[,treble[,bpm]];...", one entry per frame, the last
/// repeated. A test hook; unset is silence.
fn audio_script() -> &'static [u64] {
    static SCRIPT: OnceLock<Vec<u64>> = OnceLock::new();
    SCRIPT.get_or_init(|| {
        std::env::var("TTFX_ASM_AUDIO_SCRIPT")
            .unwrap_or_default()
            .split(';')
            .filter_map(|entry| {
                let mut parts = entry
                    .split(',')
                    .map(|p| p.trim().parse::<u64>().unwrap_or(0));
                let (volume, bass, beat) = (parts.next()?, parts.next()?, parts.next()?);
                let treble = parts.next().unwrap_or(0);
                let bpm = parts.next().unwrap_or(0);
                Some(
                    volume.min(255)
                        | bass.min(255) << 8
                        | u64::from(beat != 0) << 16
                        | treble.min(255) << 24
                        | bpm.min(255) << 32,
                )
            })
            .collect()
    })
}

/// The CLI's audio_poll: silence, or the scripted word for this frame.
extern "C" fn audio_poll_default(_context: *mut c_void) -> u64 {
    let frame = SCRIPT_FRAME.fetch_add(1, Ordering::Relaxed) as usize;
    let script = audio_script();
    if script.is_empty() {
        return 0;
    }
    script[frame.min(script.len() - 1)]
}

/// The default color parameters, parsed once from TTFX_ASM_AUDIO_PARAMS:
/// "brightness,m0,...,m8" (10 f32). A test hook; unset is inactive.
fn audio_params_script() -> Option<[f32; 10]> {
    static PARAMS: OnceLock<Option<[f32; 10]>> = OnceLock::new();
    *PARAMS.get_or_init(|| {
        let text = std::env::var("TTFX_ASM_AUDIO_PARAMS").ok()?;
        let mut params = [0.0; 10];
        let mut count = 0;
        for (slot, part) in params.iter_mut().zip(text.split(',')) {
            *slot = part.trim().parse().ok()?;
            count += 1;
        }
        (count == 10).then_some(params)
    })
}

/// The CLI's audio_params: an inactive transform, or the scripted constants.
extern "C" fn audio_params_default(_context: *mut c_void, out: *mut f32) -> u64 {
    match audio_params_script() {
        Some(params) => {
            // SAFETY: the engine passes its own 40-byte audio_params slot.
            unsafe { std::ptr::copy_nonoverlapping(params.as_ptr(), out, 10) };
            1
        }
        None => 0,
    }
}

const FL_XTERM_COLORS: u64 = 1 << 0;
const FL_NO_COLOR: u64 = 1 << 1;
const FL_WRAP_TEXT: u64 = 1 << 2;
const FL_IGNORE_DIMS: u64 = 1 << 3;
const FL_REUSE_CANVAS: u64 = 1 << 4;
const FL_NO_EOL: u64 = 1 << 5;
const FL_NO_RESTORE: u64 = 1 << 6;
const FL_PARITY_DUMP: u64 = 1 << 7;
const FL_VIRTUAL_CLOCK: u64 = 1 << 8;
const FL_TTY_OUTPUT: u64 = 1 << 9;

const OUT_DECLINED: i64 = 0;
const OUT_COMPLETE: i64 = 1;
const OUT_INTERRUPTED: i64 = 2;
const OUT_TERMINATED: i64 = 3;
const OUT_RESIZED: i64 = 4;
const OUT_OUTPUT_CLOSED: i64 = 5;
const OUT_ERROR: i64 = 6;

const STOP_NONE: u64 = 0;
const STOP_INTERRUPT: u64 = 1;
const STOP_TERMINATE: u64 = 2;
const STOP_RESIZE: u64 = 3;

const ERR_ANSI: u64 = 1;

extern "C" {
    /// The best x86-64 level (1-4) the CPU and OS support (asm/tier.asm);
    /// always 1 on aarch64 (asm/aarch64/lib.s).
    fn ttfx_asm_tier() -> i32;
}

/// The best engine tier the CPU and OS support (asm/mod.rs's tier()).
pub fn tier() -> i32 {
    // SAFETY: ttfx_asm_tier only executes CPUID and XGETBV (or, on aarch64,
    // returns a constant).
    unsafe { ttfx_asm_tier() }
}

/// One tier's copy of the engine (asm/lib.asm assembled with -DTIER=n).
struct Engine {
    tier: u32,
    effect_supported: unsafe extern "C" fn(u64) -> i32,
    run: unsafe extern "C" fn(*mut Request) -> i64,
    #[cfg(any(target_arch = "x86_64", target_arch = "aarch64"))]
    audio_stats: unsafe extern "C" fn(*mut u64),
}

macro_rules! tiers {
    ($($tier:literal, $cfg:literal => $supported:ident, $run:ident, $stats:ident;)*) => {
        $(
            #[cfg(ttfx_asm_tier = $cfg)]
            extern "C" {
                fn $supported(effect: u64) -> i32;
                fn $run(request: *mut Request) -> i64;
                #[cfg(any(target_arch = "x86_64", target_arch = "aarch64"))]
                fn $stats(out: *mut u64);
            }
        )*

        /// The engine assembled for `tier`, when build.rs linked that tier.
        fn engine(tier: u32) -> Option<Engine> {
            $(
                #[cfg(ttfx_asm_tier = $cfg)]
                if tier == $tier {
                    return Some(Engine { tier, effect_supported: $supported, run: $run,
                        #[cfg(any(target_arch = "x86_64", target_arch = "aarch64"))] audio_stats: $stats });
                }
            )*
            None
        }
    };
}

tiers! {
    1, "1" => ttfx_asm_effect_supported_v1, ttfx_asm_run_v1, ttfx_asm_audio_export_stats_v1;
    2, "2" => ttfx_asm_effect_supported_v2, ttfx_asm_run_v2, ttfx_asm_audio_export_stats_v2;
    3, "3" => ttfx_asm_effect_supported_v3, ttfx_asm_run_v3, ttfx_asm_audio_export_stats_v3;
    4, "4" => ttfx_asm_effect_supported_v4, ttfx_asm_run_v4, ttfx_asm_audio_export_stats_v4;
}

/// The engine to run: the tier `TTFX_ASM_TIER` names (1-4, at most the
/// CPU's), or else the best linked tier the CPU supports.
fn select_engine() -> Result<Engine, &'static str> {
    // SAFETY: ttfx_asm_tier only executes CPUID and XGETBV (or, on aarch64,
    // returns a constant).
    let cpu = unsafe { ttfx_asm_tier() }.clamp(1, 4) as u32;
    let forced = std::env::var("TTFX_ASM_TIER").unwrap_or_default();
    if forced.is_empty() {
        return (1..=cpu)
            .rev()
            .find_map(engine)
            .ok_or("this CPU lacks the instruction sets of every assembled tier");
    }
    let tier = match forced.as_str() {
        "1" => 1,
        "2" => 2,
        "3" => 3,
        "4" => 4,
        _ => return Err("TTFX_ASM_TIER must be 1, 2, 3 or 4"),
    };
    if tier > cpu {
        return Err("TTFX_ASM_TIER names a tier this CPU does not support");
    }
    engine(tier).ok_or("TTFX_ASM_TIER names a tier this build left out")
}

/// What the stop check needs; lives on the stack for the duration of a run.
struct StopContext<'a> {
    config: &'a TerminalConfig,
    tty_output: bool,
    dimensions: (i64, i64),
    request: *const Request,
    resize_seen_at: Option<Instant>,
}

/// run_effect's requested_stop, called by the engine at the same points.
extern "C" fn stop_check(context: *mut c_void) -> u64 {
    // SAFETY: the engine passes back the pointer offer() gave it, which
    // outlives the run; nothing else holds a reference to it meanwhile.
    let context = unsafe { &mut *(context as *mut StopContext<'_>) };
    if crate::interrupted() {
        return STOP_INTERRUPT;
    }
    if crate::terminated() {
        return STOP_TERMINATE;
    }
    if context.tty_output {
        // SAFETY: the engine publishes the line lengths before its first stop
        // check and keeps them alive for the whole run.
        let line_lengths = unsafe {
            let request = &*context.request;
            if request.line_lengths.is_null() {
                &[][..]
            } else {
                std::slice::from_raw_parts(request.line_lengths, request.line_count as usize)
            }
        };
        if terminal::resize_settled(
            &mut context.resize_seen_at,
            context.config,
            line_lengths,
            context.dimensions,
        ) {
            return STOP_RESIZE;
        }
    }
    STOP_NONE
}

/// The engine's color format: 0xRRGGBB, plus the xterm code in bits 32-39 and
/// bit 40 for colors built from one (they render as that code under
/// --xterm-colors instead of the nearest match).
pub fn color_word(color: &Color) -> u64 {
    let (r, g, b) = color.rgb_ints();
    let rgb = (r as u64) << 16 | (g as u64) << 8 | b as u64;
    match color.xterm_color {
        Some(code) => rgb | (code as u64) << 32 | 1 << 40,
        None => rgb,
    }
}

pub fn offer(run: Run<'_>) -> Result<Result<RunOutcome, EngineError>, &'static str> {
    offer_with_hook(run, None)
}

#[cfg(any(target_arch = "x86_64", target_arch = "aarch64"))]
pub fn offer_direct(
    run: Run<'_>,
    hook: DirectFrameHook,
) -> Result<Result<RunOutcome, EngineError>, &'static str> {
    offer_with_hook(run, Some(hook))
}

fn offer_with_hook(
    run: Run<'_>,
    direct: Option<DirectFrameHook>,
) -> Result<Result<RunOutcome, EngineError>, &'static str> {
    with_engine_lock(|| offer_with_hook_locked(run, direct))?
}

fn offer_with_hook_locked(
    run: Run<'_>,
    direct: Option<DirectFrameHook>,
) -> Result<Result<RunOutcome, EngineError>, &'static str> {
    let engine = select_engine()?;
    if std::env::var_os("TTFX_ASM_SHOW_TIER").is_some() {
        crate::errln!("ttfx: asm engine tier {}", engine.tier);
    }
    let config = run.config;
    let (id, words): (u64, Words) = effects::marshal(run.effect)?;
    // SAFETY: a pure query of the engine's effect table.
    if unsafe { (engine.effect_supported)(id) } == 0 {
        return Err("this effect is not ported yet");
    }

    let dimensions = terminal::get_terminal_dimensions();
    let flags = [
        (config.xterm_colors, FL_XTERM_COLORS),
        (config.no_color, FL_NO_COLOR),
        (config.wrap_text, FL_WRAP_TEXT),
        (config.ignore_terminal_dimensions, FL_IGNORE_DIMS),
        (config.reuse_canvas, FL_REUSE_CANVAS),
        (config.no_eol, FL_NO_EOL),
        (config.no_restore_cursor, FL_NO_RESTORE),
        (run.parity_dump, FL_PARITY_DUMP),
        (run.virtual_clock, FL_VIRTUAL_CLOCK),
        (run.tty_output, FL_TTY_OUTPUT),
    ]
    .iter()
    .filter(|(on, _)| *on)
    .fold(0, |flags, (_, bit)| flags | bit);

    let mut request = Box::new(Request {
        input_ptr: run.input.as_ptr(),
        input_len: run.input.len() as u64,
        effect: id,
        effect_config: words.as_ptr(),
        tab_width: config.tab_width,
        frame_rate: config.frame_rate,
        canvas_width: config.canvas_width,
        canvas_height: config.canvas_height,
        anchor_canvas: config.anchor_canvas as u64,
        anchor_text: config.anchor_text as u64,
        existing_colors: config.existing_color_handling as u64,
        background: color_word(&config.terminal_background_color),
        flags,
        max_frames: run.max_frames.unwrap_or(u64::MAX),
        term_width: dimensions.0,
        term_height: dimensions.1,
        rng_state: run.rng.state(),
        stop_check,
        stop_ctx: std::ptr::null_mut(),
        line_lengths: std::ptr::null(),
        line_count: 0,
        error_ptr: std::ptr::null(),
        error_len: 0,
        error_kind: 0,
        audio_poll: audio_poll_default,
        audio_params: audio_params_default,
        audio_ctx: std::ptr::null_mut(),
        direct_emit: direct.as_ref().map(|hook| hook.emit),
        direct_ctx: direct
            .as_ref()
            .map_or(std::ptr::null_mut(), |hook| hook.ctx),
    });
    if let Some(hook) = run.audio {
        request.audio_poll = hook.poll;
        request.audio_params = hook.params;
        request.audio_ctx = hook.ctx;
    }
    SCRIPT_FRAME.store(0, Ordering::Relaxed);
    let mut stop = StopContext {
        config,
        tty_output: run.tty_output,
        dimensions,
        request: &*request,
        resize_seen_at: None,
    };
    request.stop_ctx = &mut stop as *mut StopContext<'_> as *mut c_void;

    // SAFETY: the request and everything it points to (input, config words,
    // stop context) outlive the call; the engine writes only the out fields.
    #[cfg(any(target_arch = "x86_64", target_arch = "aarch64"))]
    ACTIVE_AUDIO_STATS.with(|active| active.set(Some(engine.audio_stats)));
    let outcome = unsafe { (engine.run)(&mut *request) };
    drop(words);
    if outcome == OUT_DECLINED {
        return Err("the engine declined the run");
    }
    *run.rng = Rng::from_state(request.rng_state);
    Ok(match outcome {
        OUT_COMPLETE => Ok(RunOutcome::Complete),
        OUT_INTERRUPTED => Ok(RunOutcome::Interrupted),
        OUT_TERMINATED => Ok(RunOutcome::Terminated),
        OUT_RESIZED => Ok(RunOutcome::TerminalResized),
        OUT_OUTPUT_CLOSED => Ok(RunOutcome::OutputClosed),
        OUT_ERROR => {
            // SAFETY: the engine points the error at static data or at the
            // input, both alive here.
            let bytes = unsafe {
                std::slice::from_raw_parts(request.error_ptr, request.error_len as usize)
            };
            let text = String::from_utf8_lossy(bytes).into_owned();
            Err(if request.error_kind == ERR_ANSI {
                EngineError::UnsupportedAnsiSequence(text)
            } else {
                EngineError::Other(text)
            })
        }
        errno if errno < 0 => Err(EngineError::Other(format!(
            "io error: {}",
            std::io::Error::from_raw_os_error(-errno as i32)
        ))),
        other => Err(EngineError::Other(format!(
            "asm engine: unknown outcome {other}"
        ))),
    })
}

#[cfg(test)]
mod run_gate_tests {
    use super::with_engine_lock;
    use std::sync::mpsc;
    use std::time::Duration;

    #[test]
    fn independent_assembly_runs_wait_for_the_active_run() {
        let (entered_tx, entered_rx) = mpsc::channel();
        let (release_tx, release_rx) = mpsc::channel();
        let first = std::thread::spawn(move || {
            with_engine_lock(|| {
                entered_tx.send(()).unwrap();
                release_rx.recv().unwrap();
            })
            .unwrap();
        });
        entered_rx.recv_timeout(Duration::from_secs(2)).unwrap();

        let (attempt_tx, attempt_rx) = mpsc::channel();
        let (second_entered_tx, second_entered_rx) = mpsc::channel();
        let second = std::thread::spawn(move || {
            attempt_tx.send(()).unwrap();
            with_engine_lock(|| second_entered_tx.send(()).unwrap()).unwrap();
        });
        attempt_rx.recv_timeout(Duration::from_secs(2)).unwrap();
        assert!(second_entered_rx
            .recv_timeout(Duration::from_millis(100))
            .is_err());
        release_tx.send(()).unwrap();
        first.join().unwrap();
        second_entered_rx
            .recv_timeout(Duration::from_secs(2))
            .unwrap();
        second.join().unwrap();
    }

    #[test]
    fn nested_assembly_run_from_callback_returns_without_deadlock() {
        let (done_tx, done_rx) = mpsc::channel();
        std::thread::spawn(move || {
            done_tx
                .send(with_engine_lock(|| with_engine_lock(|| ())))
                .unwrap();
        });
        let result = done_rx.recv_timeout(Duration::from_millis(200));
        assert!(
            matches!(result, Ok(Ok(Err(_)))),
            "a callback trying to re-enter ASM must be rejected promptly: {result:?}"
        );
    }
}

#[cfg(test)]
mod direct_request_layout {
    use super::Request;

    #[test]
    fn request_offsets_match_nasm_and_aarch64_definitions() {
        for (field, actual, expected) in [
            ("input_ptr", std::mem::offset_of!(Request, input_ptr), 0),
            ("input_len", std::mem::offset_of!(Request, input_len), 8),
            ("effect", std::mem::offset_of!(Request, effect), 16),
            (
                "effect_config",
                std::mem::offset_of!(Request, effect_config),
                24,
            ),
            ("tab_width", std::mem::offset_of!(Request, tab_width), 32),
            ("frame_rate", std::mem::offset_of!(Request, frame_rate), 40),
            (
                "canvas_width",
                std::mem::offset_of!(Request, canvas_width),
                48,
            ),
            (
                "canvas_height",
                std::mem::offset_of!(Request, canvas_height),
                56,
            ),
            (
                "anchor_canvas",
                std::mem::offset_of!(Request, anchor_canvas),
                64,
            ),
            (
                "anchor_text",
                std::mem::offset_of!(Request, anchor_text),
                72,
            ),
            (
                "existing_colors",
                std::mem::offset_of!(Request, existing_colors),
                80,
            ),
            ("background", std::mem::offset_of!(Request, background), 88),
            ("flags", std::mem::offset_of!(Request, flags), 96),
            ("max_frames", std::mem::offset_of!(Request, max_frames), 104),
            ("term_width", std::mem::offset_of!(Request, term_width), 112),
            (
                "term_height",
                std::mem::offset_of!(Request, term_height),
                120,
            ),
            ("rng_state", std::mem::offset_of!(Request, rng_state), 128),
            ("stop_check", std::mem::offset_of!(Request, stop_check), 160),
            ("stop_ctx", std::mem::offset_of!(Request, stop_ctx), 168),
            (
                "line_lengths",
                std::mem::offset_of!(Request, line_lengths),
                176,
            ),
            ("line_count", std::mem::offset_of!(Request, line_count), 184),
            ("error_ptr", std::mem::offset_of!(Request, error_ptr), 192),
            ("error_len", std::mem::offset_of!(Request, error_len), 200),
            ("error_kind", std::mem::offset_of!(Request, error_kind), 208),
            ("audio_poll", std::mem::offset_of!(Request, audio_poll), 216),
            (
                "audio_params",
                std::mem::offset_of!(Request, audio_params),
                224,
            ),
            ("audio_ctx", std::mem::offset_of!(Request, audio_ctx), 232),
            (
                "direct_emit",
                std::mem::offset_of!(Request, direct_emit),
                240,
            ),
            ("direct_ctx", std::mem::offset_of!(Request, direct_ctx), 248),
        ] {
            assert_eq!(actual, expected, "Request.{field} ABI offset");
        }
        assert_eq!(std::mem::size_of::<Request>(), 256);
    }
}

#[cfg(any(target_arch = "x86_64", target_arch = "aarch64"))]
pub fn direct_audio_export_stats() -> Option<[u64; 3]> {
    if !ENGINE_RUN_ACTIVE.with(Cell::get) { return None; }
    ACTIVE_AUDIO_STATS.with(|active| active.get().map(|read| {
        let mut out=[0u64;3];
        // SAFETY: same-thread active run holds the engine lock; getter only
        // copies that tier's numeric counters to this writable 24-byte array.
        unsafe { read(out.as_mut_ptr()); }
        out
    }))
}
