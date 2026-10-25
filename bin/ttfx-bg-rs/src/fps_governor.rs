//! Frame-rate ceiling, fixed or automatic.
//!
//! On Hyprland every commit of a layer surface repaints the whole output, so
//! the cost of an animated background is proportional to its frame rate on
//! any machine. `TTFX_BG_MAX_FPS=<n>` fixes the ceiling. `TTFX_BG_MAX_FPS=auto`
//! measures what one frame costs this process in CPU time and picks the
//! highest ceiling that keeps the process inside a CPU budget (default 8% of
//! one core, `TTFX_BG_CPU_BUDGET` as a fraction). A 2015 MacBook Pro lands
//! around 24 fps; a current machine releases the ceiling up to 60 fps.

use std::sync::atomic::{AtomicU32, AtomicU64, Ordering};
use std::time::Duration;

/// Ceilings the governor moves between. Animation speed follows the frame
/// rate, so the ceiling changes rarely and by one level at a time.
pub const LEVELS: [f64; 5] = [20.0, 24.0, 30.0, 40.0, 60.0];
pub const AUTO_START_FPS: f64 = 24.0;
const DEFAULT_BUDGET: f64 = 0.08;
/// Step up only when the next level would still sit comfortably inside the
/// budget; step down only when the current one clearly exceeds it.
const UP_MARGIN: f64 = 1.6;
const DOWN_MARGIN: f64 = 0.9;
const PERIOD: Duration = Duration::from_secs(5);
/// Periods ignored after start: they include loading, the intro and warm-up.
const WARMUP_PERIODS: u32 = 2;
const MIN_TICKS_PER_PERIOD: u64 = 40;
/// Weight of the newest period in the running cost estimate.
const EMA_ALPHA: f64 = 0.25;

/// Current ceiling in millihertz; 0 means no ceiling.
static CAP_MILLI: AtomicU32 = AtomicU32::new(0);
/// Producer ticks: one per animation frame computed, drawn or not. Counting
/// ticks rather than presented frames keeps the cost estimate stable when an
/// effect goes quiet and skips redraws.
static TICKS: AtomicU64 = AtomicU64::new(0);

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Mode {
    /// No ceiling beyond the renderer's own 60 fps floor.
    Unlimited,
    Fixed(f64),
    Auto,
}

pub fn parse_mode(value: Option<&str>) -> Mode {
    match value.map(str::trim) {
        None | Some("") => Mode::Unlimited,
        Some(v) if v.eq_ignore_ascii_case("auto") => Mode::Auto,
        Some(v) => match v.parse::<f64>() {
            Ok(fps) if fps.is_finite() && fps >= 1.0 => Mode::Fixed(fps.min(240.0)),
            _ => Mode::Unlimited,
        },
    }
}

pub fn cap() -> Option<f64> {
    match CAP_MILLI.load(Ordering::Relaxed) {
        0 => None,
        m => Some(m as f64 / 1000.0),
    }
}

fn set_cap(fps: f64) {
    CAP_MILLI.store((fps * 1000.0).round() as u32, Ordering::Relaxed);
}

/// Called once per animation frame the producer computes.
pub fn note_tick() {
    TICKS.fetch_add(1, Ordering::Relaxed);
}

/// The ceiling for the next period given the current one and what a frame
/// costs in process CPU time: one level up when the next level fits the
/// budget with margin, one level down when the current one exceeds it.
pub fn next_cap(current: f64, cost_us_per_frame: f64, budget: f64) -> f64 {
    if cost_us_per_frame <= 0.0 {
        return current;
    }
    let affordable = budget * 1_000_000.0 / cost_us_per_frame;
    let index = LEVELS.iter().position(|l| *l >= current).unwrap_or(LEVELS.len() - 1);
    let level = LEVELS[index];
    if index + 1 < LEVELS.len() && affordable >= LEVELS[index + 1] * UP_MARGIN {
        LEVELS[index + 1]
    } else if index > 0 && affordable < level * DOWN_MARGIN {
        LEVELS[index - 1]
    } else {
        level
    }
}

fn process_cpu_us() -> u64 {
    let mut ts = libc::timespec { tv_sec: 0, tv_nsec: 0 };
    unsafe { libc::clock_gettime(libc::CLOCK_PROCESS_CPUTIME_ID, &mut ts) };
    ts.tv_sec as u64 * 1_000_000 + ts.tv_nsec as u64 / 1000
}

/// Apply the mode; in `Auto` this spawns the measuring thread. `log` receives
/// one line per ceiling change.
pub fn start(mode: Mode, log: impl Fn(String) + Send + 'static) {
    match mode {
        Mode::Unlimited => CAP_MILLI.store(0, Ordering::Relaxed),
        Mode::Fixed(fps) => set_cap(fps),
        Mode::Auto => {
            set_cap(AUTO_START_FPS);
            let budget = std::env::var("TTFX_BG_CPU_BUDGET")
                .ok()
                .and_then(|v| v.trim().parse::<f64>().ok())
                .filter(|b| b.is_finite() && *b > 0.0 && *b <= 1.0)
                .unwrap_or(DEFAULT_BUDGET);
            log(format!("fps governor: auto, budget {:.1}% of one core, starting at {AUTO_START_FPS} fps", budget * 100.0));
            std::thread::Builder::new()
                .name("ttfx-fps-gov".into())
                .spawn(move || {
                    let mut last_cpu = process_cpu_us();
                    let mut last_ticks = TICKS.load(Ordering::Relaxed);
                    let mut periods = 0u32;
                    let mut cost_ema: Option<f64> = None;
                    loop {
                        std::thread::sleep(PERIOD);
                        let cpu = process_cpu_us();
                        let ticks = TICKS.load(Ordering::Relaxed);
                        let (d_cpu, d_ticks) = (cpu - last_cpu, ticks - last_ticks);
                        last_cpu = cpu;
                        last_ticks = ticks;
                        periods += 1;
                        // Paused (covered, locked, screen off) or still warming up: no data.
                        if periods <= WARMUP_PERIODS || d_ticks < MIN_TICKS_PER_PERIOD {
                            continue;
                        }
                        let sample = d_cpu as f64 / d_ticks as f64;
                        let cost = match cost_ema {
                            Some(prev) => prev + EMA_ALPHA * (sample - prev),
                            None => sample,
                        };
                        cost_ema = Some(cost);
                        let current = cap().unwrap_or(AUTO_START_FPS);
                        let next = next_cap(current, cost, budget);
                        if (next - current).abs() >= 0.5 {
                            set_cap(next);
                            log(format!(
                                "fps governor: {current:.0} -> {next:.0} fps ({cost:.0} us/frame avg, {sample:.0} now, {:.1}% cpu over {d_ticks} frames)",
                                d_cpu as f64 / PERIOD.as_micros() as f64 * 100.0
                            ));
                        }
                    }
                })
                .expect("spawn fps governor");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn modes_parse() {
        assert_eq!(parse_mode(None), Mode::Unlimited);
        assert_eq!(parse_mode(Some("auto")), Mode::Auto);
        assert_eq!(parse_mode(Some(" AUTO ")), Mode::Auto);
        assert_eq!(parse_mode(Some("24")), Mode::Fixed(24.0));
        assert_eq!(parse_mode(Some("0")), Mode::Unlimited);
        assert_eq!(parse_mode(Some("nope")), Mode::Unlimited);
        assert_eq!(parse_mode(Some("1000")), Mode::Fixed(240.0));
    }

    #[test]
    fn an_old_machine_holds_24() {
        // mbp-intel: 1.3 to 2.5 ms of process CPU per frame with the 8% budget.
        for cost in [1700.0, 2100.0, 2500.0, 3300.0] {
            assert_eq!(next_cap(24.0, cost, 0.08), 24.0, "{cost}");
        }
    }

    #[test]
    fn a_heavy_effect_steps_down_one_level() {
        assert_eq!(next_cap(24.0, 4000.0, 0.08), 20.0);
        assert_eq!(next_cap(20.0, 10_000.0, 0.05), 20.0);
    }

    #[test]
    fn a_fast_machine_releases_the_ceiling_one_level_at_a_time() {
        let mut cap = AUTO_START_FPS;
        let mut path = vec![cap];
        while cap < LEVELS[LEVELS.len() - 1] {
            cap = next_cap(cap, 300.0, 0.05);
            path.push(cap);
            assert!(path.len() < 10);
        }
        assert_eq!(path, vec![24.0, 30.0, 40.0, 60.0]);
    }

    #[test]
    fn stepping_up_needs_margin_and_stepping_down_needs_excess() {
        // 1.0 ms affords 50 fps: not 1.6 x 60, so 40 holds; 30 needs 64 to step up.
        assert_eq!(next_cap(40.0, 1000.0, 0.05), 40.0);
        assert_eq!(next_cap(30.0, 1000.0, 0.05), 30.0);
        assert_eq!(next_cap(30.0, 700.0, 0.05), 40.0);
        // 1.3 ms affords 38 fps: below 0.9 x 40 only once under 36.
        assert_eq!(next_cap(40.0, 1300.0, 0.05), 40.0);
        assert_eq!(next_cap(40.0, 1400.0, 0.05), 30.0);
    }

    #[test]
    fn no_frames_means_no_change() {
        assert_eq!(next_cap(30.0, 0.0, 0.05), 30.0);
    }
}
