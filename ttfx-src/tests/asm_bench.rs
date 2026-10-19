#![cfg(any(target_arch = "x86_64", target_arch = "aarch64"))]

//! Engine-level A/B benchmark: the Rust reference engine against the
//! assembly engine on the same effect, input, canvas, seed and virtual
//! clock, interleaved ABBA within one process. Wall time plus process CPU
//! (utime+stime from /proc/self/stat) per rep. Both engines write their ANSI
//! bytes to fd 1 — run with stdout redirected so the sink cost is real and
//! equal:
//!
//!   cargo test --release --locked --test asm_bench -- --ignored --nocapture > /dev/null
//!
//! Engine-only producer numbers: no display, compositor, GTK, or scanout.
//! They say nothing about end-to-end application CPU or physical FPS.

use clap::Parser;
use std::ffi::c_void;
use std::time::Instant;
use ttfx::cli::Cli;
use ttfx::engine::ctx::{Clock, EngineCtx};
use ttfx::engine::effect::{run_effect, RunOutcome};
use ttfx::utils::rng::Rng;

const INPUT: &str = "ABCDEFGHIJKLMNOPQRSTUVWXYZ\n0123456789 ASM AUDIO\nabcdefghijklmnopqrstuvwxyz\n\
                     ABCDEFGHIJKLMNOPQRSTUVWXYZ\n0123456789 ASM AUDIO\nabcdefghijklmnopqrstuvwxyz\n\
                     ABCDEFGHIJKLMNOPQRSTUVWXYZ\n0123456789 ASM AUDIO\nabcdefghijklmnopqrstuvwxyz\n\
                     ABCDEFGHIJKLMNOPQRSTUVWXYZ\n0123456789 ASM AUDIO\nabcdefghijklmnopqrstuvwxyz\n\
                     ABCDEFGHIJKLMNOPQRSTUVWXYZ\n0123456789 ASM AUDIO\nabcdefghijklmnopqrstuvwxyz\n\
                     ABCDEFGHIJKLMNOPQRSTUVWXYZ\n0123456789 ASM AUDIO\nabcdefghijklmnopqrstuvwxyz\n\
                     ABCDEFGHIJKLMNOPQRSTUVWXYZ\n0123456789 ASM AUDIO\nabcdefghijklmnopqrstuvwxyz\n\
                     ABCDEFGHIJKLMNOPQRSTUVWXYZ\n0123456789 ASM AUDIO\nabcdefghijklmnopqrstuvwxyz";
const SEED: u64 = 20261004;
const REPS: usize = 6;

/// The full ASM catalog, mirrored from asm_effect_audio_profiles.rs so the
/// coverage claim is the same 35 effects the parity suite proves.
const CATALOG: [&str; 35] = [
    "beams", "binarypath", "blackhole", "bouncyballs", "bubbles", "burn", "colorshift", "crumble",
    "decrypt", "errorcorrect", "expand", "fireworks", "highlight", "laseretch", "middleout",
    "orbittingvolley", "overflow", "pour", "print", "randomsequence", "rings", "scattered",
    "slice", "slide", "smoke", "spotlights", "spray", "swarm", "sweep", "synthgrid",
    "thunderstorm", "unstable", "vhstape", "waves", "wipe",
];

/// utime + stime of this process in nanoseconds (100 Hz ticks on Linux).
fn process_cpu_ns() -> u64 {
    let stat = std::fs::read_to_string("/proc/self/stat").unwrap();
    let after_comm = stat.rsplit_once(')').unwrap().1;
    let fields: Vec<&str> = after_comm.split_whitespace().collect();
    // fields[0] is state (field 3); utime is field 14, stime field 15.
    let utime: u64 = fields[11].parse().unwrap();
    let stime: u64 = fields[12].parse().unwrap();
    (utime + stime) * 10_000_000
}

fn rust_run(argv: &[&str]) -> RunOutcome {
    let cli = Cli::try_parse_from(argv).unwrap();
    let config = cli.terminal_config();
    let effect_command = cli.effect.unwrap();
    let clock = Clock::virtual_with_frame_rate(config.frame_rate);
    let mut ctx = EngineCtx::new(INPUT, config, Rng::seeded(SEED), clock).unwrap();
    let mut effect = effect_command.build_effect();
    run_effect(effect.as_mut(), &mut ctx, false).unwrap()
}

fn asm_run(argv: &[&str]) -> RunOutcome {
    assert!(ttfx::asm::tier() > 0, "the assembly engine must be linked");
    let cli = Cli::try_parse_from(argv).unwrap();
    let config = cli.terminal_config();
    let effect = cli.effect.unwrap();
    let mut rng = Rng::seeded(SEED);
    let run = ttfx::asm::Run {
        effect: &effect,
        input: INPUT,
        config: &config,
        rng: &mut rng,
        parity_dump: false,
        virtual_clock: true,
        max_frames: None,
        tty_output: false,
        audio: None,
    };
    // try_run is safe: with no hooks installed nothing borrows past the run.
    ttfx::asm::try_run(run)
        .expect("the assembly engine must accept these effects")
        .unwrap()
}

fn timed(mut f: impl FnMut() -> RunOutcome) -> (u64, u64) {
    let cpu0 = process_cpu_ns();
    let wall = Instant::now();
    let outcome = f();
    let wall_ns = wall.elapsed().as_nanos() as u64;
    assert_eq!(outcome, RunOutcome::Complete);
    (wall_ns, process_cpu_ns() - cpu0)
}

fn median(v: &mut Vec<u64>) -> u64 {
    v.sort();
    v[v.len() / 2]
}

/// Direct-path phase probe (the app's producer route): run_direct with a
/// frame counter, once without audio (export only) and once with a color
/// matrix whose epoch changes every frame (export plus re-tint, the
/// worst-case music-active state). Reports ns per frame for each phase.
struct DirectProbe {
    frames: u64,
    polls: u64,
}

extern "C" fn count_frame(
    context: *mut c_void,
    _cols: u64,
    _rows: u64,
    _grid: *const u32,
    _pool: *const u8,
) -> u64 {
    let probe = unsafe { &mut *(context as *mut DirectProbe) };
    probe.frames += 1;
    0
}

extern "C" fn probe_poll(_context: *mut c_void) -> u64 {
    // volume/bass/beat/treble/bpm present so the audio word is non-zero.
    (180 << 8) | (1 << 16) | (120 << 24) | (128 << 32)
}

extern "C" fn probe_params(context: *mut c_void, out: *mut f32) -> u64 {
    let probe = unsafe { &mut *(context as *mut DirectProbe) };
    probe.polls += 1;
    let matrix = [1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0 + probe.polls as f32 * 1e-6];
    unsafe {
        std::ptr::copy_nonoverlapping(matrix.as_ptr(), out, 9);
        *out.add(9) = 1.0;
    }
    0
}

fn direct_run(argv: &[&str], audio: bool, probe: &mut DirectProbe) -> (u64, u64) {
    let cli = Cli::try_parse_from(argv).unwrap();
    let config = cli.terminal_config();
    let effect = cli.effect.unwrap();
    let mut rng = Rng::seeded(SEED);
    let hook = ttfx::asm::DirectFrameHook {
        emit: count_frame,
        ctx: probe as *mut DirectProbe as *mut c_void,
    };
    let audio_hook = ttfx::asm::AudioHook {
        poll: probe_poll,
        params: probe_params,
        ctx: probe as *mut DirectProbe as *mut c_void,
    };
    let run = ttfx::asm::Run {
        effect: &effect,
        input: INPUT,
        config: &config,
        rng: &mut rng,
        parity_dump: false,
        virtual_clock: true,
        max_frames: None,
        tty_output: false,
        audio: audio.then_some(audio_hook),
    };
    let cpu0 = process_cpu_ns();
    let wall = Instant::now();
    // SAFETY: probe stays live and exclusive throughout the synchronous
    // run; the callback never retains the borrowed grid/pool pointers.
    let outcome = unsafe { ttfx::asm::run_direct(run, hook) }.unwrap();
    let wall_ns = wall.elapsed().as_nanos() as u64;
    assert_eq!(outcome, RunOutcome::Complete);
    (wall_ns, process_cpu_ns() - cpu0)
}

#[test]
#[ignore = "benchmark: run explicitly with --ignored --nocapture, fd1 to /dev/null"]
fn direct_path_phases() {
    for (effect, args) in [("thunderstorm", &["--storm-time", "30"][..]), ("rings", &["--spin-disperse-cycles", "2", "--spin-duration", "600", "--disperse-duration", "5"][..])] {
        let mut argv = vec!["ttfx", "--canvas-width", "120", "--canvas-height", "30", "--ignore-terminal-dimensions", effect];
        argv.extend_from_slice(args);
        for audio in [false, true] {
            let mut per_frame = vec![];
            for _ in 0..REPS {
                let mut probe = DirectProbe { frames: 0, polls: 0 };
                let (wall, _cpu) = direct_run(&argv, audio, &mut probe);
                per_frame.push(wall / probe.frames.max(1));
            }
            let med = median(&mut per_frame);
            eprintln!(
                "DIRECT {effect} audio={audio}: {med} ns/frame ({} reps)",
                REPS
            );
        }
    }
}

#[test]
#[ignore = "benchmark: run explicitly with --ignored --nocapture, fd1 to /dev/null"]
fn rust_vs_asm_catalog() {
    // All 35 catalog effects, default args, natural completion for both
    // engines (parity suites already prove the bytes match). 3 ABBA reps;
    // the effect name prints BEFORE its runs so a hung effect is visible.
    let canvas = [
        "--canvas-width",
        "80",
        "--canvas-height",
        "24",
        "--ignore-terminal-dimensions",
    ];
    eprintln!("effect,rust_wall_ms,asm_wall_ms,ratio");
    for effect in CATALOG {
        eprintln!("START {effect}");
        let mut argv = vec!["ttfx"];
        argv.extend_from_slice(&canvas);
        argv.push(effect);
        let mut rust_wall = vec![];
        let mut asm_wall = vec![];
        for rep in 0..3 {
            if rep % 2 == 0 {
                let (w, _) = timed(|| rust_run(&argv));
                rust_wall.push(w);
            }
            let (w, _) = timed(|| asm_run(&argv));
            asm_wall.push(w);
            if rep % 2 != 0 {
                let (w, _) = timed(|| rust_run(&argv));
                rust_wall.push(w);
            }
        }
        let rw = median(&mut rust_wall);
        let aw = median(&mut asm_wall);
        eprintln!(
            "CATALOG {effect}: rust {:.3} ms, asm {:.3} ms, ratio {:.2}",
            rw / 1_000_000,
            aw / 1_000_000,
            rw as f64 / aw.max(1) as f64
        );
    }
}

#[test]
#[ignore = "benchmark: run explicitly with --ignored --nocapture, fd1 to /dev/null"]
fn rust_vs_asm_engine() {
    let effects: [(&str, &[&str]); 5] = [
        ("print", &[]),
        ("decrypt", &[]),
        ("rain", &[]),
        ("rings", &["--spin-disperse-cycles", "2", "--spin-duration", "600", "--disperse-duration", "5"]),
        ("thunderstorm", &["--storm-time", "30"]),
    ];
    let canvas = ["--canvas-width", "120", "--canvas-height", "30", "--ignore-terminal-dimensions"];
    eprintln!("effect,engine,rep,wall_ms,cpu_ms");
    for (effect, args) in effects {
        // Canvas flags are top-level; effect options follow the subcommand.
        let mut argv = vec!["ttfx"];
        argv.extend_from_slice(&canvas);
        argv.push(effect);
        argv.extend_from_slice(args);
        let mut rust_wall = vec![];
        let mut rust_cpu = vec![];
        let mut asm_wall = vec![];
        let mut asm_cpu = vec![];
        for rep in 0..REPS {
            // ABBA: interleave so drift hits both engines evenly.
            let (first_rust, then_rust) = (rep % 2 == 0, rep % 2 != 0);
            if first_rust {
                let (w, c) = timed(|| rust_run(&argv));
                rust_wall.push(w);
                rust_cpu.push(c);
            }
            let (w, c) = timed(|| asm_run(&argv));
            asm_wall.push(w);
            asm_cpu.push(c);
            if then_rust {
                let (w, c) = timed(|| rust_run(&argv));
                rust_wall.push(w);
                rust_cpu.push(c);
            }
            eprintln!(
                "{effect},rust,{rep},{:.3},{:.3}",
                rust_wall.last().unwrap_or(&0) / 1_000_000,
                rust_cpu.last().unwrap_or(&0) / 1_000_000
            );
            eprintln!(
                "{effect},asm,{rep},{:.3},{:.3}",
                asm_wall.last().unwrap() / 1_000_000,
                asm_cpu.last().unwrap() / 1_000_000
            );
        }
        let (rw, rc) = (median(&mut rust_wall), median(&mut rust_cpu));
        let (aw, ac) = (median(&mut asm_wall), median(&mut asm_cpu));
        eprintln!(
            "SUMMARY {effect}: rust median wall {:.3} ms cpu {:.3} ms | asm median wall {:.3} ms cpu {:.3} ms | cpu ratio {:.2}",
            rw / 1_000_000,
            rc / 1_000_000,
            aw / 1_000_000,
            ac / 1_000_000,
            rc as f64 / ac.max(1) as f64
        );
    }
}
