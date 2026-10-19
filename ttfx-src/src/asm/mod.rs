//! In-process dispatch to the assembly engine: x86-64 (asm/, plans/asm-x86.md)
//! or its aarch64 port (asm/aarch64/, asm/aarch64/PORTING.md).
//!
//! Rust stays the front end: it parses the command line, reads and validates
//! the input, seeds the RNG and installs the signal handlers. Each run is then
//! offered to the assembly engine, which either declines before doing anything
//! observable — so the Rust engine runs it instead — or runs the effect to the
//! end and reports how it ended. Output is byte-identical either way.
//!
//! `TTFX_ASM=0` forces the Rust engine; `TTFX_ASM=force` turns a decline into
//! an error, so tests cannot silently fall back.
//!
//! The engine is assembled once per x86-64 level (v1 = SSE2, v2, v3 = AVX2,
//! v4 = AVX-512), and the best one the CPU supports runs; every tier's output
//! is byte-identical. aarch64 has a single tier (1: ARMv8.0-A with NEON).
//! `TTFX_ASM_TIER=1|2|3|4` forces a lower tier for testing:
//! a tier above the CPU's, or one the build left out, is a decline (exit 3
//! under `TTFX_ASM=force`). `TTFX_ASM_SHOW_TIER=1` prints on stderr which
//! engine ran: the asm tier chosen, or why the Rust engine ran instead. See asm/PORTING.md, "CPU tiers".

use std::ffi::c_void;

use crate::effects::EffectCommand;
use crate::engine::effect::RunOutcome;
use crate::engine::error::EngineError;
use crate::engine::terminal::TerminalConfig;
use crate::utils::rng::Rng;

#[cfg(ttfx_asm)]
mod effects;
#[cfg(ttfx_asm)]
mod ffi;

/// The audio hook an embedder installs for a run; both callbacks take the
/// context pointer and are called once per frame by the engine
/// (audio_frame_poll in asm/lib.asm). `poll` packs volume (byte 0), bass
/// (byte 1), held beat flag (byte 2), treble (byte 3), and BPM (byte 4),
/// all zero for silence. The x86 engine ignores stale tempo/beat when the
/// three energy channels are zero. `params` writes a
/// brightness and a row-major 3x3 hue matrix (10 f32) and returns 1 when the
/// color transform is active. Without a hook the run is byte-identical to
/// the reference engine.
pub struct AudioHook {
    pub poll: extern "C" fn(*mut c_void) -> u64,
    pub params: extern "C" fn(*mut c_void, *mut f32) -> u64,
    pub ctx: *mut c_void,
}

/// Borrowed final-frame grid emitted by the assembly renderer. The callback
/// must copy any data it needs before returning; all pointers expire with it.
pub struct DirectFrameHook {
    /// Called only with the context and borrowed frame promised by `run_direct`.
    pub emit: unsafe extern "C" fn(*mut c_void, u64, u64, *const u32, *const u8) -> u64,
    pub ctx: *mut c_void,
}

/// One run as the front end sees it.
pub struct Run<'a> {
    pub effect: &'a EffectCommand,
    pub input: &'a str,
    pub config: &'a TerminalConfig,
    pub rng: &'a mut Rng,
    pub parity_dump: bool,
    pub virtual_clock: bool,
    pub max_frames: Option<u64>,
    pub tty_output: bool,
    /// The audio hook the engine polls once per frame; None is silence.
    pub audio: Option<AudioHook>,
}

/// The best engine tier this build offers the CPU (0 without the assembly
/// engine): evidence that a run is assembly, not the Rust fallback.
pub fn tier() -> i32 {
    #[cfg(ttfx_asm)]
    {
        ffi::tier()
    }
    #[cfg(not(ttfx_asm))]
    {
        0
    }
}

/// Run the effect on the assembly engine when it can take it. None means the
/// Rust engine must run it (nothing was written and the RNG is untouched).
/// Assembly runs are serialized process-wide, including direct-frame runs.
pub fn try_run(run: Run<'_>) -> Option<Result<RunOutcome, EngineError>> {
    let mode = std::env::var("TTFX_ASM").unwrap_or_default();
    let show = std::env::var_os("TTFX_ASM_SHOW_TIER").is_some();
    if mode == "0" || mode == "off" {
        if show {
            crate::errln!("ttfx: Rust engine (TTFX_ASM={mode})");
        }
        return None;
    }
    let result = offer(run);
    if let Err(reason) = &result {
        if mode == "force" {
            crate::errln!("ttfx: TTFX_ASM=force, but the assembly engine declined: {reason}");
            std::process::exit(3);
        }
        if show {
            crate::errln!("ttfx: Rust engine (the assembly engine declined: {reason})");
        }
    }
    result.ok()
}

/// Run the real x86 assembly engine with typed frames, never falling back to
/// the Rust effect implementation or serializing terminal output.
///
/// # Safety
/// `hook.ctx` must remain valid and exclusively accessible to `hook.emit` for
/// this entire synchronous run. The callback must accept the borrowed frame
/// grid (`cols * rows` readable `u32` handles) and visual pool supplied by
/// the assembly engine, and must not retain either pointer after it returns.
/// Every nonzero handle's pool bytes and preceding 32-byte header are valid
/// only during the callback. The callback must not unwind across the C ABI.
/// The assembly engine has process-global state: calls from other threads wait
/// for this run to finish; callbacks must not re-enter either assembly entry
/// point on the same thread (such an attempt is rejected).
pub unsafe fn run_direct(run: Run<'_>, hook: DirectFrameHook) -> Result<RunOutcome, EngineError> {
    #[cfg(all(ttfx_asm, any(target_arch = "x86_64", target_arch = "aarch64")))]
    {
        if run.parity_dump || run.tty_output {
            return Err(EngineError::Other(
                "direct frames cannot use terminal output or parity dump".into(),
            ));
        }
        ffi::offer_direct(run, hook).map_err(|reason| EngineError::Other(reason.into()))?
    }
    #[cfg(not(all(ttfx_asm, any(target_arch = "x86_64", target_arch = "aarch64"))))]
    {
        let _ = (run, hook);
        Err(EngineError::Other(
            "direct ASM frames require an assembly engine for this architecture".into(),
        ))
    }
}

#[cfg(not(ttfx_asm))]
fn offer(_run: Run<'_>) -> Result<Result<RunOutcome, EngineError>, &'static str> {
    Err("this build has no assembly engine")
}

#[cfg(ttfx_asm)]
fn offer(run: Run<'_>) -> Result<Result<RunOutcome, EngineError>, &'static str> {
    ffi::offer(run)
}

#[cfg(test)]
mod direct_frame_contract {
    use super::*;

    unsafe extern "C" fn accept_frame(
        _ctx: *mut c_void,
        _cols: u64,
        _rows: u64,
        _handles: *const u32,
        _pool: *const u8,
    ) -> u64 {
        0
    }

    #[test]
    fn direct_frame_hook_has_the_borrowed_grid_contract() {
        let hook = DirectFrameHook {
            emit: accept_frame,
            ctx: std::ptr::null_mut(),
        };
        let _ = (hook.emit, hook.ctx);
    }
}

/// Numeric current-frame audio export counters, only inside a synchronous ASM
/// callback: [active, unique visuals transformed, unique RGB headers changed].
/// This observer never enters another run or retains borrowed frame pointers.
pub fn direct_audio_export_stats() -> Option<[u64; 3]> {
    #[cfg(all(ttfx_asm, any(target_arch = "x86_64", target_arch = "aarch64")))]
    { ffi::direct_audio_export_stats() }
    #[cfg(not(all(ttfx_asm, any(target_arch = "x86_64", target_arch = "aarch64"))))]
    { None }
}
