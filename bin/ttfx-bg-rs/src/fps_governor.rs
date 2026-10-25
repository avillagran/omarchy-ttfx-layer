//! Frame-rate ceiling, fixed or automatic.
//!
//! On Hyprland every commit of a layer surface repaints the whole output, so
//! the cost of an animated background is proportional to its frame rate on
//! any machine. `TTFX_BG_MAX_FPS=<n>` fixes the ceiling. `TTFX_BG_MAX_FPS=auto`
//! measures what one frame costs this process in CPU time and picks the
//! highest ceiling that keeps the process inside a CPU budget (default 8% of
//! one core, `TTFX_BG_CPU_BUDGET` as a fraction). Where the kernel exposes
//! how busy the GPU is (Intel RC6 residency, AMD `gpu_busy_percent`), the
//! ceiling must also keep the whole GPU under `TTFX_BG_GPU_BUDGET` (default
//! 30%): on old laptops compositing each frame costs more GPU than drawing it
//! costs CPU. A 2015 MacBook Pro lands around 24 fps; a current machine
//! releases the ceiling up to 60 fps.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU32, AtomicU64, Ordering};
use std::time::{Duration, Instant};

/// Ceilings the governor moves between. Animation speed follows the frame
/// rate, so the ceiling changes rarely and by one level at a time.
pub const LEVELS: [f64; 5] = [20.0, 24.0, 30.0, 40.0, 60.0];
pub const AUTO_START_FPS: f64 = 24.0;
const DEFAULT_BUDGET: f64 = 0.08;
const DEFAULT_GPU_BUDGET: f64 = 0.30;
/// Step up only when the next level would still sit comfortably inside the
/// budget, for `UP_PERIODS` periods in a row; step down as soon as the
/// current one clearly exceeds it. Process CPU time per frame is noisier than
/// whole-GPU busy time, so it needs the wider margin.
const UP_MARGIN: f64 = 1.6;
const UP_MARGIN_GPU: f64 = 1.15;
const UP_PERIODS: u32 = 2;
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
#[cfg(test)]
pub fn next_cap(current: f64, cost_us_per_frame: f64, budget: f64) -> f64 {
    if cost_us_per_frame <= 0.0 {
        return current;
    }
    pick_level(current, affordable_fps(cost_us_per_frame, budget, None, 1.0))
}

/// Frame rates each budget allows; infinite when that resource is not measured.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Affordable {
    pub cpu: f64,
    pub gpu: f64,
}

/// `gpu_per_fps` is the fraction of the GPU kept busy per frame per second.
pub fn affordable_fps(cpu_us_per_frame: f64, cpu_budget: f64, gpu_per_fps: Option<f64>, gpu_budget: f64) -> Affordable {
    Affordable {
        cpu: if cpu_us_per_frame > 0.0 { cpu_budget * 1_000_000.0 / cpu_us_per_frame } else { f64::INFINITY },
        gpu: match gpu_per_fps {
            Some(per_fps) if per_fps > 0.0 => gpu_budget / per_fps,
            _ => f64::INFINITY,
        },
    }
}

/// One level up when the next level fits both budgets with margin, one level
/// down when the current one does not fit either of them.
pub fn pick_level(current: f64, a: Affordable) -> f64 {
    let index = LEVELS.iter().position(|l| *l >= current).unwrap_or(LEVELS.len() - 1);
    let level = LEVELS[index];
    if index > 0 && a.cpu.min(a.gpu) < level * DOWN_MARGIN {
        return LEVELS[index - 1];
    }
    if index + 1 < LEVELS.len() {
        let next = LEVELS[index + 1];
        if a.cpu >= next * UP_MARGIN && a.gpu >= next * UP_MARGIN_GPU {
            return next;
        }
    }
    level
}

/// How busy the GPU is, from the kernel.
enum GpuMeter {
    /// Milliseconds the GPU spent idle (Intel RC6); busy = 1 - idle / elapsed.
    IdleResidency(PathBuf),
    /// Instantaneous busy percentage (AMD), averaged over samples.
    BusyPercent(PathBuf),
}

impl GpuMeter {
    fn find(drm: &Path) -> Option<Self> {
        let mut cards: Vec<PathBuf> = std::fs::read_dir(drm)
            .ok()?
            .filter_map(|e| e.ok().map(|e| e.path()))
            .filter(|p| p.file_name().and_then(|n| n.to_str()).is_some_and(|n| n.starts_with("card") && !n.contains('-')))
            .collect();
        cards.sort();
        for card in cards {
            for idle in ["power/rc6_residency_ms", "gt/gt0/rc6_residency_ms", "device/tile0/gt0/gtidle/idle_residency_ms"] {
                let path = card.join(idle);
                if read_number(&path).is_some() {
                    return Some(GpuMeter::IdleResidency(path));
                }
            }
            let busy = card.join("device/gpu_busy_percent");
            if read_number(&busy).is_some() {
                return Some(GpuMeter::BusyPercent(busy));
            }
        }
        None
    }

    fn describe(&self) -> String {
        match self {
            GpuMeter::IdleResidency(p) | GpuMeter::BusyPercent(p) => p.display().to_string(),
        }
    }
}

fn read_number(path: &Path) -> Option<f64> {
    std::fs::read_to_string(path).ok()?.trim().parse().ok()
}

/// Busy fraction of the GPU over a period, sampled once a second.
struct GpuSampler {
    meter: GpuMeter,
    idle_start: f64,
    since: Instant,
    busy_sum: f64,
    samples: u32,
}

impl GpuSampler {
    fn new(meter: GpuMeter) -> Self {
        let idle_start = match &meter {
            GpuMeter::IdleResidency(p) => read_number(p).unwrap_or(0.0),
            GpuMeter::BusyPercent(_) => 0.0,
        };
        Self { meter, idle_start, since: Instant::now(), busy_sum: 0.0, samples: 0 }
    }

    fn sample(&mut self) {
        if let GpuMeter::BusyPercent(p) = &self.meter {
            if let Some(v) = read_number(p) {
                self.busy_sum += v / 100.0;
                self.samples += 1;
            }
        }
    }

    /// Busy fraction since the last call, and restart the period.
    fn take(&mut self) -> Option<f64> {
        let busy = match &self.meter {
            GpuMeter::IdleResidency(p) => {
                let idle = read_number(p)?;
                let elapsed_ms = self.since.elapsed().as_secs_f64() * 1000.0;
                let busy = 1.0 - (idle - self.idle_start) / elapsed_ms.max(1.0);
                self.idle_start = idle;
                Some(busy.clamp(0.0, 1.0))
            }
            GpuMeter::BusyPercent(_) => (self.samples > 0).then(|| self.busy_sum / self.samples as f64),
        };
        self.since = Instant::now();
        self.busy_sum = 0.0;
        self.samples = 0;
        busy
    }
}

fn budget_from_env(key: &str, default: f64) -> f64 {
    std::env::var(key)
        .ok()
        .and_then(|v| v.trim().parse::<f64>().ok())
        .filter(|b| b.is_finite() && *b > 0.0 && *b <= 1.0)
        .unwrap_or(default)
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
            let budget = budget_from_env("TTFX_BG_CPU_BUDGET", DEFAULT_BUDGET);
            let gpu_budget = budget_from_env("TTFX_BG_GPU_BUDGET", DEFAULT_GPU_BUDGET);
            let meter = GpuMeter::find(Path::new("/sys/class/drm"));
            log(format!(
                "fps governor: auto, budget {:.1}% of one core{}, starting at {AUTO_START_FPS} fps",
                budget * 100.0,
                meter.as_ref().map_or(String::new(), |m| format!(" and {:.0}% of the GPU ({})", gpu_budget * 100.0, m.describe()))
            ));
            std::thread::Builder::new()
                .name("ttfx-fps-gov".into())
                .spawn(move || {
                    let mut gpu = meter.map(GpuSampler::new);
                    let mut last_cpu = process_cpu_us();
                    let mut last_ticks = TICKS.load(Ordering::Relaxed);
                    let mut periods = 0u32;
                    let mut cpu_ema: Option<f64> = None;
                    let mut gpu_ema: Option<f64> = None;
                    let mut up_streak = 0u32;
                    let ema = |prev: Option<f64>, sample: f64| prev.map_or(sample, |p| p + EMA_ALPHA * (sample - p));
                    loop {
                        for _ in 0..PERIOD.as_secs() {
                            std::thread::sleep(Duration::from_secs(1));
                            if let Some(g) = gpu.as_mut() {
                                g.sample();
                            }
                        }
                        let cpu = process_cpu_us();
                        let ticks = TICKS.load(Ordering::Relaxed);
                        let (d_cpu, d_ticks) = (cpu - last_cpu, ticks - last_ticks);
                        last_cpu = cpu;
                        last_ticks = ticks;
                        let gpu_busy = gpu.as_mut().and_then(GpuSampler::take);
                        periods += 1;
                        // Paused (covered, locked, screen off) or still warming up: no data.
                        if periods <= WARMUP_PERIODS || d_ticks < MIN_TICKS_PER_PERIOD {
                            continue;
                        }
                        let fps_now = d_ticks as f64 / PERIOD.as_secs_f64();
                        let cpu_cost = ema(cpu_ema, d_cpu as f64 / d_ticks as f64);
                        cpu_ema = Some(cpu_cost);
                        if let Some(busy) = gpu_busy {
                            gpu_ema = Some(ema(gpu_ema, busy / fps_now));
                        }
                        let affordable = affordable_fps(cpu_cost, budget, gpu_ema, gpu_budget);
                        let current = cap().unwrap_or(AUTO_START_FPS);
                        let mut next = pick_level(current, affordable);
                        if next > current {
                            up_streak += 1;
                            if up_streak < UP_PERIODS {
                                next = current;
                            }
                        } else {
                            up_streak = 0;
                        }
                        if (next - current).abs() >= 0.5 {
                            up_streak = 0;
                            set_cap(next);
                            log(format!(
                                "fps governor: {current:.0} -> {next:.0} fps ({cpu_cost:.0} us/frame, {:.1}% cpu{} over {d_ticks} frames; affordable cpu {:.0} gpu {:.0})",
                                d_cpu as f64 / PERIOD.as_micros() as f64 * 100.0,
                                gpu_busy.map_or(String::new(), |b| format!(", gpu {:.0}% busy", b * 100.0)),
                                affordable.cpu,
                                affordable.gpu,
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
    fn the_gpu_budget_holds_an_old_laptop_at_24() {
        // mbp-intel: ~2 ms CPU per frame, ~0.96% GPU busy per fps.
        let a = affordable_fps(2000.0, 0.08, Some(0.0096), 0.30);
        assert!((a.gpu - 31.25).abs() < 0.1, "{a:?}");
        assert_eq!(pick_level(20.0, a), 24.0, "recovers from a slow start");
        assert_eq!(pick_level(24.0, a), 24.0, "does not climb to 30");
        assert_eq!(pick_level(30.0, a), 30.0, "30 fps is still inside the budget");
        assert_eq!(pick_level(30.0, affordable_fps(2000.0, 0.08, Some(0.012), 0.30)), 24.0, "over it, comes down");
        // A start-up period that looks expensive drops one level only.
        assert_eq!(pick_level(24.0, affordable_fps(2400.0, 0.08, Some(0.015), 0.30)), 20.0);
    }

    #[test]
    fn without_a_gpu_meter_only_the_cpu_counts() {
        assert_eq!(pick_level(30.0, affordable_fps(1600.0, 0.08, None, 0.30)), 30.0);
        assert_eq!(pick_level(30.0, affordable_fps(1000.0, 0.08, None, 0.30)), 40.0);
    }

    #[test]
    fn gpu_meters_are_found_in_sysfs() {
        let root = std::env::temp_dir().join(format!("ttfx-gov-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(root.join("card1-eDP-1")).unwrap();
        assert!(GpuMeter::find(&root).is_none());
        std::fs::create_dir_all(root.join("card1/device")).unwrap();
        std::fs::write(root.join("card1/device/gpu_busy_percent"), "17\n").unwrap();
        assert!(matches!(GpuMeter::find(&root), Some(GpuMeter::BusyPercent(_))));
        std::fs::create_dir_all(root.join("card0/power")).unwrap();
        std::fs::write(root.join("card0/power/rc6_residency_ms"), "123456\n").unwrap();
        assert!(matches!(GpuMeter::find(&root), Some(GpuMeter::IdleResidency(_))));
        std::fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn no_frames_means_no_change() {
        assert_eq!(next_cap(30.0, 0.0, 0.05), 30.0);
    }
}
