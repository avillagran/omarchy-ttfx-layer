//! Diagnóstico del hilo productor. No cambia timer slack, reloj ni pacing.
use std::{fs::OpenOptions, io::{BufWriter, Write}, time::{Duration, Instant}};
use serde_json::{json, Value};

pub fn monotonic_ns() -> u64 {
    let mut t = libc::timespec { tv_sec: 0, tv_nsec: 0 };
    unsafe { libc::clock_gettime(libc::CLOCK_MONOTONIC, &mut t); }
    t.tv_sec as u64 * 1_000_000_000 + t.tv_nsec as u64
}
pub fn thread_cpu_ns() -> u64 {
    let mut t = libc::timespec { tv_sec: 0, tv_nsec: 0 };
    unsafe { libc::clock_gettime(libc::CLOCK_THREAD_CPUTIME_ID, &mut t); }
    t.tv_sec as u64 * 1_000_000_000 + t.tv_nsec as u64
}
// Espejo de la decisión del pacer, sólo para observar su espera solicitada.
pub fn requested_wait_ns(cap: Option<f64>, next_due: Option<Instant>, now: Instant) -> u64 {
    let Some(cap) = cap else { return 0 };
    let period = Duration::from_secs_f64(1.0 / cap.max(1.0));
    let due = match next_due {
        Some(due) if now.duration_since(due) < period * 2 => due,
        _ => now,
    };
    due.saturating_duration_since(now).as_nanos() as u64
}

pub struct ProducerProbe {
    file: BufWriter<std::fs::File>,
    flush: Instant,
    failed: bool,
    pub sequence: u64,
}
impl ProducerProbe {
    pub fn open() -> anyhow::Result<Option<Self>> {
        let Some(prefix) = std::env::var_os("TTFX_BG_PRODUCER_LOG") else { return Ok(None) };
        let tid = unsafe { libc::syscall(libc::SYS_gettid) };
        let file = OpenOptions::new().write(true).create_new(true)
            .open(format!("{}.producer.{}.{tid}.jsonl", prefix.to_string_lossy(), std::process::id()))?;
        // PR_GET únicamente: se observa el valor del hilo que produce realmente.
        let slack = unsafe { libc::syscall(libc::SYS_prctl, libc::PR_GET_TIMERSLACK, 0_u64, 0_u64, 0_u64, 0_u64) };
        let error = (slack < 0).then(|| std::io::Error::last_os_error().raw_os_error());
        let mut probe = Self { file: BufWriter::new(file), flush: Instant::now(), failed: false, sequence: 0 };
        probe.record(json!({"event":"producer_start", "schema":1, "pid":std::process::id(), "tid":tid,
            "clock_id":libc::CLOCK_MONOTONIC, "timerslack_ns":if slack >= 0 {Some(slack)} else {None},
            "timerslack_errno":error, "asm_timing_requested":std::env::var("TTFX_ASM_TIMING").as_deref()==Ok("1"),
            "asm_timing_arch_supported":cfg!(target_arch="x86_64")}));
        Ok(Some(probe))
    }
    pub fn record(&mut self, mut event: Value) {
        if self.failed { return }
        event["client_monotonic_ns"] = json!(monotonic_ns());
        event["thread_cpu_ns"] = json!(thread_cpu_ns());
        if let Err(error) = writeln!(self.file, "{event}") {
            self.failed = true;
            eprintln!("producer-probe write failed: {error}");
        }
        if self.flush.elapsed().as_secs() >= 1 {
            if let Err(error) = self.file.flush() {
                self.failed = true;
                eprintln!("producer-probe flush failed: {error}");
            }
            self.flush = Instant::now();
        }
    }
}
impl Drop for ProducerProbe {
    fn drop(&mut self) {
        self.record(json!({"event":"producer_end", "frames":self.sequence}));
        let _ = self.file.flush();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn diagnostic_wait_plan_handles_uncapped_future_late_and_reset() {
        let now = Instant::now();
        assert_eq!(requested_wait_ns(None, Some(now+Duration::from_secs(1)), now), 0);
        assert_eq!(requested_wait_ns(Some(60.), None, now), 0);
        assert_eq!(requested_wait_ns(Some(60.), Some(now+Duration::from_millis(5)), now), 5_000_000);
        assert_eq!(requested_wait_ns(Some(60.), Some(now-Duration::from_millis(5)), now), 0);
        assert_eq!(requested_wait_ns(Some(60.), Some(now-Duration::from_secs(1)), now), 0);
    }
}
