#![cfg(any(target_arch = "x86_64", target_arch = "aarch64"))]

use clap::Parser;
use std::{ffi::c_void, path::PathBuf};
use ttfx::asm::{self, AudioHook, DirectFrameHook};
use ttfx::cli::Cli;
use ttfx::engine::{effect::RunOutcome, terminal::TerminalConfig};
use ttfx::utils::rng::Rng;

const CATALOG: [&str; 35] = [
    "beams",
    "binarypath",
    "blackhole",
    "bouncyballs",
    "bubbles",
    "burn",
    "colorshift",
    "crumble",
    "decrypt",
    "errorcorrect",
    "expand",
    "fireworks",
    "highlight",
    "laseretch",
    "middleout",
    "orbittingvolley",
    "overflow",
    "pour",
    "print",
    "randomsequence",
    "rings",
    "scattered",
    "slice",
    "slide",
    "smoke",
    "spotlights",
    "spray",
    "swarm",
    "sweep",
    "synthgrid",
    "thunderstorm",
    "unstable",
    "vhstape",
    "waves",
    "wipe",
];
const INPUT: &str = "ABCDEFGHIJKLMNOPQRSTUVWXYZ\n0123456789 ASM AUDIO\nabcdefghijklmnopqrstuvwxyz";

#[derive(Clone, Debug, PartialEq, Eq)]
struct Cell {
    symbol: u64,
    fg: u64,
    bg: u64,
    attrs: u64,
}
#[derive(Clone, Copy)]
enum Signal {
    Silence,
    Treble,
    Bass,
    BeatPulse,
    BeatHeld,
    StaleTempo,
    Maximum,
}
struct Recorder {
    frames: Vec<Vec<Cell>>,
    signal: Signal,
    color: bool,
}
extern "C" fn poll(ctx: *mut c_void) -> u64 {
    let r = unsafe { &*(ctx as *const Recorder) };
    let tick = r.frames.len();
    match r.signal {
        Signal::Silence => 0,
        Signal::Treble => 255 << 24,
        Signal::Bass => 255 << 8,
        Signal::BeatPulse | Signal::BeatHeld => {
            // Identical rising edges, but one vs eight frames of held beat.
            let width = if matches!(r.signal, Signal::BeatHeld) {
                8
            } else {
                1
            };
            (64 << 8) | (u64::from(tick % 24 < width) << 16) | (120 << 32)
        }
        Signal::StaleTempo => 180 << 32,
        Signal::Maximum => {
            255 | (255 << 8) | (255 << 24) | (240 << 32) | (u64::from(tick % 24 < 8) << 16)
        }
    }
}
fn strength(frame: usize) -> Option<f32> {
    [Some(0.5), Some(1.5), None, Some(1.0)][frame % 4]
}
extern "C" fn params(ctx: *mut c_void, out: *mut f32) -> u64 {
    let r = unsafe { &*(ctx as *const Recorder) };
    let Some(brightness) = r.color.then(|| strength(r.frames.len())).flatten() else {
        return 0;
    };
    let values = [brightness, 1., 0., 0., 0., 1., 0., 0., 0., 1.];
    unsafe { std::ptr::copy_nonoverlapping(values.as_ptr(), out, values.len()) };
    1
}
unsafe extern "C" fn emit(
    ctx: *mut c_void,
    cols: u64,
    rows: u64,
    grid: *const u32,
    pool: *const u8,
) -> u64 {
    let r = unsafe { &mut *(ctx as *mut Recorder) };
    let handles = unsafe { std::slice::from_raw_parts(grid, (cols * rows) as usize) };
    let frame = handles
        .iter()
        .map(|&h| {
            let offset = (h & 0xffffff) as usize;
            assert!((32..0xffff00).contains(&offset));
            assert!((1..=128).contains(&((h >> 24) as usize)));
            let p = unsafe { pool.add(offset - 32) };
            let word = |o| unsafe { (p.add(o) as *const u64).read_unaligned() };
            Cell {
                symbol: word(0),
                fg: word(8),
                bg: word(16),
                attrs: word(24),
            }
        })
        .collect();
    r.frames.push(frame);
    0
}
fn run(effect: &str, args: &[&str], signal: Signal, color: bool, frames: u64) -> Recorder {
    assert!(
        asm::tier() > 0,
        "requires actual ASM, never a Rust fallback"
    );
    let mut argv = vec!["ttfx", effect];
    argv.extend_from_slice(args);
    let e = Cli::try_parse_from(argv).unwrap().effect.unwrap();
    let cfg = TerminalConfig {
        canvas_width: 40,
        canvas_height: 16,
        ignore_terminal_dimensions: true,
        frame_rate: 60,
        ..TerminalConfig::default()
    };
    let mut rng = Rng::seeded(20261004);
    let mut r = Recorder {
        frames: vec![],
        signal,
        color,
    };
    let ctx = (&mut r as *mut Recorder).cast();
    let result = unsafe {
        asm::run_direct(
            asm::Run {
                effect: &e,
                input: INPUT,
                config: &cfg,
                rng: &mut rng,
                parity_dump: false,
                virtual_clock: true,
                max_frames: Some(frames),
                tty_output: false,
                audio: Some(AudioHook { poll, params, ctx }),
            },
            DirectFrameHook { emit, ctx },
        )
    };
    assert_eq!(result.unwrap(), RunOutcome::Complete, "{effect}");
    assert!(!r.frames.is_empty(), "{effect} must emit frames");
    r
}
fn bytes(r: &Recorder) -> Vec<u8> {
    let mut out = Vec::new();
    out.extend_from_slice(&(r.frames.len() as u64).to_le_bytes());
    for f in &r.frames {
        out.extend_from_slice(&(f.len() as u64).to_le_bytes());
        for c in f {
            for word in [c.symbol, c.fg, c.bg, c.attrs] {
                out.extend_from_slice(&word.to_le_bytes());
            }
        }
    }
    out
}
#[test]
#[ignore = "compares retained, pre-change live-version reference data"]
fn silent_catalog_matches_live_version_reference_bytes() {
    let Some(dir) = std::env::var_os("TTFX_REACTIVITY_REFERENCE_DIR") else {
        panic!("set TTFX_REACTIVITY_REFERENCE_DIR to the retained live-version goldens");
    };
    let dir = PathBuf::from(dir);
    for effect in CATALOG {
        // Extended duration reaches Rings' spinning paths and storm phase too.
        let frames = if ["thunderstorm", "rings", "decrypt"].contains(&effect) {
            384
        } else {
            64
        };
        let data = bytes(&run(effect, &[], Signal::Silence, false, frames));
        let path = dir.join(format!("{effect}.bin"));
        if std::env::var_os("TTFX_REACTIVITY_CAPTURE_REFERENCE").is_some() {
            assert!(
                !path.exists(),
                "reference must be captured once before changes"
            );
            std::fs::write(path, &data).unwrap();
        } else {
            assert!(
                data == std::fs::read(path).unwrap(),
                "silence bytes {effect}"
            );
        }
        eprintln!("SILENCE {effect}: {} bytes", data.len());
    }
}
fn tint(word: u64, b: f32) -> u64 {
    [16, 8, 0].into_iter().fold(0, |rgb, s| {
        rgb | (((((word >> s) & 255) as f32 * b).clamp(0., 255.) as u64) << s)
    })
}
#[test]
fn every_catalog_effect_uses_current_frame_color_and_restores_silence() {
    for effect in CATALOG {
        let plain = run(effect, &[], Signal::Silence, false, 64);
        let audio = run(effect, &[], Signal::Silence, true, 64);
        assert_eq!(plain.frames.len(), audio.frames.len(), "{effect}");
        let mut changed = 0;
        for (n, (p, a)) in plain.frames.iter().zip(&audio.frames).enumerate() {
            for (p, a) in p.iter().zip(a) {
                assert_eq!(
                    (p.symbol, p.attrs),
                    (a.symbol, a.attrs),
                    "style/geometry {effect}"
                );
                let blank =
                    p.symbol == (1 << 32) | u64::from(b' ') && p.fg == u64::MAX && p.bg == u64::MAX;
                let (fg, bg) = if let Some(b) = strength(n).filter(|_| !blank) {
                    (
                        tint(if p.fg == u64::MAX { 0xcccccc } else { p.fg }, b),
                        if p.bg == u64::MAX {
                            p.bg
                        } else {
                            tint(p.bg, b)
                        },
                    )
                } else {
                    (p.fg, p.bg)
                };
                assert_eq!((a.fg, a.bg), (fg, bg), "frame color {effect} frame{n}");
                changed += usize::from((p.fg, p.bg) != (a.fg, a.bg));
            }
        }
        assert!(changed > 0, "must exercise real colored output: {effect}");
        eprintln!(
            "COLOR {effect}: {} frames, {changed} changed cells",
            audio.frames.len()
        );
    }
}
#[test]
fn decrypt_treble_changes_typing_without_color_transform() {
    let quiet = run(
        "decrypt",
        &["--typing-speed", "1"],
        Signal::Silence,
        false,
        96,
    );
    let treble = run(
        "decrypt",
        &["--typing-speed", "1"],
        Signal::Treble,
        false,
        96,
    );
    let changed = quiet
        .frames
        .iter()
        .zip(&treble.frames)
        .filter(|(a, b)| a != b)
        .count();
    assert!(changed > 0, "ASM typing must respond to treble");
    eprintln!(
        "MOTION decrypt treble: {changed}/{} differing frames",
        quiet.frames.len()
    );
}
#[test]
fn rings_bass_changes_rotation_without_color_transform() {
    let args = [
        "--disperse-duration",
        "1",
        "--spin-duration",
        "500",
        "--spin-disperse-cycles",
        "1",
    ];
    let quiet = run("rings", &args, Signal::Silence, false, 512);
    let bass = run("rings", &args, Signal::Bass, false, 512);
    let changed = quiet
        .frames
        .iter()
        .zip(&bass.frames)
        .filter(|(a, b)| a != b)
        .count();
    assert!(changed > 0, "ASM rotation must respond to bass");
    eprintln!(
        "MOTION rings bass: {changed}/{} differing frames",
        quiet.frames.len()
    );
    assert_eq!(
        &quiet.frames[..100],
        &bass.frames[..100],
        "initial home movement is preserved"
    );
}
#[test]
fn thunderstorm_held_beat_does_not_repeat_beat_bonus() {
    let pulse = run("thunderstorm", &[], Signal::BeatPulse, false, 384);
    let held = run("thunderstorm", &[], Signal::BeatHeld, false, 384);
    assert!(
        pulse.frames == held.frames,
        "same beat edges must yield the same geometry"
    );
    let quiet = run("thunderstorm", &[], Signal::Silence, false, 384);
    let changed = pulse
        .frames
        .iter()
        .zip(&quiet.frames)
        .filter(|(a, b)| a != b)
        .count();
    assert!(changed > 0, "beat input must actually affect the storm");
    eprintln!(
        "MOTION thunderstorm beat: {changed}/{} differing frames; held=pulse",
        quiet.frames.len()
    );
}
#[test]
fn tempo_without_energy_is_silence() {
    for effect in ["thunderstorm", "rings", "decrypt"] {
        assert!(
            run(effect, &[], Signal::Silence, false, 384).frames
                == run(effect, &[], Signal::StaleTempo, false, 384).frames,
            "{effect}"
        );
    }
}

#[test]
fn decrypt_held_beat_rewards_only_the_edge() {
    let pulse = run(
        "decrypt",
        &["--typing-speed", "1"],
        Signal::BeatPulse,
        false,
        96,
    );
    let held = run(
        "decrypt",
        &["--typing-speed", "1"],
        Signal::BeatHeld,
        false,
        96,
    );
    assert!(
        pulse.frames == held.frames,
        "held beat must not become repeated typing bonuses"
    );
    let quiet = run(
        "decrypt",
        &["--typing-speed", "1"],
        Signal::Silence,
        false,
        96,
    );
    assert!(
        pulse.frames != quiet.frames,
        "new beat must actually change typing"
    );
}

#[test]
fn maximum_channels_keep_all_three_effects_bounded_and_progressing() {
    for effect in ["thunderstorm", "decrypt", "rings"] {
        // Not a CPU benchmark: fixed small canvas, virtual time, bounded frames.
        // emit checks every borrowed handle and header on the maximum inputs.
        let active = run(effect, &[], Signal::Maximum, false, 768);
        assert!(active.frames.len() >= 96, "{effect} must keep progressing");
        assert!(active.frames.iter().all(|f| f.len() == 40 * 16));
        eprintln!(
            "BOUNDS {effect}: {} frames, maximum channels",
            active.frames.len()
        );
    }
}
