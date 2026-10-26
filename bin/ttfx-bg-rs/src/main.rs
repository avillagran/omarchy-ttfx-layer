// ttfx-bg-rs: 100% Rust audio-reactive desktop background.
//
// Omarchy paints its desktop background as a gtk4-layer-shell bottom layer.
// We do the same, ONE layer per connected monitor. Inside each layer we embed
// a Vte terminal and draw the effect directly. The renderer is a child process
// (`ttfx-bg-rs --render <effect> <cols> <rows> ...`) per monitor.
//
// Config lives in the plugin's state.json (written by the panel / bar widget):
//   running, effect, effects[], intensity, audio, byline, restart
// The parent polls it every 700ms and rebuilds layers on change. A `restart`
// counter bump forces a rebuild (replays the intro). When `effects` has more
// than one entry, the parent rotates the active effect every 20s.
//
// Roadmap:
//   - Step 1: vendor ttfx's effect engine as a lib and call it here instead.
//   - Step 2: (done here) audio level from parec modulates flow speed/density.

mod asm_direct_bridge;
mod audio_spectrum;
mod bars_layout;
mod direct_glyph_atlas;
mod frame_cadence;
mod frame_handoff;
mod fps_governor;
mod standalone;
mod frame_wakeup;
mod gpu_grid;
mod occlusion;
mod wl_layer;
mod presentation_probe;
mod producer_probe;
mod native_publication;
mod pixel_paintable;
use ttfx_bg_rs::opaque_bgra;
mod wordmark;
use asm_direct_bridge::{DirectCell, FrameSink};
use direct_glyph_atlas::{DirectGlyphAtlas, RetainedPixels};

use anyhow::Result;
use gtk4::gdk;
use gtk4::prelude::*;
use gtk4_layer_shell::{Edge, Layer, LayerShell};
use std::cell::RefCell;
use std::io::{Read, Write};
use std::path::PathBuf;
use std::rc::Rc;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::Arc;
use std::thread;
use std::time::Duration;
#[cfg(feature = "vte")]
use vte4::{TerminalExt, TerminalExtManual};

// Thread-local palette shared between the watcher (run_render) and the effects
// (fx_matrix, etc.). The watcher writes the current palette here whenever the
// theme changes; effects read it every frame so color changes apply live
// without restarting the plugin.
thread_local! {
    static THEME_PALETTE: RefCell<Vec<String>> = RefCell::new(Vec::new());
}

fn set_theme_palette(palette: Vec<String>) {
    THEME_PALETTE.with(|p| *p.borrow_mut() = palette);
}

// Throttled theme-color watcher: reads the theme files at most every `interval`
// and reports only actual changes. Without this, the frame loop did 2 file reads
// + TOML parse + string building at 60fps (~120 file opens/sec) — visible stutter
// under disk load. Theme changes are user-initiated and rare; 500ms detection
// latency is invisible.
struct ThemeWatcher {
    last_check: std::time::Instant,
    interval: Duration,
    cached: Vec<(String, String)>,
    key: String,
}

impl ThemeWatcher {
    fn new() -> Self {
        Self {
            last_check: std::time::Instant::now() - Duration::from_secs(60), // force first read
            interval: Duration::from_millis(500),
            cached: Vec::new(),
            key: String::new(),
        }
    }
    /// Returns the theme colors only when they changed since the last call
    /// (and at most once per `interval`). Cheap no-op otherwise.
    fn changed(&mut self) -> Option<&[(String, String)]> {
        if self.last_check.elapsed() < self.interval {
            return None;
        }
        self.last_check = std::time::Instant::now();
        let new = read_theme_colors();
        let new_key: String = new
            .iter()
            .map(|(k, v)| format!("{}:{}", k, v))
            .collect::<Vec<_>>()
            .join("|");
        if new_key != self.key {
            self.key = new_key;
            self.cached = new;
            Some(&self.cached)
        } else {
            None
        }
    }
}

// Slider updates are sampled at a bounded rate by the renderer. This avoids
// killing the PTY and replaying an effect whenever the user drags intensity.
struct IntensityWatcher {
    last_check: std::time::Instant,
    intensity: i64,
}

impl IntensityWatcher {
    fn current(&mut self, fallback: i64) -> i64 {
        if self.last_check.elapsed() >= Duration::from_millis(250) {
            self.last_check = std::time::Instant::now();
            self.intensity = read_config().intensity.clamp(0, 10);
        }
        if self.intensity == -1 {
            fallback.clamp(0, 10)
        } else {
            self.intensity
        }
    }
}

thread_local! {
    static INTENSITY_WATCHER: RefCell<IntensityWatcher> = RefCell::new(IntensityWatcher {
        last_check: std::time::Instant::now() - Duration::from_secs(60),
        intensity: -1,
    });
}

fn live_intensity(fallback: i64) -> i64 {
    INTENSITY_WATCHER.with(|watcher| watcher.borrow_mut().current(fallback))
}

// Speed is a separate live setting: visual intensity and audio reactivity must
// not be the only ways to change animation pace.
struct SpeedWatcher {
    last_check: std::time::Instant,
    speed: i64,
}

impl SpeedWatcher {
    fn current(&mut self, fallback: i64) -> i64 {
        if self.last_check.elapsed() >= Duration::from_millis(250) {
            self.last_check = std::time::Instant::now();
            self.speed = read_config().speed.clamp(1, 100);
        }
        if self.speed == -1 {
            fallback.clamp(1, 100)
        } else {
            self.speed
        }
    }
}

thread_local! {
    static SPEED_WATCHER: RefCell<SpeedWatcher> = RefCell::new(SpeedWatcher {
        last_check: std::time::Instant::now() - Duration::from_secs(60),
        speed: -1,
    });
}

fn live_speed(fallback: i64) -> i64 {
    SPEED_WATCHER.with(|watcher| watcher.borrow_mut().current(fallback))
}

fn measured_grid(width: i32, height: i32, cell_width: f64, cell_height: f64) -> (usize, usize) {
    (
        (width as f64 / cell_width).floor().max(80.0) as usize,
        (height as f64 / cell_height).floor().max(24.0) as usize,
    )
}

// Stored in fifths of normal speed: existing default 5 = 1x, 100 = 20x.
// Both backends divide their ordinary (intensity/audio-adjusted) delay by this
// multiplier. Apply after native minimum-delay clamping so it cannot cap SPEED.
fn speed_multiplier(speed: i64) -> f64 {
    speed.clamp(1, 100) as f64 / 5.0
}

fn speed_delay(baseline: Duration, speed: i64) -> Duration {
    baseline.div_f64(speed_multiplier(speed))
}

// Render pacing ceiling. Uncapped pacing (high speed slider + audio boost)
// reaches ~75-125fps, and every frame is a full-screen ANSI repaint that the
// controller's Vte widgets must parse per monitor — that flood cost ~45% CPU.
// 60fps keeps backgrounds fluid while limiting worst-case output overhead.
// Applied at the render loops (not in speed_delay) so the speed multiplier
// contract stays unbounded for effect-internal timing.
const MIN_RENDER_FRAME: Duration = Duration::from_micros(16_667);

fn render_frame_interval(intended: Duration, direct: bool) -> Duration {
    fps_governor::note_tick();
    let interval = if direct {
        intended
    } else {
        intended.max(MIN_RENDER_FRAME)
    };
    match max_fps() {
        Some(fps) => interval.max(Duration::from_secs_f64(1.0 / fps)),
        None => interval,
    }
}

// Optional user ceiling (TTFX_BG_MAX_FPS): a number, or `auto` to let the
// governor pick one from the measured CPU cost per frame. The effect speed
// setting still controls animation timing below the ceiling.
fn max_fps() -> Option<f64> {
    fps_governor::cap()
}

// --- Auto-degrade: detect sustained FPS drops and coarsen the grid -------------
const AUTO_DEGRADE_WINDOW_SECS: f64 = 15.0;
const AUTO_DEGRADE_MIN_FPS: f64 = 10.0;

fn should_auto_degrade(frames: u32, elapsed_secs: f64) -> bool {
    elapsed_secs >= AUTO_DEGRADE_WINDOW_SECS
        && frames as f64 / elapsed_secs.max(0.001) < AUTO_DEGRADE_MIN_FPS
}

fn auto_degrade_window_active(enabled: bool) -> bool {
    enabled
}

thread_local! {
    static AUTO_DEGRADE: RefCell<AutoDegrade> = RefCell::new(AutoDegrade::new());
    static AUTO_DEGRADE_ENABLED: std::cell::Cell<bool> = std::cell::Cell::new(false);
}

fn set_auto_degrade_enabled(on: bool) {
    AUTO_DEGRADE_ENABLED.with(|e| e.set(on));
    if on {
        // The intro is intentionally unmeasured. Start a fresh 15-second window
        // when real background rendering begins, otherwise intro time makes the
        // first sample look artificially slow.
        AUTO_DEGRADE.with(|a| *a.borrow_mut() = AutoDegrade::new());
    }
}

struct AutoDegrade {
    last: Option<std::time::Instant>,
    win_frames: u32,
    win_since: std::time::Instant,
    escalated: bool,
}

impl AutoDegrade {
    fn new() -> Self {
        Self {
            last: None,
            win_frames: 0,
            win_since: std::time::Instant::now(),
            escalated: false,
        }
    }

    /// Call once per rendered frame. Degrade only after a full, deliberately
    /// slow 15-second observation window averaging below 10 real FPS.
    fn tick(&mut self, _intended_ms: f64) {
        let enabled = AUTO_DEGRADE_ENABLED.with(|e| e.get());
        if !auto_degrade_window_active(enabled) {
            return;
        }
        let now = std::time::Instant::now();
        if self.last.is_some() {
            self.win_frames += 1;
        }
        self.last = Some(now);
        let win_elapsed = self.win_since.elapsed().as_secs_f64();
        if win_elapsed >= AUTO_DEGRADE_WINDOW_SECS {
            let fps = self.win_frames as f64 / win_elapsed.max(0.001);
            log_dbg(&format!(
                "auto-degrade: {:.1}s window, {} frames, average={:.1} FPS",
                win_elapsed, self.win_frames, fps
            ));
            if should_auto_degrade(self.win_frames, win_elapsed) && !self.escalated {
                self.escalated = true;
                self.win_frames = 0;
                self.win_since = std::time::Instant::now();
                escalate_resolution();
                return;
            }
            self.win_frames = 0;
            self.win_since = std::time::Instant::now();
        }
    }
}

fn claim_auto_degrade_step() -> bool {
    use std::io::{Read, Seek, Write};
    use std::os::fd::AsRawFd;
    let base = std::env::var("XDG_RUNTIME_DIR").unwrap_or_else(|_| "/tmp".into());
    let path = format!(
        "{base}/omarchy-audio-background-autodegrade-{}.lock",
        unsafe { libc::getuid() }
    );
    let Ok(mut file) = std::fs::OpenOptions::new()
        .create(true)
        .read(true)
        .write(true)
        .open(path)
    else {
        return false;
    };
    if unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_EX) } != 0 {
        return false;
    }
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    let mut previous = String::new();
    let _ = file.read_to_string(&mut previous);
    if now.saturating_sub(previous.trim().parse::<u64>().unwrap_or(0))
        < AUTO_DEGRADE_WINDOW_SECS as u64
    {
        log_dbg("auto-degrade: another monitor already escalated this 15s window");
        return false;
    }
    let _ = file.set_len(0);
    let _ = file.seek(std::io::SeekFrom::Start(0));
    write!(file, "{now}").is_ok()
}

/// One-step escalation: resolution+1 (cap 8) and intro_size -> 1 (smaller boot).
/// Patches state.json IN-PROCESS (no child process, no PATH lookup, no script
/// execution) with an atomic tmp+rename; the controller's 700ms poll sees the
/// change and rebuilds. Once per process: the rebuild respawns us.
fn escalate_resolution() {
    let cfg = read_config();
    if cfg.resolution >= 8 {
        log_dbg("auto-degrade: resolution already at max (8), not escalating");
        return;
    }
    if !claim_auto_degrade_step() {
        return;
    }
    let new_res = (cfg.resolution + 1).min(8);
    let new_intro = if cfg.intro_size > 1 {
        1
    } else {
        cfg.intro_size
    };
    log_dbg(&format!("auto-degrade: FPS drop detected -> resolution {res} -> {new_res}, intro_size {intro} -> {new_intro}",
        res = cfg.resolution, intro = cfg.intro_size));
    patch_state_nums(&[("resolution", new_res), ("intro_size", new_intro)]);
}

/// Replace `"key":<int>` in the state file, preserving every other byte
/// (including fields this binary doesn't know about, like panel_opacity).
/// Writes tmp+rename so a concurrent reader never sees a torn file.
fn patch_state_nums(patches: &[(&str, i64)]) {
    use std::io::Write;
    let path = config_path();
    let text =
        std::fs::read_to_string(&path).or_else(|_| std::fs::read_to_string(legacy_config_path()));
    let Ok(text) = text else {
        log_dbg("auto-degrade: state file unreadable, skipping patch");
        return;
    };
    let mut out = text;
    for (key, value) in patches {
        let needle = format!("\"{key}\":");
        if let Some(pos) = out.find(&needle) {
            let num_start = pos + needle.len();
            let rest = &out[num_start..];
            let num_len = rest
                .find(|c: char| !(c.is_ascii_digit() || c == '-'))
                .unwrap_or(rest.len());
            out = format!("{}{}{}", &out[..num_start], value, &rest[num_len..]);
        }
    }
    let tmp = path.with_extension("json.tmp");
    if let Ok(mut f) = std::fs::File::create(&tmp) {
        if f.write_all(out.as_bytes()).is_ok() {
            let _ = std::fs::rename(&tmp, &path);
        } else {
            let _ = std::fs::remove_file(&tmp);
        }
    }
}

// Convert a tint index (0=default, 1..=palette.len()) to an ANSI color string.
fn tint_to_color(tint: u8, palette: &[String]) -> String {
    if tint == 0 || palette.is_empty() {
        String::new()
    } else {
        let idx = ((tint - 1) as usize).min(palette.len() - 1);
        palette[idx].clone()
    }
}
const DEFAULT_EFFECTS: [&str; 8] = [
    "matrix",
    "rain",
    "wave",
    "bars",
    "donut",
    "fire",
    "starfield",
    "life",
];
const DEFAULT_BYLINE: &str = "By x.com/avillagran";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum RenderBackend {
    Vte,
    Direct,
    Gpu,
}

fn render_backend_from_value(value: Option<&str>) -> RenderBackend {
    match value.map(str::trim).map(str::to_ascii_lowercase).as_deref() {
        Some("direct") => RenderBackend::Direct,
        Some("gpu") => RenderBackend::Gpu,
        _ => RenderBackend::Vte,
    }
}

fn render_backend() -> RenderBackend {
    render_backend_from_value(std::env::var("TTFX_BG_RENDERER").ok().as_deref())
}

#[derive(Clone, PartialEq)]
struct Config {
    running: bool,
    effect: String,
    effects: Vec<String>,
    intensity: i64,
    // Overall animation pace, separate from visual intensity and audio reactivity.
    speed: i64,
    audio: bool,
    byline: String,
    restart: i64,
    intro_size: i64,
    show_fps: bool,
    // Show the boot splash when ROTATING between backgrounds (manual restart always
    // shows it). Time each background stays on screen before rotating.
    boot_between: bool,
    rotate_secs: i64,
    // Canvas text for ttfx effects (they animate text), rendered large as ASCII art. And a
    // resolution scale: bigger cells => fewer cols/rows => less CPU on old machines.
    ttfx_text: String,
    resolution: i64,
    // How strongly ttfx effects react to audio: slider 0..5 (0 = off, 2 = normal, 5 =
    // strong). Scales the frame-pacing speed boost driven by the live volume/beat.
    reactivity: i64,
    // When true, the boot intro types one letter per audio beat (with a timeout
    // fallback if no music is playing), so the letters appear synced to the rhythm.
    intro_beat_sync: bool,
    // When true, use the active Omarchy theme colors for the effect palettes instead
    // of the built-in hardcoded colors. Requires omarchy-theme-current to be installed.
    use_theme_colors: bool,
    // When true, the background is transparent so the user's wallpaper shows through.
    // Off by default (opaque black background).
    transparent_background: bool,
    // Glyph treatment for the input text animated by ttfx; effect symbols stay native.
    char_style: String,
    // Glyph treatment for the boot intro's ASCII-art letters. It is deliberately
    // independent from the TTFX input text and hand-rolled effects.
    boot_char_style: String,
}

impl Default for Config {
    fn default() -> Self {
        Config {
            running: true,
            effect: "matrix".into(),
            effects: DEFAULT_EFFECTS.iter().map(|s| s.to_string()).collect(),
            intensity: 5,
            speed: 5,
            audio: true,
            byline: String::new(),
            restart: 0,
            intro_size: 2,
            show_fps: false,
            boot_between: true,
            rotate_secs: 20,
            ttfx_text: "OMARCHY".into(),
            resolution: 1,
            reactivity: 2,
            intro_beat_sync: true,
            use_theme_colors: true,
            transparent_background: false,
            char_style: "native".into(),
            boot_char_style: "native".into(),
        }
    }
}

// state.json lives in ~/.local/state/omarchy/... — NOT in the plugin source dir.
// The shell runs `inotifywait -r` on ~/.config/omarchy/plugins and RELOADS the
// plugin on any file change there, which closed the open panel on every write
// (and respawned everything). Runtime state belongs in the XDG state dir, like
// the other plugins (control-panel-prefs.json etc.).
fn config_path() -> PathBuf {
    let home = std::env::var("HOME").unwrap_or_else(|_| ".".into());
    PathBuf::from(home).join(".local/state/omarchy/audio-background/state.json")
}

// Legacy location (inside the plugin dir) — read once for migration if the new
// path doesn't exist yet.
fn legacy_config_path() -> PathBuf {
    let home = std::env::var("HOME").unwrap_or_else(|_| ".".into());
    PathBuf::from(home)
        .join(".config/omarchy/plugins/io.github.avillagran.omarchy-audio-background/state.json")
}

// Best-effort read of the panel's state.json. Missing/invalid => defaults.
// Tolerant of compact or pretty JSON. Independent `if`s on purpose: a compact
// line carries every key at once.
// Last config that parsed successfully. read_config() returns this instead of
// defaults when the state file is momentarily unreadable/empty (a non-atomic
// writer truncates-then-writes; even with an atomic mv, reading mid-flight used
// to return Config::default() = "matrix", and the controller interpreted that as
// the user switching effects -> visible rebuild/restart every write).
thread_local! {
    static LAST_GOOD_CONFIG: RefCell<Option<Config>> = RefCell::new(None);
}

fn read_config() -> Config {
    let text = std::fs::read_to_string(config_path())
        .or_else(|_| std::fs::read_to_string(legacy_config_path()));
    // Empty or brace-less content = torn write window (or a corrupt file); never
    // treat it as "user wants defaults".
    let text = match text {
        Ok(t) if t.contains('{') => t,
        Ok(t) => {
            log_dbg(&format!(
                "read_config: state file empty/partial ({} bytes) — keeping last good",
                t.len()
            ));
            return LAST_GOOD_CONFIG.with(|c| c.borrow().clone().unwrap_or_default());
        }
        Err(e) => {
            log_dbg(&format!(
                "read_config: no state file ({e}) — keeping last good"
            ));
            return LAST_GOOD_CONFIG.with(|c| c.borrow().clone().unwrap_or_default());
        }
    };
    let mut cfg = Config::default();
    if let Some(v) = json_bool(&text, "running") {
        cfg.running = v;
    }
    if let Some(v) = json_bool(&text, "audio") {
        cfg.audio = v;
    }
    if let Some(v) = json_str(&text, "effect") {
        cfg.effect = v;
    }
    if let Some(v) = json_str(&text, "byline") {
        cfg.byline = v;
    }
    if let Some(v) = json_num(&text, "intensity") {
        cfg.intensity = v;
    }
    if let Some(v) = json_num(&text, "speed") {
        cfg.speed = v.clamp(1, 100);
    }
    if let Some(v) = json_num(&text, "restart") {
        cfg.restart = v;
    }
    if let Some(v) = json_num(&text, "intro_size") {
        cfg.intro_size = v;
    }
    if let Some(v) = json_bool(&text, "show_fps") {
        cfg.show_fps = v;
    }
    if let Some(v) = json_bool(&text, "boot_between") {
        cfg.boot_between = v;
    }
    if let Some(v) = json_num(&text, "rotate_secs") {
        cfg.rotate_secs = v;
    }
    if let Some(v) = json_str(&text, "ttfx_text") {
        if !v.trim().is_empty() {
            cfg.ttfx_text = v;
        }
    }
    if let Some(v) = json_num(&text, "resolution") {
        cfg.resolution = v;
    }
    if let Some(v) = json_num(&text, "reactivity") {
        cfg.reactivity = v;
    }
    if let Some(v) = json_bool(&text, "intro_beat_sync") {
        cfg.intro_beat_sync = v;
    }
    if let Some(v) = json_bool(&text, "use_theme_colors") {
        cfg.use_theme_colors = v;
    }
    if let Some(v) = json_bool(&text, "transparent_background") {
        cfg.transparent_background = v;
    }
    if let Some(v) = json_str(&text, "char_style") {
        cfg.char_style = sanitize_char_style(&v).into();
    }
    if let Some(v) = json_str(&text, "boot_char_style") {
        cfg.boot_char_style = sanitize_char_style(&v).into();
    }
    if let Some(v) = json_str_list(&text, "effects") {
        if !v.is_empty() {
            cfg.effects = v;
        }
    }
    LAST_GOOD_CONFIG.with(|c| *c.borrow_mut() = Some(cfg.clone()));
    cfg
}

// --- tiny tolerant JSON field extractors (no serde dependency) ---
fn json_value<'a>(text: &'a str, key: &str) -> Option<&'a str> {
    let pat = format!("\"{key}\"");
    let pos = text.find(&pat)?;
    let rest = &text[pos + pat.len()..];
    let colon = rest.find(':')?;
    Some(rest[colon + 1..].trim_start())
}

fn json_bool(text: &str, key: &str) -> Option<bool> {
    let v = json_value(text, key)?;
    if v.starts_with("true") {
        Some(true)
    } else if v.starts_with("false") {
        Some(false)
    } else {
        None
    }
}

fn json_str(text: &str, key: &str) -> Option<String> {
    let v = json_value(text, key)?;
    let inner = v.strip_prefix('"')?;
    let end = inner.find('"')?;
    Some(inner[..end].to_string())
}

fn json_num(text: &str, key: &str) -> Option<i64> {
    let v = json_value(text, key)?;
    let end = v.find(|c: char| !(c.is_ascii_digit() || c == '-'))?;
    v[..end].parse().ok()
}

fn json_str_list(text: &str, key: &str) -> Option<Vec<String>> {
    let v = json_value(text, key)?;
    let inner = v.strip_prefix('[')?;
    let end = inner.find(']')?;
    let body = &inner[..end];
    let mut out = Vec::new();
    let mut rest = body;
    while let Some(start) = rest.find('"') {
        let after = &rest[start + 1..];
        match after.find('"') {
            Some(stop) => {
                out.push(after[..stop].to_string());
                rest = &after[stop + 1..];
            }
            None => break,
        }
    }
    Some(out)
}

fn sanitize_char_style(style: &str) -> &'static str {
    match style {
        "block" => "block",
        "dark" => "dark",
        "medium" => "medium",
        "light" => "light",
        "hash" => "hash",
        "dot" => "dot",
        "lower_o" => "lower_o",
        "upper_o" => "upper_o",
        _ => "native",
    }
}

fn styled_glyph(style: &str, ch: char) -> char {
    if ch == ' ' {
        return ch;
    }
    match sanitize_char_style(style) {
        "block" => '█',
        "dark" => '▓',
        "medium" => '▒',
        "light" => '░',
        "hash" => '#',
        "dot" => '·',
        "lower_o" => 'o',
        "upper_o" => 'O',
        _ => ch,
    }
}

// Parse a simple TOML colors.toml file from Omarchy themes.
// Only handles `key = "value"` lines (no sections, no tables). Returns a map
// of color name -> hex string (without the leading #).
fn parse_theme_colors(content: &str) -> Vec<(String, String)> {
    let mut out = Vec::new();
    for line in content.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') || line.starts_with('[') {
            continue;
        }
        if let Some(eq) = line.find('=') {
            let key = line[..eq].trim().to_string();
            let val = line[eq + 1..].trim();
            let val = val.strip_prefix('"').unwrap_or(val);
            let val = val.strip_suffix('"').unwrap_or(val);
            if !key.is_empty() && !val.is_empty() {
                out.push((key, val.to_string()));
            }
        }
    }
    out
}

// Convert a hex color string (with or without leading #) to an ANSI 24-bit SGR code.
// Supports both 6-digit (#RRGGBB) and 3-digit (#RGB) forms.
fn hex_to_ansi(hex: &str) -> String {
    let hex = hex.strip_prefix('#').unwrap_or(hex);
    let bytes = if hex.len() == 6 {
        [
            u8::from_str_radix(&hex[..2], 16).unwrap_or(0),
            u8::from_str_radix(&hex[2..4], 16).unwrap_or(0),
            u8::from_str_radix(&hex[4..6], 16).unwrap_or(0),
        ]
    } else if hex.len() == 3 {
        let r = u8::from_str_radix(&hex[..1], 16).unwrap_or(0);
        let g = u8::from_str_radix(&hex[1..2], 16).unwrap_or(0);
        let b = u8::from_str_radix(&hex[2..3], 16).unwrap_or(0);
        [r * 17, g * 17, b * 17]
    } else {
        [0, 0, 0]
    };
    format!("\x1b[38;2;{};{};{}m", bytes[0], bytes[1], bytes[2])
}

// Parse a hex color string (#RRGGBB) into (r, g, b) bytes. Returns None if invalid.
fn parse_hex_color(hex: &str) -> Option<(u8, u8, u8)> {
    let hex = hex.strip_prefix('#').unwrap_or(hex);
    if hex.len() == 6 {
        let r = u8::from_str_radix(&hex[..2], 16).ok()?;
        let g = u8::from_str_radix(&hex[2..4], 16).ok()?;
        let b = u8::from_str_radix(&hex[4..6], 16).ok()?;
        Some((r, g, b))
    } else {
        None
    }
}

// Read the active Omarchy theme's colors.toml and return a map of color name -> ANSI code.
// Returns empty map if the theme file can't be read (graceful degradation to hardcoded colors).
fn read_theme_colors() -> Vec<(String, String)> {
    let theme_name_path =
        std::path::PathBuf::from(std::env::var("HOME").unwrap_or_else(|_| ".".into()))
            .join(".local/state/omarchy/current/theme.name");
    let theme_name = match std::fs::read_to_string(&theme_name_path) {
        Ok(t) => t.trim().to_string(),
        Err(_) => return Vec::new(),
    };
    if theme_name.is_empty() {
        return Vec::new();
    }
    let omarchy_path = std::env::var("OMARCHY_PATH").unwrap_or_else(|_| {
        std::path::PathBuf::from(std::env::var("HOME").unwrap_or_else(|_| ".".into()))
            .join(".local/share/omarchy")
            .to_string_lossy()
            .to_string()
    });
    let colors_path = format!("{}/themes/{}/colors.toml", omarchy_path, theme_name);
    match std::fs::read_to_string(&colors_path) {
        Ok(c) => parse_theme_colors(&c),
        Err(_) => Vec::new(),
    }
}

// Build an effect palette from the active theme colors.
fn theme_palette(theme: &[(String, String)], effect: &str) -> Vec<String> {
    let get = |name: &str, fallback: &str| -> String {
        for (k, v) in theme {
            if k == name {
                return hex_to_ansi(v);
            }
        }
        fallback.to_string()
    };
    let accent = get("accent", "\x1b[96m");
    let foreground = get("foreground", "\x1b[37m");
    let background = get("background", "\x1b[40m");
    let red = get("red", "\x1b[31m");
    let green = get("green", "\x1b[32m");
    let blue = get("blue", "\x1b[34m");
    let cyan = get("cyan", "\x1b[36m");
    let yellow = get("yellow", "\x1b[33m");
    let magenta = get("magenta", "\x1b[35m");
    let bright_red = get("bright_red", "\x1b[91m");
    let bright_green = get("bright_green", "\x1b[92m");
    let bright_cyan = get("bright_cyan", "\x1b[96m");
    let bright_yellow = get("bright_yellow", "\x1b[93m");

    match effect {
        "rain" => vec![accent.clone(), cyan.clone(), foreground.clone()],
        "matrix" => vec![accent.clone(), green.clone(), blue.clone()],
        "wave" => vec![accent.clone(), magenta.clone(), foreground.clone()],
        "bars" => vec![yellow.clone(), bright_yellow.clone(), foreground.clone()],
        "fire" => vec![red.clone(), bright_red.clone(), yellow.clone()],
        "life" => vec![green.clone(), bright_green.clone(), foreground.clone()],
        "starfield" => vec![foreground.clone(), bright_cyan.clone(), background.clone()],
        "donut" => vec![accent.clone(), cyan.clone(), yellow.clone()],
        _ => vec![foreground.clone(), green.clone(), blue.clone()],
    }
}

// Hardcoded fallback palettes (used when use_theme_colors is false OR theme unavailable).
fn hardcoded_palette(effect: &str) -> Vec<String> {
    match effect {
        "rain" => vec![
            "\x1b[96m".to_string(),
            "\x1b[36m".to_string(),
            "\x1b[37m".to_string(),
        ],
        "wave" => vec![
            "\x1b[95m".to_string(),
            "\x1b[35m".to_string(),
            "\x1b[37m".to_string(),
        ],
        "bars" => vec![
            "\x1b[93m".to_string(),
            "\x1b[33m".to_string(),
            "\x1b[37m".to_string(),
        ],
        "fire" => vec![
            "\x1b[91m".to_string(),
            "\x1b[93m".to_string(),
            "\x1b[31m".to_string(),
        ],
        "life" => vec![
            "\x1b[92m".to_string(),
            "\x1b[32m".to_string(),
            "\x1b[90m".to_string(),
        ],
        "starfield" => vec![
            "\x1b[97m".to_string(),
            "\x1b[37m".to_string(),
            "\x1b[90m".to_string(),
        ],
        "donut" => vec![
            "\x1b[96m".to_string(),
            "\x1b[95m".to_string(),
            "\x1b[93m".to_string(),
        ],
        _ => vec![
            "\x1b[97m".to_string(),
            "\x1b[92m".to_string(),
            "\x1b[32m".to_string(),
        ],
    }
}

fn arg_value(args: &[String], key: &str) -> Option<String> {
    let mut it = args.iter();
    while let Some(a) = it.next() {
        if a == key {
            return it.next().cloned();
        }
        if let Some(v) = a.strip_prefix(&format!("{key}=")) {
            return Some(v.to_string());
        }
    }
    None
}

fn arm_parent_death_signal() {
    let parent = unsafe { libc::getppid() };
    if parent <= 1 {
        return;
    }
    unsafe {
        libc::prctl(libc::PR_SET_PDEATHSIG, libc::SIGTERM);
    }
    // Parent may have died between getppid() and prctl().
    if unsafe { libc::getppid() } != parent {
        std::process::exit(0);
    }
}

fn try_controller_lock() -> Option<std::fs::File> {
    use std::os::fd::AsRawFd;
    let base = std::env::var("XDG_RUNTIME_DIR").unwrap_or_else(|_| "/tmp".into());
    let path = format!("{base}/omarchy-audio-background-{}.lock", unsafe {
        libc::getuid()
    });
    let file = std::fs::OpenOptions::new()
        .create(true)
        .read(true)
        .write(true)
        .open(path)
        .ok()?;
    let rc = unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) };
    if rc == 0 {
        Some(file)
    } else {
        None
    }
}

fn main() -> Result<()> {
    let args: Vec<String> = std::env::args().collect();
    // One-shot config bump for the background switcher's +/- keys: adjust the
    // render resolution in state.json and print the new value. The running
    // controller polls state.json and rebuilds layers within ~700ms, so no
    // restart is needed. Must run before the controller singleton lock.
    if let Some(delta) = arg_value(&args, "--bump-resolution").and_then(|s| s.parse::<i64>().ok()) {
        let new = (read_config().resolution + delta).clamp(1, 8);
        patch_state_nums(&[("resolution", new)]);
        println!("{new}");
        return Ok(());
    }
    // Without the assembly engine every effect fails at spawn time. Say so
    // once here instead of leaving a blank layer and an error buried in the log.
    if cfg!(target_arch = "x86_64") && ttfx::asm::tier() == 0 {
        let message = "ttfx-bg-rs: built without the ttfx assembly engine (NASM was missing); effects cannot render";
        eprintln!("{message}");
        log_dbg(message);
        #[cfg(not(feature = "vte"))]
        std::process::exit(2);
    }
    // omarchy-bg-live's `ttfx` source: one output, no plugin state or lock.
    if standalone::invoked_as_standalone(&args[0]) || args.iter().any(|a| a == "--standalone") {
        return standalone::run(&args);
    }
    fps_governor::start(
        fps_governor::parse_mode(std::env::var("TTFX_BG_MAX_FPS").ok().as_deref()),
        |line| log_dbg(&line),
    );
    if args.iter().any(|a| a == "--render") {
        let effect = arg_value(&args, "--effect").unwrap_or_else(|| "matrix".into());
        let cols = arg_value(&args, "--cols")
            .and_then(|s| s.parse::<usize>().ok())
            .unwrap_or(200);
        let rows = arg_value(&args, "--rows")
            .and_then(|s| s.parse::<usize>().ok())
            .unwrap_or(100);
        let intensity = arg_value(&args, "--intensity")
            .and_then(|s| s.parse::<i64>().ok())
            .unwrap_or(5);
        let speed = arg_value(&args, "--speed")
            .and_then(|s| s.parse::<i64>().ok())
            .unwrap_or(5)
            .clamp(1, 100);
        let audio = arg_value(&args, "--audio")
            .map(|s| s == "1")
            .unwrap_or(false);
        let byline = arg_value(&args, "--byline").unwrap_or_default();
        let ttfx_text = arg_value(&args, "--ttfx-text").unwrap_or_else(|| "OMARCHY".into());
        let reactivity = arg_value(&args, "--reactivity")
            .and_then(|s| s.parse::<i64>().ok())
            .unwrap_or(2);
        let intro_size = arg_value(&args, "--intro-size")
            .and_then(|s| s.parse::<i64>().ok())
            .unwrap_or(2);
        let cell_aspect = arg_value(&args, "--cell-aspect")
            .and_then(|s| s.parse::<f32>().ok())
            .unwrap_or(2.0);
        let show_fps = arg_value(&args, "--show-fps")
            .map(|s| s == "1")
            .unwrap_or(false);
        let show_intro = !args.iter().any(|a| a == "--no-intro");
        let cfg = read_config();
        let intro_beat_sync = arg_value(&args, "--intro-beat-sync")
            .map(|s| s == "1")
            .unwrap_or(cfg.intro_beat_sync);
        return run_render(
            &cfg,
            &effect,
            cols,
            rows,
            intensity,
            speed,
            audio,
            &byline,
            intro_size,
            cell_aspect,
            show_fps,
            show_intro,
            &ttfx_text,
            reactivity,
            intro_beat_sync,
            true,
        );
    }

    // Drive one effect standalone on this pty (preview daemon bridge).
    if args.iter().any(|a| a == "--ttfx") {
        let effect = arg_value(&args, "--effect").unwrap_or_else(|| "matrix".into());
        let cols = arg_value(&args, "--cols")
            .and_then(|s| s.parse::<usize>().ok())
            .unwrap_or(80);
        let rows = arg_value(&args, "--rows")
            .and_then(|s| s.parse::<usize>().ok())
            .unwrap_or(24);
        let ttfx_text = arg_value(&args, "--ttfx-text").unwrap_or_else(|| "OMARCHY".into());
        let reactivity = arg_value(&args, "--reactivity")
            .and_then(|s| s.parse::<i64>().ok())
            .unwrap_or(2);
        let show_fps = arg_value(&args, "--show-fps")
            .map(|s| s == "1")
            .unwrap_or(false);
        // Route through run_render so the hand-rolled native effects work
        // here too: run_ttfx alone only knows the vendored catalog and died
        // "unknown ttfx effect" for wave/bars/donut/fire/starfield/life,
        // which the preview daemon kept respawning forever. No audio capture
        // and no auto-degrade: previews must never rewrite shared state.json.
        let cfg = read_config();
        return run_render(
            &cfg, &effect, cols, rows, 5, 5, false, "", 1, 2.0, show_fps, false, &ttfx_text,
            reactivity, false, false,
        );
    }

    // Preview daemon for the QML switcher: pure in-process effect rendering
    // with a JSON-lines protocol. Runs WITHOUT the controller lock — it must
    // coexist with the fullscreen controller.
    if args.iter().any(|a| a == "--preview-daemon") {
        return run_preview_daemon();
    }

    arm_parent_death_signal();
    let _controller_lock = match try_controller_lock() {
        Some(lock) => lock,
        None => {
            log_dbg("controller already running; duplicate exits");
            return Ok(());
        }
    };

    gtk4::init()?;
    let self_bin = std::env::current_exe()
        .map(|p| p.to_string_lossy().to_string())
        .unwrap_or_else(|_| "ttfx-bg-rs".into());

    let windows: Rc<RefCell<Vec<gtk4::ApplicationWindow>>> = Rc::new(RefCell::new(Vec::new()));
    let cfg0 = read_config();
    let active_effect: Rc<RefCell<String>> = Rc::new(RefCell::new(
        // Only honor the saved effect if it's still enabled in the rotation set.
        // Otherwise the user disabled it — start with the first enabled one.
        if is_valid_effect(&cfg0.effect) && cfg0.effects.contains(&cfg0.effect) {
            cfg0.effect.clone()
        } else {
            cfg0.effects
                .first()
                .cloned()
                .unwrap_or_else(|| "matrix".into())
        },
    ));
    let last_cfg: Rc<RefCell<Config>> = Rc::new(RefCell::new(cfg0));

    rebuild_layers(
        &windows,
        &self_bin,
        &last_cfg.borrow(),
        &active_effect.borrow(),
        true,
    );

    // Rebuild ONLY when the monitor geometry actually changed (count + size +
    // scale). items-changed also fires on spurious signals; rebuilding on those
    // was the source of visible flicker (screen clear + renderer respawn).
    if let Some(display) = gdk::Display::default() {
        let monitors = display.monitors();
        let w = windows.clone();
        let b = self_bin.clone();
        let lc = last_cfg.clone();
        let ae = active_effect.clone();
        let last_sig: Rc<RefCell<String>> = Rc::new(RefCell::new(monitor_signature()));
        let rebuilding: Rc<RefCell<bool>> = Rc::new(RefCell::new(false));
        monitors.connect_items_changed(move |_, _pos, _removed, _added| {
            let sig = monitor_signature();
            let previous = last_sig.replace(sig.clone());
            if sig == previous {
                return;
            }
            log_dbg(&format!("monitors changed: {previous} -> {sig}"));
            if *rebuilding.borrow() {
                return;
            }
            *rebuilding.borrow_mut() = true;
            // Debounce: coalesce a burst of items-changed into one rebuild.
            let w = w.clone();
            let b = b.clone();
            let lc = lc.clone();
            let ae = ae.clone();
            let rebuilding = rebuilding.clone();
            glib::timeout_add_local_once(Duration::from_millis(250), move || {
                log_dbg("monitors changed: rebuilding background layers");
                let cfg = read_config();
                *lc.borrow_mut() = cfg.clone();
                rebuild_layers(&w, &b, &cfg, &ae.borrow(), true);
                *rebuilding.borrow_mut() = false;
            });
        });
    }

    // Poll the panel's state.json; if anything changed, rebuild.
    {
        let w = windows.clone();
        let b = self_bin.clone();
        let lc = last_cfg.clone();
        let ae = active_effect.clone();
        glib::timeout_add_local(Duration::from_millis(700), move || {
            let cfg = read_config();
            let changed = *lc.borrow() != cfg;
            if changed {
                let old = lc.borrow().clone();
                log_dbg(&format!("poll changed: old effect={} effects={:?} -> new effect={} effects={:?} ae={} running={}", old.effect, old.effects, cfg.effect, cfg.effects, ae.borrow(), cfg.running));
                // If the active-effect selection changed, honor it; otherwise
                // keep rotating from where we are. Only honor if still enabled.
                if cfg.effect != old.effect
                    && is_valid_effect(&cfg.effect)
                    && cfg.effects.contains(&cfg.effect)
                {
                    *ae.borrow_mut() = cfg.effect.clone();
                } else if !cfg.effects.contains(&ae.borrow().clone()) {
                    // Active effect was disabled via toggle — jump to first enabled.
                    if let Some(first) = cfg.effects.first() {
                        *ae.borrow_mut() = first.clone();
                    }
                }
                *lc.borrow_mut() = cfg.clone();
                // Rotation timing and intensity are applied by the running renderer.
                // Keep the layer alive for those controls: killing its PTY on every
                // slider tick was the visible "restart" users reported.
                let visual = cfg.running != old.running
                    || cfg.effect != old.effect
                    || cfg.effects != old.effects
                    || cfg.audio != old.audio
                    || cfg.byline != old.byline
                    || cfg.ttfx_text != old.ttfx_text
                    || cfg.restart != old.restart
                    || cfg.intro_size != old.intro_size
                    || cfg.show_fps != old.show_fps
                    || cfg.resolution != old.resolution
                    || cfg.intro_beat_sync != old.intro_beat_sync
                    || cfg.use_theme_colors != old.use_theme_colors
                    || cfg.transparent_background != old.transparent_background
                    || cfg.char_style != old.char_style
                    || cfg.boot_char_style != old.boot_char_style;
                // Boot Between Backgrounds controls every ordinary effect transition,
                // including a manual picker selection. Explicit restart and changes to
                // intro content still deliberately replay it.
                let intro = (cfg.effect != old.effect && cfg.boot_between)
                    || cfg.restart != old.restart
                    || cfg.intro_size != old.intro_size
                    || cfg.ttfx_text != old.ttfx_text
                    || cfg.byline != old.byline
                    || cfg.intro_beat_sync != old.intro_beat_sync
                    || cfg.boot_char_style != old.boot_char_style;
                if visual {
                    rebuild_layers(&w, &b, &cfg, &ae.borrow(), intro);
                }
            }
            glib::ControlFlow::Continue
        });
    }

    // Rotate through the enabled effects when more than one is enabled. Fires every
    // second and counts up to cfg.rotate_secs so the interval is live-configurable
    // from the panel (no respawn needed to change it). Rotating respects
    // boot_between for whether the splash replays.
    {
        let w = windows.clone();
        let b = self_bin.clone();
        let lc = last_cfg.clone();
        let ae = active_effect.clone();
        let elapsed = std::rc::Rc::new(std::cell::Cell::new(0u64));
        glib::timeout_add_local(Duration::from_secs(1), move || {
            let cfg = lc.borrow().clone();
            if cfg.running && cfg.effects.len() > 1 {
                // If the currently displayed effect was just disabled, switch immediately
                // instead of waiting for the full interval (otherwise a disabled
                // effect stays visible for up to rotate_secs).
                let cur = ae.borrow().clone();
                if !cfg.effects.contains(&cur) {
                    let next = cfg.effects[0].clone();
                    *ae.borrow_mut() = next.clone();
                    elapsed.set(0);
                    rebuild_layers(&w, &b, &cfg, &next, false);
                    return glib::ControlFlow::Continue;
                }
                let e = elapsed.get() + 1;
                if e >= cfg.rotate_secs.clamp(3, 3600) as u64 {
                    elapsed.set(0);
                    let next = match cfg.effects.iter().position(|x| *x == cur) {
                        Some(i) => cfg.effects[(i + 1) % cfg.effects.len()].clone(),
                        None => cfg.effects[0].clone(),
                    };
                    if next != cur {
                        *ae.borrow_mut() = next.clone();
                        rebuild_layers(&w, &b, &cfg, &next, cfg.boot_between);
                    }
                } else {
                    elapsed.set(e);
                }
            } else {
                elapsed.set(0);
            }
            glib::ControlFlow::Continue
        });
    }

    glib::MainLoop::new(None, false).run();
    Ok(())
}

// Signature of the current monitor set: count + geometry + scale. Rebuilds
// only happen when this actually changes.
fn monitor_signature() -> String {
    let display = match gdk::Display::default() {
        Some(d) => d,
        None => return String::new(),
    };
    let monitors = display.monitors();
    let mut parts = Vec::new();
    for i in 0..monitors.n_items() {
        if let Some(m) = monitors.item(i).and_downcast::<gdk::Monitor>() {
            let g = m.geometry();
            parts.push(format!(
                "{}x{}@{},{}s{}",
                g.width(),
                g.height(),
                g.x(),
                g.y(),
                m.scale_factor()
            ));
        }
    }
    parts.join("|")
}

fn log_dbg(msg: &str) {
    let ts = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() % 86400)
        .unwrap_or(0);
    let h = ts / 3600;
    let m = (ts % 3600) / 60;
    let s = ts % 60;
    let line = format!("[{h:02}:{m:02}:{s:02} {}] {msg}\n", std::process::id());
    let _ = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open("/tmp/ttfx-bg-debug.log")
        .and_then(|mut f| f.write_all(line.as_bytes()));
    eprintln!("{msg}");
}

const LAYER_STOP_KEY: &str = "ttfx-layer-stop";

fn rebuild_layers(
    windows: &Rc<RefCell<Vec<gtk4::ApplicationWindow>>>,
    self_bin: &str,
    cfg: &Config,
    effect: &str,
    show_intro: bool,
) {
    log_dbg(&format!("rebuild_layers: effect={effect} show_intro={show_intro} running={} audio={} intensity={} reactivity={} resolution={} rotate_secs={} boot_between={} effect_field={} effects={:?} ttfx_text={}", cfg.running, cfg.audio, cfg.intensity, cfg.reactivity, cfg.resolution, cfg.rotate_secs, cfg.boot_between, cfg.effect, cfg.effects, cfg.ttfx_text));
    // Kill only OUR direct renderer children. A broad `pkill -f` also matched
    // unrelated terminal commands that happened to inspect renderer arguments,
    // terminating the caller during a rotation/rebuild.
    let _ = std::process::Command::new("pkill")
        .args(["-TERM", "-P", &std::process::id().to_string()])
        .output();
    for w in windows.borrow_mut().drain(..) {
        // Windows that were never shown (the Wayland GPU layer's handle) get no
        // close-request, so stop their renderer explicitly.
        if let Some(stop) = unsafe { w.data::<Arc<std::sync::atomic::AtomicBool>>(LAYER_STOP_KEY) } {
            unsafe { stop.as_ref() }.store(true, Ordering::Release);
        }
        w.close();
    }
    // Toggled off: leave NO layer window at all. The desktop then shows the
    // default wallpaper instead of an empty black VTE covering the screen.
    if !cfg.running {
        println!("running=false: no layer windows, desktop wallpaper visible");
        log_dbg("rebuild_layers: running=false, no windows spawned");
        return;
    }
    let display = match gdk::Display::default() {
        Some(d) => d,
        None => return,
    };
    let monitors = display.monitors();
    let n = monitors.n_items();
    println!(
        "found {n} monitor(s), running={} effect={effect}",
        cfg.running
    );
    for i in 0..n {
        if let Some(monitor) = monitors.item(i).and_downcast::<gdk::Monitor>() {
            let w = spawn_layer_for_monitor(&monitor, self_bin, cfg, effect, show_intro);
            windows.borrow_mut().push(w);
        }
    }
}

fn spawn_layer_for_monitor(
    monitor: &gdk::Monitor,
    self_bin: &str,
    cfg: &Config,
    effect: &str,
    show_intro: bool,
) -> gtk4::ApplicationWindow {
    match render_backend() {
        RenderBackend::Direct => return spawn_direct_layer_for_monitor(monitor, cfg, effect, show_intro),
        RenderBackend::Gpu if wl_layer::available() => {
            return spawn_gpu_wayland_layer_for_monitor(monitor, cfg, effect, show_intro)
        }
        RenderBackend::Gpu => {
            log_dbg("gpu renderer unavailable (EGL/GLES 3); using the direct renderer");
            return spawn_direct_layer_for_monitor(monitor, cfg, effect, show_intro);
        }
        RenderBackend::Vte => spawn_vte_layer_for_monitor(monitor, self_bin, cfg, effect, show_intro),
    }
}

// Builds without the `vte` feature do not link libvte, which a fresh Omarchy
// does not install. They fall back to the GPU layer, then the direct renderer.
#[cfg(not(feature = "vte"))]
fn spawn_vte_layer_for_monitor(
    monitor: &gdk::Monitor,
    _self_bin: &str,
    cfg: &Config,
    effect: &str,
    show_intro: bool,
) -> gtk4::ApplicationWindow {
    if wl_layer::available() {
        return spawn_gpu_wayland_layer_for_monitor(monitor, cfg, effect, show_intro);
    }
    spawn_direct_layer_for_monitor(monitor, cfg, effect, show_intro)
}

#[cfg(feature = "vte")]
fn spawn_vte_layer_for_monitor(
    monitor: &gdk::Monitor,
    self_bin: &str,
    cfg: &Config,
    effect: &str,
    show_intro: bool,
) -> gtk4::ApplicationWindow {
    let window = gtk4::ApplicationWindow::builder().title("ttfx-bg").build();

    window.init_layer_shell();
    window.set_namespace(Some("ttfx-bg"));
    window.set_layer(Layer::Bottom);
    window.set_monitor(Some(monitor));
    for e in [Edge::Left, Edge::Right, Edge::Top, Edge::Bottom] {
        window.set_anchor(e, true);
    }
    window.set_exclusive_zone(-1);

    let term = vte4::Terminal::new();
    // Font size in *physical pixels* (not points) so the grid scales with the
    // panel resolution: GTK applies the monitor scale to point sizes, which on
    // a 4K/scaled display produced a handful of giant characters.
    let scale = monitor.scale_factor().max(1) as f64;
    // `resolution` scales the cell size: 1 = full-res grid (most CPU), higher =
    // bigger cells => fewer cols/rows => much less CPU (for old machines).
    let cell_px = (9.0 / scale * cfg.resolution.clamp(1, 8) as f64).max(3.0);
    let font = gtk4::gdk::pango::FontDescription::from_string(&format!("monospace {cell_px:.0}px"));
    term.set_font(Some(&font));
    term.set_scrollback_lines(0);
    term.set_hexpand(true);
    term.set_vexpand(true);
    // When transparent_background is enabled, make Vte's background transparent
    // so the user's wallpaper shows through the empty cells.
    if cfg.transparent_background {
        term.set_color_background(&gtk4::gdk::RGBA::new(0.0, 0.0, 0.0, 0.0));
    }
    window.set_child(Some(&term));

    if cfg.running {
        let geo = monitor.geometry();
        // Measure the real cell size from the font metrics so the grid fills
        // the screen exactly regardless of DPI / scale factor.
        let font_size = font.size() as f64 / gtk4::pango::SCALE as f64;
        let ctx = term.pango_context();
        let metrics = ctx.metrics(Some(&font), None);
        let char_w = metrics.approximate_char_width() as f64 / gtk4::pango::SCALE as f64;
        let ascent = metrics.ascent() as f64 / gtk4::pango::SCALE as f64;
        let descent = metrics.descent() as f64 / gtk4::pango::SCALE as f64;
        let cw = if char_w > 0.0 {
            char_w
        } else {
            font_size * 0.6
        };
        let ch = if (ascent + descent) > 0.0 {
            ascent + descent
        } else {
            font_size * 1.2
        };
        let (cols, rows) = measured_grid(geo.width(), geo.height(), cw, ch);
        // Cell aspect (height/width in px) so effects can draw true circles.
        let cell_aspect = if cw > 0.0 { ch / cw } else { 2.0 };
        let byline = if cfg.byline.trim().is_empty() {
            DEFAULT_BYLINE
        } else {
            cfg.byline.trim()
        }
        .to_string();
        let audio = if cfg.audio { "1" } else { "0" };
        let mut argv: Vec<String> = vec![
            self_bin.to_string(),
            "--render".into(),
            "--effect".into(),
            effect.to_string(),
            "--cols".into(),
            cols.to_string(),
            "--rows".into(),
            rows.to_string(),
            "--intensity".into(),
            cfg.intensity.to_string(),
            "--speed".into(),
            cfg.speed.to_string(),
            "--audio".into(),
            audio.into(),
            "--byline".into(),
            byline,
            "--ttfx-text".into(),
            cfg.ttfx_text.clone(),
            "--reactivity".into(),
            cfg.reactivity.to_string(),
            "--intro-size".into(),
            cfg.intro_size.to_string(),
            "--cell-aspect".into(),
            format!("{cell_aspect:.3}"),
            "--show-fps".into(),
            if cfg.show_fps { "1".into() } else { "0".into() },
        ];
        // Rotation with "boot between backgrounds" off skips the splash.
        if !show_intro {
            argv.push("--no-intro".into());
        }
        // Pass intro_beat_sync as CLI arg so the renderer uses it
        let intro_beat_sync_arg = if cfg.intro_beat_sync { "1" } else { "0" };
        argv.push("--intro-beat-sync".into());
        argv.push(intro_beat_sync_arg.into());
        let argv_refs: Vec<&str> = argv.iter().map(|s| s.as_str()).collect();
        // Pre-clear so Vte doesn't flash its "N by M cells" placeholder while
        // the renderer's PTY is still connecting. Use transparent background
        // when the option is enabled.
        if cfg.transparent_background {
            term.feed(b"\x1b[2J\x1b[H");
        } else {
            term.feed(b"\x1b[2J\x1b[H\x1b[40m");
        }
        let effect_for_log = effect.to_string();
        log_dbg(&format!("spawn_async: effect={effect_for_log} {cols}x{rows} intensity={} speed={} reactivity={} audio={} ttfx_text={} show_intro={}", cfg.intensity, cfg.speed, cfg.reactivity, cfg.audio, cfg.ttfx_text, show_intro));
        term.spawn_async(
            vte4::PtyFlags::DEFAULT,
            None,
            &argv_refs,
            &[],
            gtk4::glib::SpawnFlags::empty(),
            || {},
            2000,
            None::<&gtk4::gio::Cancellable>,
            move |res| match res {
                Ok(_) => {
                    println!("renderer spawned ({cols}x{rows}, effect={effect_for_log})");
                    log_dbg(&format!("spawn ok: {effect_for_log} {cols}x{rows}"));
                }
                Err(e) => {
                    eprintln!("spawn error: {e:?}");
                    log_dbg(&format!("spawn error {effect_for_log}: {e:?}"));
                }
            },
        );
    }

    window.present();
    window
}

// Compatibility for the existing Screen publication tests: a consumed duplicate
// is still a publication, even when presentation needs no new pixels.
#[cfg(test)]
fn consume_direct_snapshot(
    snapshot: &std::sync::Mutex<Vec<DirectCell>>,
    generation: &std::sync::atomic::AtomicU64,
    consumed: &std::cell::Cell<u64>,
    visible: &mut Vec<DirectCell>,
) -> bool {
    frame_handoff::consume_latest(snapshot, generation, consumed, visible, false).new_frame
}

fn memory_texture_bytes_owned(pixels: Vec<u8>) -> gtk4::glib::Bytes {
    gtk4::glib::Bytes::from_owned(pixels)
}

fn bounded_damage_policy(effect: &str, requested: Option<&str>) -> bool {
    requested == Some("bounded") && effect == "wave"
}

fn direct_retained_raster(effect: &str, requested: Option<&str>) -> bool {
    // Native effects with dense/changing frames regressed in the same-binary
    // FULL/retained audit. Keep their full raster without changing publication,
    // opaque texture, wakeup, audio or pacing. `waves` is a separate ASM effect.
    requested == Some("retained") && !matches!(effect, "fire" | "life" | "matrix" | "rain" | "wave")
}

fn direct_memory_format(transparent: bool, opaque_requested: bool) -> gdk::MemoryFormat {
    // The opaque raster path fills every pixel's alpha with 255. RGBX describes
    // those same bytes while allowing GSK to infer opaque texture/surface regions.
    if opaque_requested && !transparent {
        gdk::MemoryFormat::B8g8r8x8
    } else {
        gdk::MemoryFormat::B8g8r8a8Premultiplied
    }
}

// Direct backend: the effect owns a headless Screen and publishes only its latest
// sparse cell grid. GTK reads the newest grid on the compositor's frame
// clock, without a fixed application FPS target or accumulating ANSI work.
fn spawn_direct_layer_for_monitor(
    monitor: &gdk::Monitor,
    cfg: &Config,
    effect: &str,
    show_intro: bool,
) -> gtk4::ApplicationWindow {
    use std::sync::atomic::{AtomicBool, AtomicU64};
    use std::sync::Mutex;

    let window = gtk4::ApplicationWindow::builder()
        .title("ttfx-bg-direct")
        .build();
    window.init_layer_shell();
    window.set_namespace(Some("ttfx-bg-direct"));
    window.set_layer(Layer::Bottom);
    window.set_monitor(Some(monitor));
    for edge in [Edge::Left, Edge::Right, Edge::Top, Edge::Bottom] {
        window.set_anchor(edge, true);
    }
    window.set_exclusive_zone(-1);

    let geo = monitor.geometry();
    let scale = monitor.scale_factor().max(1) as f64;
    let cell_px = (9.0 / scale * cfg.resolution.clamp(1, 8) as f64).max(3.0);
    // Use the VTE baseline's actual Pango font metrics. Approximate multipliers
    // produced a larger grid on Kuyen and invalidated CPU/visual comparisons.
    let font = gtk4::gdk::pango::FontDescription::from_string(&format!("monospace {cell_px:.0}px"));
    let font_size = font.size() as f64 / gtk4::pango::SCALE as f64;
    let transparent = cfg.transparent_background;

    let picture = gtk4::Picture::new();
    let metrics = picture.pango_context().metrics(Some(&font), None);
    let char_w = metrics.approximate_char_width() as f64 / gtk4::pango::SCALE as f64;
    let ascent = metrics.ascent() as f64 / gtk4::pango::SCALE as f64;
    let descent = metrics.descent() as f64 / gtk4::pango::SCALE as f64;
    let cw = if char_w > 0.0 {
        char_w
    } else {
        font_size * 0.6
    };
    let ch = if ascent + descent > 0.0 {
        ascent + descent
    } else {
        font_size * 1.2
    };
    let (cols, rows) = measured_grid(geo.width(), geo.height(), cw, ch);
    let cell_aspect = if cw > 0.0 { (ch / cw) as f32 } else { 2.0 };
    picture.set_hexpand(true);
    picture.set_vexpand(true);
    picture.set_content_fit(gtk4::ContentFit::Fill);
    let overlay = gtk4::Overlay::new();
    overlay.set_child(Some(&picture));
    let fps_label = gtk4::Label::new(None);
    fps_label.set_halign(gtk4::Align::Start);
    fps_label.set_valign(gtk4::Align::End);
    fps_label.set_margin_bottom(2);
    fps_label.set_markup(
        "<span foreground='#eeeeee' background='#000000' font_family='monospace'>FPS --</span>",
    );
    fps_label.set_visible(cfg.show_fps);
    overlay.add_overlay(&fps_label);
    let show_fps = cfg.show_fps;
    let fps_meter = RefCell::new((std::time::Instant::now(), 0u32));
    let visible_cells: Rc<RefCell<Vec<DirectCell>>> = Rc::new(RefCell::new(Vec::new()));
    let glyph_atlas = RefCell::new(DirectGlyphAtlas::new(
        (geo.width() as f64 / cols as f64).ceil() as i32,
        (geo.height() as f64 / rows as f64).ceil() as i32,
        cell_px,
    ));
    window.set_child(Some(&overlay));

    let snapshot = Arc::new(Mutex::new(Vec::<DirectCell>::new()));
    let generation = Arc::new(AtomicU64::new(0));
    let stop = Arc::new(AtomicBool::new(false));
    let (publication_wakeup, publication_clock) =
        if frame_wakeup::opt_in(std::env::var("TTFX_BG_WAKEUP").ok().as_deref()) {
            // A realized native owns its frame clock even before mapping. GTK
            // objects and fd-source installation remain on this UI thread.
            gtk4::prelude::WidgetExt::realize(&window);
            if let Some(clock) = window.frame_clock() {
                match frame_wakeup::FrameWakeup::new(stop.clone()) {
                    Ok(wakeup) => (Some(Arc::new(wakeup)), Some(clock)),
                    Err(error) => {
                        log_dbg(&format!("publication wakeup unavailable, using ticks: {error}"));
                        (None, None)
                    }
                }
            } else {
                log_dbg("publication frame clock unavailable, using ticks");
                (None, None)
            }
        } else {
            (None, None)
        };
    let wake_source = Rc::new(RefCell::new(None::<gtk4::glib::SourceId>));
    let wake_signal = Rc::new(RefCell::new(None::<(gdk::FrameClock, gtk4::glib::SignalHandlerId)>));
    let cleanup = {
        let shutdown_snapshot = snapshot.clone();
        let shutdown_stop = stop.clone();
        let shutdown_wakeup = RefCell::new(publication_wakeup.clone());
        let source = wake_source.clone();
        let signal = wake_signal.clone();
        Rc::new(move || {
            {
                // Publication checks stop under this same lock. Once shutdown
                // returns, neither the native nor ASM producer can publish.
                let _guard = shutdown_snapshot.lock().unwrap_or_else(|error| error.into_inner());
                shutdown_stop.store(true, Ordering::Release);
                if let Some(wakeup) = shutdown_wakeup.borrow_mut().take() { wakeup.disable(); }
            }
            if let Some(source) = source.borrow_mut().take() { source.remove(); }
            if let Some((clock, signal)) = signal.borrow_mut().take() { clock.disconnect(signal); }
        })
    };
    let poll_snapshot = snapshot.clone();
    let poll_generation = generation.clone();
    let poll_picture = picture.clone();
    let poll_stop = stop.clone();
    let requested_raster = match std::env::var("TTFX_BG_RASTER").ok().as_deref() {
        Some("retained") => "retained",
        _ => "full",
    };
    let requested_damage = match std::env::var("TTFX_BG_DAMAGE").ok().as_deref() {
        Some("bounded") => "bounded",
        _ => "legacy",
    };
    let bounded_damage = bounded_damage_policy(effect, Some(requested_damage));
    let retained_raster = requested_raster == "retained"
        && (bounded_damage || direct_retained_raster(effect, Some(requested_raster)));
    let effective_damage = if retained_raster && !transparent {
        if bounded_damage { "bounded" } else { "legacy" }
    } else { "full" };
    let effective_raster = if retained_raster && !transparent { "retained" } else { "full" };
    let cadence =
        frame_cadence::enabled_from_env().then(|| Arc::new(frame_cadence::FrameCadence::new()));
    if let (Some(metrics), Ok(path)) = (cadence.clone(), std::env::var("TTFX_BG_CADENCE_FILE")) {
        static SESSION: AtomicU64 = AtomicU64::new(0);
        let session = SESSION.fetch_add(1, Ordering::Relaxed);
        let path = format!("{path}.{}.{session}.jsonl", std::process::id());
        let metadata = format!("{{\"schema\":0,\"pid\":{},\"session\":{session},\"cols\":{cols},\"rows\":{rows},\"cpu_tier\":{},\"effect\":\"{effect}\",\"raster_requested\":\"{requested_raster}\",\"raster_effective\":\"{effective_raster}\",\"damage_requested\":\"{requested_damage}\",\"damage_effective\":\"{effective_damage}\"}}\n", std::process::id(), ttfx::asm::tier());
        let writer_stop = stop.clone();
        thread::Builder::new()
            .name("ttfx-cadence".into())
            .spawn(move || {
                use std::os::unix::fs::OpenOptionsExt;
                let mut file = match std::fs::OpenOptions::new()
                    .write(true)
                    .create_new(true)
                    .mode(0o600)
                    .custom_flags(libc::O_NOFOLLOW)
                    .open(&path)
                {
                    Ok(file) => file,
                    Err(error) => {
                        log_dbg(&format!("cadence writer unavailable: {error}"));
                        return;
                    }
                };
                if file.write_all(metadata.as_bytes()).is_err() {
                    return;
                }
                while !writer_stop.load(Ordering::Relaxed) {
                    thread::sleep(Duration::from_secs(1));
                    if metrics.snapshot().write_json(&mut file).is_err() {
                        break;
                    }
                }
            })
            .expect("spawn cadence writer");
    }
    let poll_cadence = cadence.clone();
    let pending_feedback = RefCell::new(std::collections::VecDeque::<i64>::with_capacity(64));
    let renderer_reported = std::cell::Cell::new(false);
    let consumed = std::cell::Cell::new(0u64);
    let texture_geometry = std::cell::Cell::new(None);
    let previous_texture = RefCell::new(None::<gdk::Texture>);
    let texture_format = direct_memory_format(
        transparent,
        std::env::var("TTFX_BG_OPAQUE").ok().as_deref() == Some("1"),
    );
    // Consume every publication; rasterize/upload only changes or invalidation.
    // Exact cell equality includes colors, backgrounds and visual attributes.
    let render_update = move |clock: &gdk::FrameClock| {
        if poll_stop.load(Ordering::Relaxed) {
            return gtk4::glib::ControlFlow::Break;
        }
        let width = poll_picture.width().max(1);
        let height = poll_picture.height().max(1);
        if let Some(metrics) = &poll_cadence {
            metrics.tick();
            if width > 1 && height > 1 && !renderer_reported.replace(true) {
                let renderer = poll_picture.native().and_then(|native| native.renderer())
                    .map(|renderer| renderer.type_().name().to_string()).unwrap_or_else(|| "unavailable".into());
                log_dbg(&format!("cadence renderer={renderer} grid={cols}x{rows} pixels={width}x{height} format={texture_format:?}"));
            }
            let mut pending = pending_feedback.borrow_mut();
            while let Some(&counter) = pending.front() {
                if counter < clock.history_start() {
                    metrics.feedback_expired(counter as u64);
                    pending.pop_front();
                } else if let Some(timing) = clock.timings(counter).filter(|timing| timing.is_complete()) {
                    let presentation = timing.presentation_time();
                    metrics.feedback_completed(counter as u64, (presentation > 0).then_some(presentation as u64 * 1000));
                    pending.pop_front();
                } else { break; }
            }
        }
        let force_redraw = texture_geometry.get() != Some((width, height));
        let next = poll_generation.load(Ordering::Acquire);
        let update = if next != consumed.get() || force_redraw {
            frame_handoff::consume_latest(
                &poll_snapshot,
                &poll_generation,
                &consumed,
                &mut visible_cells.borrow_mut(),
                force_redraw,
            )
        } else {
            frame_handoff::FrameUpdate { new_frame: false, redraw: false }
        };
        if update.new_frame {
            if let Some(metrics) = &poll_cadence { metrics.consumed(consumed.get()); }
        }
        if update.redraw {
            let raster_started = poll_cadence.as_ref().map(|_| std::time::Instant::now());
            let result = if retained_raster {
                glyph_atlas.borrow_mut().rasterize_retained_pixels(
                    &visible_cells.borrow(), cols, rows, width, height, transparent,
                )
            } else {
                glyph_atlas.borrow_mut().rasterize_pixels(
                    &visible_cells.borrow(), cols, rows, width, height, transparent,
                ).map(|pixels| RetainedPixels { pixels, damage: Vec::new(), full_redraw: true })
            };
            if let (Some(metrics), Some(started)) = (&poll_cadence, raster_started) {
                metrics.raster(started.elapsed().as_nanos() as u64);
            }
            match result {
                Ok(frame) if !frame.full_redraw && frame.damage.is_empty() && !force_redraw => {}
                Ok(mut frame) => {
                    if bounded_damage {
                        frame.bound_damage(width, height);
                    }
                    let submission_started = poll_cadence.as_ref().map(|_| std::time::Instant::now());
                    let bytes = memory_texture_bytes_owned(frame.pixels);
                    let texture: gdk::Texture = if retained_raster {
                        let mut builder = gdk::MemoryTextureBuilder::new()
                            .set_width(width).set_height(height)
                            .set_format(texture_format)
                            .set_stride(width as usize * 4).set_bytes(Some(&bytes));
                        if !frame.full_redraw && !force_redraw {
                            if let Some(previous) = previous_texture.borrow().as_ref() {
                                let rectangles: Vec<_> = frame.damage.iter().map(|rect|
                                    gtk4::cairo::RectangleInt::new(rect.x, rect.y, rect.width, rect.height)).collect();
                                let region = gtk4::cairo::Region::create_rectangles(&rectangles);
                                if region.status().is_ok() {
                                    builder = builder.set_update_texture(Some(previous)).set_update_region(Some(&region));
                                }
                            }
                        }
                        builder.build()
                    } else {
                        gdk::MemoryTexture::new(width, height,
                            texture_format, &bytes, width as usize * 4).upcast()
                    };
                    poll_picture.set_paintable(Some(&pixel_paintable::PixelPaintable::new(&texture)));
                    *previous_texture.borrow_mut() = Some(texture);
                    if let (Some(metrics), Some(started)) = (&poll_cadence, submission_started) {
                        metrics.enqueue(started.elapsed().as_nanos() as u64);
                        let mut pending = pending_feedback.borrow_mut();
                        if pending.len() == 64 {
                            metrics.feedback_expired(pending.pop_front().unwrap() as u64);
                        }
                        pending.push_back(clock.frame_counter());
                    }
                    texture_geometry.set(Some((width, height)));
                }
                Err(error) => {
                    // Retry even if the next publication has identical cells.
                    texture_geometry.set(None);
                    log_dbg(&format!("direct glyph atlas error: {error:?}"));
                }
            }
        }
        if show_fps && update.new_frame {
            // Count consumed publications, including identical ones, so skipping
            // redundant uploads does not make the overlay report slower pacing.
            let mut meter = fps_meter.borrow_mut();
            meter.1 += 1;
            let elapsed = meter.0.elapsed();
            if elapsed >= Duration::from_millis(900) {
                let fps = meter.1 as f64 / elapsed.as_secs_f64();
                fps_label.set_markup(&format!(
                    "<span foreground='#eeeeee' background='#000000' font_family='monospace'>FPS {fps:.0}</span>"
                ));
                *meter = (std::time::Instant::now(), 0);
            }
        }
        gtk4::glib::ControlFlow::Continue
    };

    if let (Some(wakeup), Some(clock)) = (&publication_wakeup, &publication_clock) {
        let update_cleanup = cleanup.clone();
        let signal = clock.connect_update(move |clock| {
            if render_update(clock) == gtk4::glib::ControlFlow::Break {
                update_cleanup();
            }
        });
        *wake_signal.borrow_mut() = Some((clock.clone(), signal));
        let source_clock = clock.clone();
        let source_wakeup = wakeup.clone();
        let source_id = wake_source.clone();
        let source_cleanup = cleanup.clone();
        let source = gtk4::glib::source::unix_fd_add_local(
            wakeup.fd(),
            gtk4::glib::IOCondition::IN | gtk4::glib::IOCondition::ERR
                | gtk4::glib::IOCondition::HUP | gtk4::glib::IOCondition::NVAL,
            move |_, condition| {
                let bad_fd = condition.intersects(gtk4::glib::IOCondition::ERR
                    | gtk4::glib::IOCondition::HUP | gtk4::glib::IOCondition::NVAL);
                if bad_fd || !source_wakeup.is_active() {
                    // Returning Break removes the currently dispatching source;
                    // take its id first so cleanup cannot remove it twice.
                    source_id.borrow_mut().take();
                    if bad_fd || source_wakeup.failed() { log_dbg("publication wakeup failed; stopping direct renderer"); }
                    source_cleanup();
                    return gtk4::glib::ControlFlow::Break;
                }
                match source_wakeup.drain() {
                    Ok(Some(_)) => source_clock.request_phase(gdk::FrameClockPhase::UPDATE),
                    Ok(None) => {},
                    Err(error) => {
                        source_id.borrow_mut().take();
                        log_dbg(&format!("publication wakeup read failed; stopping direct renderer: {error}"));
                        source_cleanup();
                        return gtk4::glib::ControlFlow::Break;
                    }
                }
                gtk4::glib::ControlFlow::Continue
            },
        );
        *wake_source.borrow_mut() = Some(source);
        clock.request_phase(gdk::FrameClockPhase::UPDATE); // Initial geometry.
        log_dbg("direct pacing=publication-eventfd");
    } else {
        picture.add_tick_callback(move |_, clock| render_update(clock));
    }

    let cfg = cfg.clone();
    let effect = effect.to_string();
    let render_effect = effect.clone();
    let render_stop = stop.clone();
    occlusion::start();
    let render_monitor = monitor.connector().map(|c| c.to_string());
    thread::Builder::new()
        .name(format!("ttfx-direct-{effect}"))
        .spawn(move || {
            occlusion::set_current(render_monitor);
            frame_wakeup::set_current(publication_wakeup);
            frame_cadence::set_current(cadence);
            let failure_stop = render_stop.clone();
            if let Err(error) = run_direct_render(
                &cfg,
                &render_effect,
                cols,
                rows,
                show_intro,
                cell_aspect,
                snapshot,
                generation,
                render_stop,
            ) {
                log_dbg(&format!("direct renderer error: {error:?}"));
                failure_stop.store(true, Ordering::Relaxed);
            }
            frame_cadence::set_current(None);
            frame_wakeup::set_current(None);
        })
        .expect("spawn direct renderer");
    let close_cleanup = cleanup.clone();
    window.connect_close_request(move |_| {
        close_cleanup();
        gtk4::glib::Propagation::Proceed
    });
    window.connect_destroy(move |_| cleanup());
    log_dbg(&format!(
        "direct raster requested={requested_raster} effective={effective_raster} damage_requested={requested_damage} damage_effective={effective_damage} effect={effect}"
    ));
    log_dbg(&format!("direct renderer spawned: {effect} {cols}x{rows}"));
    window.present();
    window
}

// GPU backend: the effect runs on the same producer thread as the direct
// backend, but the background surface is our own Wayland layer drawn with GLES
// (see wl_layer.rs), so each frame reports only the changed rectangles to the
// compositor. GTK keeps an unmapped window as the handle rebuild_layers closes.
fn spawn_gpu_wayland_layer_for_monitor(
    monitor: &gdk::Monitor,
    cfg: &Config,
    effect: &str,
    show_intro: bool,
) -> gtk4::ApplicationWindow {
    use std::sync::atomic::{AtomicBool, AtomicU64};
    use std::sync::Mutex;

    let window = gtk4::ApplicationWindow::builder().title("ttfx-bg-wayland").build();
    let geo = monitor.geometry();
    let scale = monitor.scale_factor().max(1) as f64;
    let cell_px = (9.0 / scale * cfg.resolution.clamp(1, 8) as f64).max(3.0);
    let font = gtk4::gdk::pango::FontDescription::from_string(&format!("monospace {cell_px:.0}px"));
    let font_size = font.size() as f64 / gtk4::pango::SCALE as f64;
    let metrics = window.pango_context().metrics(Some(&font), None);
    let char_w = metrics.approximate_char_width() as f64 / gtk4::pango::SCALE as f64;
    let ascent = metrics.ascent() as f64 / gtk4::pango::SCALE as f64;
    let descent = metrics.descent() as f64 / gtk4::pango::SCALE as f64;
    let cw = if char_w > 0.0 { char_w } else { font_size * 0.6 };
    let ch = if ascent + descent > 0.0 { ascent + descent } else { font_size * 1.2 };
    let (cols, rows) = measured_grid(geo.width(), geo.height(), cw, ch);
    let cell_aspect = if cw > 0.0 { (ch / cw) as f32 } else { 2.0 };

    let snapshot = Arc::new(Mutex::new(Vec::<DirectCell>::new()));
    let generation = Arc::new(AtomicU64::new(0));
    let stop = Arc::new(AtomicBool::new(false));
    let wakeup = match frame_wakeup::FrameWakeup::new(stop.clone()) {
        Ok(wakeup) => Arc::new(wakeup),
        Err(error) => {
            log_dbg(&format!("gpu layer: wakeup unavailable ({error}); using the direct renderer"));
            return spawn_direct_layer_for_monitor(monitor, cfg, effect, show_intro);
        }
    };

    let spec = wl_layer::LayerSpec {
        output: monitor.connector().map(|c| c.to_string()),
        namespace: "ttfx-bg-gpu".into(),
        overlay: false,
        keyboard_exclusive: false,
        cols,
        rows,
        cell_px,
        opaque: !cfg.transparent_background,
        show_fps: cfg.show_fps,
    };
    {
        let snapshot = snapshot.clone();
        let generation = generation.clone();
        let stop = stop.clone();
        let wakeup = wakeup.clone();
        thread::Builder::new()
            .name("ttfx-wayland".into())
            .spawn(move || {
                if let Err(error) = wl_layer::run(spec, snapshot, generation, stop.clone(), wakeup) {
                    log_dbg(&format!("wayland layer error: {error:#}"));
                }
                stop.store(true, Ordering::Release);
            })
            .expect("spawn wayland layer");
    }

    let cfg = cfg.clone();
    let render_effect = effect.to_string();
    let render_stop = stop.clone();
    occlusion::start();
    let render_monitor = monitor.connector().map(|c| c.to_string());
    thread::Builder::new()
        .name(format!("ttfx-gpu-{effect}"))
        .spawn(move || {
            occlusion::set_current(render_monitor);
            frame_wakeup::set_current(Some(wakeup));
            let failure_stop = render_stop.clone();
            if let Err(error) = run_direct_render(
                &cfg, &render_effect, cols, rows, show_intro, cell_aspect,
                snapshot, generation, render_stop,
            ) {
                log_dbg(&format!("gpu renderer error: {error:?}"));
                failure_stop.store(true, Ordering::Relaxed);
            }
            frame_wakeup::set_current(None);
        })
        .expect("spawn gpu renderer");
    unsafe { window.set_data(LAYER_STOP_KEY, stop.clone()) };
    let close_stop = stop.clone();
    window.connect_close_request(move |_| {
        close_stop.store(true, Ordering::Release);
        gtk4::glib::Propagation::Proceed
    });
    window.connect_destroy(move |_| stop.store(true, Ordering::Release));
    log_dbg(&format!("gpu wayland renderer spawned: {effect} {cols}x{rows} max_fps={:?}", max_fps()));
    window
}

// ---------------------------------------------------------------------------
// Renderer child process
// ---------------------------------------------------------------------------

// Shared audio state fed by the parec capture thread. Per-band spectrum (like
// the node@192.168.1.177 analyzer_light.py that reacted correctly): Goertzel
// per band with adaptive rolling-peak normalization, plus volume + beat.
const NBANDS: usize = 24;
const SAMPLE_RATE: f32 = 24000.0;
const CHUNK: usize = 1024; // samples per analysis frame (~43ms)

#[derive(Clone)]
struct AudioState {
    bands: Arc<Vec<AtomicU32>>, // per-band energy 0..1 (f32 bits)
    volume: Arc<AtomicU32>,     // overall volume 0..1
    beat: Arc<AtomicU32>,       // 1 shortly after a beat
    bpm: Arc<AtomicU32>,        // smoothed tempo estimate; zero until known
}

impl AudioState {
    fn start(enabled: bool) -> Self {
        let st = AudioState {
            bands: Arc::new(
                (0..NBANDS)
                    .map(|_| AtomicU32::new(0f32.to_bits()))
                    .collect(),
            ),
            volume: Arc::new(AtomicU32::new(0f32.to_bits())),
            beat: Arc::new(AtomicU32::new(0)),
            bpm: Arc::new(AtomicU32::new(0)),
        };
        if enabled {
            let s = st.clone();
            thread::Builder::new()
                .name("ttfx-audio".into())
                .spawn(move || audio_capture_loop(s))
                .expect("spawn audio analyzer");
        }
        st
    }
    fn capture_orphaned(&self) -> bool {
        // The capture loop owns one clone; the renderer owns the other. Stop
        // the loop after a direct layer is closed and its renderer exits.
        Arc::strong_count(&self.bands) == 1
    }
    fn volume(&self) -> f32 {
        f32::from_bits(self.volume.load(Ordering::Relaxed))
    }
    fn beat(&self) -> bool {
        self.beat.load(Ordering::Relaxed) != 0
    }
    fn bpm(&self) -> u32 {
        self.bpm.load(Ordering::Relaxed)
    }
    fn treble(&self) -> f32 {
        let n = NBANDS / 3;
        (NBANDS - n..NBANDS)
            .map(|b| f32::from_bits(self.bands[b].load(Ordering::Relaxed)))
            .sum::<f32>()
            / n as f32
    }
    // Band energy for screen column `c` out of `cols` (rain-equalizer mapping).
    fn band_at(&self, c: usize, cols: usize) -> f32 {
        let b = c * NBANDS / cols.max(1);
        f32::from_bits(self.bands[b.min(NBANDS - 1)].load(Ordering::Relaxed))
    }
}

// Capture the default sink monitor via parec and keep per-band energies.
// Log-spaced bands 60Hz..10kHz, rolling-peak auto-normalization (quiet audio
// still moves), fast-attack/slow-decay per band so it follows without jitter.
fn audio_capture_loop(st: AudioState) {
    const _: () = assert!(NBANDS == audio_spectrum::BANDS);
    const _: () = assert!(CHUNK == audio_spectrum::SAMPLES);
    const _: () = assert!(SAMPLE_RATE == audio_spectrum::RATE);
    let spectrum = audio_spectrum::Spectrum::default();
    loop {
        if st.capture_orphaned() {
            return;
        }
        let sink = std::process::Command::new("pactl")
            .args(["get-default-sink"])
            .output()
            .ok()
            .and_then(|o| String::from_utf8(o.stdout).ok())
            .map(|s| s.trim().to_string());
        let sink = match sink {
            Some(s) if !s.is_empty() => s,
            _ => {
                thread::sleep(Duration::from_secs(2));
                continue;
            }
        };
        let monitor = format!("{sink}.monitor");
        let child = std::process::Command::new("parec")
            .args([
                "--device",
                &monitor,
                "--format=s16le",
                "--rate",
                &format!("{}", SAMPLE_RATE as u32),
                "--channels=1",
                "--latency-msec=40",
            ])
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::null())
            .spawn();
        let mut child = match child {
            Ok(c) => c,
            Err(_) => {
                thread::sleep(Duration::from_secs(2));
                continue;
            }
        };
        let mut out = child.stdout.take().unwrap();
        let mut buf = vec![0u8; CHUNK * 2];
        let mut samples = [0f32; CHUNK];
        let mut peak = 1e-6f32;
        let mut prev_rms = 0f32;
        let mut beat_hold = 0u32;
        let mut last_onset: Option<std::time::Instant> = None;
        loop {
            if st.capture_orphaned() {
                break;
            }
            match out.read_exact(&mut buf) {
                Ok(()) => {
                    for i in 0..CHUNK {
                        samples[i] =
                            i16::from_le_bytes([buf[i * 2], buf[i * 2 + 1]]) as f32 / 32768.0;
                    }
                    let analyzed = spectrum.analyze(&samples);
                    let rms = analyzed.rms;
                    let mags = analyzed.magnitudes;
                    let frame_max = analyzed.maximum;
                    // rolling-peak adaptive normalization
                    peak = (peak * 0.995).max(frame_max).max(1e-6);
                    for (bi, &m) in mags.iter().enumerate() {
                        let norm = (m / peak).sqrt().min(1.0);
                        st.bands[bi].store(norm.to_bits(), Ordering::Relaxed);
                    }
                    // volume + beat
                    st.volume
                        .store((rms * 4.0).min(1.0).to_bits(), Ordering::Relaxed);
                    let onset = rms > prev_rms * 1.35 && rms > 0.02;
                    let beat = if onset {
                        beat_hold = 3;
                        1
                    } else if beat_hold > 0 {
                        beat_hold -= 1;
                        1
                    } else {
                        0
                    };
                    st.beat.store(beat, Ordering::Relaxed);
                    if onset {
                        let now = std::time::Instant::now();
                        if let Some(prev) = last_onset {
                            let secs = now.duration_since(prev).as_secs_f32();
                            if secs > 0.2 {
                                let raw = (60.0 / secs).clamp(40.0, 240.0);
                                let old = st.bpm.load(Ordering::Relaxed) as f32;
                                let next = if old == 0.0 {
                                    raw
                                } else {
                                    old * 0.7 + raw * 0.3
                                };
                                st.bpm.store(next.round() as u32, Ordering::Relaxed);
                            }
                        }
                        last_onset = Some(now);
                    }
                    prev_rms = rms;
                }
                Err(_) => break,
            }
        }
        let _ = child.kill();
        let _ = child.wait();
        if st.capture_orphaned() {
            return;
        }
        thread::sleep(Duration::from_secs(1));
    }
}

// Bitset of nonblank direct cells. Scanning words and their set bits avoids
// walking every empty cell in the sparse graphical backend on each frame.
struct DirectOccupancy {
    cols: usize,
    rows: usize,
    words: Vec<u64>,
}

impl DirectOccupancy {
    fn new(cols: usize, rows: usize) -> Self {
        Self {
            cols,
            rows,
            words: vec![0; cols.saturating_mul(rows).div_ceil(64)],
        }
    }
    fn set(&mut self, x: usize, y: usize) {
        if x < self.cols && y < self.rows {
            let index = y * self.cols + x;
            self.words[index / 64] |= 1u64 << (index % 64);
        }
    }
    fn unset(&mut self, x: usize, y: usize) {
        if x < self.cols && y < self.rows {
            let index = y * self.cols + x;
            self.words[index / 64] &= !(1u64 << (index % 64));
        }
    }
    #[cfg(test)]
    fn clear(&mut self) {
        self.words.fill(0);
    }
    fn iter(&self) -> impl Iterator<Item = (usize, usize)> + '_ {
        self.words
            .iter()
            .enumerate()
            .flat_map(move |(word_index, &word)| {
                let mut bits = word;
                std::iter::from_fn(move || {
                    if bits == 0 {
                        return None;
                    }
                    let index = word_index * 64 + bits.trailing_zeros() as usize;
                    bits &= bits - 1;
                    Some((index % self.cols, index / self.cols))
                })
            })
    }
}

// Frame writer: alt-screen + hidden cursor, home each frame, and crucially NO
// trailing newline on the last row (a trailing newline scrolls the terminal
// and was the source of the visible flicker).
struct Screen {
    cols: usize,
    rows: usize,
    out: String,
    grid: Vec<Vec<char>>,
    color: Vec<Vec<String>>, // per-cell ANSI color string (empty = default)
    dirty: Vec<Vec<bool>>,   // cells painted this frame (need clearing next frame)
    show_fps: bool,
    fps_frames: u32,
    fps_since: std::time::Instant,
    fps_value: f32,
    // Headless mode (preview daemon): present() serializes the grid into a
    // shared snapshot instead of printing ANSI to stdout, and maybe_resize()
    // reports a fake resize when the daemon asks the effect to stop.
    headless: bool,
    snapshot: Option<std::sync::Arc<std::sync::Mutex<String>>>,
    direct_snapshot: Option<std::sync::Arc<std::sync::Mutex<Vec<DirectCell>>>>,
    // Reused by the producer; copying into the slot retains both Vec
    // allocations across frames instead of allocating a fresh publish Vec.
    direct_cells: Option<Vec<DirectCell>>,
    native_swap: bool,
    // Direct catalog frames decode SGR once per run, then retain typed RGB at
    // the cell. Native effects still use their existing ANSI-string storage.
    direct_colors: Option<Vec<Vec<Option<(u8, u8, u8)>>>>,
    occupied: Option<DirectOccupancy>,
    snapshot_gen: Option<std::sync::Arc<std::sync::atomic::AtomicU64>>,
    stop_flag: Option<std::sync::Arc<std::sync::atomic::AtomicBool>>,
}

impl Screen {
    fn new(cols: usize, rows: usize, show_fps: bool) -> Self {
        print!("\x1b[?1049h\x1b[?25l\x1b[2J\x1b[H");
        std::io::stdout().flush().ok();
        Screen {
            cols,
            rows,
            out: String::with_capacity(cols * rows * 8),
            grid: vec![vec![' '; cols]; rows],
            color: vec![vec![String::new(); cols]; rows],
            dirty: vec![vec![false; cols]; rows],
            show_fps,
            fps_frames: 0,
            fps_since: std::time::Instant::now(),
            fps_value: 0.0,
            headless: false,
            snapshot: None,
            direct_snapshot: None,
            direct_cells: None,
            native_swap: false,
            direct_colors: None,
            occupied: None,
            snapshot_gen: None,
            stop_flag: None,
        }
    }
    // Headless grid for the preview daemon: no alt-screen, no cursor codes,
    // no stdout writes — present() publishes JSON cells to the snapshot.
    fn new_headless(
        cols: usize,
        rows: usize,
        snapshot: std::sync::Arc<std::sync::Mutex<String>>,
        gen: std::sync::Arc<std::sync::atomic::AtomicU64>,
        stop: std::sync::Arc<std::sync::atomic::AtomicBool>,
    ) -> Self {
        let mut scr = Screen::new_quiet(cols, rows);
        scr.headless = true;
        scr.snapshot = Some(snapshot);
        scr.snapshot_gen = Some(gen);
        scr.stop_flag = Some(stop);
        scr
    }
    fn new_direct(
        cols: usize,
        rows: usize,
        snapshot: Arc<std::sync::Mutex<Vec<DirectCell>>>,
        gen: Arc<std::sync::atomic::AtomicU64>,
        stop: Arc<std::sync::atomic::AtomicBool>,
    ) -> Self {
        let mut scr = Screen::new_quiet(cols, rows);
        scr.headless = true;
        scr.direct_snapshot = Some(snapshot);
        scr.direct_cells = Some(Vec::new());
        scr.native_swap = std::env::var("TTFX_BG_NATIVE_SWAP").as_deref()==Ok("1");
        scr.direct_colors = Some(vec![vec![None; cols]; rows]);
        scr.occupied = Some(DirectOccupancy::new(cols, rows));
        scr.snapshot_gen = Some(gen);
        scr.stop_flag = Some(stop);
        scr
    }
    // Screen::new prints the alt-screen prologue; headless grids must not.
    fn new_quiet(cols: usize, rows: usize) -> Self {
        Screen {
            cols,
            rows,
            out: String::new(),
            grid: vec![vec![' '; cols]; rows],
            color: vec![vec![String::new(); cols]; rows],
            dirty: vec![vec![false; cols]; rows],
            show_fps: false,
            fps_frames: 0,
            fps_since: std::time::Instant::now(),
            fps_value: 0.0,
            headless: false,
            snapshot: None,
            direct_snapshot: None,
            direct_cells: None,
            native_swap: false,
            direct_colors: None,
            occupied: None,
            snapshot_gen: None,
            stop_flag: None,
        }
    }
    // Serialize painted cells as the preview protocol's "x,y,ch,#rrggbb;…".
    fn dump_cells(&self) -> String {
        let mut out = String::with_capacity(self.cols * self.rows * 6);
        for y in 0..self.rows {
            for x in 0..self.cols {
                let ch = self.grid[y][x];
                if ch == ' ' {
                    continue;
                }
                let Some((r, g, b)) = ansi_sgr_to_rgb(&self.color[y][x]) else {
                    continue;
                };
                out.push_str(&format!("{x},{y},"));
                json_push_escaped_char(&mut out, ch);
                out.push_str(&format!(",#{r:02x}{g:02x}{b:02x};"));
            }
        }
        out
    }
    // Call once per frame; draw the FPS readout at bottom-left, clear of the
    // persistent Omarchy bar that covers the top terminal rows.
    // the user enabled it in preferences. `intended_ms` is the frame's intended
    // sleep — feeding it to the auto-degrade watcher lets it detect sustained
    // pacing drops (actual >> intended) and coarsen the grid.
    fn fps_overlay(&mut self, intended_ms: f64) {
        AUTO_DEGRADE.with(|a| a.borrow_mut().tick(intended_ms));
        self.fps_frames += 1;
        let el = self.fps_since.elapsed().as_secs_f32();
        if el >= 0.5 {
            self.fps_value = self.fps_frames as f32 / el;
            self.fps_frames = 0;
            self.fps_since = std::time::Instant::now();
        }
        if !self.show_fps {
            return;
        }
        let text = format!("FPS {:.0}", self.fps_value);
        let row = self.rows.saturating_sub(1);
        for (i, ch) in text.chars().enumerate() {
            if i < self.cols {
                self.put(i, row, ch, "\x1b[37m".to_string());
            }
        }
    }
    // Clear only cells that were painted the previous frame (dirty tracking).
    // Cells NOT painted keep their color — this enables crossfade between themes.
    fn clear_dirty(&mut self) {
        for r in 0..self.rows {
            for c in 0..self.cols {
                if self.dirty[r][c] {
                    self.grid[r][c] = ' ';
                    self.color[r][c].clear();
                    self.dirty[r][c] = false;
                    if let Some(colors) = &mut self.direct_colors {
                        colors[r][c] = None;
                    }
                    if let Some(occupied) = &mut self.occupied {
                        occupied.unset(c, r);
                    }
                }
            }
        }
    }
    fn put(&mut self, x: usize, y: usize, ch: char, color: String) {
        if x < self.cols && y < self.rows {
            self.grid[y][x] = ch;
            self.color[y][x] = color;
            if let Some(colors) = &mut self.direct_colors {
                colors[y][x] = None;
            }
            self.dirty[y][x] = true;
            if let Some(occupied) = &mut self.occupied {
                if ch == ' ' {
                    occupied.unset(x, y);
                } else {
                    occupied.set(x, y);
                }
            }
        }
    }
    fn put_direct_rgb(&mut self, x: usize, y: usize, ch: char, color: Option<(u8, u8, u8)>) {
        if x < self.cols && y < self.rows {
            self.grid[y][x] = ch;
            self.color[y][x].clear();
            if let Some(colors) = &mut self.direct_colors {
                colors[y][x] = color;
            }
            self.dirty[y][x] = true;
            if let Some(occupied) = &mut self.occupied {
                if ch == ' ' {
                    occupied.unset(x, y);
                } else {
                    occupied.set(x, y);
                }
            }
        }
    }
    // Erase a single cell (restore to blank, no color) without dirty tracking.
    fn erase(&mut self, x: usize, y: usize) {
        if x < self.cols && y < self.rows {
            self.grid[y][x] = ' ';
            self.color[y][x].clear();
            if let Some(colors) = &mut self.direct_colors {
                colors[y][x] = None;
            }
            self.dirty[y][x] = false;
            if let Some(occupied) = &mut self.occupied {
                occupied.unset(x, y);
            }
        }
    }
    fn present(&mut self) {
        if let Some(stop) = &self.stop_flag {
            occlusion::wait_while_hidden(stop);
        }
        if let (Some(slot), Some(gen), Some(cells)) = (
            &self.direct_snapshot,
            &self.snapshot_gen,
            &mut self.direct_cells,
        ) {
            cells.clear();
            // Native direct effects retain ANSI strings, so parse each
            // distinct SGR at most once. Catalog frames arrive with typed RGB.
            let mut colors: std::collections::HashMap<&str, Option<(u8, u8, u8)>> =
                std::collections::HashMap::new();
            for (x, y) in self.occupied.as_ref().expect("direct occupancy").iter() {
                let ch = self.grid[y][x];
                let typed = self.direct_colors.as_ref().and_then(|rgb| rgb[y][x]);
                let sgr = self.color[y][x].as_str();
                let color =
                    typed.or_else(|| *colors.entry(sgr).or_insert_with(|| ansi_sgr_to_rgb(sgr)));
                let Some(color) = color else {
                    continue;
                };
                cells.push(DirectCell {
                    x,
                    y,
                    ch,
                    color,
                    background: None,
                    attrs: 0,
                });
            }
            if let Ok(mut current) = slot.lock() {
                if self.stop_flag.as_ref().is_some_and(|stop| stop.load(Ordering::Acquire)) {
                    return;
                }
                native_publication::publish(&mut current, cells, self.native_swap);
                #[cfg(test)]
                native_publication::capture_frame(&current, self.stop_flag.as_deref());
                // Publish the frame and its generation under one lock. The
                // consumer must not observe a newer frame with an older ID.
                let committed = gen.fetch_add(1, Ordering::Release) + 1;
                frame_cadence::record_produced(committed);
                frame_wakeup::notify_current();
            }
            return;
        }
        if self.headless {
            if let (Some(snap), Some(gen)) = (&self.snapshot, &self.snapshot_gen) {
                let cells = self.dump_cells();
                if let Ok(mut s) = snap.lock() {
                    *s = cells;
                }
                gen.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            }
            return;
        }
        self.out.clear();
        self.out.push_str("\x1b[H");
        let mut cur: &str = "";
        for r in 0..self.rows {
            for c in 0..self.cols {
                let col = &self.color[r][c];
                if col.is_empty() {
                    if cur != "" {
                        self.out.push_str("\x1b[0m");
                        cur = "";
                    }
                } else if col != cur {
                    self.out.push_str(col);
                    cur = col;
                }
                self.out.push(self.grid[r][c]);
            }
            if r + 1 < self.rows {
                self.out.push('\n');
            }
        }
        if cur != "" {
            self.out.push_str("\x1b[0m");
        }
        print!("{}", self.out);
        std::io::stdout().flush().ok();
    }
    // Re-read the PTY size; if it changed (window resized / Vte expanded it
    // after spawn), reallocate the grid and report the change so the caller
    // restarts the effect cleanly at the new size.
    fn maybe_resize(&mut self) -> bool {
        if let Some(flag) = &self.stop_flag {
            if flag.load(std::sync::atomic::Ordering::Relaxed) {
                return true; // daemon asked us to stop: exit like a resize
            }
        }
        if let Some((c, r)) = pty_size() {
            if c != self.cols || r != self.rows {
                self.cols = c;
                self.rows = r;
                self.grid = vec![vec![' '; c]; r];
                self.color = vec![vec![String::new(); c]; r];
                if self.direct_colors.is_some() {
                    self.direct_colors = Some(vec![vec![None; c]; r]);
                }
                self.dirty = vec![vec![false; c]; r];
                if self.occupied.is_some() {
                    self.occupied = Some(DirectOccupancy::new(c, r));
                }
                self.out = String::with_capacity(c * r * 8);
                print!("\x1b[2J\x1b[H");
                return true;
            }
        }
        false
    }
}

// Actual size of the PTY we render into. The parent computes cols/rows from
// font metrics, but Vte may round differently — rendering more rows than the
// PTY has caused a visible one-line scroll ("jump") every frame. The child
// queries the real size and clamps to it.
fn pty_size() -> Option<(usize, usize)> {
    unsafe {
        let mut ws: libc::winsize = std::mem::zeroed();
        if libc::ioctl(libc::STDOUT_FILENO, libc::TIOCGWINSZ, &mut ws) == 0
            && ws.ws_col > 0
            && ws.ws_row > 0
        {
            Some((ws.ws_col as usize, ws.ws_row as usize))
        } else {
            None
        }
    }
}

// Wait for Vte to size the PTY to the real widget dimensions (it spawns at 80x24
// and resizes a beat later), returning the settled (cols, rows). Reading too early
// lays a one-shot layout (the intro, a ttfx canvas) out on an 80x24 grid.
fn settle_pty_size(fallback_cols: usize, fallback_rows: usize) -> (usize, usize) {
    let fallback = (fallback_cols, fallback_rows);
    let mut prev = pty_size().unwrap_or(fallback);
    let mut settled = if prev != (80, 24) { prev } else { fallback };
    for _ in 0..150 {
        // up to ~1.5s
        let cur = pty_size().unwrap_or(fallback);
        if cur != (80, 24) && cur == prev {
            settled = cur;
            break;
        }
        prev = cur;
        thread::sleep(Duration::from_millis(10));
    }
    settled
}

// 5-row ASCII bitmap font (FIGlet-style, pure ASCII '#'/space). Each glyph is 5
// rows tall; `intro_size` scales each font pixel into an N×N cell block, so the
// title renders as real ASCII-art letters — not a per-char solid block.
fn glyph_rows(c: char) -> [&'static str; 5] {
    match c {
        'A' => [" ### ", "#   #", "#   #", "#####", "#   #"],
        'B' => ["#### ", "#   #", "#### ", "#   #", "#### "],
        'C' => [" ####", "#    ", "#    ", "#    ", " ####"],
        'D' => ["#### ", "#   #", "#   #", "#   #", "#### "],
        'E' => ["#####", "#    ", "#### ", "#    ", "#####"],
        'F' => ["#####", "#    ", "#### ", "#    ", "#    "],
        'G' => [" ####", "#    ", "#  ##", "#   #", " ####"],
        'H' => ["#   #", "#   #", "#####", "#   #", "#   #"],
        'I' => ["#####", "  #  ", "  #  ", "  #  ", "#####"],
        'J' => ["#####", "   # ", "   # ", "#  # ", " ##  "],
        'K' => ["#   #", "#  # ", "###  ", "#  # ", "#   #"],
        'L' => ["#    ", "#    ", "#    ", "#    ", "#####"],
        'M' => ["#   #", "## ##", "# # #", "#   #", "#   #"],
        'N' => ["#   #", "##  #", "# # #", "#  ##", "#   #"],
        'O' => [" ### ", "#   #", "#   #", "#   #", " ### "],
        'P' => ["#### ", "#   #", "#### ", "#    ", "#    "],
        'Q' => [" ### ", "#   #", "# # #", "#  # ", " ## #"],
        'R' => ["#### ", "#   #", "#### ", "#  # ", "#   #"],
        'S' => [" ####", "#    ", " ### ", "    #", "#### "],
        'T' => ["#####", "  #  ", "  #  ", "  #  ", "  #  "],
        'U' => ["#   #", "#   #", "#   #", "#   #", " ### "],
        'V' => ["#   #", "#   #", "#   #", " # # ", "  #  "],
        'W' => ["#   #", "#   #", "# # #", "## ##", "#   #"],
        'X' => ["#   #", " # # ", "  #  ", " # # ", "#   #"],
        'Y' => ["#   #", " # # ", "  #  ", "  #  ", "  #  "],
        'Z' => ["#####", "   # ", "  #  ", " #   ", "#####"],
        '0' => [" ### ", "#  ##", "# # #", "##  #", " ### "],
        '1' => ["  #  ", " ##  ", "  #  ", "  #  ", "#####"],
        '2' => [" ### ", "#   #", "  ## ", " #   ", "#####"],
        '3' => ["#### ", "    #", " ### ", "    #", "#### "],
        '4' => ["#  # ", "#  # ", "#####", "   # ", "   # "],
        '5' => ["#####", "#    ", "#### ", "    #", "#### "],
        '6' => [" ### ", "#    ", "#### ", "#   #", " ### "],
        '7' => ["#####", "   # ", "  #  ", " #   ", "#    "],
        '8' => [" ### ", "#   #", " ### ", "#   #", " ### "],
        '9' => [" ### ", "#   #", " ####", "    #", " ### "],
        '.' => ["     ", "     ", "     ", "     ", "  #  "],
        ',' => ["     ", "     ", "     ", "  #  ", " #   "],
        '/' => ["    #", "   # ", "  #  ", " #   ", "#    "],
        '@' => [" ### ", "# ###", "# # #", "# ## ", " ### "],
        '-' => ["     ", "     ", " ### ", "     ", "     "],
        ':' => ["     ", "  #  ", "     ", "  #  ", "     "],
        '!' => ["  #  ", "  #  ", "  #  ", "     ", "  #  "],
        '?' => [" ### ", "#   #", "  ## ", "     ", "  #  "],
        '+' => ["     ", "  #  ", " ### ", "  #  ", "     "],
        '_' => ["     ", "     ", "     ", "     ", "#####"],
        '(' => ["   # ", "  #  ", "  #  ", "  #  ", "   # "],
        ')' => ["#    ", " #   ", " #   ", " #   ", "#    "],
        '\'' => ["  #  ", "  #  ", "     ", "     ", "     "],
        _ => ["     ", "     ", "     ", "     ", "     "],
    }
}

// Uppercase + normalize dashes so any byline / effect name maps onto the font.
fn prep(s: &str) -> String {
    s.chars()
        .map(|c| match c {
            '—' | '–' => '-',
            other => other.to_ascii_uppercase(),
        })
        .collect()
}

// Render the first `upto` chars of `text` as scaled ASCII-art rows (each font
// pixel becomes an N×N cell block). All glyphs are 5 rows × 5 cols.
fn art_prefix(text: &str, upto: usize, scale: usize, gap: usize) -> Vec<String> {
    let mut rows = vec![String::new(); 5 * scale];
    for ch in text.chars().take(upto) {
        let g = glyph_rows(ch);
        for r in 0..5 {
            let mut scaled = String::new();
            for c in g[r].chars() {
                for _ in 0..scale {
                    scaled.push(c);
                }
            }
            for sy in 0..scale {
                rows[r * scale + sy].push_str(&scaled);
                for _ in 0..gap {
                    rows[r * scale + sy].push(' ');
                }
            }
        }
    }
    rows
}

// Nominal rendered width of `text` as ASCII art (glyphs are 5 cols + `gap`).
fn art_width(text: &str, scale: usize, gap: usize) -> usize {
    let n = text.chars().count();
    if n == 0 {
        0
    } else {
        n * (5 * scale + gap) - gap
    }
}

// Intro: the title, byline and effect tag ALL render as ASCII art (scaled
// ×1..×3), centered on BOTH axes as a single stack. The title types in letter by
// letter, then the byline, then the effect tag, then it holds.
// Audio-reactive: each letter step pulses to the beat (volume/beat => faster typing).
fn show_intro(
    scr: &mut Screen,
    palette: &[String],
    byline: &str,
    effect: &str,
    intro_size: i64,
    audio: &AudioState,
    intro_beat_sync: bool,
    boot_char_style: &str,
) {
    let scale = intro_size.clamp(1, 3) as usize;
    let gap = scale.max(1); // spaces between ASCII-art letters
    let title = "OMARCHY AUDIO BACKGROUND".to_string();
    let by = prep(if byline.trim().is_empty() {
        DEFAULT_BYLINE
    } else {
        byline.trim()
    });
    let tag = format!("- {} -", prep(effect));

    let bh = 5 * scale; // every block is 5 glyph rows × scale
    let vgap = 1usize; // blank row between blocks

    let title_w = art_width(&title, scale, gap);
    let by_w = art_width(&by, scale, gap);
    let tag_w = art_width(&tag, scale, gap);

    // Center the whole 3-block stack vertically, each block centered horizontally.
    let total_h = 3 * bh + 2 * vgap;
    let ty = scr.rows.saturating_sub(total_h) / 2;
    let by_y = ty + bh + vgap;
    let tag_y = by_y + bh + vgap;
    let tx_title = scr.cols.saturating_sub(title_w) / 2;
    let tx_by = scr.cols.saturating_sub(by_w) / 2;
    let tx_tag = scr.cols.saturating_sub(tag_w) / 2;

    // Draw the first `upto` letters of a block at its (centered) left edge, so the
    // typewriter fills left-to-right and ends centered.
    let draw = |scr: &mut Screen, text: &str, upto: usize, x: usize, y: usize, color: u8| {
        for (r, line) in art_prefix(text, upto, scale, gap).iter().enumerate() {
            for (i, ch) in line.chars().enumerate() {
                if ch != ' ' {
                    scr.put(
                        x + i,
                        y + r,
                        styled_glyph(boot_char_style, ch),
                        tint_to_color(color, palette),
                    );
                }
            }
        }
    };

    let title_len = title.chars().count();
    let by_len = by.chars().count();
    let tag_len = tag.chars().count();

    // Helper: wait for the next audio beat (with timeout fallback if no music).
    let wait_for_beat = |audio: &AudioState, timeout_ms: u64| -> bool {
        let start = std::time::Instant::now();
        loop {
            if audio.beat() {
                return true;
            }
            if start.elapsed() >= Duration::from_millis(timeout_ms) {
                return false;
            }
            thread::sleep(Duration::from_millis(20));
        }
    };

    if intro_beat_sync && audio.volume() > 0.02 {
        // Efectos internos de ttfx que NO usan colores de tema; usar palett...[truncated]
        for step in 0..=title_len {
            scr.clear_dirty();
            draw(scr, &title, step, tx_title, ty, 1);
            scr.fps_overlay(70.0);
            scr.present();
            let beat_sync = wait_for_beat(audio, 1500);
            // If we got a beat, great; otherwise fall back to volume-paced delay.
            if !beat_sync {
                let vol = audio.volume().clamp(0.0, 1.0);
                let speed = 1.0 + vol * 2.5;
                thread::sleep(Duration::from_millis((70.0 / speed).max(12.0) as u64));
            }
        }
        // Phase 2: type the byline under the complete title.
        for step in 0..=by_len {
            scr.clear_dirty();
            draw(scr, &title, title_len, tx_title, ty, 1);
            draw(scr, &by, step, tx_by, by_y, 2);
            scr.fps_overlay(70.0);
            scr.present();
            let beat_sync = wait_for_beat(audio, 1500);
            if !beat_sync {
                let vol = audio.volume().clamp(0.0, 1.0);
                let speed = 1.0 + vol * 2.5;
                thread::sleep(Duration::from_millis((45.0 / speed).max(10.0) as u64));
            }
        }
    } else {
        // Phase 1: type the title, one ASCII-art letter at a time.
        for step in 0..=title_len {
            scr.clear_dirty();
            draw(scr, &title, step, tx_title, ty, 1);
            scr.fps_overlay(70.0);
            scr.present();
            let vol = audio.volume().clamp(0.0, 1.0);
            let speed = 1.0 + vol * 2.5 + if audio.beat() { 1.0 } else { 0.0 };
            thread::sleep(Duration::from_millis((70.0 / speed).max(12.0) as u64));
        }
        // Phase 2: type the byline under the complete title.
        for step in 0..=by_len {
            scr.clear_dirty();
            draw(scr, &title, title_len, tx_title, ty, 1);
            draw(scr, &by, step, tx_by, by_y, 2);
            scr.fps_overlay(45.0);
            scr.present();
            let vol = audio.volume().clamp(0.0, 1.0);
            let speed = 1.0 + vol * 2.5 + if audio.beat() { 1.0 } else { 0.0 };
            thread::sleep(Duration::from_millis((45.0 / speed).max(10.0) as u64));
        }
    }

    // Phase 3: stamp the effect tag, then hold the finished splash.
    scr.clear_dirty();
    draw(scr, &title, title_len, tx_title, ty, 1);
    draw(scr, &by, by_len, tx_by, by_y, 2);
    draw(scr, &tag, tag_len, tx_tag, tag_y, 3);
    scr.fps_overlay(2600.0);
    scr.present();
    // Hold pulses with audio: beat shortens hold, quiet holds full 2600ms.
    let hold_base = 2600.0;
    // If audio active, let the hold be slightly shorter on loud sections so boot feels synced.
    let vol = audio.volume().clamp(0.0, 1.0);
    let hold_speed = 1.0 + vol * 0.8 + if audio.beat() { 0.5 } else { 0.0 };
    thread::sleep(Duration::from_millis(
        (hold_base / hold_speed).max(900.0) as u64
    ));
}

// ttfx effects we route to the vendored engine (everything in the catalog EXCEPT
// matrix/rain, which we keep hand-rolled because ours are audio-reactive).
const TTFX_EFFECTS: [&str; 35] = [
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

fn is_ttfx_effect(name: &str) -> bool {
    TTFX_EFFECTS.contains(&name)
}

// Any effect the renderer can actually run: our hand-rolled set or the ttfx catalog.
fn is_valid_effect(name: &str) -> bool {
    DEFAULT_EFFECTS.contains(&name) || is_ttfx_effect(name)
}

// Drive a vendored ttfx effect on our Vte PTY, looping so it runs as a continuous
// background (ttfx effects settle when done; rebuild and replay). Handles PTY resize
// by rebuilding at the new size. Runs after our ASCII intro (same stdout).
// The official wordmark is a bitmap; every other input remains ordinary text.
#[cfg(test)]
fn ttfx_canvas_input(text: &str, cols: usize, rows: usize, char_style: &str) -> String {
    wordmark::canvas_input(text, cols, rows, char_style, 2.0)
}

type ThemeRgb = (u8, u8, u8);

fn gradient_rgb_set(stops: &[&str], steps: i64) -> std::collections::HashSet<ThemeRgb> {
    use ttfx::utils::graphics::{Color, Gradient};
    let colors = stops
        .iter()
        .map(|hex| Color::from_hex(hex).expect("valid effect color"))
        .collect::<Vec<_>>();
    Gradient::with_steps(&colors, steps, false)
        .expect("valid effect gradient")
        .spectrum
        .iter()
        .filter_map(|color| parse_hex_color(&color.rgb_color.to_string()))
        .collect()
}

fn theme_palette_mapper(theme: &[(String, String)]) -> impl Fn(u8, u8, u8) -> ThemeRgb + use<> {
    let get = |key: &str, fallback: ThemeRgb| {
        theme
            .iter()
            .find(|(name, _)| name == key)
            .and_then(|(_, value)| parse_hex_color(value))
            .unwrap_or(fallback)
    };
    let accent = get("accent", (128, 160, 255));
    let foreground = get("foreground", accent);
    let wheel = [
        get("red", accent),
        get("yellow", accent),
        get("green", accent),
        get("cyan", accent),
        get("blue", accent),
        get("magenta", accent),
        get("red", accent),
    ];
    move |r, g, b| {
        if (r, g, b) == (0, 0, 0) {
            return (0, 0, 0);
        }
        let rf = r as f32 / 255.0;
        let gf = g as f32 / 255.0;
        let bf = b as f32 / 255.0;
        let max = rf.max(gf).max(bf);
        let min = rf.min(gf).min(bf);
        let delta = max - min;
        let target = if delta / max.max(0.001) < 0.12 {
            foreground
        } else {
            let hue = if max == rf {
                ((gf - bf) / delta).rem_euclid(6.0)
            } else if max == gf {
                (bf - rf) / delta + 2.0
            } else {
                (rf - gf) / delta + 4.0
            };
            let index = hue.floor() as usize;
            let fraction = hue - index as f32;
            let a = wheel[index];
            let z = wheel[index + 1];
            (
                ((a.0 as f32 + (z.0 as f32 - a.0 as f32) * fraction).round()) as u8,
                ((a.1 as f32 + (z.1 as f32 - a.1 as f32) * fraction).round()) as u8,
                ((a.2 as f32 + (z.2 as f32 - a.2 as f32) * fraction).round()) as u8,
            )
        };
        (
            (target.0 as f32 * max).round() as u8,
            (target.1 as f32 * max).round() as u8,
            (target.2 as f32 * max).round() as u8,
        )
    }
}

fn final_accent_set(effect: &str) -> std::collections::HashSet<ThemeRgb> {
    let (stops, steps): (&[&str], i64) = match effect {
        "binarypath" => (&["00d500", "007500"], 12),
        "blackhole" => (&["8A008A", "00D1FF", "ffffff"], 9),
        "bubbles" => (&["d33aff", "02ff7f"], 12),
        "crumble" => (&["5CE1FF", "FF8C00"], 12),
        "decrypt" => (&["eda000"], 12),
        "errorcorrect" | "fireworks" | "smoke" | "thunderstorm" | "unstable" => {
            (&["8A008A", "00D1FF", "FFFFFF"], 12)
        }
        "laseretch" | "sweep" => (&["8A008A", "00D1FF", "ffffff"], 8),
        "swarm" => (&["31b900", "f0ff65"], 12),
        "synthgrid" => (&["8a008a", "00d1ff", "ffffff"], 12),
        "vhstape" => (&["ab48ff", "e7b2b2", "fffebd"], 12),
        _ => return std::collections::HashSet::new(),
    };
    gradient_rgb_set(stops, steps)
}

fn ttfx_theme_transform(
    effect_name: &str,
    theme: &[(String, String)],
) -> Option<ttfx::utils::ansi::ColorTransform> {
    // A missing theme file is not a theme. Leaving the transform disabled keeps
    // TTFX's built-in palette intact instead of applying fallback accent colors.
    if theme.is_empty() {
        return None;
    }
    let map = theme_palette_mapper(theme);
    match effect_name {
        // These were deliberately left unthemed when the initial catalog landed,
        // which made Colorshift and Waves ignore the user's theme entirely. They
        // are abstract effects, so their colors are decorative rather than semantic:
        // map their full spectrum like the rest of the abstract catalog.
        "colorshift" | "waves" => Some(std::rc::Rc::new(map)),
        // Physical/semantic effects: map only the unambiguous final-text spectrum.
        "binarypath" | "blackhole" | "bubbles" | "crumble" | "decrypt" | "errorcorrect"
        | "fireworks" | "laseretch" | "smoke" | "swarm" | "sweep" | "synthgrid"
        | "thunderstorm" | "unstable" | "vhstape" => {
            let accents = final_accent_set(effect_name);
            Some(std::rc::Rc::new(move |r, g, b| {
                if accents.contains(&(r, g, b)) {
                    map(r, g, b)
                } else {
                    (r, g, b)
                }
            }))
        }
        "burn" => {
            // Product decision: preserve the complete fire/ember spectrum. Starting,
            // final and smoke accents may follow the host palette.
            let fire = gradient_rgb_set(&["ffffff", "fff75d", "fe650d", "8A003C", "510100"], 10);
            Some(std::rc::Rc::new(move |r, g, b| {
                if fire.contains(&(r, g, b)) {
                    (r, g, b)
                } else {
                    map(r, g, b)
                }
            }))
        }
        // Abstract/motion effects have no physically meaningful hue: all authored
        // colors are decorative accents, mapped across the full theme color wheel.
        _ if is_ttfx_effect(effect_name) => Some(std::rc::Rc::new(map)),
        _ => None,
    }
}

// Drive a vendored ttfx effect on our Vte PTY, audio-reactively. The effect runs on
// a VIRTUAL clock and we pace the frames by the live audio level — loud music
// advances the effect faster, quiet slows it — so the whole ttfx catalog reacts to
// the music like the hand-rolled effects do. Loops so it runs as a continuous
// background; rebuilds on PTY resize. Runs after our ASCII intro (same stdout).
fn run_ttfx(
    effect_name: &str,
    cols: usize,
    rows: usize,
    ttfx_text: &str,
    audio: &AudioState,
    audio_enabled: bool,
    intensity: i64,
    speed: i64,
    reactivity: i64,
    use_theme_colors: bool,
    char_style: &str,
    show_fps: bool,
    cell_aspect: f32,
) -> Result<()> {
    log_dbg(&format!("run_ttfx enter: effect={effect_name} {cols}x{rows} ttfx_text={ttfx_text} reactivity={reactivity} audio_enabled={audio_enabled}"));
    use clap::Parser;
    use std::io::Write;
    use ttfx::engine::ctx::{Clock, EngineCtx};
    use ttfx::engine::terminal::TerminalConfig;
    use ttfx::utils::rng::Rng;

    let (mut cols, mut rows) = settle_pty_size(cols, rows);
    let fps = 60i64;
    let frame_secs = 1.0 / fps as f64;
    // reactivity is live-adjustable: the frame loop re-reads state.json so a slider
    // change applies WITHOUT restarting the background (no respawn, no intro replay).
    let mut reactivity = reactivity;
    let mut frames = 0u32;
    let mut fps_frames = 0u32;
    let mut fps_since = std::time::Instant::now();
    let mut fps_value = 0.0f32;
    let mut theme_watcher = ThemeWatcher::new();
    loop {
        // Build the effect with default config via the clap parser (like upstream's
        // --random-effect), then drive it ourselves so we can pace it by audio.
        let mut effect = match ttfx::cli::Cli::try_parse_from(["ttfx", effect_name]) {
            Ok(ttfx::cli::Cli {
                effect: Some(e), ..
            }) => e.build_effect(),
            _ => {
                let m = format!("unknown ttfx effect: {effect_name}");
                eprintln!("{m}");
                log_dbg(&m);
                return Ok(());
            }
        };
        // Match the official pixel proportions to the actual terminal cells.
        let input = wordmark::canvas_input(ttfx_text, cols, rows, char_style, cell_aspect);
        let mut config = TerminalConfig::default();
        config.canvas_width = cols as i64;
        config.canvas_height = rows as i64;
        config.frame_rate = fps;
        let mut ctx = match EngineCtx::new(
            &input,
            config,
            Rng::from_entropy(),
            Clock::virtual_with_frame_rate(fps),
        ) {
            Ok(c) => c,
            Err(e) => {
                let m = format!("ttfx ctx error for {effect_name}: {e:?}");
                eprintln!("{m}");
                log_dbg(&m);
                return Ok(());
            }
        };
        ctx.final_text_bands = true;
        if let Err(e) = effect.build(&mut ctx) {
            let m = format!("ttfx build error for {effect_name}: {e:?}");
            eprintln!("{m}");
            log_dbg(&m);
            return Ok(());
        }

        // Audio-paced frame loop: with a virtual clock each next_frame() advances the
        // animation by one tick, so pacing the calls by the live audio level makes the
        // effect speed up on loud passages / beats and slow down when quiet.
        let stdout = std::io::stdout();
        let mut out = stdout.lock();
        if ctx.terminal.prep_canvas(&mut out).is_err() {
            return Ok(());
        }
        loop {
            // Audio-reactive color: bass -> hue shift, volume -> brightness. Same
            // reactivity slider that drives speed: 0=off, 3=triple. Applied in the
            // global sgr_color hook so every ttfx effect recolors without per-effect
            // patches. This is the bridge (vendored engine) - no upstream PR needed.
            {
                let vol = audio.volume().clamp(0.0, 1.0);
                let bass =
                    (audio.band_at(0, NBANDS) + audio.band_at(1, NBANDS) * 0.5).clamp(0.0, 1.0);
                let active = audio_enabled && reactivity > 0 && (vol > 0.02 || bass > 0.05);
                if active {
                    // Brightness 1.0..1.5 (reactivity 3 = stronger), hue up to ~60 deg from bass + vol.
                    let bright = 1.0 + vol * 0.45 * (reactivity as f32 / 2.0);
                    let hue_deg = if effect_name == "burn" {
                        0.0 // fire hue is semantic; audio may brighten it, never recolor it
                    } else {
                        (bass * 45.0 + vol * 18.0) * (reactivity as f32 / 2.0)
                            + if audio.beat() { 10.0 } else { 0.0 }
                    };
                    let rad = hue_deg.to_radians();
                    let (c, s) = (rad.cos(), rad.sin());
                    let t = 1.0 - c;
                    let w1 = 0.57735026; // 1/sqrt(3) for hue rotation around gray axis
                    let m = [
                        c + t / 3.0,
                        t / 3.0 - w1 * s,
                        t / 3.0 + w1 * s,
                        t / 3.0 + w1 * s,
                        c + t / 3.0,
                        t / 3.0 - w1 * s,
                        t / 3.0 - w1 * s,
                        t / 3.0 + w1 * s,
                        c + t / 3.0,
                    ];
                    ttfx::utils::ansi::set_audio_color(true, bright, m);
                } else {
                    ttfx::utils::ansi::set_audio_color(
                        false,
                        1.0,
                        [1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0],
                    );
                }
            }
            // Effect-aware live theme mapping: semantic colors stay intact;
            // only accents selected for this specific effect follow the theme.
            if use_theme_colors {
                if let Some(new_colors) = theme_watcher.changed() {
                    let accent = new_colors
                        .iter()
                        .find(|(key, _)| key == "accent")
                        .map(|(_, value)| value.as_str())
                        .unwrap_or("unknown");
                    ttfx::utils::ansi::set_color_transform(ttfx_theme_transform(
                        effect_name,
                        new_colors,
                    ));
                    log_dbg(&format!(
                        "ttfx theme colors changed: {effect_name} accent={accent}"
                    ));
                }
            } else {
                ttfx::utils::ansi::set_color_transform(None);
            }
            // Audio-reactive per-effect hook (e.g. thunderstorm lightning on loud beats).
            // Runs before next_frame so the effect can inject a strike this tick.
            if audio_enabled && reactivity > 0 {
                let vol = audio.volume().clamp(0.0, 1.0);
                let bass =
                    (audio.band_at(0, NBANDS) + audio.band_at(1, NBANDS) * 0.5).clamp(0.0, 1.0);
                let beat = audio.beat();
                effect.on_audio(&mut ctx, vol, bass, beat);
            }
            // Stop if the PTY went away (effect settled / terminal gone).
            let frame = match effect.next_frame(&mut ctx) {
                Some(f) => f,
                None => break,
            };
            if ctx.terminal.print_frame(&mut out, &frame).is_err() {
                let _ = ctx.terminal.restore_cursor(&mut out, "");
                return Ok(());
            }
            // ttfx renders directly through its own terminal, bypassing Screen. Keep
            // its overlay here so the FPS switch works for every catalog effect too.
            fps_frames += 1;
            let fps_elapsed = fps_since.elapsed().as_secs_f32();
            if fps_elapsed >= 0.5 {
                fps_value = fps_frames as f32 / fps_elapsed;
                fps_frames = 0;
                fps_since = std::time::Instant::now();
            }
            if show_fps {
                if write!(out, "\x1b[{};1H\x1b[97mFPS {:.0}\x1b[0m", rows, fps_value).is_err() {
                    let _ = ctx.terminal.restore_cursor(&mut out, "");
                    return Ok(());
                }
                if out.flush().is_err() {
                    let _ = ctx.terminal.restore_cursor(&mut out, "");
                    return Ok(());
                }
            }
            // Live config poll (~every 45 frames): apply reactivity changes without a
            // restart. Do not compare `live.effect` here: it is the user's selected
            // effect, while the controller can temporarily render another enabled
            // effect during rotation. That comparison killed every rotated ttfx child
            // after 45 frames, leaving a blank/restarting background.
            frames += 1;
            if frames % 45 == 0 {
                let live = read_config();
                reactivity = live.reactivity;
                if !live.running {
                    let _ = ctx.terminal.restore_cursor(&mut out, "");
                    return Ok(());
                }
            }
            // Pace by the live audio level, scaled by the user's reactivity setting.
            // Louder => smaller delay => faster animation, capped at 3x (triple).
            // reactivity 0 disables it; higher reactivity reaches the cap more easily.
            let vol = audio.volume().clamp(0.0, 1.0);
            let boost = vol * reactivity as f32 + if audio.beat() { 0.5 } else { 0.0 };
            let audio_speed = if audio_enabled && reactivity > 0 {
                (1.0 + boost).clamp(1.0, 3.0)
            } else {
                1.0
            };
            // Apply intensity as a live baseline speed control. It is sampled
            // independently of reactivity, so moving this slider never resets
            // a running ttfx scene.
            let intensity_speed = 0.70 + live_intensity(intensity) as f32 * 0.06;
            let baseline =
                Duration::from_secs_f64(frame_secs / (audio_speed * intensity_speed) as f64);
            let intended = speed_delay(baseline, live_speed(speed)).max(MIN_RENDER_FRAME);
            thread::sleep(intended);
            AUTO_DEGRADE.with(|a| a.borrow_mut().tick(intended.as_secs_f64() * 1000.0));
        }
        let _ = ctx.terminal.restore_cursor(&mut out, "");
        // Continuous background: loop all ttfx effects for the full rotate_secs
        // without a gap. The 400ms pause caused the visible "para y vuelve".
        // A selected effect is different from the controller's current rotation
        // item, so only the running flag is safe to inspect here. The controller
        // terminates and replaces this child for a real effect change.
        {
            let live = read_config();
            if !live.running {
                let _ = ctx.terminal.restore_cursor(&mut out, "\n");
                return Ok(());
            }
        }
        // Tiny settle without blanking — next pass rebuilds immediately.
        let (c2, r2) = settle_pty_size(cols, rows);
        if (c2, r2) != (cols, rows) {
            cols = c2;
            rows = r2;
        }
    }
}

fn run_render(
    cfg: &Config,
    effect: &str,
    cols: usize,
    rows: usize,
    intensity: i64,
    speed: i64,
    audio: bool,
    byline: &str,
    intro_size: i64,
    cell_aspect: f32,
    show_fps: bool,
    with_intro: bool,
    ttfx_text: &str,
    reactivity: i64,
    intro_beat_sync: bool,
    enable_degrade: bool,
) -> Result<()> {
    log_dbg(&format!("run_render enter: effect={effect} {cols}x{rows} intensity={intensity} speed={speed} audio={audio} ttfx_text={ttfx_text} reactivity={reactivity} with_intro={with_intro} byline={byline} intro_beat_sync={intro_beat_sync}"));
    let intensity = intensity.clamp(0, 10);
    let state = AudioState::start(audio);
    // Use the REAL PTY size, but WAIT for it to settle first (Vte spawns at 80x24).
    let (cols, rows) = settle_pty_size(cols, rows);
    let mut scr = Screen::new(cols, rows, show_fps);

    let palette: Vec<String> = if cfg.use_theme_colors {
        // Use theme colors when user opts in; fall back gracefully if theme unavailable
        let tc = read_theme_colors();
        if !tc.is_empty() {
            theme_palette(&tc, effect).into_iter().collect()
        } else {
            hardcoded_palette(effect)
        }
    } else {
        hardcoded_palette(effect)
    };
    // Initialize the thread-local palette so effects can read live theme colors
    if cfg.use_theme_colors {
        set_theme_palette(palette.clone());
    }

    set_auto_degrade_enabled(false);
    if with_intro {
        show_intro(
            &mut scr,
            &palette,
            byline,
            effect,
            intro_size,
            &state,
            intro_beat_sync,
            &cfg.boot_char_style,
        );
    }

    // From here on, sustained FPS drops auto-escalate the grid resolution and
    // shrink the boot text (the intro replaying at the new size confirms it).
    // Preview sessions pass enable_degrade=false: several run concurrently
    // against the shared state.json and must never rewrite it.
    if enable_degrade {
        set_auto_degrade_enabled(true);
    }

    // Keep the installed Kuyen ASM/VTE catalog path as the default. The GTK
    // backend is a separate opt-in that changes only the frame transport.
    if is_ttfx_effect(effect) {
        if let Some(result) = run_ttfx_asm_vte(cfg, effect, cols, rows, cell_aspect, &state) {
            return result;
        }
        let _ = run_ttfx(
            effect,
            cols,
            rows,
            ttfx_text,
            &state,
            audio,
            intensity,
            speed,
            reactivity,
            cfg.use_theme_colors,
            &cfg.char_style,
            show_fps,
            cell_aspect,
        );
        return Ok(());
    }

    // Each effect returns Ok(()) when it detects a PTY resize; re-dispatch so
    // it restarts with fresh state at the new size (no jump, no stale grid).
    loop {
        let pal: &[String] = &palette;
        let res = match effect {
            "donut" => fx_donut(
                &mut scr,
                pal,
                intensity,
                &state,
                cell_aspect,
                cfg.use_theme_colors,
                effect,
            ),
            "fire" => fx_fire(
                &mut scr,
                pal,
                intensity,
                &state,
                cfg.use_theme_colors,
                effect,
            ),
            "starfield" => fx_starfield(
                &mut scr,
                pal,
                intensity,
                &state,
                cfg.use_theme_colors,
                effect,
            ),
            "life" => fx_life(
                &mut scr,
                pal,
                intensity,
                &state,
                cfg.use_theme_colors,
                effect,
            ),
            "wave" => fx_wave(
                &mut scr,
                pal,
                intensity,
                &state,
                cfg.use_theme_colors,
                effect,
            ),
            "bars" => fx_bars(
                &mut scr,
                pal,
                intensity,
                &state,
                cfg.use_theme_colors,
                effect,
            ),
            _ => fx_matrix(
                &mut scr,
                pal,
                intensity,
                &state,
                effect == "rain",
                cfg.use_theme_colors,
                effect,
            ),
        };
        if let Err(e) = res {
            eprintln!("render error: {e:?}");
            break;
        }
        // Ok(()) => resize happened; loop and restart the effect.
    }
    Ok(())
}

// Preserve the Kuyen VTE baseline: offer the real assembly engine first and
// use the existing Rust catalog path only when it declines or ASM is disabled.
fn run_ttfx_asm_vte(
    cfg: &Config,
    effect_name: &str,
    cols: usize,
    rows: usize,
    cell_aspect: f32,
    state: &AudioState,
) -> Option<Result<()>> {
    use clap::Parser;
    use ttfx::asm::{self, AudioHook};
    use ttfx::engine::effect::RunOutcome;
    use ttfx::engine::terminal::TerminalConfig;
    use ttfx::utils::rng::Rng;

    if asm::tier() == 0 || std::env::var_os("TTFX_BG_FX").is_some() {
        return None;
    }
    let effect = ttfx::cli::Cli::try_parse_from(["ttfx", effect_name])
        .ok()?
        .effect?;
    let input = wordmark::canvas_input(&cfg.ttfx_text, cols, rows, &cfg.char_style, cell_aspect);
    let mut config = TerminalConfig::default();
    config.canvas_width = cols as i64;
    config.canvas_height = rows as i64;
    config.frame_rate = (60.0 * speed_multiplier(cfg.speed))
        .round()
        .clamp(1.0, 60.0) as i64;
    config.ignore_terminal_dimensions = true;
    let mut rng = Rng::from_entropy();
    let audio = AsmAudio {
        state,
        enabled: cfg.audio,
        reactivity: LiveReactivity::new(cfg.reactivity),
    };
    loop {
        let result = asm::try_run(asm::Run {
            effect: &effect,
            input: &input,
            config: &config,
            rng: &mut rng,
            parity_dump: false,
            virtual_clock: false,
            max_frames: None,
            tty_output: true,
            audio: Some(AudioHook {
                poll: asm_audio_poll,
                params: asm_audio_params,
                ctx: (&audio as *const AsmAudio<'_>).cast_mut().cast(),
            }),
        });
        match result {
            None => return None,
            Some(Ok(RunOutcome::Complete)) => {
                if !read_config().running {
                    return Some(Ok(()));
                }
            }
            Some(Ok(outcome)) => {
                log_dbg(&format!("ASM/VTE renderer ended: {outcome:?}"));
                return Some(Ok(()));
            }
            Some(Err(error)) => return Some(Err(error.into())),
        }
    }
}

// Catalog effects on the direct backend must execute the actual assembly engine,
// not the legacy Rust next_frame() path or an ANSI parser.
// Configuration glue only: the visual audio transform remains in ASM.
// Sample the slider without destroying its layer or reading state every frame.
struct LiveReactivity {
    value: std::cell::Cell<i64>,
    checked: std::cell::Cell<std::time::Instant>,
}
impl LiveReactivity {
    fn new(value: i64) -> Self {
        Self { value: std::cell::Cell::new(value), checked: std::cell::Cell::new(std::time::Instant::now()) }
    }
    fn current_with(&self, now: std::time::Instant, read: impl FnOnce() -> i64) -> i64 {
        if now.saturating_duration_since(self.checked.get()) >= Duration::from_millis(250) {
            self.value.set(read().clamp(0, 10));
            self.checked.set(now);
        }
        self.value.get()
    }
    fn current(&self) -> i64 {
        self.current_with(std::time::Instant::now(), || read_config().reactivity)
    }
}
struct AsmAudio<'a> {
    state: &'a AudioState,
    enabled: bool,
    reactivity: LiveReactivity,
}

extern "C" fn asm_audio_poll(ctx: *mut std::ffi::c_void) -> u64 {
    let audio = unsafe { &*(ctx as *const AsmAudio<'_>) };
    let reactivity = audio.reactivity.current();
    if !audio.enabled || reactivity == 0 {
        return 0;
    }
    let volume = (audio.state.volume().clamp(0.0, 1.0) * 255.0) as u64;
    let bass = ((audio.state.band_at(0, NBANDS) + audio.state.band_at(1, NBANDS) * 0.5)
        .clamp(0.0, 1.0)
        * 255.0) as u64;
    let treble = (audio.state.treble().clamp(0.0, 1.0) * 255.0) as u64;
    let bpm = u64::from(audio.state.bpm().min(255));
    volume | (bass << 8) | (u64::from(audio.state.beat()) << 16) | (treble << 24) | (bpm << 32)
}

extern "C" fn asm_audio_params(ctx: *mut std::ffi::c_void, out: *mut f32) -> u64 {
    let audio = unsafe { &*(ctx as *const AsmAudio<'_>) };
    let reactivity = audio.reactivity.current();
    if !audio.enabled || reactivity == 0 || out.is_null() {
        return 0;
    }
    let volume = audio.state.volume().clamp(0.0, 1.0);
    let bass =
        (audio.state.band_at(0, NBANDS) + audio.state.band_at(1, NBANDS) * 0.5).clamp(0.0, 1.0);
    if volume <= 0.02 && bass <= 0.05 {
        return 0;
    }
    let strength = reactivity as f32 / 2.0;
    let brightness = 1.0 + volume * 0.45 * strength;
    let hue =
        (bass * 45.0 + volume * 18.0) * strength + if audio.state.beat() { 10.0 } else { 0.0 };
    let (sin, cos) = hue.to_radians().sin_cos();
    let t = (1.0 - cos) / 3.0;
    let v = 0.57735026 * sin;
    let matrix = [
        cos + t,
        t - v,
        t + v,
        t + v,
        cos + t,
        t - v,
        t - v,
        t + v,
        cos + t,
    ];
    let params = [
        brightness, matrix[0], matrix[1], matrix[2], matrix[3], matrix[4], matrix[5], matrix[6],
        matrix[7], matrix[8],
    ];
    // SAFETY: the engine gives us a writable 10-f32 audio parameter slot.
    unsafe { std::ptr::copy_nonoverlapping(params.as_ptr(), out, params.len()) };
    1
}

// All theme reads and RGB remapping stay in the ASM producer thread. The
// borrowed pool is decoded and restyled before the frame is published to GTK.
struct AsmThemeSink<'a> {
    sink: FrameSink,
    effect_name: &'a str,
    theme_watcher: ThemeWatcher,
    use_theme_colors: bool,
    /// Paces the engine to the live frame-rate ceiling; see `pace_to_cap`.
    next_due: Option<std::time::Instant>,
    producer_probe: Option<producer_probe::ProducerProbe>,
}

/// The assembly engine paces itself to the frame rate fixed when the run
/// started. The ceiling can change while it runs (auto mode), so each emitted
/// frame also waits for the live ceiling: the effective rate is the lower of
/// the two. Returns the deadline for the next frame.
fn pace_to_cap(
    cap: Option<f64>,
    next_due: Option<std::time::Instant>,
    now: std::time::Instant,
) -> Option<std::time::Instant> {
    let cap = cap?;
    let period = Duration::from_secs_f64(1.0 / cap.max(1.0));
    let due = match next_due {
        // Far behind (effect switch, pause): restart the cadence from now.
        Some(due) if now.duration_since(due) < period * 2 => due,
        _ => now,
    };
    if due > now {
        thread::sleep(due - now);
    }
    Some(due + period)
}

unsafe extern "C" fn asm_emit_themed_frame(
    ctx: *mut std::ffi::c_void,
    cols: u64,
    rows: u64,
    grid: *const u32,
    pool: *const u8,
) -> u64 {
    if ctx.is_null() {
        return 1;
    }
    // SAFETY: the ASM callback is synchronous and its caller holds this stack
    // value exclusively until run_direct returns. It cannot escape to GTK.
    let themed = unsafe { &mut *(ctx as *mut AsmThemeSink<'_>) };
    fps_governor::note_tick();
    let diagnostic = themed.producer_probe.as_ref().map(|_| {
        (producer_probe::monotonic_ns(), producer_probe::thread_cpu_ns(), ttfx::asm::direct_timing_snapshot())
    });
    let cap = max_fps();
    let now = std::time::Instant::now();
    let requested = diagnostic.as_ref().map(|_| producer_probe::requested_wait_ns(cap, themed.next_due, now));
    themed.next_due = pace_to_cap(cap, themed.next_due, now);
    let wait_end = diagnostic.as_ref().map(|_| producer_probe::monotonic_ns());
    let result = match std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        asm_emit_themed_frame_inner(themed, cols, rows, grid, pool)
    })) {
        Ok(result) => result,
        Err(_) => {
            themed.sink.error = Some("assembly themed callback panicked");
            1
        }
    };
    if let (Some(probe), Some((enter, cpu_enter, asm))) = (themed.producer_probe.as_mut(), diagnostic) {
        probe.sequence += 1;
        let enabled = asm.filter(|v| v[0] != 0 && v[1] >= v[0] && v[3] >= v[1] && v[4] >= v[3]);
        probe.record(serde_json::json!({"event":"producer_frame", "sequence":probe.sequence,
            "generation":themed.sink.generation.load(std::sync::atomic::Ordering::Acquire),
            "callback_enter_ns":enter, "callback_cpu_enter_ns":cpu_enter,
            "cap":cap, "rust_wait_requested_ns":requested, "rust_wait_end_ns":wait_end,
            "asm_raw_ns":enabled,
            "asm_compute_ns":enabled.map(|v|v[1]-v[0]),
            "asm_wait_requested_ns":enabled.map(|v|v[2]),
            "asm_wait_elapsed_ns":enabled.map(|v|v[3]-v[1]),
            "asm_export_ns":enabled.map(|v|v[4]-v[3]),
            "callback_result":result}));
    }
    result
}

fn asm_emit_themed_frame_inner(
    themed: &mut AsmThemeSink<'_>,
    cols: u64,
    rows: u64,
    grid: *const u32,
    pool: *const u8,
) -> u64 {
    if themed.use_theme_colors {
        if let Some(colors) = themed.theme_watcher.changed() {
            themed.sink.color_transform = ttfx_theme_transform(themed.effect_name, colors);
        }
    }
    // SAFETY: themed.sink is exclusively borrowed here; the ASM producer
    // keeps grid and pool alive until this synchronous callback returns.
    unsafe {
        asm_direct_bridge::emit_frame(
            (&mut themed.sink as *mut FrameSink).cast(),
            cols,
            rows,
            grid,
            pool,
        )
    }
}

fn run_ttfx_asm_direct(
    cfg: &Config,
    effect_name: &str,
    cols: usize,
    rows: usize,
    cell_aspect: f32,
    state: &AudioState,
    sink: FrameSink,
) -> Result<()> {
    use clap::Parser;
    use ttfx::asm::{self, AudioHook, DirectFrameHook};
    use ttfx::engine::effect::RunOutcome;
    use ttfx::engine::terminal::TerminalConfig;
    use ttfx::utils::rng::Rng;

    if asm::tier() == 0 {
        anyhow::bail!("direct catalog rendering requires an assembly-enabled ttfx build");
    }
    let effect = match ttfx::cli::Cli::try_parse_from(["ttfx", effect_name])? {
        ttfx::cli::Cli {
            effect: Some(effect),
            ..
        } => effect,
        _ => anyhow::bail!("unknown assembly catalog effect: {effect_name}"),
    };
    let input = wordmark::canvas_input(&cfg.ttfx_text, cols, rows, &cfg.char_style, cell_aspect);
    let config = TerminalConfig {
        canvas_width: cols as i64,
        canvas_height: rows as i64,
        // The live ceiling is applied per frame in `pace_to_cap`.
        frame_rate: (60.0 * speed_multiplier(cfg.speed)).round().clamp(1.0, 60.0) as i64,
        ignore_terminal_dimensions: true,
        ..TerminalConfig::default()
    };
    // Explicit measurement seed only; usual sessions retain entropy.
    let measurement_seed = frame_cadence::enabled_from_env()
        .then(|| {
            std::env::var("TTFX_BG_CADENCE_SEED")
                .ok()
                .and_then(|seed| seed.parse::<u64>().ok())
        })
        .flatten();
    let mut rng = measurement_seed
        .map(Rng::seeded)
        .unwrap_or_else(Rng::from_entropy);
    let mut audio = AsmAudio {
        state,
        enabled: cfg.audio,
        reactivity: LiveReactivity::new(cfg.reactivity),
    };
    let mut themed = AsmThemeSink {
        sink,
        effect_name,
        theme_watcher: ThemeWatcher::new(),
        use_theme_colors: cfg.use_theme_colors,
        next_due: None,
        producer_probe: producer_probe::ProducerProbe::open()?,
    };
    while !themed.sink.stop.load(Ordering::Relaxed) {
        let hook = DirectFrameHook {
            emit: asm_emit_themed_frame,
            ctx: (&mut themed as *mut AsmThemeSink<'_>).cast(),
        };
        // SAFETY: themed and audio remain live and exclusively owned by this
        // worker for the synchronous run; the callback consumes borrowed
        // ASM grid/pool data before returning and never retains its pointers.
        let result = unsafe {
            asm::run_direct(
                asm::Run {
                    effect: &effect,
                    input: &input,
                    config: &config,
                    rng: &mut rng,
                    parity_dump: false,
                    virtual_clock: false,
                    max_frames: None,
                    tty_output: false,
                    audio: Some(AudioHook {
                        poll: asm_audio_poll,
                        params: asm_audio_params,
                        ctx: (&mut audio as *mut AsmAudio<'_>).cast(),
                    }),
                },
                hook,
            )
        }?;
        if let Some(error) = themed.sink.error {
            anyhow::bail!("assembly frame decode failed: {error}");
        }
        if themed.sink.stop.load(Ordering::Relaxed) {
            break;
        }
        if result != RunOutcome::Complete {
            anyhow::bail!("assembly direct renderer stopped unexpectedly: {result:?}");
        }
        if !read_config().running {
            break;
        }
    }
    Ok(())
}

fn run_direct_render(
    cfg: &Config,
    effect: &str,
    cols: usize,
    rows: usize,
    with_intro: bool,
    cell_aspect: f32,
    snapshot: Arc<std::sync::Mutex<Vec<DirectCell>>>,
    generation: Arc<std::sync::atomic::AtomicU64>,
    stop: Arc<std::sync::atomic::AtomicBool>,
) -> Result<()> {
    let state = AudioState::start(cfg.audio);
    let mut scr = Screen::new_direct(cols, rows, snapshot, generation, stop.clone());
    let palette = if cfg.use_theme_colors {
        let colors = read_theme_colors();
        if colors.is_empty() {
            hardcoded_palette(effect)
        } else {
            theme_palette(&colors, effect).into_iter().collect()
        }
    } else {
        hardcoded_palette(effect)
    };
    if cfg.use_theme_colors {
        set_theme_palette(palette.clone());
    }
    set_auto_degrade_enabled(false);
    if with_intro {
        show_intro(
            &mut scr,
            &palette,
            if cfg.byline.trim().is_empty() {
                DEFAULT_BYLINE
            } else {
                cfg.byline.trim()
            },
            effect,
            cfg.intro_size,
            &state,
            cfg.intro_beat_sync,
            &cfg.boot_char_style,
        );
    }
    if stop.load(Ordering::Relaxed) {
        return Ok(());
    }
    if is_ttfx_effect(effect) {
        let sink = FrameSink::new(
            scr.direct_snapshot
                .as_ref()
                .expect("direct frame slot")
                .clone(),
            scr.snapshot_gen
                .as_ref()
                .expect("direct generation")
                .clone(),
            stop,
            (cols, rows),
        );
        return run_ttfx_asm_direct(cfg, effect, cols, rows, cell_aspect, &state, sink);
    }
    while !stop.load(Ordering::Relaxed) {
        let result = match effect {
            "donut" => fx_donut(
                &mut scr,
                &palette,
                cfg.intensity,
                &state,
                cell_aspect,
                cfg.use_theme_colors,
                effect,
            ),
            "fire" => fx_fire(
                &mut scr,
                &palette,
                cfg.intensity,
                &state,
                cfg.use_theme_colors,
                effect,
            ),
            "starfield" => fx_starfield(
                &mut scr,
                &palette,
                cfg.intensity,
                &state,
                cfg.use_theme_colors,
                effect,
            ),
            "life" => fx_life(
                &mut scr,
                &palette,
                cfg.intensity,
                &state,
                cfg.use_theme_colors,
                effect,
            ),
            "wave" => fx_wave(
                &mut scr,
                &palette,
                cfg.intensity,
                &state,
                cfg.use_theme_colors,
                effect,
            ),
            "bars" => fx_bars(
                &mut scr,
                &palette,
                cfg.intensity,
                &state,
                cfg.use_theme_colors,
                effect,
            ),
            _ => fx_matrix(
                &mut scr,
                &palette,
                cfg.intensity,
                &state,
                effect == "rain",
                cfg.use_theme_colors,
                effect,
            ),
        };
        if let Err(error) = result {
            return Err(error);
        }
    }
    Ok(())
}

// ==================== preview daemon (in-process, pure Rust) ====================
// Replaces the old python fxlive daemon: the QML switcher spawns this binary
// with --preview-daemon and talks JSON lines on stdin/stdout. Each subscribed
// effect renders IN-PROCESS on its own headless Screen (no ptys, no child
// processes, no ANSI stream parsing): native effects paint the Screen via
// put(), catalog effects have their engine frame strings fed back into the
// Screen, and present() publishes "x,y,ch,#rrggbb;…" snapshots that the
// emitter thread streams to QML at a capped rate.

// Parse one stored SGR color string ("\x1b[38;2;r;g;bm", "\x1b[95m", …) to RGB.
fn ansi_sgr_to_rgb(code: &str) -> Option<(u8, u8, u8)> {
    let body = code.strip_prefix("\x1b[")?.strip_suffix('m')?;
    let nums: Vec<i64> = body
        .split(';')
        .filter(|p| !p.is_empty())
        .map(|p| p.parse().unwrap_or(-1))
        .collect();
    const BASIC: [(u8, u8, u8); 8] = [
        (0, 0, 0),
        (205, 49, 49),
        (0, 205, 0),
        (205, 205, 0),
        (0, 0, 238),
        (205, 30, 205),
        (0, 205, 205),
        (229, 229, 229),
    ];
    const BRIGHT: [(u8, u8, u8); 8] = [
        (127, 127, 127),
        (255, 85, 85),
        (85, 255, 85),
        (255, 255, 85),
        (85, 85, 255),
        (255, 85, 255),
        (85, 255, 255),
        (255, 255, 255),
    ];
    let mut rgb = None;
    let mut i = 0;
    while i < nums.len() {
        let n = nums[i];
        match n {
            0 => rgb = None,
            39 => rgb = Some((204, 204, 204)),
            30..=37 => rgb = Some(BASIC[(n - 30) as usize]),
            90..=97 => rgb = Some(BRIGHT[(n - 90) as usize]),
            38 | 48 if i + 1 < nums.len() => match nums[i + 1] {
                2 if i + 4 < nums.len() => {
                    rgb = Some((nums[i + 2] as u8, nums[i + 3] as u8, nums[i + 4] as u8));
                    i += 4;
                }
                5 if i + 2 < nums.len() => {
                    let v = nums[i + 2];
                    rgb = Some(match v {
                        0..=7 => BASIC[v as usize],
                        8..=15 => BRIGHT[(v - 8) as usize],
                        16..=231 => {
                            let v = v - 16;
                            let scale = [0u8, 95, 135, 175, 215, 255];
                            (
                                scale[(v / 36) as usize],
                                scale[((v % 36) / 6) as usize],
                                scale[(v % 6) as usize],
                            )
                        }
                        232..=255 => {
                            let g = (8 + (v - 232) * 10) as u8;
                            (g, g, g)
                        }
                        _ => (204, 204, 204),
                    });
                    i += 2;
                }
                _ => {}
            },
            _ => {}
        }
        i += 1;
    }
    rgb
}

// Decode the sparse cell protocol emitted by a headless Screen. It is a data
// path, not a terminal parser: the direct GTK renderer receives only final
// cells and colors, so it never processes a PTY or ANSI cursor state.
#[cfg(test)]
fn parse_direct_cells(snapshot: &str, cols: usize, rows: usize) -> Vec<DirectCell> {
    let mut cells = Vec::new();
    for entry in snapshot.split(';') {
        let mut parts = entry.splitn(4, ',');
        let (Some(x), Some(y), Some(raw_ch), Some(raw_color)) =
            (parts.next(), parts.next(), parts.next(), parts.next())
        else {
            continue;
        };
        let (Ok(x), Ok(y)) = (x.parse::<usize>(), y.parse::<usize>()) else {
            continue;
        };
        if x >= cols || y >= rows || !raw_color.starts_with('#') || raw_color.len() != 7 {
            continue;
        }
        let Ok(rgb) = u32::from_str_radix(&raw_color[1..], 16) else {
            continue;
        };
        let ch = match raw_ch {
            r#"\""# => '"',
            r#"\\"# => '\\',
            _ => raw_ch.chars().next().unwrap_or(' '),
        };
        cells.push(DirectCell {
            x,
            y,
            ch,
            color: (
                ((rgb >> 16) & 0xff) as u8,
                ((rgb >> 8) & 0xff) as u8,
                (rgb & 0xff) as u8,
            ),
            background: None,
            attrs: 0,
        });
    }
    cells
}

fn json_push_escaped_char(out: &mut String, ch: char) {
    match ch {
        '"' => out.push_str("\\\""),
        '\\' => out.push_str("\\\\"),
        c if (c as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", c as u32)),
        c => out.push(c),
    }
}

// Feed one engine frame string (full canvas rows with SGR runs) into the grid.
// Direct screens decode each SGR run once and retain RGB per cell, while preview
// screens preserve their existing ANSI-string representation.
fn feed_ttfx_frame(scr: &mut Screen, frame: &str) {
    let direct = scr.direct_colors.is_some();
    for (y, row) in frame.lines().enumerate() {
        if y >= scr.rows {
            break;
        }
        let mut x = 0usize;
        let mut cur = String::new();
        let mut cur_rgb = None;
        let mut chars = row.chars();
        while let Some(ch) = chars.next() {
            if ch == '\x1b' {
                if chars.next() == Some('[') {
                    let mut seq = String::from("\x1b[");
                    for c in chars.by_ref() {
                        seq.push(c);
                        if c.is_ascii_alphabetic() {
                            break;
                        }
                    }
                    if seq.ends_with('m') {
                        if seq == "\x1b[0m" || seq == "\x1b[m" {
                            cur.clear();
                            cur_rgb = None;
                        } else {
                            cur = seq;
                            cur_rgb = ansi_sgr_to_rgb(&cur);
                        }
                    }
                }
                continue;
            }
            if x >= scr.cols {
                break;
            }
            if direct {
                scr.put_direct_rgb(x, y, ch, cur_rgb);
            } else {
                scr.put(x, y, ch, cur.clone());
            }
            x += 1;
        }
        for xx in x..scr.cols {
            scr.erase(xx, y);
        }
    }
}

type SharedCells = std::sync::Arc<std::sync::Mutex<String>>;
type SharedGen = std::sync::Arc<std::sync::atomic::AtomicU64>;
type SharedStop = std::sync::Arc<std::sync::atomic::AtomicBool>;
type FailedQueue = std::sync::Arc<std::sync::Mutex<Vec<String>>>;

// One effect session: renders on a headless Screen until the stop flag rises.
fn preview_worker(
    fx: String,
    cols: usize,
    rows: usize,
    cells: SharedCells,
    gen: SharedGen,
    stop: SharedStop,
    failed: FailedQueue,
) {
    use std::sync::atomic::Ordering;
    if is_ttfx_effect(&fx) {
        // Catalog effect via the vendored engine; frame strings feed the grid.
        use clap::Parser;
        use ttfx::engine::canvas::Anchor;
        use ttfx::engine::ctx::{Clock, EngineCtx};
        use ttfx::engine::terminal::TerminalConfig;
        use ttfx::utils::rng::Rng;
        let fps: i64 = 20;
        let mut scr = Screen::new_headless(cols, rows, cells, gen, stop.clone());
        'outer: loop {
            if stop.load(Ordering::Relaxed) {
                break;
            }
            let mut effect = match ttfx::cli::Cli::try_parse_from(["ttfx", fx.as_str()]) {
                Ok(ttfx::cli::Cli {
                    effect: Some(e), ..
                }) => e.build_effect(),
                _ => {
                    if let Ok(mut f) = failed.lock() {
                        f.push(fx.clone());
                    }
                    return;
                }
            };
            // Tight wordmark block + C anchors: the engine centers it on the
            // effect canvas exactly. (A full-canvas input loses its blank
            // padding to the engine's space-dropping and re-anchoring.)
            let input = wordmark::wordmark_input("OMARCHY", cols, rows, "native", 2.0);
            let mut config = TerminalConfig::default();
            config.canvas_width = cols as i64;
            config.canvas_height = rows as i64;
            config.frame_rate = fps;
            config.anchor_canvas = Anchor::C;
            config.anchor_text = Anchor::C;
            // The daemon's stdout is a pipe: ambient terminal dimensions are
            // meaningless and would shift the canvas off-center.
            config.ignore_terminal_dimensions = true;
            let mut ctx = match EngineCtx::new(
                &input,
                config,
                Rng::from_entropy(),
                Clock::virtual_with_frame_rate(fps),
            ) {
                Ok(c) => c,
                Err(_) => {
                    if let Ok(mut f) = failed.lock() {
                        f.push(fx.clone());
                    }
                    return;
                }
            };
            ctx.final_text_bands = true;
            if effect.build(&mut ctx).is_err() {
                if let Ok(mut f) = failed.lock() {
                    f.push(fx.clone());
                }
                return;
            }
            loop {
                if stop.load(Ordering::Relaxed) {
                    break 'outer;
                }
                match effect.next_frame(&mut ctx) {
                    Some(frame) => {
                        feed_ttfx_frame(&mut scr, &frame);
                        scr.present();
                        // The QML side consumes at most ~12fps; rendering the
                        // catalog faster than that burns CPU on frames nobody
                        // sees (31% for one effect at 232x66 -> ~15%).
                        std::thread::sleep(Duration::from_millis(90));
                    }
                    // Animation finished: rebuild it so the preview loops
                    // continuously instead of freezing on the last frame.
                    None => break,
                }
            }
        }
    } else {
        // Native hand-rolled effects paint the Screen directly; they return
        // Ok(()) on (fake) resize, which for the daemon means "stop".
        let cfg = read_config();
        let palette: Vec<String> = if cfg.use_theme_colors {
            let tc = read_theme_colors();
            if !tc.is_empty() {
                theme_palette(&tc, &fx).into_iter().collect()
            } else {
                hardcoded_palette(&fx)
            }
        } else {
            hardcoded_palette(&fx)
        };
        let state = AudioState::start(false); // previews: no audio capture
        let mut scr = Screen::new_headless(cols, rows, cells, gen, stop.clone());
        let res = match fx.as_str() {
            "donut" => fx_donut(
                &mut scr,
                &palette,
                5,
                &state,
                2.0,
                cfg.use_theme_colors,
                &fx,
            ),
            "fire" => fx_fire(&mut scr, &palette, 5, &state, cfg.use_theme_colors, &fx),
            "starfield" => fx_starfield(&mut scr, &palette, 5, &state, cfg.use_theme_colors, &fx),
            "life" => fx_life(&mut scr, &palette, 5, &state, cfg.use_theme_colors, &fx),
            "wave" => fx_wave(&mut scr, &palette, 5, &state, cfg.use_theme_colors, &fx),
            "bars" => fx_bars(&mut scr, &palette, 5, &state, cfg.use_theme_colors, &fx),
            _ => fx_matrix(
                &mut scr,
                &palette,
                5,
                &state,
                fx == "rain",
                cfg.use_theme_colors,
                &fx,
            ),
        };
        if res.is_err() {
            if let Ok(mut f) = failed.lock() {
                f.push(fx.clone());
            }
        }
    }
}

// Minimal JSON field readers for the daemon protocol (flat objects only).
fn daemon_json_str(line: &str, key: &str) -> String {
    let pat = format!("\"{key}\"");
    let Some(pos) = line.find(&pat) else {
        return String::new();
    };
    let rest = &line[pos + pat.len()..];
    let Some(q1) = rest.find('"') else {
        return String::new();
    };
    let rest = &rest[q1 + 1..];
    let Some(q2) = rest.find('"') else {
        return String::new();
    };
    rest[..q2].to_string()
}

fn daemon_json_num(line: &str, key: &str, default: usize) -> usize {
    let pat = format!("\"{key}\"");
    let Some(pos) = line.find(&pat) else {
        return default;
    };
    let rest = &line[pos + pat.len()..];
    let digits: String = rest
        .chars()
        .skip_while(|c| !c.is_ascii_digit())
        .take_while(|c| c.is_ascii_digit())
        .collect();
    digits.parse().unwrap_or(default)
}

fn run_preview_daemon() -> Result<()> {
    use std::collections::HashMap;
    use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
    use std::sync::{Arc, Mutex};

    struct Session {
        stop: SharedStop,
        cells: SharedCells,
        gen: SharedGen,
        last_gen: u64,
        last_emit: std::time::Instant,
        w: usize,
        h: usize,
        join: Option<std::thread::JoinHandle<()>>,
    }

    // stdin is owned by a reader thread; commands arrive over a channel so the
    // emitter never blocks on I/O.
    let (tx, rx) = std::sync::mpsc::channel::<String>();
    std::thread::spawn(move || {
        use std::io::BufRead;
        let stdin = std::io::stdin();
        for line in stdin.lock().lines() {
            match line {
                Ok(l) => {
                    if tx.send(l).is_err() {
                        return;
                    }
                }
                Err(_) => return,
            }
        }
        let _ = tx.send(String::new()); // EOF sentinel
    });

    let mut sessions: HashMap<String, Session> = HashMap::new();
    let failed: FailedQueue = Arc::new(Mutex::new(Vec::new()));
    let budget = Duration::from_millis(83); // ~12fps per effect
    let stdout = std::io::stdout();

    let stop_session = |sessions: &mut HashMap<String, Session>, fx: &str| {
        if let Some(mut sess) = sessions.remove(fx) {
            sess.stop.store(true, Ordering::Relaxed);
            if let Some(j) = sess.join.take() {
                let _ = j.join();
            }
        }
    };

    'main: loop {
        while let Ok(line) = rx.try_recv() {
            if line.is_empty() {
                break 'main; // EOF
            }
            match daemon_json_str(&line, "cmd").as_str() {
                "sub" | "resize" => {
                    let fx = daemon_json_str(&line, "fx");
                    if fx.is_empty() {
                        continue;
                    }
                    // cols up to 232+ let the wordmark bitmap render at
                    // integer pitch (crisp, like the fullscreen backend).
                    let cols = daemon_json_num(&line, "cols", 60).clamp(10, 260);
                    let rows = daemon_json_num(&line, "rows", 20).clamp(8, 80);
                    stop_session(&mut sessions, &fx);
                    let stop: SharedStop = Arc::new(AtomicBool::new(false));
                    let cells: SharedCells = Arc::new(Mutex::new(String::new()));
                    let gen: SharedGen = Arc::new(AtomicU64::new(0));
                    let join = std::thread::spawn({
                        let (fx, stop, cells, gen, failed) = (
                            fx.clone(),
                            stop.clone(),
                            cells.clone(),
                            gen.clone(),
                            failed.clone(),
                        );
                        move || preview_worker(fx, cols, rows, cells, gen, stop, failed)
                    });
                    sessions.insert(
                        fx.clone(),
                        Session {
                            stop,
                            cells,
                            gen,
                            last_gen: 0,
                            last_emit: std::time::Instant::now() - budget,
                            w: cols,
                            h: rows,
                            join: Some(join),
                        },
                    );
                }
                "unsub" => {
                    let fx = daemon_json_str(&line, "fx");
                    stop_session(&mut sessions, &fx);
                }
                "quit" => break 'main,
                _ => {}
            }
        }
        // Announce terminal worker failures once and drop the session: with
        // in-process rendering there is nothing to respawn, so a storm is
        // impossible by construction.
        {
            let mut f = failed.lock().unwrap();
            for fx in f.drain(..) {
                if let Some(sess) = sessions.remove(&fx) {
                    let mut out = stdout.lock();
                    use std::io::Write;
                    let _ = writeln!(
                        out,
                        "{{\"fx\":\"{fx}\",\"cells\":\"\",\"w\":{},\"h\":{},\"failed\":true}}",
                        sess.w, sess.h
                    );
                    let _ = out.flush();
                }
            }
        }
        // Emit changed frames within the per-effect budget.
        for (fx, sess) in sessions.iter_mut() {
            let g = sess.gen.load(Ordering::Relaxed);
            if g == sess.last_gen || sess.last_emit.elapsed() < budget {
                continue;
            }
            let cells = sess.cells.lock().unwrap().clone();
            sess.last_gen = g;
            sess.last_emit = std::time::Instant::now();
            use std::io::Write;
            let mut out = stdout.lock();
            let _ = writeln!(
                out,
                "{{\"fx\":\"{fx}\",\"cells\":\"{cells}\",\"w\":{},\"h\":{}}}",
                sess.w, sess.h
            );
            let _ = out.flush();
        }
        std::thread::sleep(Duration::from_millis(40));
    }

    let fxs: Vec<String> = sessions.keys().cloned().collect();
    for fx in fxs {
        stop_session(&mut sessions, &fx);
    }
    Ok(())
}

// Audio-reactive pacing: more sound => faster flow (lower delay), smoothly.
fn frame_delay(base_ms: i64, intensity: i64, speed: i64, audio: &AudioState) -> Duration {
    let base = (base_ms - intensity * 3).clamp(8, 120) as f32;
    let audio_speed = 1.0 + audio.volume() * 2.5;
    let baseline = Duration::from_millis((base / audio_speed).max(6.0) as u64);
    speed_delay(baseline, speed)
}

// --- matrix / rain: column rain where EACH COLUMN follows its frequency band
// (rain equalizer, like the node implementation): band energy raises that
// column's fall speed and brightness. Global spawn density follows volume. ---
fn fx_matrix(
    scr: &mut Screen,
    palette: &[String],
    intensity: i64,
    audio: &AudioState,
    rain: bool,
    use_theme_colors: bool,
    effect: &str,
) -> Result<()> {
    use rand::SeedableRng;
    if let Some(raw)=std::env::var_os("TTFX_BG_NATIVE_SEED") {
        let seed=raw.to_string_lossy().parse::<u64>()
            .map_err(|_|anyhow::anyhow!("TTFX_BG_NATIVE_SEED debe ser un entero u64"))?;
        let mut rng=rand::rngs::StdRng::seed_from_u64(seed);
        fx_matrix_with_rng(scr,palette,intensity,audio,rain,use_theme_colors,effect,&mut rng,Some(seed))
    } else {
        let mut rng=rand::thread_rng();
        fx_matrix_with_rng(scr,palette,intensity,audio,rain,use_theme_colors,effect,&mut rng,None)
    }
}

fn fx_matrix_with_rng<R: rand::Rng + ?Sized>(
    scr:&mut Screen, palette:&[String], intensity:i64, audio:&AudioState,
    rain:bool, use_theme_colors:bool, effect:&str, rng:&mut R, native_seed:Option<u64>,
) -> Result<()> {
    let mut native_probe = if scr.direct_snapshot.is_some() {
        producer_probe::ProducerProbe::open()?
    } else { None };
    if let Some(probe) = native_probe.as_mut() {
        probe.record(serde_json::json!({"event":"producer_route", "route":"native_matrix",
            "effect":effect, "uses_asm":false, "rng":if native_seed.is_some(){"StdRng"}else{"rand::thread_rng"}, "seed_controlled":native_seed.is_some(), "native_seed":native_seed,
            "native_swap":scr.native_swap,
            "asm_seed_ignored":std::env::var_os("TTFX_BG_CADENCE_SEED").is_some(),
            "asm_timing_ignored":std::env::var("TTFX_ASM_TIMING").as_deref()==Ok("1"),
            "cols":scr.cols, "rows":scr.rows, "base_delay_ms":40}));
    }
    let chars: Vec<char> = if rain {
        "ｱｲｳｴｵｶｷｸｹｺｻｼｽｾｿ0123456789".chars().collect()
    } else {
        "ｱｲｳｴｵｶｷｸｹｺｻｼｽｾｿﾀﾁﾂﾃﾄﾅﾆﾇﾈﾉﾊﾋﾌﾍﾎﾏﾐﾑﾒﾓﾔﾕﾖﾗﾘﾙﾚﾛﾜﾝ0123456789ABCDEF"
            .chars()
            .collect()
    };
    let speed_base: i32 = 1 + (intensity / 4) as i32;
    let trail_base: usize = 8 + (intensity as usize) * 2;
    let (cols, rows) = (scr.cols, scr.rows);
    let mut head: Vec<i32> = (0..cols).map(|_| rng.gen_range(0..rows as i32)).collect();
    let mut prev_head: Vec<i32> = head.clone();
    let mut speed: Vec<i32> = (0..cols)
        .map(|_| rng.gen_range(speed_base..speed_base + 2).max(1))
        .collect();
    let mut trail: Vec<usize> = (0..cols)
        .map(|_| rng.gen_range(trail_base..trail_base + 12))
        .collect();
    // When the theme changes, only drops that RESPAWN after the change adopt the
    // new palette; drops already on screen keep their born color until they
    // scroll away. Each column's trail is painted entirely from its born palette
    // so every cell always has a real hue (never the white default foreground).
    let mut cur_pal: Vec<String> = palette.to_vec();
    let mut col_pal: Vec<Vec<String>> = (0..cols).map(|_| palette.to_vec()).collect();
    let mut theme_watcher = ThemeWatcher::new();
    loop {
        let diagnostic_begin = native_probe.as_ref().map(|_| {
            (producer_probe::monotonic_ns(), producer_probe::thread_cpu_ns())
        });
        if scr.maybe_resize() {
            return Ok(());
        }
        // Detect theme changes live and update the palette future drops use.
        if use_theme_colors {
            if let Some(new_colors) = theme_watcher.changed() {
                cur_pal = theme_palette(new_colors, effect);
                set_theme_palette(cur_pal.clone());
                log_dbg(&format!(
                    "theme colors changed: {effect} palette updated ({})",
                    cur_pal.len()
                ));
            }
        }
        let vol = audio.volume();
        let spawn_chance = 0.35 + vol * 0.65;
        // A) Erase cells this column's trail vacated since the previous frame.
        for c in 0..cols {
            let h_prev = prev_head[c];
            let h = head[c];
            let vac_top = h_prev - trail[c] as i32 + 1;
            let vac_end = h - trail[c] as i32 + 1; // exclusive
            if vac_end > vac_top {
                for y in vac_top..vac_end.min(rows as i32) {
                    if y >= 0 && y < rows as i32 {
                        scr.erase(c, y as usize);
                    }
                }
            }
            // Column was reset/rested (head negative): clear its entire old trail
            if h < 0 && vac_top >= 0 {
                for y in vac_top..h_prev.min(rows as i32) {
                    if y >= 0 && y < rows as i32 {
                        scr.erase(c, y as usize);
                    }
                }
            }
        }
        let band = |c: usize| audio.band_at(c, cols);
        for c in 0..cols {
            let h = head[c];
            // Paint this column's drop entirely with ITS born palette.
            let pal = &col_pal[c];
            for t in 0..trail[c] {
                let y = h - t as i32;
                if y >= 0 && y < rows as i32 {
                    let ch = chars[rng.gen_range(0..chars.len())];
                    let idx = if t == 0 {
                        0
                    } else if t < 4 {
                        1
                    } else {
                        2
                    };
                    let idx = idx.min(pal.len().saturating_sub(1));
                    // fall back to a neutral theme hue if palette is empty
                    let color = pal
                        .get(idx)
                        .cloned()
                        .filter(|s| !s.is_empty())
                        .unwrap_or_else(|| "\x1b[32m".to_string());
                    scr.put(c, y as usize, ch, color);
                }
            }
            // Advance drop; on respawn adopt the current theme palette.
            prev_head[c] = h;
            head[c] = h + speed[c] + (band(c) * 2.0) as i32;
            if head[c] - trail[c] as i32 > rows as i32 {
                if rng.gen::<f32>() < spawn_chance {
                    head[c] = -(rng.gen_range(0..30));
                    speed[c] = rng.gen_range(speed_base..speed_base + 2).max(1);
                    trail[c] = rng.gen_range(trail_base..trail_base + 12);
                    col_pal[c] = cur_pal.clone();
                } else {
                    head[c] = -(rows as i32 + 10);
                }
            }
        }
        let effective_intensity = live_intensity(intensity);
        let effective_speed = live_speed(5);
        let intended = frame_delay(40, effective_intensity, effective_speed, audio);
        let frame_dur = render_frame_interval(intended, scr.direct_snapshot.is_some());
        let audio_volume_sample = native_probe.as_ref().map(|_| audio.volume());
        scr.fps_overlay(frame_dur.as_secs_f64() * 1000.0);
        let before_present = native_probe.as_ref().map(|_| producer_probe::monotonic_ns());
        scr.present();
        let sleep_enter = native_probe.as_ref().map(|_| producer_probe::monotonic_ns());
        thread::sleep(frame_dur);
        if let (Some(probe), Some((begin, cpu_begin)), Some(before), Some(sleep_begin)) =
            (native_probe.as_mut(), diagnostic_begin, before_present, sleep_enter) {
            let sleep_end = producer_probe::monotonic_ns();
            probe.sequence += 1;
            probe.record(serde_json::json!({"event":"native_matrix_frame", "route":"native_matrix",
                "sequence":probe.sequence, "frame_begin_ns":begin, "frame_cpu_begin_ns":cpu_begin,
                "effective_intensity":effective_intensity, "effective_speed":effective_speed,
                "audio_volume_sample":audio_volume_sample, "cap":max_fps(),
                "intended_delay_ns":intended.as_nanos() as u64,
                "sleep_requested_ns":frame_dur.as_nanos() as u64,
                "compute_ns":before-begin, "present_ns":sleep_begin-before,
                "sleep_enter_ns":sleep_begin, "sleep_end_ns":sleep_end,
                "sleep_elapsed_ns":sleep_end-sleep_begin,
                "generation":scr.snapshot_gen.as_ref().map(|g|g.load(std::sync::atomic::Ordering::Acquire))}));
        }
    }
}

// --- wave: layered sine waves scrolling horizontally ---
fn fx_wave(
    scr: &mut Screen,
    palette: &[String],
    intensity: i64,
    audio: &AudioState,
    use_theme_colors: bool,
    effect: &str,
) -> Result<()> {
    let mut t = 0f32;
    let mut cur_pal: Vec<String> = palette.to_vec();
    let mut theme_watcher = ThemeWatcher::new();
    loop {
        if scr.maybe_resize() {
            return Ok(());
        }
        if use_theme_colors {
            if let Some(new_colors) = theme_watcher.changed() {
                cur_pal = theme_palette(new_colors, effect);
                set_theme_palette(cur_pal.clone());
                log_dbg(&format!(
                    "theme colors changed: {effect} palette updated ({})",
                    cur_pal.len()
                ));
            }
        }
        scr.clear_dirty();
        let lv = audio.volume();
        let (cols, rows) = (scr.cols, scr.rows);
        let cy = rows as f32 / 2.0;
        for layer in 0..3u8 {
            let amp = (rows as f32 * 0.18) * (0.5 + lv) * (1.0 - layer as f32 * 0.25);
            let freq = 0.045 + layer as f32 * 0.02;
            let phase = t * (1.0 + layer as f32 * 0.6);
            for x in 0..cols {
                let y = cy + amp * (x as f32 * freq + phase).sin();
                if y >= 0.0 && (y as usize) < rows {
                    scr.put(
                        x,
                        y as usize,
                        if layer == 0 { '~' } else { '-' },
                        tint_to_color(layer + 1, &cur_pal),
                    );
                }
            }
        }
        let frame_dur = render_frame_interval(
            frame_delay(45, live_intensity(intensity), live_speed(5), audio),
            scr.direct_snapshot.is_some(),
        );
        scr.fps_overlay(frame_dur.as_secs_f64() * 1000.0);
        scr.present();
        t += 0.12 + lv * 0.25;
        thread::sleep(frame_dur);
    }
}

// --- bars: equalizer driven by the REAL per-band spectrum (each bar = one
// frequency band's energy), smoothed so it follows without flicker. ---
fn paint_bars_frame(scr: &mut Screen, audio: &AudioState, heights: &mut [f32], cur_pal: &[String]) {
    let bands = NBANDS;
    let (cols, rows) = (scr.cols, scr.rows);
    let maxh = rows as f32 * 0.9;
    for b in 0..bands {
        let energy = audio.band_at(b, NBANDS);
        let target = (energy * 1.1).min(1.0) * maxh;
        heights[b] += (target - heights[b]) * 0.4;
        let h = heights[b] as usize;
        for y in 0..h.min(rows) {
            let tint = if y as f32 > h as f32 * 0.7 {
                1
            } else if y as f32 > h as f32 * 0.4 {
                2
            } else {
                3
            };
            for x in bars_layout::columns_for_band(b, cols) {
                scr.put(x, rows - 1 - y, '#', tint_to_color(tint, cur_pal));
            }
        }
    }
}

fn fx_bars(
    scr: &mut Screen,
    palette: &[String],
    intensity: i64,
    audio: &AudioState,
    use_theme_colors: bool,
    effect: &str,
) -> Result<()> {
    const _: () = assert!(NBANDS == bars_layout::BANDS);
    let bands = NBANDS;
    let mut heights = vec![0f32; bands];
    let mut cur_pal: Vec<String> = palette.to_vec();
    let mut theme_watcher = ThemeWatcher::new();
    loop {
        if scr.maybe_resize() {
            return Ok(());
        }
        if use_theme_colors {
            if let Some(new_colors) = theme_watcher.changed() {
                cur_pal = theme_palette(new_colors, effect);
                set_theme_palette(cur_pal.clone());
                log_dbg(&format!(
                    "theme colors changed: {effect} palette updated ({})",
                    cur_pal.len()
                ));
            }
        }
        scr.clear_dirty();
        paint_bars_frame(scr, audio, &mut heights, &cur_pal);
        let frame_dur = render_frame_interval(
            frame_delay(50, live_intensity(intensity), live_speed(5), audio),
            scr.direct_snapshot.is_some(),
        );
        scr.fps_overlay(frame_dur.as_secs_f64() * 1000.0);
        scr.present();
        thread::sleep(frame_dur);
    }
}

// --- donut: classic 3D torus, scaled to the grid. Spin speed follows audio. ---
// Scale follows the original donut.c proportions (30/80 horizontal, 15/22
// vertical), which already bake in the terminal cell aspect. That keeps the
// torus round on any screen; compressing by cell_aspect over-flattened it.
fn fx_donut(
    scr: &mut Screen,
    palette: &[String],
    intensity: i64,
    audio: &AudioState,
    cell_aspect: f32,
    use_theme_colors: bool,
    effect: &str,
) -> Result<()> {
    let _ = cell_aspect;
    let mut a = 0f32;
    let mut e = 1f32;
    let (cols, rows) = (scr.cols, scr.rows);
    let cx = cols as f32 / 2.0;
    let cy = rows as f32 / 2.0;
    let sx = cols as f32 * (30.0 / 80.0);
    let sy = rows as f32 * (15.0 / 22.0);
    let mut zbuf = vec![0f32; cols * rows];
    let mut cur_pal: Vec<String> = palette.to_vec();
    let mut theme_watcher = ThemeWatcher::new();
    loop {
        if scr.maybe_resize() {
            return Ok(());
        }
        if use_theme_colors {
            if let Some(new_colors) = theme_watcher.changed() {
                cur_pal = theme_palette(new_colors, effect);
                set_theme_palette(cur_pal.clone());
                log_dbg(&format!(
                    "theme colors changed: {effect} palette updated ({})",
                    cur_pal.len()
                ));
            }
        }
        scr.clear_dirty();
        for b in zbuf.iter_mut() {
            *b = 0.0;
        }
        let lv = audio.volume();
        let mut j = 0f32;
        while j < 6.28 {
            let mut i = 0f32;
            while i < 6.28 {
                let (sj, cj) = (j.sin(), j.cos());
                let (si, ci) = (i.sin(), i.cos());
                let (sa, ca) = (a.sin(), a.cos());
                let (se, ce) = (e.sin(), e.cos());
                let h = cj + 2.0;
                let d = 1.0 / (si * h * sa + sj * ca + 5.0);
                let t = si * h * ca - sj * sa;
                let x = (cx + sx * d * (ci * h * ce - t * se)) as i32;
                let y = (cy + sy * d * (ci * h * se + t * ce)) as i32;
                let lum = ((sj * sa - si * ca) * ce - ci * h * se - sj * ca - ci * h * sa) * 8.0;
                if y >= 0
                    && y < rows as i32
                    && x >= 0
                    && x < cols as i32
                    && d > zbuf[y as usize * cols + x as usize]
                {
                    zbuf[y as usize * cols + x as usize] = d;
                    let chars = b".,-~:;=!*#$@";
                    let ci2 = lum.max(0.0) as usize;
                    let ch = chars[ci2.min(chars.len() - 1)] as char;
                    let tint = if ci2 > 8 {
                        1
                    } else if ci2 > 4 {
                        2
                    } else {
                        3
                    };
                    scr.put(x as usize, y as usize, ch, tint_to_color(tint, &cur_pal));
                }
                i += 0.02;
            }
            j += 0.07;
        }
        let frame_dur = render_frame_interval(
            frame_delay(45, live_intensity(intensity), live_speed(5), audio),
            scr.direct_snapshot.is_some(),
        );
        scr.fps_overlay(frame_dur.as_secs_f64() * 1000.0);
        scr.present();
        let spin = 1.0 + lv * 2.0;
        a += 0.04 * spin;
        e += 0.02 * spin;
        thread::sleep(frame_dur);
    }
}

// --- fire: classic doom fire from the bottom row. Height licks with audio. ---
fn fx_fire(
    scr: &mut Screen,
    palette: &[String],
    intensity: i64,
    audio: &AudioState,
    use_theme_colors: bool,
    effect: &str,
) -> Result<()> {
    use rand::Rng;
    let mut rng = rand::thread_rng();
    let (cols, rows) = (scr.cols, scr.rows);
    let palette_chars: Vec<char> = " .:-=+*#%@".chars().collect();
    let mut heat = vec![vec![0u8; cols]; rows];
    let mut cur_pal: Vec<String> = palette.to_vec();
    let mut theme_watcher = ThemeWatcher::new();
    loop {
        if scr.maybe_resize() {
            return Ok(());
        }
        if use_theme_colors {
            if let Some(new_colors) = theme_watcher.changed() {
                cur_pal = theme_palette(new_colors, effect);
                set_theme_palette(cur_pal.clone());
                log_dbg(&format!(
                    "theme colors changed: {effect} palette updated ({})",
                    cur_pal.len()
                ));
            }
        }
        let lv = audio.volume();
        let fuel = (28.0 + lv * 8.0 + live_intensity(intensity) as f32 * 0.4) as u8;
        for x in 0..cols {
            heat[rows - 1][x] = fuel.min(36);
        }
        for y in 0..rows - 1 {
            for x in 0..cols {
                let src_x = (x as i32 + rng.gen_range(-1..=1)).clamp(0, cols as i32 - 1) as usize;
                let decay = rng.gen_range(0..=2);
                heat[y][x] = heat[y + 1][src_x].saturating_sub(decay);
            }
        }
        scr.clear_dirty();
        for y in 0..rows {
            for x in 0..cols {
                let h = heat[y][x] as usize;
                if h > 0 {
                    let ci = (h * palette_chars.len() / 37).min(palette_chars.len() - 1);
                    let tint = if h > 24 {
                        1
                    } else if h > 12 {
                        2
                    } else {
                        3
                    };
                    scr.put(x, y, palette_chars[ci], tint_to_color(tint, &cur_pal));
                }
            }
        }
        let frame_dur = render_frame_interval(
            frame_delay(45, live_intensity(intensity), live_speed(5), audio),
            scr.direct_snapshot.is_some(),
        );
        scr.fps_overlay(frame_dur.as_secs_f64() * 1000.0);
        scr.present();
        thread::sleep(frame_dur);
    }
}

// --- starfield: stars flying outward from the center. Speed follows audio. ---
fn fx_starfield(
    scr: &mut Screen,
    palette: &[String],
    intensity: i64,
    audio: &AudioState,
    use_theme_colors: bool,
    effect: &str,
) -> Result<()> {
    use rand::Rng;
    let mut rng = rand::thread_rng();
    let (cols, rows) = (scr.cols, scr.rows);
    let cx = cols as f32 / 2.0;
    let cy = rows as f32 / 2.0;
    let n = 160usize;
    let mut stars: Vec<(f32, f32, f32)> = (0..n)
        .map(|_| {
            (
                rng.gen_range(-1.0..1.0),
                rng.gen_range(-1.0..1.0),
                rng.gen_range(0.05..1.0),
            )
        })
        .collect();
    let mut cur_pal: Vec<String> = palette.to_vec();
    let mut theme_watcher = ThemeWatcher::new();
    loop {
        if scr.maybe_resize() {
            return Ok(());
        }
        if use_theme_colors {
            if let Some(new_colors) = theme_watcher.changed() {
                cur_pal = theme_palette(new_colors, effect);
                set_theme_palette(cur_pal.clone());
                log_dbg(&format!(
                    "theme colors changed: {effect} palette updated ({})",
                    cur_pal.len()
                ));
            }
        }
        scr.clear_dirty();
        let lv = audio.volume();
        let speed = (0.006 + live_intensity(intensity) as f32 * 0.0012) * (1.0 + lv * 2.2);
        for s in stars.iter_mut() {
            s.2 -= speed;
            if s.2 <= 0.02 {
                *s = (rng.gen_range(-1.0..1.0), rng.gen_range(-1.0..1.0), 1.0);
            }
            let px = cx + s.0 / s.2 * cx * 0.5;
            let py = cy + s.1 / s.2 * cy * 0.5;
            if px >= 0.0 && px < cols as f32 && py >= 0.0 && py < rows as f32 {
                let depth = 1.0 - s.2;
                let (ch, tint) = if depth > 0.75 {
                    ('@', 1)
                } else if depth > 0.45 {
                    ('*', 2)
                } else {
                    ('.', 3)
                };
                scr.put(px as usize, py as usize, ch, tint_to_color(tint, &cur_pal));
            }
        }
        let frame_dur = render_frame_interval(
            frame_delay(40, live_intensity(intensity), live_speed(5), audio),
            scr.direct_snapshot.is_some(),
        );
        scr.fps_overlay(frame_dur.as_secs_f64() * 1000.0);
        scr.present();
        thread::sleep(frame_dur);
    }
}

// --- life: Conway's Game of Life, reseeded on stagnation ---
fn fx_life(
    scr: &mut Screen,
    palette: &[String],
    intensity: i64,
    audio: &AudioState,
    use_theme_colors: bool,
    effect: &str,
) -> Result<()> {
    use rand::Rng;
    let mut rng = rand::thread_rng();
    let (cols, rows) = (scr.cols, scr.rows);
    let mut grid = vec![vec![false; cols]; rows];
    let mut next = vec![vec![false; cols]; rows];
    let mut age = vec![vec![0u8; cols]; rows];
    let density = 0.18 + audio.volume() * 0.1;
    for y in 0..rows {
        for x in 0..cols {
            grid[y][x] = rng.gen::<f32>() < density;
        }
    }
    let mut stagnant = 0u32;
    let mut cur_pal: Vec<String> = palette.to_vec();
    let mut theme_watcher = ThemeWatcher::new();
    loop {
        if scr.maybe_resize() {
            return Ok(());
        }
        if use_theme_colors {
            if let Some(new_colors) = theme_watcher.changed() {
                cur_pal = theme_palette(new_colors, effect);
                set_theme_palette(cur_pal.clone());
                log_dbg(&format!(
                    "theme colors changed: {effect} palette updated ({})",
                    cur_pal.len()
                ));
            }
        }
        scr.clear_dirty();
        for y in 0..rows {
            for x in 0..cols {
                if grid[y][x] {
                    age[y][x] = age[y][x].saturating_add(1);
                    let tint = if age[y][x] > 30 {
                        1
                    } else if age[y][x] > 8 {
                        2
                    } else {
                        3
                    };
                    scr.put(
                        x,
                        y,
                        if age[y][x] > 8 { 'O' } else { 'o' },
                        tint_to_color(tint, &cur_pal),
                    );
                } else {
                    age[y][x] = 0;
                }
            }
        }
        let frame_dur = render_frame_interval(
            frame_delay(70, live_intensity(intensity), live_speed(5), audio),
            scr.direct_snapshot.is_some(),
        );
        scr.fps_overlay(frame_dur.as_secs_f64() * 1000.0);
        scr.present();
        let mut changed = 0u32;
        for y in 0..rows {
            for x in 0..cols {
                let mut n = 0;
                for dy in [-1i32, 0, 1] {
                    for dx in [-1i32, 0, 1] {
                        if dy == 0 && dx == 0 {
                            continue;
                        }
                        let yy = (y as i32 + dy).rem_euclid(rows as i32) as usize;
                        let xx = (x as i32 + dx).rem_euclid(cols as i32) as usize;
                        if grid[yy][xx] {
                            n += 1;
                        }
                    }
                }
                next[y][x] = if grid[y][x] { n == 2 || n == 3 } else { n == 3 };
                if next[y][x] != grid[y][x] {
                    changed += 1;
                }
            }
        }
        std::mem::swap(&mut grid, &mut next);
        if changed < cols as u32 / 8 {
            stagnant += 1;
            if stagnant > 30 {
                stagnant = 0;
                let density = 0.18 + audio.volume() * 0.1;
                for y in 0..rows {
                    for x in 0..cols {
                        if rng.gen::<f32>() < density * 0.25 {
                            grid[y][x] = true;
                        }
                    }
                }
            }
        } else {
            stagnant = 0;
        }
        thread::sleep(frame_dur);
    }
}

#[cfg(test)]
mod audio_lifecycle_tests {
    use super::*;

    #[test]
    fn capture_abandons_audio_when_its_renderer_drops() {
        let capture = AudioState::start(false);
        let renderer = capture.clone();
        assert!(!capture.capture_orphaned());
        drop(renderer);
        assert!(capture.capture_orphaned());
    }
}

#[cfg(test)]
mod pace_tests {
    use super::*;
    use std::time::{Duration, Instant};

    #[test]
    fn without_a_ceiling_nothing_is_scheduled() {
        assert!(pace_to_cap(None, None, Instant::now()).is_none());
    }

    #[test]
    fn frames_are_spaced_one_period_apart() {
        let t0 = Instant::now();
        let due1 = pace_to_cap(Some(50.0), None, t0).unwrap();
        assert_eq!(due1 - t0, Duration::from_millis(20));
        let t1 = t0 + Duration::from_millis(5);
        let started = Instant::now();
        let due2 = pace_to_cap(Some(50.0), Some(due1), t1).unwrap();
        assert!(started.elapsed() >= Duration::from_millis(10), "waited for the deadline");
        assert_eq!(due2 - t0, Duration::from_millis(40));
    }

    #[test]
    fn a_long_gap_restarts_the_cadence() {
        let t0 = Instant::now();
        let late = t0 + Duration::from_secs(1);
        let due = pace_to_cap(Some(50.0), Some(t0), late).unwrap();
        assert_eq!(due - late, Duration::from_millis(20));
    }
}

#[cfg(test)]
mod speed_tests {
    use super::*;

    #[test]
    fn speed_panel_displays_multiplier_and_reaches_twenty() {
        let panel = include_str!("../../../Panel.qml");
        assert!(panel.contains("(root.speed / 5).toFixed(1) + \"x\""));
        assert!(panel.contains("minimum: 1; maximum: 100; step: 1; integer: true"));
        assert!(panel.contains("Math.min(100, s.speed)"));
    }

    #[test]
    fn speed_writer_round_trips_and_bounds_values() {
        let home = std::env::temp_dir().join(format!(
            "ttfx-speed-test-{}-{}",
            std::process::id(),
            rand::random::<u64>()
        ));
        std::fs::create_dir_all(&home).unwrap();
        let state = home.join(".local/state/omarchy/audio-background/state.json");
        let write = |args: &[&str]| {
            let result = std::process::Command::new("sh")
                .arg(concat!(env!("CARGO_MANIFEST_DIR"), "/../write_state.sh"))
                .args(args)
                .env("HOME", &home)
                .output()
                .unwrap();
            assert!(result.status.success(), "{:?}", result);
            std::fs::read_to_string(&state).unwrap()
        };
        assert_eq!(json_num(&write(&[]), "speed"), Some(5));
        for (input, expected) in [
            ("1", 1),
            ("5", 5),
            ("20", 20),
            ("100", 100),
            ("101", 100),
            ("999999999999999999999", 100),
            ("0", 1),
            ("-1", 1),
            ("oops", 5),
        ] {
            let text = write(&[&format!("speed={input}"), "effect=rain"]);
            assert_eq!(json_num(&text, "speed"), Some(expected), "input={input}");
            assert_eq!(json_str(&text, "effect").as_deref(), Some("rain"));
        }
        write(&["speed=100"]);
        assert_eq!(json_num(&write(&["audio=0"]), "speed"), Some(100));
        std::fs::remove_dir_all(home).unwrap();
    }

    #[test]
    fn speed_pacing_is_shared_and_clamped_without_a_six_ms_ceiling() {
        let audio = AudioState::start(false);
        for value in [1, 5, 20, 100] {
            let expected = Duration::from_millis(8).div_f64(value as f64 / 5.0);
            assert_eq!(speed_delay(Duration::from_millis(8), value), expected);
            assert_eq!(frame_delay(8, 0, value, &audio), expected);
        }
        assert_eq!(speed_multiplier(i64::MIN), 0.2);
        assert_eq!(speed_multiplier(i64::MAX), 20.0);
    }

    #[test]
    fn render_pacing_ceiling_allows_at_least_60_fps() {
        assert!(MIN_RENDER_FRAME <= Duration::from_micros(16_667));
    }

    #[test]
    fn speed_default_is_unchanged_and_max_is_twenty_times_faster() {
        let audio = AudioState::start(false);
        assert_eq!(Config::default().speed, 5);
        let baseline = frame_delay(40, 5, 5, &audio);
        assert_eq!(baseline, Duration::from_millis(25));
        assert_eq!(frame_delay(40, 5, 100, &audio), baseline.div_f64(20.0));
    }
}

#[cfg(test)]
mod direct_frame_tests {
    use super::*;

    #[test]
    fn raster_profiles_preserve_catalog_and_exact_native_effect_ids() {
        let catalog: Vec<_> = TTFX_EFFECTS
            .iter()
            .copied()
            .chain([
                "matrix",
                "rain",
                "wave",
                "bars",
                "donut",
                "fire",
                "starfield",
                "life",
            ])
            .collect();
        assert_eq!(catalog.len(), 43);
        let selected_full: Vec<_> = catalog
            .iter()
            .copied()
            .filter(|effect| !direct_retained_raster(effect, Some("retained")))
            .collect();
        assert_eq!(selected_full, ["matrix", "rain", "wave", "fire", "life"]);
        assert!(direct_retained_raster("waves", Some("retained")));
        assert!(!direct_retained_raster("wave", Some("retained")));
        for requested in [None, Some("full"), Some("unknown")] {
            assert!(catalog
                .iter()
                .all(|effect| !direct_retained_raster(effect, requested)));
        }
    }

    #[test]
    fn bounded_policy_only_overrides_native_wave_when_requested() {
        for effect in ["wave"] {
            assert!(bounded_damage_policy(effect, Some("bounded")));
            assert!(!direct_retained_raster(effect, Some("retained")));
            for request in [None, Some("legacy"), Some("unknown")] {
                assert!(!bounded_damage_policy(effect, request));
            }
        }
        for effect in TTFX_EFFECTS.iter().copied().chain(["waves", "fire", "life", "matrix", "rain", "bars", "donut", "starfield"]) {
            assert!(!bounded_damage_policy(effect, Some("bounded")));
        }
    }

    #[test]
    fn opaque_texture_format_only_applies_to_an_opaque_framebuffer() {
        for (transparent, requested, expected) in [
            (false, false, gdk::MemoryFormat::B8g8r8a8Premultiplied),
            (false, true, gdk::MemoryFormat::B8g8r8x8),
            (true, false, gdk::MemoryFormat::B8g8r8a8Premultiplied),
            (true, true, gdk::MemoryFormat::B8g8r8a8Premultiplied),
        ] {
            assert_eq!(direct_memory_format(transparent, requested), expected);
        }
    }

    #[test]
    fn owned_texture_bytes_preserve_pixels_without_a_second_copy() {
        let mut atlas = DirectGlyphAtlas::new(4, 7, 6.0);
        let cells = [DirectCell {
            x: 0,
            y: 0,
            ch: '█',
            color: (200, 100, 50),
            background: None,
            attrs: 0,
        }];
        let pixels = atlas.rasterize_pixels(&cells, 2, 1, 8, 7, true).unwrap();
        let original_ptr = pixels.as_ptr();
        let expected = pixels.clone();
        let bytes = memory_texture_bytes_owned(pixels);
        assert_eq!(bytes.as_ref(), expected.as_slice());
        assert_eq!(bytes.as_ref().as_ptr(), original_ptr);
    }

    #[test]
    fn direct_occupancy_tracks_sparse_cells_in_row_major_order_across_word_boundaries() {
        let mut occupied = DirectOccupancy::new(16, 10);
        for (x, y) in [(0, 0), (15, 3), (0, 4), (1, 8)] {
            occupied.set(x, y);
        }
        assert_eq!(
            occupied.iter().collect::<Vec<_>>(),
            vec![(0, 0), (15, 3), (0, 4), (1, 8)]
        );
        occupied.unset(0, 4);
        occupied.unset(0, 4);
        assert_eq!(
            occupied.iter().collect::<Vec<_>>(),
            vec![(0, 0), (15, 3), (1, 8)]
        );
        occupied.clear();
        assert_eq!(occupied.iter().count(), 0);
    }

    #[test]
    fn direct_effect_pacing_has_no_extra_frame_ceiling() {
        let fast = Duration::from_millis(2);
        assert_eq!(render_frame_interval(fast, true), fast);
        assert_eq!(render_frame_interval(fast, false), MIN_RENDER_FRAME);
    }

    #[test]
    fn atlas_paints_rgb_cells_and_keeps_empty_background_transparent() {
        let mut atlas = DirectGlyphAtlas::new(4, 7, 6.0);
        let cells = [DirectCell {
            x: 1,
            y: 0,
            ch: '█',
            color: (200, 100, 50),
            background: None,
            attrs: 0,
        }];
        let mut surface = atlas.rasterize(&cells, 2, 1, 8, 7, true).unwrap();
        surface.flush();
        let data = surface.data().unwrap();
        assert_eq!(&data[0..4], &[0, 0, 0, 0]);
        assert!(data
            .chunks_exact(4)
            .any(|px| px[3] > 0 && px[2] > px[1] && px[1] > px[0]));
        assert!(data.chunks_exact(4).all(|px| px[3] == 0 || px[2] <= px[3]));
    }

    #[test]
    fn direct_screen_occupancy_tracks_erase_and_dirty_clears() {
        use std::sync::atomic::{AtomicBool, AtomicU64};
        let slot = Arc::new(std::sync::Mutex::new(Vec::new()));
        let mut scr = Screen::new_direct(
            70,
            2,
            slot.clone(),
            Arc::new(AtomicU64::new(0)),
            Arc::new(AtomicBool::new(false)),
        );
        scr.put(1, 0, 'A', "\x1b[32m".into());
        scr.put(64, 1, 'B', "\x1b[32m".into());
        assert_eq!(
            scr.occupied.as_ref().unwrap().iter().collect::<Vec<_>>(),
            vec![(1, 0), (64, 1)]
        );
        scr.erase(1, 0);
        assert_eq!(
            scr.occupied.as_ref().unwrap().iter().collect::<Vec<_>>(),
            vec![(64, 1)]
        );
        scr.clear_dirty();
        assert_eq!(scr.occupied.as_ref().unwrap().iter().count(), 0);
        scr.present();
        assert!(slot.lock().unwrap().is_empty());
    }

    #[test]
    #[ignore = "native-only producer/consumer microbenchmark; not a live effect benchmark"]
    fn benchmark_frame_handoff_copy_vs_swap() {
        let cells: Vec<DirectCell> = (0..176)
            .flat_map(|y| {
                (0..533).filter_map(move |x| {
                    ((x + y) % 4 == 0).then_some(DirectCell {
                        x,
                        y,
                        ch: 'A',
                        color: (20, 120, 80),
                        background: None,
                        attrs: 0,
                    })
                })
            })
            .collect();
        let measure = |swap: bool| {
            let slot = std::sync::Mutex::new(Vec::with_capacity(cells.len()));
            let generation = std::sync::atomic::AtomicU64::new(0);
            let consumed = std::cell::Cell::new(0);
            let mut visible = Vec::with_capacity(cells.len());
            let started = std::time::Instant::now();
            for _ in 0..2_000 {
                if let Ok(mut frame) = slot.lock() {
                    frame.clone_from(&cells);
                    generation.fetch_add(1, Ordering::Release);
                }
                if swap {
                    consume_direct_snapshot(&slot, &generation, &consumed, &mut visible);
                } else if let Ok(frame) = slot.lock() {
                    visible.clone_from(&frame);
                }
                std::hint::black_box(visible.as_slice());
            }
            started.elapsed().as_secs_f64() * 1_000_000.0 / 2_000.0
        };
        eprintln!(
            "handoff_clone_a1_us={:.4} handoff_swap_us={:.4} handoff_clone_a2_us={:.4}",
            measure(false),
            measure(true),
            measure(false)
        );
    }

    #[test]
    fn direct_consumer_does_not_replay_a_stale_frame_after_a_racing_publish() {
        use std::sync::atomic::{AtomicBool, AtomicU64};
        let slot = Arc::new(std::sync::Mutex::new(Vec::with_capacity(4)));
        let generation = Arc::new(AtomicU64::new(0));
        let mut screen = Screen::new_direct(
            2,
            1,
            slot.clone(),
            generation.clone(),
            Arc::new(AtomicBool::new(false)),
        );
        screen.put_direct_rgb(0, 0, 'A', Some((1, 2, 3)));
        screen.present();
        let observed = generation.load(Ordering::Acquire);
        // The producer publishes again after the consumer samples generation
        // but before it acquires the cell slot.
        screen.clear_dirty();
        screen.put_direct_rgb(1, 0, 'B', Some((4, 5, 6)));
        screen.present();
        let mut visible = Vec::new();
        let consumed = std::cell::Cell::new(0);
        if observed != consumed.get() {
            assert!(consume_direct_snapshot(
                &slot,
                &generation,
                &consumed,
                &mut visible,
            ));
        }
        assert_eq!(consumed.get(), 2);
        assert!(!consume_direct_snapshot(
            &slot,
            &generation,
            &consumed,
            &mut visible
        ));
        assert_eq!(visible[0].ch, 'B', "old visible cells must not be replayed");
    }

    #[test]
    fn direct_consumer_swaps_frames_without_copying_or_returning_stale_cells() {
        use std::sync::atomic::{AtomicBool, AtomicU64};
        let slot = Arc::new(std::sync::Mutex::new(Vec::with_capacity(32)));
        let generation = Arc::new(AtomicU64::new(0));
        let mut screen = Screen::new_direct(
            4,
            1,
            slot.clone(),
            generation.clone(),
            Arc::new(AtomicBool::new(false)),
        );
        let mut visible = Vec::with_capacity(32);
        let consumed = std::cell::Cell::new(0);
        let spare_ptr = visible.as_ptr();
        screen.put_direct_rgb(0, 0, 'A', Some((1, 2, 3)));
        screen.present();
        let published_ptr = slot.lock().unwrap().as_ptr();
        assert!(consume_direct_snapshot(
            &slot,
            &generation,
            &consumed,
            &mut visible
        ));
        assert_eq!(visible.as_ptr(), published_ptr);
        assert_eq!(slot.lock().unwrap().as_ptr(), spare_ptr);
        assert_eq!(visible[0].ch, 'A');

        screen.clear_dirty();
        screen.put_direct_rgb(1, 0, 'B', Some((4, 5, 6)));
        screen.present();
        assert!(consume_direct_snapshot(
            &slot,
            &generation,
            &consumed,
            &mut visible
        ));
        assert_eq!(visible.as_ptr(), spare_ptr);
        assert_eq!(
            visible,
            vec![DirectCell {
                x: 1,
                y: 0,
                ch: 'B',
                color: (4, 5, 6),
                background: None,
                attrs: 0,
            }]
        );
        assert_eq!(slot.lock().unwrap().as_ptr(), published_ptr);
    }

    #[test]
    fn direct_screen_reuses_the_publish_buffer_between_frames() {
        use std::sync::atomic::{AtomicBool, AtomicU64};
        let slot = Arc::new(std::sync::Mutex::new(Vec::with_capacity(32)));
        let mut screen = Screen::new_direct(
            4,
            1,
            slot,
            Arc::new(AtomicU64::new(0)),
            Arc::new(AtomicBool::new(false)),
        );
        screen.put(0, 0, 'A', "\x1b[32m".into());
        screen.present();
        let first_capacity = screen.direct_cells.as_ref().unwrap().capacity();
        screen.clear_dirty();
        screen.put(1, 0, 'B', "\x1b[32m".into());
        screen.present();
        assert!(screen.direct_cells.as_ref().unwrap().capacity() >= first_capacity);
    }

    #[test]
    fn every_catalog_effect_can_publish_a_typed_direct_frame() {
        use clap::Parser;
        use std::sync::atomic::{AtomicBool, AtomicU64};
        use ttfx::engine::canvas::Anchor;
        use ttfx::engine::ctx::{Clock, EngineCtx};
        use ttfx::engine::terminal::TerminalConfig;
        use ttfx::utils::rng::Rng;

        for effect_name in TTFX_EFFECTS {
            let mut effect = ttfx::cli::Cli::try_parse_from(["ttfx", effect_name])
                .unwrap()
                .effect
                .unwrap()
                .build_effect();
            let mut config = TerminalConfig::default();
            config.canvas_width = 80;
            config.canvas_height = 24;
            config.anchor_canvas = Anchor::C;
            config.anchor_text = Anchor::C;
            config.ignore_terminal_dimensions = true;
            let mut ctx = EngineCtx::new(
                &wordmark::wordmark_input("OMARCHY", 80, 24, "native", 2.0),
                config,
                Rng::from_entropy(),
                Clock::virtual_with_frame_rate(60),
            )
            .unwrap();
            ctx.final_text_bands = true;
            effect.build(&mut ctx).unwrap();
            let frame = effect
                .next_frame(&mut ctx)
                .unwrap_or_else(|| panic!("{effect_name} emitted no first frame"));
            let slot = Arc::new(std::sync::Mutex::new(Vec::new()));
            let generation = Arc::new(AtomicU64::new(0));
            let mut screen = Screen::new_direct(
                80,
                24,
                slot.clone(),
                generation.clone(),
                Arc::new(AtomicBool::new(false)),
            );
            feed_ttfx_frame(&mut screen, &frame);
            screen.present();
            assert_eq!(generation.load(Ordering::Relaxed), 1, "{effect_name}");
            assert!(
                screen.color.iter().flatten().all(String::is_empty),
                "{effect_name} retained SGR strings in direct mode"
            );
        }
    }

    #[test]
    fn direct_catalog_frames_store_predecoded_rgb_without_per_cell_sgr_strings() {
        use std::sync::atomic::{AtomicBool, AtomicU64};
        let slot = Arc::new(std::sync::Mutex::new(Vec::new()));
        let mut screen = Screen::new_direct(
            3,
            1,
            slot.clone(),
            Arc::new(AtomicU64::new(0)),
            Arc::new(AtomicBool::new(false)),
        );
        feed_ttfx_frame(&mut screen, "\x1b[38;2;12;34;56mABC\x1b[0m");
        assert!(screen.color[0].iter().all(String::is_empty));
        assert_eq!(
            screen.direct_colors.as_ref().unwrap()[0],
            vec![Some((12, 34, 56)), Some((12, 34, 56)), Some((12, 34, 56))]
        );
        screen.present();
        assert_eq!(slot.lock().unwrap().len(), 3);
    }

    #[test]
    fn direct_screen_publishes_typed_cells_and_replaces_stale_frames() {
        use std::sync::atomic::{AtomicBool, AtomicU64};
        use std::sync::Mutex;
        let cells = Arc::new(Mutex::new(Vec::new()));
        let generation = Arc::new(AtomicU64::new(0));
        let stop = Arc::new(AtomicBool::new(false));
        let mut screen = Screen::new_direct(3, 2, cells.clone(), generation.clone(), stop);
        screen.put(1, 0, 'A', "\x1b[38;2;12;34;56m".into());
        screen.present();
        assert_eq!(generation.load(Ordering::Relaxed), 1);
        assert_eq!(
            *cells.lock().unwrap(),
            vec![DirectCell {
                x: 1,
                y: 0,
                ch: 'A',
                color: (12, 34, 56),
                background: None,
                attrs: 0,
            }]
        );
        screen.clear_dirty();
        screen.put(0, 1, 'B', "\x1b[32m".into());
        screen.present();
        assert_eq!(generation.load(Ordering::Relaxed), 2);
        assert_eq!(
            *cells.lock().unwrap(),
            vec![DirectCell {
                x: 0,
                y: 1,
                ch: 'B',
                color: (0, 205, 0),
                background: None,
                attrs: 0,
            }]
        );
    }

    #[test]
    fn direct_screen_reuses_palette_rgb_across_cells_without_losing_color() {
        use std::sync::atomic::{AtomicBool, AtomicU64};
        use std::sync::Mutex;
        let cells = Arc::new(Mutex::new(Vec::new()));
        let mut screen = Screen::new_direct(
            4,
            1,
            cells.clone(),
            Arc::new(AtomicU64::new(0)),
            Arc::new(AtomicBool::new(false)),
        );
        screen.put(0, 0, 'A', "\x1b[38;2;16;32;48m".into());
        screen.put(1, 0, 'B', "\x1b[38;2;16;32;48m".into());
        screen.put(2, 0, 'C', "\x1b[38;2;48;32;16m".into());
        screen.put(3, 0, 'D', "invalid".into());
        screen.present();
        assert_eq!(
            *cells.lock().unwrap(),
            vec![
                DirectCell {
                    x: 0,
                    y: 0,
                    ch: 'A',
                    color: (16, 32, 48),
                    background: None,
                    attrs: 0,
                },
                DirectCell {
                    x: 1,
                    y: 0,
                    ch: 'B',
                    color: (16, 32, 48),
                    background: None,
                    attrs: 0,
                },
                DirectCell {
                    x: 2,
                    y: 0,
                    ch: 'C',
                    color: (48, 32, 16),
                    background: None,
                    attrs: 0,
                },
            ]
        );
    }

    #[test]
    fn disabled_auto_degrade_does_not_open_an_empty_observation_window() {
        assert!(!auto_degrade_window_active(false));
        assert!(auto_degrade_window_active(true));
    }

    #[test]
    fn both_backends_use_the_same_measured_grid() {
        // Cell metrics from Pango, not the font-size estimate, determine the
        // dimensions. Both GTK and VTE call this shared helper.
        assert_eq!(
            measured_grid(1280, 800, 1280.0 / 236.5, 800.0 / 78.5),
            (236, 78)
        );
        assert_eq!(measured_grid(30, 20, 8.0, 10.0), (80, 24));
    }

    #[test]
    fn asm_audio_poll_packs_treble_and_bpm_in_the_legacy_slots() {
        let state = AudioState::start(false);
        state.volume.store(1f32.to_bits(), Ordering::Relaxed);
        state.bands[0].store(0.5f32.to_bits(), Ordering::Relaxed);
        for band in NBANDS - NBANDS / 3..NBANDS {
            state.bands[band].store(0.25f32.to_bits(), Ordering::Relaxed);
        }
        state.beat.store(1, Ordering::Relaxed);
        state.bpm.store(119, Ordering::Relaxed);
        let audio = AsmAudio {
            state: &state,
            enabled: true,
            reactivity: LiveReactivity::new(2),
        };
        let encoded = asm_audio_poll((&audio as *const AsmAudio<'_>).cast_mut().cast());
        assert_eq!(encoded.to_le_bytes()[..5], [255, 127, 1, 63, 119]);
    }

    #[test]
    fn direct_backend_is_an_explicit_opt_in_while_vte_remains_the_fallback() {
        assert_eq!(
            render_backend_from_value(Some("direct")),
            RenderBackend::Direct
        );
        assert_eq!(render_backend_from_value(Some("VTE")), RenderBackend::Vte);
        assert_eq!(render_backend_from_value(None), RenderBackend::Vte);
    }

    #[test]
    fn direct_snapshot_decodes_sparse_colored_cells_without_terminal_state() {
        let cells = parse_direct_cells("0,1,A,#ff0080;12,3,█,#00ff40;2,4,\\\",#112233;", 20, 10);
        assert_eq!(cells.len(), 3);
        assert_eq!(
            cells[0],
            DirectCell {
                x: 0,
                y: 1,
                ch: 'A',
                color: (255, 0, 128),
                background: None,
                attrs: 0,
            }
        );
        assert_eq!(
            cells[1],
            DirectCell {
                x: 12,
                y: 3,
                ch: '█',
                color: (0, 255, 64),
                background: None,
                attrs: 0,
            }
        );
        assert_eq!(
            cells[2],
            DirectCell {
                x: 2,
                y: 4,
                ch: '"',
                color: (17, 34, 51),
                background: None,
                attrs: 0,
            }
        );
    }
}

#[cfg(test)]
mod selective_theme_tests {
    use super::*;
    use ttfx::utils::graphics::{Color, Gradient};

    #[test]
    fn burn_preserves_fire_and_themes_only_accents() {
        let theme = vec![("accent".to_string(), "#204080".to_string())];
        let transform = ttfx_theme_transform("burn", &theme).unwrap();
        let stops = ["ffffff", "fff75d", "fe650d", "8A003C", "510100"]
            .iter()
            .map(|hex| Color::from_hex(hex).unwrap())
            .collect::<Vec<_>>();
        for color in &Gradient::with_steps(&stops, 10, false).unwrap().spectrum {
            let rgb = parse_hex_color(&color.rgb_color.to_string()).unwrap();
            assert_eq!(
                transform(rgb.0, rgb.1, rgb.2),
                rgb,
                "fire color changed: {rgb:?}"
            );
        }
        assert_eq!(transform(0, 0, 0), (0, 0, 0));
        assert_eq!(transform(0, 195, 255), (32, 64, 128));
        assert_eq!(transform(128, 128, 128), (16, 32, 64));
    }

    #[test]
    fn every_effect_has_a_safe_policy() {
        let theme = vec![("accent".to_string(), "#204080".to_string())];
        for effect in TTFX_EFFECTS {
            let transform = ttfx_theme_transform(effect, &theme);
            assert!(transform.is_some(), "{effect} has no theme policy");
        }
    }

    #[test]
    fn missing_theme_configuration_keeps_every_ttfx_effect_native() {
        for effect in TTFX_EFFECTS {
            assert!(
                ttfx_theme_transform(effect, &[]).is_none(),
                "{effect} should retain its built-in palette without theme colors"
            );
        }
    }

    #[test]
    fn semantic_effects_map_only_final_accents() {
        let theme = vec![("accent".to_string(), "#204080".to_string())];
        let blackhole = ttfx_theme_transform("blackhole", &theme).unwrap();
        assert_eq!(blackhole(255, 204, 13), (255, 204, 13)); // explosion yellow
        assert_eq!(blackhole(138, 0, 138), (17, 35, 69)); // final magenta, value preserved
        let thunder = ttfx_theme_transform("thunderstorm", &theme).unwrap();
        assert_eq!(thunder(104, 163, 232), (104, 163, 232)); // lightning blue
        assert_eq!(thunder(138, 0, 138), (17, 35, 69)); // final magenta, value preserved
    }

    #[test]
    fn auto_degrade_requires_full_sustained_sub_10_fps_window() {
        assert!(!should_auto_degrade(90, 9.0)); // 10 FPS, but only 9 seconds
        assert!(!should_auto_degrade(149, 14.99));
        assert!(!should_auto_degrade(150, 15.0)); // exactly 10 FPS is healthy
        assert!(should_auto_degrade(149, 15.0)); // 9.93 FPS for full window
        assert!(should_auto_degrade(100, 20.0)); // 5 FPS sustained
    }

    #[test]
    fn embedded_catalog_matches_every_upstream_effect_except_local_matrix_and_rain() {
        use clap::CommandFactory;
        let mut upstream: Vec<String> = ttfx::cli::Cli::command()
            .get_subcommands()
            .map(|command| command.get_name().to_string())
            .filter(|name| name != "matrix" && name != "rain")
            .collect();
        let mut embedded: Vec<String> = TTFX_EFFECTS.iter().map(|name| name.to_string()).collect();
        upstream.sort();
        embedded.sort();
        assert_eq!(embedded, upstream);
    }

    #[test]
    fn panel_catalog_matches_embedded_catalog() {
        let panel = include_str!("../../../Panel.qml");
        let block = panel
            .split("readonly property var ttfxEffects: [")
            .nth(1)
            .unwrap()
            .split(']')
            .next()
            .unwrap();
        let mut panel_names: Vec<String> = block
            .split('"')
            .enumerate()
            .filter_map(|(index, part)| {
                if index % 2 == 1 {
                    Some(part.to_string())
                } else {
                    None
                }
            })
            .collect();
        let mut embedded: Vec<String> = TTFX_EFFECTS.iter().map(|name| name.to_string()).collect();
        panel_names.sort();
        embedded.sort();
        assert_eq!(panel_names, embedded);
    }

    #[test]
    fn every_safe_policy_changes_an_accent_between_two_themes() {
        let warm = vec![("accent".to_string(), "#ff8040".to_string())];
        let cool = vec![("accent".to_string(), "#4080ff".to_string())];
        for effect in TTFX_EFFECTS {
            let sample = if effect == "burn" {
                (0, 195, 255)
            } else {
                final_accent_set(effect)
                    .iter()
                    .copied()
                    .next()
                    .unwrap_or((138, 0, 138))
            };
            let a = ttfx_theme_transform(effect, &warm).unwrap()(sample.0, sample.1, sample.2);
            let b = ttfx_theme_transform(effect, &cool).unwrap()(sample.0, sample.1, sample.2);
            assert_ne!(
                a, b,
                "{effect} did not react to theme change for {sample:?}"
            );
        }
    }

    #[test]
    fn glyph_style_changes_only_the_ttfx_input_letters() {
        let native = ttfx_canvas_input("OMARCHY", 200, 64, "native");
        let block = ttfx_canvas_input("OMARCHY", 200, 64, "block");
        let lower_o = ttfx_canvas_input("OMARCHY", 200, 64, "lower_o");
        let upper_o = ttfx_canvas_input("OMARCHY", 200, 64, "upper_o");
        assert!(!native.contains('#'));
        assert!(native.contains('█'));
        assert!(block.contains('█'));
        assert!(!block.contains('#'));
        assert!(lower_o.contains('o'));
        assert!(!lower_o.contains('#'));
        assert!(upper_o.contains('O'));
        assert!(!upper_o.contains('#'));
    }
}

#[cfg(test)]
mod bars_regression_tests {
    use super::*;

    fn band_state(band: usize) -> AudioState {
        let audio = AudioState::start(false);
        audio.bands[band].store(1.0f32.to_bits(), Ordering::Relaxed);
        audio
    }

    #[test]
    fn bars_production_frame_reads_every_band_independently_at_236_columns() {
        for band in 0..NBANDS {
            let audio = band_state(band);
            let mut screen = Screen::new_quiet(236, 16);
            let mut heights = [0f32; NBANDS];
            paint_bars_frame(&mut screen, &audio, &mut heights, &["\x1b[31m".into()]);
            assert!(heights[band] > 0.0);
            assert!(heights
                .iter()
                .enumerate()
                .all(|(i, h)| i == band || *h == 0.0));
            let columns = bars_layout::columns_for_band(band, 236);
            let expected_height = heights[band] as usize;
            for y in 0..screen.rows {
                for x in 0..screen.cols {
                    let expected = columns.contains(&x) && y >= screen.rows - expected_height;
                    assert_eq!(
                        screen.grid[y][x] == '#',
                        expected,
                        "band={band},x={x},y={y}"
                    );
                }
            }
        }
    }

    #[test]
    fn bars_production_frame_keeps_silence_empty_and_small_grids_bounded() {
        let silent = AudioState::start(false);
        for cols in [0, 1, 7, 23, 24, 25, 47, 48, 236, 240] {
            for rows in [0, 1, 16] {
                let mut screen = Screen::new_quiet(cols, rows);
                let mut heights = [0f32; NBANDS];
                paint_bars_frame(&mut screen, &silent, &mut heights, &[]);
                assert!(screen.grid.iter().flatten().all(|c| *c == ' '));
                let all = AudioState::start(false);
                for b in &*all.bands {
                    b.store(1.0f32.to_bits(), Ordering::Relaxed);
                }
                paint_bars_frame(&mut screen, &all, &mut heights, &["\x1b[32m".into()]);
                if cols > 0 && rows >= 16 {
                    assert!(screen.grid.iter().flatten().any(|c| *c == '#'));
                }
            }
        }
    }

    #[test]
    fn bars_production_frame_matches_old_cells_and_smoothing_at_divisible_widths() {
        for cols in [48, 72, 120, 240, 480] {
            for palette in [
                vec![],
                vec!["\x1b[31m".into()],
                vec!["\x1b[31m".into(), "\x1b[32m".into(), "\x1b[33m".into()],
            ] {
                let mut now = Screen::new_quiet(cols, 16);
                let mut old = Screen::new_quiet(cols, 16);
                let audio = AudioState::start(false);
                let mut new_heights = [0f32; NBANDS];
                let mut old_heights = [0f32; NBANDS];
                for step in 0..8 {
                    for (i, band) in audio.bands.iter().enumerate() {
                        band.store((((i + step) % 5) as f32 / 4.0).to_bits(), Ordering::Relaxed);
                    }
                    now.clear_dirty();
                    old.clear_dirty();
                    paint_bars_frame(&mut now, &audio, &mut new_heights, &palette);
                    let maxh = old.rows as f32 * 0.9;
                    let bw = cols / NBANDS;
                    for b in 0..NBANDS {
                        let energy = audio.band_at(b * bw, cols);
                        let target = (energy * 1.1).min(1.0) * maxh;
                        old_heights[b] += (target - old_heights[b]) * 0.4;
                        let h = old_heights[b] as usize;
                        for y in 0..h.min(old.rows) {
                            let tint = if y as f32 > h as f32 * 0.7 {
                                1
                            } else if y as f32 > h as f32 * 0.4 {
                                2
                            } else {
                                3
                            };
                            for x in 0..bw.saturating_sub(1) {
                                old.put(
                                    b * bw + x,
                                    old.rows - 1 - y,
                                    '#',
                                    tint_to_color(tint, &palette),
                                );
                            }
                        }
                    }
                    assert_eq!(new_heights.map(f32::to_bits), old_heights.map(f32::to_bits));
                    assert_eq!(now.grid, old.grid);
                    assert_eq!(now.color, old.color);
                }
            }
        }
    }
}

#[cfg(test)]
mod publication_wakeup_tests {
    use super::*;
    use std::sync::atomic::{AtomicBool, AtomicU64};
    use std::sync::Mutex;

    #[test]
    fn native_publication_wakes_after_commit_and_shutdown_prevents_further_frames() {
        let slot = Arc::new(Mutex::new(Vec::new()));
        let generation = Arc::new(AtomicU64::new(0));
        let stop = Arc::new(AtomicBool::new(false));
        let wake = Arc::new(frame_wakeup::FrameWakeup::new(stop.clone()).unwrap());
        frame_wakeup::set_current(Some(wake.clone()));
        let mut screen = Screen::new_direct(1, 1, slot.clone(), generation.clone(), stop.clone());
        screen.present();
        assert_eq!(generation.load(Ordering::Acquire), 1);
        assert_eq!(wake.drain().unwrap(), Some(1));
        {
            let _guard = slot.lock().unwrap();
            stop.store(true, Ordering::Release);
            wake.disable();
        }
        screen.present();
        assert_eq!(generation.load(Ordering::Acquire), 1);
        assert_eq!(wake.drain().unwrap(), None);
        frame_wakeup::set_current(None);
    }

    #[test]
    fn local_fd_source_drains_events_and_removal_releases_its_fd_owner() {
        let context = gtk4::glib::MainContext::default();
        let _owner = context.acquire().unwrap();
        let stop = Arc::new(AtomicBool::new(false));
        let wake = Arc::new(frame_wakeup::FrameWakeup::new(stop).unwrap());
        let weak = Arc::downgrade(&wake);
        let seen = Rc::new(std::cell::Cell::new(0u64));
        let callback_seen = seen.clone();
        let callback_wake = wake.clone();
        let source = gtk4::glib::source::unix_fd_add_local(
            wake.fd(), gtk4::glib::IOCondition::IN, move |_, _| {
                callback_seen.set(callback_seen.get() + callback_wake.drain().unwrap().unwrap_or(0));
                gtk4::glib::ControlFlow::Continue
            },
        );
        for _ in 0..3 { wake.notify().unwrap(); }
        for _ in 0..8 {
            context.iteration(false);
            if seen.get() != 0 { break; }
        }
        assert_eq!(seen.get(), 3);
        source.remove();
        wake.notify().unwrap();
        context.iteration(false);
        assert_eq!(seen.get(), 3, "removed source must not dispatch again");
        drop(wake);
        assert!(weak.upgrade().is_none(), "removed closure must release its Arc");
    }
}

#[cfg(test)]
mod live_reactivity_tests {
    use super::*;
    #[test]
    fn slider_zero_to_nonzero_reaches_both_asm_callbacks_without_layer_restart() {
        let state=AudioState::start(false);
        state.volume.store(0.5f32.to_bits(),Ordering::Relaxed);
        state.bands[0].store(0.5f32.to_bits(),Ordering::Relaxed);
        let audio=AsmAudio{state:&state,enabled:true,reactivity:LiveReactivity::new(0)};
        let start=audio.reactivity.checked.get();
        let ctx=(&audio as *const AsmAudio<'_>).cast_mut().cast();
        let mut out=[0f32;10];
        assert_eq!(asm_audio_poll(ctx),0);
        assert_eq!(asm_audio_params(ctx,out.as_mut_ptr()),0);
        assert_eq!(audio.reactivity.current_with(start+Duration::from_millis(249),||panic!("unthrottled config read")),0);
        assert_eq!(audio.reactivity.current_with(start+Duration::from_millis(250),||4),4);
        // Use a fresh monotonic sample time to simulate the actual callback window.
        audio.reactivity.checked.set(std::time::Instant::now());
        assert_ne!(asm_audio_poll(ctx),0);
        assert_eq!(asm_audio_params(ctx,out.as_mut_ptr()),1);
        assert!(out[0]>1.);
        let now=audio.reactivity.checked.get();
        assert_eq!(audio.reactivity.current_with(now+Duration::from_millis(250),||0),0);
        audio.reactivity.checked.set(std::time::Instant::now());
        assert_eq!(asm_audio_poll(ctx),0);
        assert_eq!(asm_audio_params(ctx,out.as_mut_ptr()),0);
    }
}

#[cfg(test)]
mod native_matrix_diagnostic_tests {
    use super::*;
    use std::time::Instant;
    #[test]
    fn matrix_route_and_default_sleep_are_native_and_25ms() {
        assert!(!is_ttfx_effect("matrix"));
        assert!(!is_ttfx_effect("rain"));
        let audio = AudioState::start(false);
        assert_eq!(frame_delay(40, 5, 5, &audio), Duration::from_millis(25));
    }
    #[test]
    #[ignore = "helper aislado invocado por native_matrix_produces_diagnostic_log"]
    fn native_probe_child() {
        use std::sync::{Arc, Mutex, atomic::{AtomicU64, AtomicBool, Ordering}};
        let generation = Arc::new(AtomicU64::new(0));
        let stop = Arc::new(AtomicBool::new(false));
        let slot = Arc::new(Mutex::new(Vec::new()));
        let (g,s) = (generation.clone(),stop.clone());
        let worker = std::thread::spawn(move || {
            INTENSITY_WATCHER.with(|w| *w.borrow_mut()=IntensityWatcher {last_check:Instant::now(),intensity:5});
            SPEED_WATCHER.with(|w| *w.borrow_mut()=SpeedWatcher {last_check:Instant::now(),speed:5});
            let cfg=Config {audio:false,use_theme_colors:false,..Config::default()};
            run_direct_render(&cfg,"matrix",80,24,false,2.,slot,g,s)
        });
        let deadline=Instant::now()+Duration::from_secs(2);
        while generation.load(Ordering::Acquire)<3 && Instant::now()<deadline {
            std::thread::sleep(Duration::from_millis(1));
        }
        stop.store(true,Ordering::Release);
        worker.join().unwrap().unwrap();
        assert!(generation.load(Ordering::Acquire)>=3);
    }
    #[test]
    fn native_matrix_produces_diagnostic_log() {
        let dir=std::env::temp_dir().join(format!("native-matrix-probe-{}-{}",std::process::id(),producer_probe::monotonic_ns()));
        std::fs::create_dir(&dir).unwrap();
        let status=std::process::Command::new(std::env::current_exe().unwrap())
            .args(["--exact","native_matrix_diagnostic_tests::native_probe_child","--ignored"])
            .env("TTFX_BG_PRODUCER_LOG",dir.join("trace"))
            .env("TTFX_BG_CADENCE_SEED","12345").env("TTFX_ASM_TIMING","1").env_remove("TTFX_BG_NATIVE_SEED")
            .status().unwrap();
        assert!(status.success());
        let file=std::fs::read_dir(&dir).unwrap().next().unwrap().unwrap().path();
        let events:Vec<serde_json::Value>=std::fs::read_to_string(file).unwrap().lines()
            .map(|l|serde_json::from_str(l).unwrap()).collect();
        assert!(events.iter().any(|e| e["event"]=="producer_route" && e["route"]=="native_matrix" && e["seed_controlled"]==false && e["asm_seed_ignored"]==true));
        let frames:Vec<_>=events.iter().filter(|e|e["event"]=="native_matrix_frame").collect();
        assert!(frames.len()>=3);
        for e in frames {
            assert_eq!(e["sleep_requested_ns"],25_000_000_u64);
            assert!(e["sleep_elapsed_ns"].as_u64().unwrap()>=25_000_000);
            assert!(e["generation"].as_u64().unwrap()>0);
        }
        std::fs::remove_dir_all(dir).unwrap();
    }
}

#[cfg(test)]
mod native_swap_equivalence_tests {
    use super::*;
    use rand::SeedableRng;
    #[test]
    fn seeded_native_matrix_publications_are_identical_for_clone_and_swap() {
        fn run(swap:bool)->Vec<Vec<DirectCell>> {
            let slot=Arc::new(std::sync::Mutex::new(Vec::new()));
            let generation=Arc::new(std::sync::atomic::AtomicU64::new(0));
            let stop=Arc::new(std::sync::atomic::AtomicBool::new(false));
            let mut screen=Screen::new_direct(237,61,slot,generation.clone(),stop);
            screen.native_swap=swap;
            INTENSITY_WATCHER.with(|w| *w.borrow_mut()=IntensityWatcher {last_check:std::time::Instant::now()+Duration::from_secs(60),intensity:5});
            SPEED_WATCHER.with(|w| *w.borrow_mut()=SpeedWatcher {last_check:std::time::Instant::now()+Duration::from_secs(60),speed:5});
            native_publication::capture_begin(24);
            let mut rng=rand::rngs::StdRng::seed_from_u64(12345);
            let audio=AudioState::start(false);
            fx_matrix_with_rng(&mut screen,&hardcoded_palette("matrix"),5,&audio,false,false,"matrix",&mut rng,Some(12345)).unwrap();
            let frames=native_publication::capture_finish();
            assert_eq!(generation.load(Ordering::Acquire),24);
            frames
        }
        let a=run(false); let b=run(true);
        assert_eq!(a,b); // Campos, orden, caracteres, RGB, ocupación y número de pasos.
        assert_eq!(a.len(),24);
        assert!(a.windows(2).any(|w|w[0]!=w[1]));
    }
}
