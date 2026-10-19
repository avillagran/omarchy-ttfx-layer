//! Opt-in, content-free cadence metrics for one monitor/session.
//! Single producer and single GTK consumer. Record publication under the existing
//! frame-slot lock, passing the generation that was just committed. No frame
//! allocation, hashing, I/O or sleeps in the event methods. The separate writer
//! may snapshot once per second; cumulative snapshots are intentionally not atomic.
//! GTK texture enqueue is not compositor presentation or physical scanout.
use std::cell::RefCell;
use std::io::{self, Write};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Instant;

const UNSEEN: u64 = u64::MAX;
pub const HISTOGRAM_BINS: usize = 64;
pub const BIN_WIDTH_NS: u64 = 1_000_000;

thread_local! {
    static CURRENT: RefCell<Option<Arc<FrameCadence>>> = const { RefCell::new(None) };
}

pub fn set_current(cadence: Option<Arc<FrameCadence>>) {
    CURRENT.with(|current| *current.borrow_mut() = cadence);
}

#[cfg(test)]
pub fn current() -> Option<Arc<FrameCadence>> {
    CURRENT.with(|current| current.borrow().clone())
}

/// Producer hot path borrows TLS without incrementing an Arc reference count.
pub fn record_produced(generation: u64) {
    CURRENT.with(|current| {
        if let Some(cadence) = current.borrow().as_ref() {
            cadence.produced(generation);
        }
    });
}

pub fn opt_in(value: Option<&str>) -> bool {
    value == Some("1")
}

pub fn enabled_from_env() -> bool {
    opt_in(std::env::var("TTFX_BG_CADENCE").ok().as_deref())
}

#[derive(Default)]
struct Totals {
    samples: AtomicU64,
    sum_ns: AtomicU64,
    max_ns: AtomicU64,
}

impl Totals {
    fn observe(&self, ns: u64) {
        self.samples.fetch_add(1, Ordering::Relaxed);
        self.sum_ns.fetch_add(ns, Ordering::Relaxed);
        self.max_ns.fetch_max(ns, Ordering::Relaxed);
    }

    fn snapshot(&self) -> DurationTotals {
        DurationTotals {
            samples: self.samples.load(Ordering::Relaxed),
            sum_ns: self.sum_ns.load(Ordering::Relaxed),
            max_ns: self.max_ns.load(Ordering::Relaxed),
        }
    }
}

struct Histogram {
    bins: [AtomicU64; HISTOGRAM_BINS],
    totals: Totals,
}

impl Default for Histogram {
    fn default() -> Self {
        Self {
            bins: std::array::from_fn(|_| AtomicU64::new(0)),
            totals: Totals::default(),
        }
    }
}

impl Histogram {
    fn observe(&self, ns: u64) {
        let bin = (ns / BIN_WIDTH_NS).min((HISTOGRAM_BINS - 1) as u64) as usize;
        self.bins[bin].fetch_add(1, Ordering::Relaxed);
        self.totals.observe(ns);
    }

    fn snapshot(&self) -> HistogramSnapshot {
        HistogramSnapshot {
            bins: std::array::from_fn(|index| self.bins[index].load(Ordering::Relaxed)),
            totals: self.totals.snapshot(),
        }
    }
}

#[derive(Clone, Debug)]
pub struct DurationTotals {
    pub samples: u64,
    pub sum_ns: u64,
    pub max_ns: u64,
}

#[derive(Clone, Debug)]
pub struct HistogramSnapshot {
    pub bins: [u64; HISTOGRAM_BINS],
    pub totals: DurationTotals,
}

pub struct FrameCadence {
    epoch: Instant,
    produced: AtomicU64,
    consumed: AtomicU64,
    ticks: AtomicU64,
    enqueued: AtomicU64,
    feedback_frames: AtomicU64,
    feedback_unavailable: AtomicU64,
    feedback_expired_frames: AtomicU64,
    feedback_frame_gaps: AtomicU64,
    dropped_publications: AtomicU64,
    unknown_age: AtomicU64,
    anomalies: AtomicU64,
    last_produced_ns: AtomicU64,
    last_consumed_ns: AtomicU64,
    last_tick_ns: AtomicU64,
    last_enqueue_ns: AtomicU64,
    last_consumed_generation: AtomicU64,
    last_feedback_frame: AtomicU64,
    last_feedback_ns: AtomicU64,
    // Sequentially consistent sequence + fields make the cross-thread stamp
    // readable without another mutex. A concurrent publication makes age unknown.
    stamp_sequence: AtomicU64,
    stamp_generation: AtomicU64,
    stamp_ns: AtomicU64,
    producer_interval: Histogram,
    consumer_interval: Histogram,
    tick_interval: Histogram,
    enqueue_interval: Histogram,
    publication_age: Histogram,
    feedback_interval: Histogram,
    raster: Totals,
    submission: Totals,
}

impl FrameCadence {
    pub fn new() -> Self {
        Self {
            epoch: Instant::now(),
            produced: AtomicU64::new(0),
            consumed: AtomicU64::new(0),
            ticks: AtomicU64::new(0),
            enqueued: AtomicU64::new(0),
            feedback_frames: AtomicU64::new(0),
            feedback_unavailable: AtomicU64::new(0),
            feedback_expired_frames: AtomicU64::new(0),
            feedback_frame_gaps: AtomicU64::new(0),
            dropped_publications: AtomicU64::new(0),
            unknown_age: AtomicU64::new(0),
            anomalies: AtomicU64::new(0),
            last_produced_ns: AtomicU64::new(UNSEEN),
            last_consumed_ns: AtomicU64::new(UNSEEN),
            last_tick_ns: AtomicU64::new(UNSEEN),
            last_enqueue_ns: AtomicU64::new(UNSEEN),
            last_consumed_generation: AtomicU64::new(0),
            last_feedback_frame: AtomicU64::new(UNSEEN),
            last_feedback_ns: AtomicU64::new(UNSEEN),
            stamp_sequence: AtomicU64::new(0),
            stamp_generation: AtomicU64::new(0),
            stamp_ns: AtomicU64::new(0),
            producer_interval: Histogram::default(),
            consumer_interval: Histogram::default(),
            tick_interval: Histogram::default(),
            enqueue_interval: Histogram::default(),
            publication_age: Histogram::default(),
            feedback_interval: Histogram::default(),
            raster: Totals::default(),
            submission: Totals::default(),
        }
    }

    pub fn now_ns(&self) -> u64 {
        self.epoch.elapsed().as_nanos().min(u64::MAX as u128) as u64
    }

    fn interval(&self, last: &AtomicU64, histogram: &Histogram, now_ns: u64) {
        let previous = last.swap(now_ns, Ordering::Relaxed);
        if previous != UNSEEN {
            if let Some(interval) = now_ns.checked_sub(previous) {
                histogram.observe(interval);
            } else {
                self.anomalies.fetch_add(1, Ordering::Relaxed);
            }
        }
    }

    pub fn produced(&self, generation: u64) {
        self.produced_at(generation, self.now_ns());
    }

    pub fn produced_at(&self, generation: u64, now_ns: u64) {
        self.interval(&self.last_produced_ns, &self.producer_interval, now_ns);
        self.stamp_sequence.fetch_add(1, Ordering::SeqCst);
        self.stamp_generation.store(generation, Ordering::SeqCst);
        self.stamp_ns.store(now_ns, Ordering::SeqCst);
        self.stamp_sequence.fetch_add(1, Ordering::SeqCst);
        self.produced.fetch_add(1, Ordering::Relaxed);
    }

    pub fn consumed(&self, generation: u64) {
        self.consumed_at(generation, self.now_ns());
    }

    pub fn consumed_at(&self, generation: u64, now_ns: u64) {
        let previous = self.last_consumed_generation.load(Ordering::Relaxed);
        if generation <= previous {
            return; // Generation 0 means no publication; duplicate ticks are not frames.
        }
        self.last_consumed_generation
            .store(generation, Ordering::Relaxed);
        self.dropped_publications
            .fetch_add(generation - previous - 1, Ordering::Relaxed);
        self.consumed.fetch_add(1, Ordering::Relaxed);
        self.interval(&self.last_consumed_ns, &self.consumer_interval, now_ns);
        let before = self.stamp_sequence.load(Ordering::SeqCst);
        let stamp_generation = self.stamp_generation.load(Ordering::SeqCst);
        let stamp_ns = self.stamp_ns.load(Ordering::SeqCst);
        let after = self.stamp_sequence.load(Ordering::SeqCst);
        if before != 0 && before == after && before % 2 == 0 && stamp_generation == generation {
            if let Some(age) = now_ns.checked_sub(stamp_ns) {
                self.publication_age.observe(age);
                return;
            }
            self.anomalies.fetch_add(1, Ordering::Relaxed);
        }
        self.unknown_age.fetch_add(1, Ordering::Relaxed);
    }

    pub fn tick(&self) {
        self.tick_at(self.now_ns());
    }

    pub fn tick_at(&self, now_ns: u64) {
        self.ticks.fetch_add(1, Ordering::Relaxed);
        self.interval(&self.last_tick_ns, &self.tick_interval, now_ns);
    }

    /// Every raster attempt, including an error; no assumption of presentation.
    pub fn raster(&self, elapsed_ns: u64) {
        self.raster.observe(elapsed_ns);
    }

    /// Successful MemoryTexture/set_paintable enqueue; CPU call duration only.
    pub fn enqueue(&self, elapsed_ns: u64) {
        self.enqueue_at(self.now_ns(), elapsed_ns);
    }

    pub fn enqueue_at(&self, now_ns: u64, elapsed_ns: u64) {
        self.enqueued.fetch_add(1, Ordering::Relaxed);
        self.submission.observe(elapsed_ns);
        self.interval(&self.last_enqueue_ns, &self.enqueue_interval, now_ns);
    }

    /// GTK thread only; call ONLY once timings.is_complete() is true. Incomplete
    /// timings can be retried later without counting them. Supply presentation
    /// time in ns (GDK reports us), never predicted_presentation_time. None/0 is
    /// unavailable; completed feedback is not evidence of physical scanout.
    /// Some Wayland backends first complete with a callback-based estimate and
    /// later revise presentation_time; this records GDK's value at observation.
    /// Its clock domain need not match epoch: only feedback-to-feedback deltas.
    pub fn feedback_completed(&self, frame_counter: u64, presentation_ns: Option<u64>) {
        self.feedback_record(frame_counter, presentation_ns, false);
    }

    /// History expired before timing completion: unknown, never completed/displayed.
    pub fn feedback_expired(&self, frame_counter: u64) {
        self.feedback_record(frame_counter, None, true);
    }

    fn feedback_record(&self, frame_counter: u64, presentation_ns: Option<u64>, expired: bool) {
        let previous = self.last_feedback_frame.load(Ordering::Relaxed);
        if previous != UNSEEN && frame_counter <= previous {
            return;
        }
        self.last_feedback_frame
            .store(frame_counter, Ordering::Relaxed);
        if previous != UNSEEN {
            self.feedback_frame_gaps
                .fetch_add(frame_counter - previous - 1, Ordering::Relaxed);
        }
        match presentation_ns.filter(|&ns| ns > 0) {
            Some(ns) => {
                self.feedback_frames.fetch_add(1, Ordering::Relaxed);
                self.interval(&self.last_feedback_ns, &self.feedback_interval, ns);
            }
            None => {
                if expired {
                    self.feedback_expired_frames.fetch_add(1, Ordering::Relaxed);
                } else {
                    self.feedback_unavailable.fetch_add(1, Ordering::Relaxed);
                }
                // A gap in unavailable feedback is not itself measured jank.
                self.last_feedback_ns.store(UNSEEN, Ordering::Relaxed);
            }
        }
    }

    pub fn snapshot(&self) -> Snapshot {
        Snapshot {
            elapsed_ns: self.now_ns(),
            produced: self.produced.load(Ordering::Relaxed),
            consumed: self.consumed.load(Ordering::Relaxed),
            ticks: self.ticks.load(Ordering::Relaxed),
            enqueued: self.enqueued.load(Ordering::Relaxed),
            feedback_frames: self.feedback_frames.load(Ordering::Relaxed),
            feedback_unavailable: self.feedback_unavailable.load(Ordering::Relaxed),
            feedback_expired_frames: self.feedback_expired_frames.load(Ordering::Relaxed),
            feedback_frame_gaps: self.feedback_frame_gaps.load(Ordering::Relaxed),
            dropped_publications: self.dropped_publications.load(Ordering::Relaxed),
            unknown_publication_age: self.unknown_age.load(Ordering::Relaxed),
            clock_anomalies: self.anomalies.load(Ordering::Relaxed),
            producer_interval: self.producer_interval.snapshot(),
            consumer_interval: self.consumer_interval.snapshot(),
            tick_interval: self.tick_interval.snapshot(),
            enqueue_interval: self.enqueue_interval.snapshot(),
            publication_age: self.publication_age.snapshot(),
            feedback_interval: self.feedback_interval.snapshot(),
            raster: self.raster.snapshot(),
            submission: self.submission.snapshot(),
        }
    }
}

pub struct Snapshot {
    pub elapsed_ns: u64,
    pub produced: u64,
    pub consumed: u64,
    pub ticks: u64,
    pub enqueued: u64,
    pub feedback_frames: u64,
    pub feedback_unavailable: u64,
    pub feedback_expired_frames: u64,
    pub feedback_frame_gaps: u64,
    pub dropped_publications: u64,
    pub unknown_publication_age: u64,
    pub clock_anomalies: u64,
    pub producer_interval: HistogramSnapshot,
    pub consumer_interval: HistogramSnapshot,
    pub tick_interval: HistogramSnapshot,
    pub enqueue_interval: HistogramSnapshot,
    pub publication_age: HistogramSnapshot,
    pub feedback_interval: HistogramSnapshot,
    pub raster: DurationTotals,
    pub submission: DurationTotals,
}

impl Snapshot {
    /// Writer-side only, approximately once/second; content-free cumulative JSONL.
    pub fn write_json<W: Write>(&self, writer: &mut W) -> io::Result<()> {
        write!(writer,"{{\"schema\":1,\"atomic_snapshot\":false,\"elapsed_ns\":{},\"produced\":{},\"consumed\":{},\"ticks\":{},\"texture_enqueues\":{},\"gdk_feedback_frames\":{},\"gdk_feedback_unavailable\":{},\"gdk_feedback_expired\":{},\"gdk_feedback_frame_gaps\":{},\"dropped_publications\":{},\"unknown_publication_age\":{},\"clock_anomalies\":{},\"bin_width_ns\":{},\"overflow_from_ns\":{},\"histograms\":{{",
            self.elapsed_ns,self.produced,self.consumed,self.ticks,self.enqueued,
            self.feedback_frames,self.feedback_unavailable,self.feedback_expired_frames,self.feedback_frame_gaps,
            self.dropped_publications,self.unknown_publication_age,self.clock_anomalies,
            BIN_WIDTH_NS,(HISTOGRAM_BINS as u64-1)*BIN_WIDTH_NS)?;
        for (index, (name, histogram)) in [
            ("producer_interval", &self.producer_interval),
            ("consumer_interval", &self.consumer_interval),
            ("gtk_tick_interval", &self.tick_interval),
            ("enqueue_interval", &self.enqueue_interval),
            ("publication_age", &self.publication_age),
            ("gdk_feedback_interval", &self.feedback_interval),
        ]
        .iter()
        .enumerate()
        {
            if index > 0 {
                write!(writer, ",")?;
            }
            write!(
                writer,
                "\"{name}\":{{\"samples\":{},\"sum_ns\":{},\"max_ns\":{},\"bins\":[",
                histogram.totals.samples, histogram.totals.sum_ns, histogram.totals.max_ns
            )?;
            for (bin, count) in histogram.bins.iter().enumerate() {
                if bin > 0 {
                    write!(writer, ",")?;
                }
                write!(writer, "{count}")?;
            }
            write!(writer, "]}}")?;
        }
        write!(writer,"}},\"raster\":{{\"samples\":{},\"sum_ns\":{},\"max_ns\":{}}},\"enqueue_cpu\":{{\"samples\":{},\"sum_ns\":{},\"max_ns\":{}}},\"enqueue_is_scanout\":false}}\n",
            self.raster.samples,self.raster.sum_ns,self.raster.max_ns,
            self.submission.samples,self.submission.sum_ns,self.submission.max_ns)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn histogram_counts_zero_intervals_and_overflow_without_losing_maximum() {
        let cadence = FrameCadence::new();
        for (generation, time) in [(1, 0), (2, 16_000_000), (3, 16_000_000), (4, 100_000_000)] {
            cadence.produced_at(generation, time);
        }
        let histogram = cadence.snapshot().producer_interval;
        assert_eq!(histogram.totals.samples, 3);
        assert_eq!(histogram.totals.sum_ns, 100_000_000);
        assert_eq!(histogram.totals.max_ns, 84_000_000);
        assert_eq!(histogram.bins[0], 1);
        assert_eq!(histogram.bins[16], 1);
        assert_eq!(histogram.bins[63], 1);
    }

    #[test]
    fn generation_gaps_count_coalescing_and_duplicate_ticks_are_not_consumption() {
        let cadence = FrameCadence::new();
        cadence.produced_at(1, 100);
        cadence.consumed_at(1, 160);
        cadence.produced_at(2, 170);
        cadence.produced_at(3, 190);
        cadence.consumed_at(3, 220);
        cadence.consumed_at(3, 230);
        let snapshot = cadence.snapshot();
        assert_eq!(snapshot.produced, 3);
        assert_eq!(snapshot.consumed, 2);
        assert_eq!(snapshot.dropped_publications, 1);
        assert_eq!(snapshot.publication_age.totals.samples, 2);
        assert_eq!(snapshot.publication_age.totals.sum_ns, 90);
    }

    #[test]
    fn a_newer_publication_cannot_supply_the_age_of_an_older_consumed_frame() {
        let cadence = FrameCadence::new();
        cadence.produced_at(1, 100);
        cadence.produced_at(2, 120);
        cadence.consumed_at(1, 150);
        let snapshot = cadence.snapshot();
        assert_eq!(snapshot.unknown_publication_age, 1);
        assert_eq!(snapshot.publication_age.totals.samples, 0);
    }

    #[test]
    fn enqueue_duration_is_separate_from_raster_and_does_not_claim_scanout() {
        let cadence = FrameCadence::new();
        cadence.tick_at(0);
        cadence.tick_at(16_000_000);
        cadence.raster(2_000_000);
        cadence.enqueue_at(17_000_000, 500_000);
        let snapshot = cadence.snapshot();
        assert_eq!(snapshot.raster.sum_ns, 2_000_000);
        assert_eq!(snapshot.submission.sum_ns, 500_000);
        assert_eq!(snapshot.enqueued, 1);
        assert_eq!(snapshot.ticks, 2);
        let mut json = Vec::new();
        snapshot.write_json(&mut json).unwrap();
        let json = String::from_utf8(json).unwrap();
        assert!(json.contains("\"enqueue_is_scanout\":false"));
        assert!(!opt_in(None));
        assert!(!opt_in(Some("0")));
        assert!(opt_in(Some("1")));
    }

    #[test]
    fn completed_feedback_deduplicates_frame_ids_and_does_not_invent_times_when_unavailable() {
        let cadence = FrameCadence::new();
        cadence.feedback_completed(10, Some(1_000_000_000));
        cadence.feedback_completed(10, Some(1_000_000_000));
        cadence.feedback_completed(11, Some(1_016_000_000));
        cadence.feedback_completed(12, None);
        cadence.feedback_completed(14, Some(1_064_000_000));
        let snapshot = cadence.snapshot();
        assert_eq!(snapshot.feedback_frames, 3);
        assert_eq!(snapshot.feedback_unavailable, 1);
        assert_eq!(snapshot.feedback_frame_gaps, 1);
        assert_eq!(snapshot.feedback_interval.totals.samples, 1);
        assert_eq!(snapshot.feedback_interval.totals.sum_ns, 16_000_000);
    }

    #[test]
    fn tls_producer_is_opt_in_per_thread_and_expired_feedback_is_not_completed() {
        set_current(None);
        record_produced(1);
        assert!(current().is_none());
        let cadence = Arc::new(FrameCadence::new());
        set_current(Some(cadence.clone()));
        record_produced(1);
        std::thread::spawn(|| {
            assert!(current().is_none());
            record_produced(2);
        })
        .join()
        .unwrap();
        assert_eq!(cadence.snapshot().produced, 1);
        cadence.feedback_expired(1);
        let snapshot = cadence.snapshot();
        assert_eq!(snapshot.feedback_expired_frames, 1);
        assert_eq!(snapshot.feedback_frames, 0);
        assert_eq!(snapshot.feedback_unavailable, 0);
        set_current(None);
    }
}
