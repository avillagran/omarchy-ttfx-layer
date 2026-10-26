//! Instrumentación optativa: no abre archivos ni solicita feedback sin la variable.
use std::{fs::OpenOptions, io::{BufWriter, Write}, time::Instant};
use serde_json::{json, Value};

pub struct Probe {
    file: BufWriter<std::fs::File>,
    start: Instant,
    flush: Instant,
    failed: bool,
    pub submitted: u64,
    pub pending: usize,
}
impl Probe {
    pub fn open() -> anyhow::Result<Option<Self>> {
        let Some(prefix) = std::env::var_os("TTFX_BG_PRESENTATION_LOG") else { return Ok(None) };
        let path = format!("{}.{}.jsonl", prefix.to_string_lossy(), std::process::id());
        let file = OpenOptions::new().write(true).create_new(true).open(path)?;
        let mut probe = Self { file: BufWriter::new(file), start: Instant::now(), flush: Instant::now(), failed: false, submitted: 0, pending: 0 };
        probe.record(json!({"event":"start", "pid":std::process::id(), "schema":1}));
        Ok(Some(probe))
    }
    pub fn record(&mut self, mut value: Value) {
        if self.failed { return; }
        value["elapsed_ns"] = json!(self.start.elapsed().as_nanos() as u64);
        // Si el disco falla, se informa una vez y se desactiva en el consumidor.
        if let Err(error) = writeln!(self.file, "{value}") {
            eprintln!("presentation-probe write failed: {error}");
            self.failed = true;
        }
        if self.flush.elapsed().as_secs() >= 1 {
            let _ = self.file.flush();
            self.flush = Instant::now();
        }
    }
}
impl Drop for Probe {
    fn drop(&mut self) {
        self.record(json!({"event":"end", "submitted":self.submitted, "pending":self.pending}));
        let _ = self.file.flush();
    }
}
