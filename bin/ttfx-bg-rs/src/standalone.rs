//! `omarchy-ttfx-layer`: the GPU layer renderer on one output, without the
//! audio-background controller.
//!
//! It is the `ttfx` source of omarchy-bg-live, which runs one copy per output
//! as a background (bottom layer) or as the screensaver (overlay layer,
//! exclusive keyboard). The engine decides visibility, pausing and dismissal,
//! so this mode never reads the plugin's state.json, never takes its
//! controller lock and never pauses itself. The layer surface lives for the
//! whole run; rotating effects only swaps the producer thread.
//!
//! ```text
//! omarchy-ttfx-layer --output <name> --layer bottom|overlay --keyboard none|exclusive
//!                    [--effects a,b,c] [--rotate <seconds>] [--fps auto|<n>]
//!                    [--text <wordmark>] [--audio] [--namespace <ns>]
//!                    [--width <px> --height <px> --scale <n>] [--resolution <1-8>]
//! ```
//!
//! The output size comes from `--width/--height/--scale` (physical pixels),
//! else from `OMARCHY_BG_LIVE_WIDTH/HEIGHT/SCALE`, else from Hyprland.

use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use anyhow::{anyhow, bail, Context, Result};
use gtk4::pango::{self, prelude::FontMapExt};

use crate::{
    fps_governor, frame_wakeup, is_valid_effect, log_dbg, measured_grid, run_direct_render, wl_layer,
    Config, DirectCell,
};

/// Effects used when `--effects` is not given: the ones measured on old
/// hardware, which also read well as a screensaver.
const DEFAULT_EFFECTS: &[&str] = &["matrix", "decrypt", "rings", "thunderstorm", "fireworks", "rain"];
const DEFAULT_ROTATE_SECS: u64 = 30;

static TERMINATED: AtomicBool = AtomicBool::new(false);

extern "C" fn on_terminate(_signal: libc::c_int) {
    TERMINATED.store(true, Ordering::SeqCst);
}

/// True when the program was started under this name (a symlink or copy).
pub fn invoked_as_standalone(argv0: &str) -> bool {
    std::path::Path::new(argv0)
        .file_name()
        .and_then(|n| n.to_str())
        .is_some_and(|n| n == "omarchy-ttfx-layer")
}

#[derive(Debug, PartialEq)]
pub struct Options {
    pub output: Option<String>,
    pub overlay: bool,
    pub keyboard_exclusive: bool,
    pub effects: Vec<String>,
    pub rotate: Duration,
    pub fps: Option<String>,
    pub text: String,
    pub audio: bool,
    pub namespace: String,
    pub size: Option<(i32, i32, f64)>,
    pub resolution: i64,
}

fn value<'a>(args: &'a [String], key: &str) -> Option<&'a str> {
    args.iter()
        .position(|a| a == key)
        .and_then(|i| args.get(i + 1))
        .map(String::as_str)
}

fn env_or(key: &str, flag: Option<&str>) -> Option<String> {
    flag.map(str::to_string).or_else(|| std::env::var(key).ok().filter(|v| !v.is_empty()))
}

pub fn parse(args: &[String]) -> Result<Options> {
    let layer = env_or("OMARCHY_BG_LIVE_LAYER", value(args, "--layer")).unwrap_or_else(|| "bottom".into());
    let overlay = match layer.as_str() {
        "bottom" => false,
        "overlay" => true,
        other => bail!("--layer must be bottom or overlay, not {other}"),
    };
    let keyboard =
        env_or("OMARCHY_BG_LIVE_KEYBOARD", value(args, "--keyboard")).unwrap_or_else(|| "none".into());
    let keyboard_exclusive = match keyboard.as_str() {
        "none" => false,
        "exclusive" => true,
        other => bail!("--keyboard must be none or exclusive, not {other}"),
    };
    let effects: Vec<String> = match value(args, "--effects") {
        Some(list) => list.split(',').map(str::trim).filter(|e| !e.is_empty()).map(str::to_string).collect(),
        None => DEFAULT_EFFECTS.iter().map(|e| e.to_string()).collect(),
    };
    if let Some(unknown) = effects.iter().find(|e| !is_valid_effect(e)) {
        bail!("unknown effect: {unknown}");
    }
    if effects.is_empty() {
        bail!("--effects is empty");
    }
    let rotate = match value(args, "--rotate") {
        Some(v) => Duration::from_secs(v.parse::<u64>().context("--rotate takes seconds")?.max(5)),
        None => Duration::from_secs(DEFAULT_ROTATE_SECS),
    };
    let width = env_or("OMARCHY_BG_LIVE_WIDTH", value(args, "--width"));
    let height = env_or("OMARCHY_BG_LIVE_HEIGHT", value(args, "--height"));
    let scale = env_or("OMARCHY_BG_LIVE_SCALE", value(args, "--scale"));
    let size = match (width, height) {
        (Some(w), Some(h)) => Some((
            w.parse::<i32>().context("width")?,
            h.parse::<i32>().context("height")?,
            scale.as_deref().unwrap_or("1").parse::<f64>().context("scale")?.max(0.5),
        )),
        _ => None,
    };
    Ok(Options {
        output: env_or("OMARCHY_BG_LIVE_OUTPUT", value(args, "--output")),
        overlay,
        keyboard_exclusive,
        effects,
        rotate,
        fps: value(args, "--fps").map(str::to_string),
        text: value(args, "--text").unwrap_or("OMARCHY").to_string(),
        audio: args.iter().any(|a| a == "--audio"),
        namespace: env_or("OMARCHY_BG_LIVE_NAMESPACE", value(args, "--namespace"))
            .unwrap_or_else(|| "omarchy-ttfx-layer".into()),
        size,
        resolution: value(args, "--resolution").and_then(|v| v.parse().ok()).unwrap_or(2).clamp(1, 8),
    })
}

/// Physical size and scale of an output, from Hyprland.
fn hyprland_output_size(output: Option<&str>) -> Result<(i32, i32, f64)> {
    let out = std::process::Command::new("hyprctl").args(["monitors", "-j"]).output()?;
    let monitors: serde_json::Value = serde_json::from_slice(&out.stdout)?;
    let monitors = monitors.as_array().ok_or_else(|| anyhow!("hyprctl monitors: not a list"))?;
    let monitor = monitors
        .iter()
        .find(|m| output.is_none_or(|o| m["name"] == o))
        .ok_or_else(|| anyhow!("output {} not found", output.unwrap_or("?")))?;
    Ok((
        monitor["width"].as_i64().unwrap_or(1920) as i32,
        monitor["height"].as_i64().unwrap_or(1080) as i32,
        monitor["scale"].as_f64().unwrap_or(1.0),
    ))
}

/// Grid for an output: the same cell size the plugin's GPU path picks.
fn grid_for(width: i32, height: i32, scale: f64, resolution: i64) -> (usize, usize, f64, f32) {
    let logical = ((width as f64 / scale).round() as i32, (height as f64 / scale).round() as i32);
    let integer_scale = scale.ceil().max(1.0);
    let cell_px = (9.0 / integer_scale * resolution as f64).max(3.0);
    let font = pango::FontDescription::from_string(&format!("monospace {cell_px:.0}px"));
    let font_size = font.size() as f64 / pango::SCALE as f64;
    let context = pangocairo::FontMap::new().create_context();
    let metrics = context.metrics(Some(&font), None);
    let char_w = metrics.approximate_char_width() as f64 / pango::SCALE as f64;
    let line_h = (metrics.ascent() + metrics.descent()) as f64 / pango::SCALE as f64;
    let cw = if char_w > 0.0 { char_w } else { font_size * 0.6 };
    let ch = if line_h > 0.0 { line_h } else { font_size * 1.2 };
    let (cols, rows) = measured_grid(logical.0, logical.1, cw, ch);
    (cols, rows, cell_px, (ch / cw) as f32)
}

/// Sleep up to `duration`, waking early when `done` returns true.
fn wait_until(duration: Duration, done: impl Fn() -> bool) {
    let end = Instant::now() + duration;
    while Instant::now() < end && !done() {
        thread::sleep(Duration::from_millis(100));
    }
}

/// Printed by `--probe`, so callers can tell this build from older ones that
/// would start the plugin controller instead.
pub const PROBE: &str = "omarchy-ttfx-layer 1";

pub fn run(args: &[String]) -> Result<()> {
    if args.iter().any(|a| a == "--probe") {
        println!("{PROBE}");
        return Ok(());
    }
    let opts = parse(args)?;
    unsafe {
        libc::signal(libc::SIGTERM, on_terminate as *const () as libc::sighandler_t);
        libc::signal(libc::SIGINT, on_terminate as *const () as libc::sighandler_t);
    }
    let mode = fps_governor::parse_mode(
        opts.fps.as_deref().or(std::env::var("TTFX_BG_MAX_FPS").ok().as_deref()).or(Some("auto")),
    );
    fps_governor::start(mode, |line| log_dbg(&line));

    let (width, height, scale) = match opts.size {
        Some(size) => size,
        None => hyprland_output_size(opts.output.as_deref())?,
    };
    let (cols, rows, cell_px, cell_aspect) = grid_for(width, height, scale, opts.resolution);
    let cfg = Config {
        effects: opts.effects.clone(),
        audio: opts.audio,
        ttfx_text: opts.text.clone(),
        resolution: opts.resolution,
        show_fps: false,
        transparent_background: false,
        boot_between: false,
        ..Config::default()
    };

    let snapshot = Arc::new(Mutex::new(Vec::<DirectCell>::new()));
    let generation = Arc::new(AtomicU64::new(0));
    let layer_stop = Arc::new(AtomicBool::new(false));
    let wakeup = Arc::new(frame_wakeup::FrameWakeup::new(layer_stop.clone())?);
    let spec = wl_layer::LayerSpec {
        output: opts.output.clone(),
        namespace: opts.namespace.clone(),
        overlay: opts.overlay,
        keyboard_exclusive: opts.keyboard_exclusive,
        cols,
        rows,
        cell_px,
        opaque: true,
        show_fps: false,
    };
    let layer = {
        let (snapshot, generation, stop, wakeup) =
            (snapshot.clone(), generation.clone(), layer_stop.clone(), wakeup.clone());
        thread::Builder::new().name("ttfx-wayland".into()).spawn(move || {
            let result = wl_layer::run(spec, snapshot, generation, stop.clone(), wakeup);
            stop.store(true, Ordering::Release);
            result
        })?
    };
    log_dbg(&format!(
        "omarchy-ttfx-layer: {} {}x{} on {} ({} layer), effects {:?} every {}s",
        opts.namespace,
        cols,
        rows,
        opts.output.as_deref().unwrap_or("any output"),
        if opts.overlay { "overlay" } else { "bottom" },
        opts.effects,
        opts.rotate.as_secs()
    ));

    let finished = || TERMINATED.load(Ordering::SeqCst) || layer_stop.load(Ordering::Acquire);
    let mut order = opts.effects.clone();
    let mut next = 0usize;
    while !finished() {
        if next == 0 && order.len() > 2 {
            use rand::seq::SliceRandom;
            let last = order.last().cloned();
            order.shuffle(&mut rand::thread_rng());
            // Never show the same effect twice in a row across a reshuffle.
            if order.first() == last.as_ref() {
                order.swap(0, 1);
            }
        }
        let effect = order[next].clone();
        next = (next + 1) % order.len();
        let producer_stop = Arc::new(AtomicBool::new(false));
        let producer = {
            let (cfg, effect, snapshot, generation, stop, wakeup) =
                (cfg.clone(), effect.clone(), snapshot.clone(), generation.clone(), producer_stop.clone(), wakeup.clone());
            thread::Builder::new().name(format!("ttfx-gpu-{effect}")).spawn(move || {
                frame_wakeup::set_current(Some(wakeup));
                let result = run_direct_render(
                    &cfg, &effect, cols, rows, false, cell_aspect, snapshot, generation, stop,
                );
                frame_wakeup::set_current(None);
                result
            })?
        };
        log_dbg(&format!("omarchy-ttfx-layer: {effect}"));
        wait_until(opts.rotate, || finished() || producer.is_finished());
        producer_stop.store(true, Ordering::Release);
        match producer.join() {
            Ok(Err(error)) => {
                log_dbg(&format!("omarchy-ttfx-layer: {effect} failed: {error:#}"));
                // A broken effect must not spin: give the next one a moment.
                wait_until(Duration::from_secs(1), finished);
            }
            Err(_) => log_dbg(&format!("omarchy-ttfx-layer: {effect} panicked")),
            Ok(Ok(())) => {}
        }
    }

    // SIGTERM or the compositor closed the layer: tear the surface down and exit.
    layer_stop.store(true, Ordering::Release);
    let _ = wakeup.drain();
    match layer.join() {
        Ok(Ok(())) => Ok(()),
        Ok(Err(error)) if TERMINATED.load(Ordering::SeqCst) => {
            log_dbg(&format!("omarchy-ttfx-layer: layer ended with {error:#}"));
            Ok(())
        }
        Ok(Err(error)) => Err(error),
        Err(_) => bail!("wayland layer thread panicked"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(list: &[&str]) -> Vec<String> {
        std::iter::once("omarchy-ttfx-layer").chain(list.iter().copied()).map(String::from).collect()
    }

    #[test]
    fn the_name_selects_standalone_mode() {
        assert!(invoked_as_standalone("/usr/bin/omarchy-ttfx-layer"));
        assert!(invoked_as_standalone("omarchy-ttfx-layer"));
        assert!(!invoked_as_standalone("/x/ttfx-bg-rs-x86_64"));
    }

    #[test]
    fn flags_parse() {
        let o = parse(&args(&[
            "--output", "eDP-1", "--layer", "overlay", "--keyboard", "exclusive", "--effects",
            "matrix, rings", "--rotate", "12", "--width", "2560", "--height", "1600", "--scale", "2",
        ]))
        .unwrap();
        assert_eq!(o.output.as_deref(), Some("eDP-1"));
        assert!(o.overlay && o.keyboard_exclusive);
        assert_eq!(o.effects, vec!["matrix", "rings"]);
        assert_eq!(o.rotate, Duration::from_secs(12));
        assert_eq!(o.size, Some((2560, 1600, 2.0)));
    }

    #[test]
    fn defaults_are_a_background() {
        let o = parse(&args(&["--width", "1920", "--height", "1080"])).unwrap();
        assert!(!o.overlay && !o.keyboard_exclusive);
        assert_eq!(o.rotate, Duration::from_secs(DEFAULT_ROTATE_SECS));
        assert!(o.effects.iter().all(|e| is_valid_effect(e)));
    }

    #[test]
    fn bad_values_are_refused() {
        assert!(parse(&args(&["--layer", "top"])).is_err());
        assert!(parse(&args(&["--keyboard", "on"])).is_err());
        assert!(parse(&args(&["--effects", "nope"])).is_err());
        assert!(parse(&args(&["--rotate", "soon"])).is_err());
    }

    #[test]
    fn rotation_has_a_floor() {
        let o = parse(&args(&["--rotate", "1"])).unwrap();
        assert_eq!(o.rotate, Duration::from_secs(5));
    }
}
