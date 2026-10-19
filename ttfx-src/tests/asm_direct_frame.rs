#![cfg(any(target_arch = "x86_64", target_arch = "aarch64"))]

use clap::Parser;
use std::ffi::c_void;
use ttfx::asm::{self, AudioHook, DirectFrameHook};
use ttfx::cli::Cli;
use ttfx::engine::effect::RunOutcome;
use ttfx::engine::terminal::TerminalConfig;
use ttfx::utils::rng::Rng;

#[derive(Default)]
struct Captures {
    dimensions: Vec<(u64, u64)>,
    occupied: Vec<usize>,
    audio_polls: u64,
    stop_after: usize,
}

extern "C" fn audio_poll(context: *mut c_void) -> u64 {
    let captures = unsafe { &mut *(context as *mut Captures) };
    captures.audio_polls += 1;
    0
}

extern "C" fn audio_params(_context: *mut c_void, _out: *mut f32) -> u64 {
    0
}

unsafe extern "C" fn capture(
    context: *mut c_void,
    cols: u64,
    rows: u64,
    grid: *const u32,
    pool: *const u8,
) -> u64 {
    let captures = unsafe { &mut *(context as *mut Captures) };
    let frame = unsafe { std::slice::from_raw_parts(grid, (cols * rows) as usize) };
    assert!(!pool.is_null());
    captures.dimensions.push((cols, rows));
    captures
        .occupied
        .push(frame.iter().filter(|&&handle| handle != frame[0]).count());
    u64::from(captures.stop_after != 0 && captures.dimensions.len() >= captures.stop_after)
}

#[test]
fn real_asm_thunderstorm_emits_bounded_borrowed_frames() {
    std::env::set_var("TTFX_ASM", "force");
    let effect = Cli::try_parse_from(["ttfx", "thunderstorm"])
        .unwrap()
        .effect
        .unwrap();
    let config = TerminalConfig {
        canvas_width: 236,
        canvas_height: 78,
        ignore_terminal_dimensions: true,
        frame_rate: 60,
        ..TerminalConfig::default()
    };
    let mut rng = Rng::seeded(5);
    let mut captures = Captures::default();
    let hook = DirectFrameHook {
        emit: capture,
        ctx: &mut captures as *mut Captures as *mut c_void,
    };
    let audio = AudioHook {
        poll: audio_poll,
        params: audio_params,
        ctx: &mut captures as *mut Captures as *mut c_void,
    };
    // SAFETY: captures stays live and exclusive throughout the synchronous
    // run; capture reads the ASM-owned grid/pool only during each callback.
    let outcome = unsafe {
        asm::run_direct(
            asm::Run {
                effect: &effect,
                input: "THUNDERSTORM",
                config: &config,
                rng: &mut rng,
                parity_dump: false,
                virtual_clock: true,
                max_frames: Some(3),
                tty_output: false,
                audio: Some(audio),
            },
            hook,
        )
    }
    .unwrap();
    assert_eq!(outcome, RunOutcome::Complete);
    assert_eq!(captures.dimensions.len(), 3);
    assert!(captures.dimensions.iter().all(|&shape| shape == (236, 78)));
    assert!(captures.occupied.iter().any(|&count| count > 0));
    assert!(captures.audio_polls >= 4, "ASM must keep polling audio");

    captures.stop_after = 2;
    captures.dimensions.clear();
    // SAFETY: captures stays live and exclusive for this second run; the
    // callback does not retain any borrowed frame pointer.
    let outcome = unsafe {
        asm::run_direct(
            asm::Run {
                effect: &effect,
                input: "THUNDERSTORM",
                config: &config,
                rng: &mut rng,
                parity_dump: false,
                virtual_clock: true,
                max_frames: Some(50),
                tty_output: false,
                audio: None,
            },
            DirectFrameHook {
                emit: capture,
                ctx: &mut captures as *mut Captures as *mut c_void,
            },
        )
    }
    .unwrap();
    assert_eq!(outcome, RunOutcome::Terminated);
    assert_eq!(captures.dimensions.len(), 2);
}
