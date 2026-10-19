#![cfg(any(target_arch = "x86_64", target_arch = "aarch64"))]

use clap::Parser;
use std::ffi::c_void;
use ttfx::asm::{self, DirectFrameHook};
use ttfx::cli::Cli;
use ttfx::engine::terminal::TerminalConfig;
use ttfx::utils::rng::Rng;

#[derive(Default)]
struct Attributes {
    frames: u64,
    background_cells: u64,
    attribute_cells: u64,
    nonblank: u64,
}

unsafe extern "C" fn inspect(
    ctx: *mut c_void,
    cols: u64,
    rows: u64,
    grid: *const u32,
    pool: *const u8,
) -> u64 {
    let stats = unsafe { &mut *(ctx as *mut Attributes) };
    stats.frames += 1;
    let handles = unsafe { std::slice::from_raw_parts(grid, (cols * rows) as usize) };
    for &handle in handles {
        if handle == 0 {
            continue;
        }
        let visual = unsafe { pool.add((handle & 0x00ff_ffff) as usize) };
        let symbol = unsafe { (visual.sub(32) as *const u64).read_unaligned() };
        let bg = unsafe { (visual.sub(16) as *const u64).read_unaligned() };
        let attrs = unsafe { (visual.sub(8) as *const u64).read_unaligned() };
        stats.nonblank += u64::from((symbol & 0xff) != b' ' as u64);
        stats.background_cells += u64::from(bg != u64::MAX);
        stats.attribute_cells += u64::from(attrs != 0);
    }
    0
}

#[test]
#[ignore = "diagnostic of full ASM Thunderstorm's real visual attributes"]
fn inspect_thunderstorm_visual_contract() {
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
    let mut stats = Attributes::default();
    // SAFETY: stats stays live and exclusive for the synchronous run; inspect
    // reads valid ASM frame storage only before each callback returns.
    let outcome = unsafe {
        asm::run_direct(
            asm::Run {
                effect: &effect,
                input: "THUNDERSTORM",
                config: &config,
                rng: &mut rng,
                parity_dump: false,
                virtual_clock: true,
                max_frames: Some(180),
                tty_output: false,
                audio: None,
            },
            DirectFrameHook {
                emit: inspect,
                ctx: (&mut stats as *mut Attributes).cast(),
            },
        )
    }
    .unwrap();
    eprintln!(
        "outcome={outcome:?} frames={} nonblank={} background={} attrs={}",
        stats.frames, stats.nonblank, stats.background_cells, stats.attribute_cells
    );
    assert_eq!(stats.frames, 180);
}
