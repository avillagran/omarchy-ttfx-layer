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
//!                    [--text <wordmark> | --text-file <UTF-8 path>] [--audio] [--namespace <ns>]
//!                    [--width <px> --height <px> --scale <n>] [--resolution <1-8|auto>]
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
    fps_governor, frame_wakeup, is_valid_effect, log_dbg, measured_grid, run_direct_render_with_input, wl_layer,
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
    // Literal file contents, read once before starting any renderer threads.
    pub literal_text: Option<String>,
    pub audio: bool,
    pub interactive_matrix: bool,
    pub interactive_eye: bool,
    pub interactive_tunnel: bool,
    pub neon_scene: Option<crate::neon_scenes::Style>,
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

// Explicit --text preserves its legacy canvas behavior and wins regardless of
// argv order. An ignored file is not opened. No implicit plugin/config lookup.
fn text_input(args: &[String]) -> Result<(String, Option<String>)> {
    if let Some(text) = value(args, "--text") {
        return Ok((text.to_string(), None));
    }
    let literal = if let Some(index) = args.iter().position(|arg| arg == "--text-file") {
        let path = args.get(index + 1).filter(|path| !path.is_empty() && !path.starts_with("--"))
            .ok_or_else(|| anyhow!("--text-file requires a UTF-8 file path"))?;
        Some(std::fs::read_to_string(path)
            .with_context(|| format!("--text-file: cannot read UTF-8 file {path:?}"))?)
    } else {
        None
    };
    Ok(("OMARCHY".into(), literal))
}

pub fn parse(args: &[String]) -> Result<Options> {
    let (text, literal_text) = text_input(args)?;
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
    let mut effects: Vec<String> = match value(args, "--effects") {
        Some(list) => list.split(',').map(str::trim).filter(|e| !e.is_empty()).map(str::to_string).collect(),
        None => DEFAULT_EFFECTS.iter().map(|e| e.to_string()).collect(),
    };
    if let Some(unknown) = effects.iter().find(|e| !is_valid_effect(e)) {
        bail!("unknown effect: {unknown}");
    }
    if effects.is_empty() {
        bail!("--effects is empty");
    }
    let interactive_matrix = args.iter().any(|a|a=="--interactive-matrix");
    let interactive_eye = args.iter().any(|a|a=="--interactive-eye");
    let neon_scene=match value(args,"--neon-scene") {Some(name)=>Some(crate::neon_scenes::Style::parse(name).ok_or_else(||anyhow::anyhow!("unknown neon scene: {name}"))?),None=>None};
    if args.iter().any(|a|a=="--neon-scene") && neon_scene.is_none(){bail!("--neon-scene needs a name");}
    if neon_scene.is_some() && args.iter().any(|a|a=="--interactive-tunnel"){bail!("choose tunnel or named scene");}
    let interactive_tunnel=args.iter().any(|a|a=="--interactive-tunnel")||neon_scene.is_some();
    if [interactive_matrix,interactive_eye,interactive_tunnel].into_iter().filter(|v|*v).count()>1 { bail!("choose one interactive background"); }
    if interactive_matrix || interactive_eye || interactive_tunnel {
        if value(args,"--effects").is_some() && effects != ["matrix"] { bail!("interactive Matrix only supports --effects matrix"); }
        effects=vec!["matrix".into()];
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
        text,
        literal_text,
        audio: interactive_matrix || interactive_eye || interactive_tunnel || args.iter().any(|a| a == "--audio"),
        interactive_matrix,
        interactive_eye,
        interactive_tunnel,
        neon_scene,
        namespace: env_or("OMARCHY_BG_LIVE_NAMESPACE", value(args, "--namespace"))
            .unwrap_or_else(|| "omarchy-ttfx-layer".into()),
        size,
        resolution: match value(args, "--resolution") {
            Some("auto") => AUTO_RESOLUTION,
            other => other.and_then(|v| v.parse().ok()).unwrap_or(2).clamp(1, 8),
        },
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
const AUTO_RESOLUTION: i64 = 0;

/// The finest grid a screen deserves. Density follows the display scale, so a
/// Retina panel is not asked for more cells than its GPU and CPU can move, and
/// a very wide output is capped: the renderer's cost follows the grid's size.
fn auto_resolution(width: i32, scale: f64) -> i64 {
    const MAX_COLS: f64 = 360.0;
    const GLYPH_WIDTH: f64 = 0.6;
    let integer_scale = scale.ceil().max(1.0);
    let logical = width as f64 / scale;
    let start = integer_scale as i64;
    (start..=8)
        .find(|&r| logical / (9.0 / integer_scale * r as f64 * GLYPH_WIDTH) <= MAX_COLS)
        .unwrap_or(8)
        .clamp(1, 8)
}

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
pub const CAPABILITIES: &str = "omarchy-ttfx-layer capabilities: text-file-v1 standalone-config-v1";

fn startup_response(args: &[String]) -> Option<&'static str> {
    // Must win over --probe: old binaries safely return PROBE for the combined
    // --standalone --probe --capabilities handshake, without starting a layer.
    if args.iter().any(|arg| arg == "--interactive-eye-probe") {
        Some("omarchy-ttfx-layer interactive-eye 1")
    } else if args.iter().any(|arg| arg == "--interactive-matrix-probe") {
        Some("omarchy-ttfx-layer interactive-matrix 1")
    } else if args.iter().any(|arg| arg == "--capabilities") {
        Some(CAPABILITIES)
    } else if args.iter().any(|arg| arg == "--probe") {
        Some(PROBE)
    } else {
        None
    }
}

pub fn run(args: &[String]) -> Result<()> {
    if let Some(response) = startup_response(args) {
        println!("{response}");
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
    let resolution = if opts.resolution == AUTO_RESOLUTION {
        auto_resolution(width, scale)
    } else {
        opts.resolution
    };
    let (cols, rows, cell_px, cell_aspect) = grid_for(width, height, scale, resolution);
    let cfg = Config {
        effects: opts.effects.clone(),
        audio: opts.audio,
        ttfx_text: opts.text.clone(),
        resolution,
        show_fps: false,
        transparent_background: false,
        boot_between: false,
        ..Config::default()
    };

    let snapshot = Arc::new(Mutex::new(Vec::<DirectCell>::new()));
    let generation = Arc::new(AtomicU64::new(0));
    let layer_stop = Arc::new(AtomicBool::new(false));
    let wakeup = Arc::new(frame_wakeup::FrameWakeup::new(layer_stop.clone())?);
    let pointer = (opts.interactive_matrix || opts.interactive_eye || opts.interactive_tunnel).then(||Arc::new(crate::interactive_matrix::PointerState::default()));
    let pointer_tracker = pointer.as_ref().map(|p|
        crate::interactive_matrix::track_global(p.clone(), layer_stop.clone(), opts.output.as_deref(), opts.interactive_eye)
    ).transpose()?;
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
            let literal_text = opts.literal_text.clone();
            let pointer = pointer.clone();
            let eye = opts.interactive_eye;
            let tunnel=opts.interactive_tunnel;let scene=opts.neon_scene;
            let (cfg, effect, snapshot, generation, stop, wakeup) =
                (cfg.clone(), effect.clone(), snapshot.clone(), generation.clone(), producer_stop.clone(), wakeup.clone());
            thread::Builder::new().name(format!("ttfx-gpu-{effect}")).spawn(move || {
                if let Some(pointer) = pointer {
                    if let Some(scene)=scene {crate::neon_scenes::run(scene,cols,rows,cell_aspect,pointer,snapshot,generation,stop,wakeup)}
                    else if tunnel {crate::interactive_tunnel::run(cols,rows,cell_aspect,pointer,snapshot,generation,stop,wakeup)}
                    else if eye { crate::interactive_eye::run(cols,rows,cell_aspect,pointer,snapshot,generation,stop,wakeup) }
                    else { crate::interactive_matrix::run(cols,rows,cell_aspect,pointer,snapshot,generation,stop,wakeup) }
                } else {
                    run_producer(&cfg, &effect, cols, rows, cell_aspect, snapshot, generation, stop, wakeup, literal_text.as_deref())
                }
            })?
        };
        log_dbg(&format!("omarchy-ttfx-layer: {effect}"));
        while (opts.interactive_matrix || opts.interactive_eye || opts.interactive_tunnel) && !finished() && !producer.is_finished() {
            wait_until(Duration::from_millis(100), || finished() || producer.is_finished());
        }
        if !opts.interactive_matrix && !opts.interactive_eye && !opts.interactive_tunnel { wait_until(opts.rotate, || finished() || producer.is_finished()); }
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
    if let Some(tracker) = pointer_tracker { let _ = tracker.join(); }
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

// Shared by real rotation workers and wiring tests. Policy is installed before
// AudioState, native watchers or ASM callbacks can consult read_config.
fn run_producer(
    cfg: &Config, effect: &str, cols: usize, rows: usize, cell_aspect: f32,
    snapshot: Arc<Mutex<Vec<DirectCell>>>, generation: Arc<AtomicU64>, stop: Arc<AtomicBool>,
    wakeup: Arc<frame_wakeup::FrameWakeup>, literal_input: Option<&str>,
) -> Result<()> {
    let _policy = crate::standalone_config::enter(cfg.clone(), stop.clone());
    frame_wakeup::set_current(Some(wakeup));
    let result = run_direct_render_with_input(cfg, effect, cols, rows, false, cell_aspect,
        snapshot, generation, stop, literal_input);
    frame_wakeup::set_current(None);
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(list: &[&str]) -> Vec<String> {
        std::iter::once("omarchy-ttfx-layer").chain(list.iter().copied()).map(String::from).collect()
    }

    #[test]
    fn auto_resolution_follows_the_scale_and_caps_wide_outputs() {
        assert_eq!(auto_resolution(3456, 2.0), 2);
        assert_eq!(auto_resolution(2560, 2.0), 2);
        assert_eq!(auto_resolution(1920, 1.0), 1);
        assert_eq!(auto_resolution(2560, 1.0), 2);
        assert_eq!(auto_resolution(3840, 1.0), 2);
        assert_eq!(auto_resolution(3840, 2.0), 2);
        assert_eq!(auto_resolution(2880, 1.5), 2);
        assert!(parse(&args(&["--neon-scene", "neon-city", "--resolution", "auto"])).is_ok());
    }
    #[test]
    fn capabilities_precede_probe_and_all_startup_work() {
        assert_eq!(CAPABILITIES, "omarchy-ttfx-layer capabilities: text-file-v1 standalone-config-v1");
        for flags in [vec!["--capabilities"], vec!["--standalone", "--probe", "--capabilities"],
                      vec!["--capabilities", "--probe"], vec!["--probe", "--text-file", "/no/such/input", "--capabilities"]] {
            assert_eq!(startup_response(&args(&flags)), Some(CAPABILITIES));
        }
        assert_eq!(startup_response(&args(&["--probe"])), Some("omarchy-ttfx-layer 1"));
        assert_eq!(startup_response(&args(&["--standalone", "--probe"])), Some(PROBE));
        assert_eq!(startup_response(&args(&[])), None);
    }

    // Run only via the parent test in a child with an isolated HOME. This never
    // reads/writes the user's real plugin state or theme.
    #[test]
    #[ignore = "isolated HOME helper for standalone_config_ignores_plugin_state"]
    fn fixed_config_policy_child() {
        use crate::{read_config, standalone_config, IntensityWatcher, SpeedWatcher, LiveReactivity,
            AsmAudio, AudioState, asm_audio_poll, asm_audio_params};
        let kind = std::env::var("TTFX_CONFIG_FIXTURE").unwrap();
        let home = std::path::PathBuf::from(std::env::var("HOME").unwrap());
        assert_eq!(crate::config_path(), home.join(".local/state/omarchy/audio-background/state.json"));
        assert_eq!(crate::legacy_config_path(), home.join(".config/omarchy/plugins/io.github.avillagran.omarchy-audio-background/state.json"));
        assert!(standalone_config::current().is_none());
        // No policy: controller/default-path behavior stays live and unchanged.
        let controller = read_config();
        if kind == "modern" || kind == "legacy" {
            assert_eq!((controller.running, controller.intensity, controller.speed, controller.reactivity), (false, 0, 100, 5));
        } else {
            assert_eq!((controller.intensity, controller.speed, controller.reactivity), (5, 5, 2));
        }
        let reads = standalone_config::plugin_reads();
        assert!(reads > 0);
        let cfg = Config { audio: false, ..Config::default() };
        let stop = Arc::new(AtomicBool::new(false));
        {
            let _policy = standalone_config::enter(cfg.clone(), stop.clone());
            let due = Instant::now() - Duration::from_secs(1);
            let mut intensity = IntensityWatcher { last_check: due, intensity: -1 };
            let mut speed = SpeedWatcher { last_check: due, speed: -1 };
            assert_eq!(intensity.current(9), 5);
            assert_eq!(speed.current(99), 5);
            let audio = AsmAudio { state: &AudioState::start(false), enabled: false,
                reactivity: LiveReactivity::new(9) };
            audio.reactivity.checked.set(due);
            let ctx = (&audio as *const AsmAudio<'_>).cast_mut().cast();
            assert_eq!(asm_audio_poll(ctx), 0);
            assert_eq!(audio.reactivity.value.get(), 2);
            audio.reactivity.checked.set(due);
            let mut params = [0f32; 16];
            assert_eq!(asm_audio_params(ctx, params.as_mut_ptr()), 0);
            assert_eq!(audio.reactivity.value.get(), 2);
            // Omarchy theme data is intentionally still read from its own paths.
            assert!(crate::read_theme_colors().iter().any(|(name, value)| name == "background" && value == "#112233"));
            stop.store(true, Ordering::Release);
            assert!(!read_config().running);
        }
        assert_eq!(standalone_config::plugin_reads(), reads);
        assert!(standalone_config::current().is_none());
        assert!(ttfx::asm::tier() > 0, "real native/ASM policy gate needs ASM");
        let slot = Arc::new(Mutex::new(vec![]));
        let generation = Arc::new(AtomicU64::new(0));
        // Same actual producer launch used by runtime rotations, same slot;
        // thread policies must neither leak nor disappear in ASM callbacks.
        for effect in ["matrix", "rings", "matrix"] {
            let stop = Arc::new(AtomicBool::new(false));
            let wakeup = Arc::new(frame_wakeup::FrameWakeup::new(stop.clone()).unwrap());
            let (worker_cfg, worker_slot, worker_gen, worker_stop) = (cfg.clone(), slot.clone(), generation.clone(), stop.clone());
            let before = generation.load(Ordering::Acquire);
            let worker = thread::spawn(move || {
                let result = run_producer(&worker_cfg, effect, 80, 24, 2., worker_slot, worker_gen, worker_stop, wakeup, Some("  ASCII\n /__\\  "));
                assert_eq!(standalone_config::plugin_reads(), 0, "{effect} read plugin state");
                assert!(standalone_config::current().is_none(), "policy leaked after producer return");
                result
            });
            let start = Instant::now();
            while start.elapsed() < Duration::from_secs(4) && !worker.is_finished() {
                if generation.load(Ordering::Acquire) >= before + 3 && start.elapsed() >= Duration::from_millis(350) { break; }
                thread::sleep(Duration::from_millis(2));
            }
            let last = { let _guard = slot.lock().unwrap(); stop.store(true, Ordering::Release); generation.load(Ordering::Acquire) };
            worker.join().unwrap().unwrap();
            assert!(last >= before + 3, "no real {effect} publications");
            assert_eq!(generation.load(Ordering::Acquire), last, "publication after stop");
            eprintln!("STANDALONE_FIXED_POLICY effect={effect} fixture={kind} generation={last} plugin_reads=0");
        }
        // The parent/controller thread still follows its original path.
        read_config();
        assert_eq!(standalone_config::plugin_reads(), reads + 1);
    }

    #[test]
    fn standalone_config_ignores_plugin_state() {
        for kind in ["modern", "legacy", "malformed", "missing"] {
            let root = std::env::temp_dir().join(format!("ttfx-config-policy-{}-{kind}-{}", std::process::id(),
                std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos()));
            std::fs::create_dir_all(&root).unwrap();
            let state = match kind {
                "legacy" => root.join(".config/omarchy/plugins/io.github.avillagran.omarchy-audio-background/state.json"),
                _ => root.join(".local/state/omarchy/audio-background/state.json"),
            };
            if kind != "missing" {
                std::fs::create_dir_all(state.parent().unwrap()).unwrap();
                std::fs::write(&state, if kind == "malformed" { "" } else {
                    r#"{"running":false,"intensity":0,"speed":100,"reactivity":5,"use_theme_colors":false}"#
                }).unwrap();
            }
            let theme = root.join(".local/state/omarchy/current/theme.name");
            std::fs::create_dir_all(theme.parent().unwrap()).unwrap();
            std::fs::write(theme, "test-theme\n").unwrap();
            let colors = root.join("omarchy/themes/test-theme/colors.toml");
            std::fs::create_dir_all(colors.parent().unwrap()).unwrap();
            std::fs::write(colors, "background = \"#112233\"\nforeground = \"#aabbcc\"\n").unwrap();
            let status = std::process::Command::new(std::env::current_exe().unwrap())
                .args(["--exact", "standalone::tests::fixed_config_policy_child", "--ignored", "--nocapture"])
                .env("HOME", &root).env("OMARCHY_PATH", root.join("omarchy")).env("TTFX_CONFIG_FIXTURE", kind)
                .env_remove("TTFX_BG_PRODUCER_LOG").env_remove("TTFX_ASM_TIMING")
                .env_remove("TTFX_BG_PRESENTATION_LOG").status().unwrap();
            // Remove only our isolated fixture; the user's actual HOME is untouched.
            std::fs::remove_dir_all(&root).unwrap();
            assert!(status.success(), "policy fixture {kind} failed");
        }
    }

    #[test]
    fn named_neon_scenes_require_valid_names_and_background_audio() {
        for name in ["neon-city","vector-highway","circuit-run","sunset-drive"] {
            let o=parse(&args(&["--neon-scene",name])).unwrap();assert!(o.neon_scene.is_some()&&o.audio&&o.interactive_tunnel);
            for conflict in ["--interactive-eye","--interactive-matrix","--interactive-tunnel"] {assert!(parse(&args(&["--neon-scene",name,conflict])).is_err());}
            assert!(parse(&args(&["--neon-scene",name,"--layer","overlay","--keyboard","exclusive"])).is_ok());
        }
        assert!(parse(&args(&["--neon-scene","unknown"])).is_err());
        assert!(parse(&args(&["--neon-scene"])).is_err());
    }
    #[test]
    fn tunnel_is_audio_reactive_and_exclusive() {
        let o=parse(&args(&["--interactive-tunnel"])).unwrap();assert!(o.interactive_tunnel&&o.audio);
        for conflict in ["--interactive-eye","--interactive-matrix"] {assert!(parse(&args(&["--interactive-tunnel",conflict])).is_err());}
        assert!(parse(&args(&["--interactive-tunnel","--layer","overlay"])).is_ok());
    }
    #[test]
    fn interactive_eye_is_background_only_and_always_sound_reactive() {
        let o=parse(&args(&["--interactive-eye"])).unwrap();assert!(o.interactive_eye && o.audio);
        assert!(parse(&args(&["--interactive-eye","--interactive-matrix"])).is_err());
        assert!(parse(&args(&["--interactive-eye","--layer","overlay"])).is_ok());
        assert_eq!(startup_response(&args(&["--probe","--interactive-eye-probe"])),Some("omarchy-ttfx-layer interactive-eye 1"));
        assert!(parse(&args(&["--interactive-matrix"])).unwrap().audio);
    }
    #[test]
    fn interactive_matrix_is_an_explicit_mode() {
        let opts=parse(&args(&["--interactive-matrix"])).unwrap();
        assert!(opts.interactive_matrix);
        assert_eq!(opts.effects, ["matrix"]);
        assert!(!parse(&args(&[])).unwrap().interactive_matrix);
        assert!(parse(&args(&["--interactive-matrix","--layer","overlay"])).is_ok());
        assert!(parse(&args(&["--interactive-matrix","--keyboard","exclusive"])).is_ok());
        assert!(parse(&args(&["--interactive-matrix","--effects","rings"])).is_err());
        assert_eq!(startup_response(&args(&["--probe","--interactive-matrix-probe"])),Some("omarchy-ttfx-layer interactive-matrix 1"));
        assert_eq!(startup_response(&args(&["--probe","--capabilities"])),Some(CAPABILITIES));
    }

    struct InputFile(std::path::PathBuf);
    impl InputFile {
        fn new(bytes: &[u8]) -> Self {
            static NEXT: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
            let path = std::env::temp_dir().join(format!("ttfx-standalone-text-{}-{}-{}", std::process::id(),
                std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos(),
                NEXT.fetch_add(1, Ordering::Relaxed)));
            std::fs::write(&path, bytes).unwrap();
            Self(path)
        }
        fn path(&self) -> &str { self.0.to_str().unwrap() }
    }
    impl Drop for InputFile {
        fn drop(&mut self) { let _ = std::fs::remove_file(&self.0); }
    }

    #[test]
    fn text_file_preserves_utf8_spaces_lines_and_empty_input() {
        for text in ["  /\\  \n /__\\ \n\n", " \té♥  \r\n\n ", "", "OMARCHY", "  OMARCHY\n"] {
            let file = InputFile::new(text.as_bytes());
            let o = parse(&args(&["--text-file", file.path()])).unwrap();
            assert_eq!(o.literal_text.as_deref(), Some(text));
            let input = crate::wordmark::direct_input(o.literal_text.as_deref(), &o.text, 120, 40, "block", 2.);
            assert_eq!(input.as_bytes(), text.as_bytes());
            assert!(matches!(input, std::borrow::Cow::Borrowed(_)));
        }
    }

    #[test]
    fn explicit_text_wins_in_both_orders_without_reading_ignored_file() {
        for flags in [vec!["--text", "Custom\n  text", "--text-file", "/no/such/ttfx-input"],
                      vec!["--text-file", "/no/such/ttfx-input", "--text", "Custom\n  text"]] {
            let o = parse(&args(&flags)).unwrap();
            assert_eq!(o.text, "Custom\n  text");
            assert_eq!(o.literal_text, None);
            assert_eq!(crate::wordmark::direct_input(None, &o.text, 80, 30, "native", 2.),
                crate::wordmark::canvas_input(&o.text, 80, 30, "native", 2.));
        }
        let o = parse(&args(&["--text", "", "--text-file", "/no/such/ttfx-input"])).unwrap();
        assert_eq!(o.text, "");
        assert!(o.literal_text.is_none());
        let o = parse(&args(&[])).unwrap();
        assert_eq!(o.text, "OMARCHY");
        assert!(o.literal_text.is_none());
        assert_eq!(crate::wordmark::direct_input(None, &o.text, 120, 40, "native", 2.),
            crate::wordmark::canvas_input("OMARCHY", 120, 40, "native", 2.));
    }

    #[test]
    fn text_file_errors_identify_flag_and_path_before_renderer_startup() {
        for flags in [vec!["--text-file"], vec!["--text-file", "--audio"], vec!["--text-file", ""]] {
            assert!(parse(&args(&flags)).unwrap_err().to_string().contains("--text-file requires"));
        }
        let bad_utf8 = InputFile::new(&[0xff, 0xfe]);
        for path in [bad_utf8.path(), "/no/such/ttfx-input"] {
            let error = format!("{:#}", parse(&args(&["--text-file", path])).unwrap_err());
            assert!(error.contains("--text-file") && error.contains(path), "{error}");
        }
        let error = parse(&args(&["--text-file", std::env::temp_dir().to_str().unwrap()])).unwrap_err();
        assert!(error.to_string().contains("cannot read UTF-8 file"));
    }

    #[test]
    #[ignore = "explicit real ASM input gate; no Wayland surface, errors if ASM unavailable"]
    fn text_file_literal_input_reaches_real_asm() {
        use clap::Parser;
        use crate::asm_direct_bridge::emit_frame;
        use ttfx::asm::{self, DirectFrameHook};
        use ttfx::engine::effect::RunOutcome;
        use ttfx::utils::rng::Rng;
        assert!(asm::tier() > 0, "real ASM input gate unavailable");
        let raw = "   @X  \n /_\\   \n\n";
        let file = InputFile::new(raw.as_bytes());
        let opts = parse(&args(&["--text-file", file.path()])).unwrap();
        let input = crate::wordmark::direct_input(opts.literal_text.as_deref(), &opts.text, 80, 24, "native", 2.);
        assert_eq!(input.as_bytes(), raw.as_bytes());
        let effect = ttfx::cli::Cli::try_parse_from(["ttfx", "print"]).unwrap().effect.unwrap();
        let config = crate::direct_catalog_terminal_config(80, 24, 5, true);
        let mut rng = Rng::seeded(12345);
        let slot = Arc::new(Mutex::new(vec![]));
        let generation = Arc::new(AtomicU64::new(0));
        let mut sink = crate::FrameSink::new(slot.clone(), generation.clone(), Arc::new(AtomicBool::new(false)), (80, 24));
        let hook = DirectFrameHook { emit: emit_frame, ctx: (&mut sink as *mut crate::FrameSink).cast() };
        // SAFETY: sink is exclusive and live for this synchronous ASM call;
        // the production callback decodes borrowed grid/pool before returning.
        let result = unsafe { asm::run_direct(asm::Run { effect: &effect, input: &input, config: &config,
            rng: &mut rng, parity_dump: false, virtual_clock: true, max_frames: Some(2000),
            tty_output: false, audio: None }, hook) }.unwrap();
        assert_eq!(result, RunOutcome::Complete);
        assert!(generation.load(Ordering::Acquire) > 0);
        assert!(sink.error.is_none());
        let frame = slot.lock().unwrap();
        let find = |ch| frame.iter().find(|cell| cell.ch == ch).unwrap_or_else(|| panic!("missing {ch:?} in {frame:?}"));
        let (at, x, slash, under, back) = (find('@'), find('X'), find('/'), find('_'), find('\\'));
        assert_eq!((x.x, x.y), (at.x + 1, at.y));
        assert_eq!((under.x, under.y), (slash.x + 1, slash.y));
        assert_eq!((back.x, back.y), (slash.x + 2, slash.y));
        assert_eq!((at.x, slash.y), (slash.x + 2, at.y + 1));
    }

    #[test]
    fn direct_catalog_file_anchors_are_centered_without_changing_legacy_text() {
        use ttfx::engine::canvas::Anchor;
        for file in [false, true] {
            let config = crate::direct_catalog_terminal_config(237, 61, 5, file);
            let anchor = if file { Anchor::C } else { Anchor::Sw };
            assert_eq!(config.anchor_canvas, anchor);
            assert_eq!(config.anchor_text, anchor);
            assert_eq!((config.canvas_width, config.canvas_height, config.frame_rate), (237, 61, 60));
        }
    }

    #[test]
    #[ignore = "real ASM absolute-position gate, no Wayland; requires ASM"]
    fn text_file_center_is_absolute_on_multiple_output_grids() {
        use clap::Parser;
        use ttfx::asm::{self, DirectFrameHook};
        use ttfx::engine::effect::RunOutcome;
        use ttfx::utils::rng::Rng;
        assert!(asm::tier() > 0);
        let raw = "@   X\n /_\\ \nL   R\n";
        let file = InputFile::new(raw.as_bytes());
        let opts = parse(&args(&["--text-file", file.path()])).unwrap();
        for (cols, rows) in [(80, 24), (237, 61), (312, 93), (173, 46)] {
            let input = crate::wordmark::direct_input(opts.literal_text.as_deref(), &opts.text, cols, rows, "native", 2.);
            assert_eq!(input.as_bytes(), raw.as_bytes());
            let config = crate::direct_catalog_terminal_config(cols, rows, 5, true);
            let effect = ttfx::cli::Cli::try_parse_from(["ttfx", "print"]).unwrap().effect.unwrap();
            let mut rng = Rng::seeded(42);
            let slot = Arc::new(Mutex::new(vec![]));
            let generation = Arc::new(AtomicU64::new(0));
            let mut sink = crate::FrameSink::new(slot.clone(), generation, Arc::new(AtomicBool::new(false)), (cols, rows));
            let hook = DirectFrameHook { emit: crate::asm_direct_bridge::emit_frame, ctx: (&mut sink as *mut crate::FrameSink).cast() };
            // SAFETY: sink and borrowed input remain live for this synchronous callback.
            let result = unsafe { asm::run_direct(asm::Run { effect: &effect, input: &input, config: &config,
                rng: &mut rng, parity_dump: false, virtual_clock: true, max_frames: Some(2000),
                tty_output: false, audio: None }, hook) }.unwrap();
            assert_eq!(result, RunOutcome::Complete);
            assert!(sink.error.is_none());
            let cells = slot.lock().unwrap();
            let glyph = |ch| cells.iter().find(|c| c.ch == ch).unwrap();
            let (left, right, top, bottom) = (glyph('L').x, glyph('R').x, glyph('@').y, glyph('L').y);
            assert_eq!(right - left, 4);
            assert_eq!(bottom - top, 2);
            // Match the engine's one-based C-anchor convention exactly. On
            // odd grids it rounds the block center one cell up/right.
            assert_eq!(left as usize, cols.div_ceil(2) - 2);
            assert_eq!(top as usize, rows - rows.div_ceil(2) - 2);
            assert!(((left + right) as i64 - (cols as i64 - 1)).abs() <= 2, "{cols}x{rows}: x={left}..{right}");
            assert!(((top + bottom) as i64 - (rows as i64 - 1)).abs() <= 2, "{cols}x{rows}: y={top}..{bottom}");
            eprintln!("CENTERED_FILE_GRID {cols}x{rows}: ({left},{top})..({right},{bottom})");
        }
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
