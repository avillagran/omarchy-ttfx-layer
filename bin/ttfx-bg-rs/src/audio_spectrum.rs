//! Fixed-format spectrum for the existing 24 kHz, 1024-sample, 24-band capture.
//!
//! Reference: candidate main.rs SHA256 cf525befd7e9e98ef83d0285357fbf7c24bb45cd335d13de554db7ead2c39c49.
//! Construct once outside capture/reconnect loops. After `analyze`, KEEP the
//! original rolling-peak update, atomic publication, volume/beat/BPM update,
//! capture ownership checks and child kill/wait unchanged. No capture is paused.
//! All-zero samples bypass only Goertzel; nonzero samples use the original
//! recurrence and operation order, including tiny nonzero PCM values.
//! Equality is checked against the original on each target architecture; libm
//! sin/cos/powf rounding need not be identical across different architectures.

pub const BANDS: usize = 24;
pub const SAMPLES: usize = 1024;
pub const RATE: f32 = 24000.0;

#[derive(Clone)]
pub struct Spectrum {
    coefficients: [f32; BANDS],
}

pub struct SpectrumFrame {
    pub rms: f32,
    pub magnitudes: [f32; BANDS],
    pub maximum: f32,
}

impl Default for Spectrum {
    fn default() -> Self {
        let coefficients = std::array::from_fn(|i| {
            let freq = 60.0 * (10000.0f32 / 60.0).powf(i as f32 / (BANDS - 1) as f32);
            let mut k = (0.5 + (SAMPLES as f32 * freq) / RATE) as i32;
            if k <= 0 || k >= SAMPLES as i32 {
                k = k.clamp(1, SAMPLES as i32 - 1);
            }
            let w = 2.0 * std::f32::consts::PI * k as f32 / SAMPLES as f32;
            2.0 * w.cos()
        });
        Self { coefficients }
    }
}

impl Spectrum {
    pub fn analyze(&self, samples: &[f32; SAMPLES]) -> SpectrumFrame {
        // Same sequential f32 sum as the reference Iterator::sum, also checking
        // exact silence without a second pass or an amplitude threshold.
        let mut squares = 0.0f32;
        let mut nonzero = false;
        for &x in samples {
            squares += x * x;
            nonzero |= x != 0.0;
        }
        let rms = (squares / SAMPLES as f32).sqrt();
        let mut magnitudes = [0.0f32; BANDS];
        let mut maximum = 0.0f32;
        if nonzero {
            for (bi, &coeff) in self.coefficients.iter().enumerate() {
                let (mut previous, mut previous2) = (0.0f32, 0.0f32);
                for &x in samples {
                    let value = x + coeff * previous - previous2;
                    previous2 = previous;
                    previous = value;
                }
                let m = previous2 * previous2 + previous * previous - coeff * previous * previous2;
                magnitudes[bi] = m;
                if m > maximum {
                    maximum = m;
                }
            }
        }
        SpectrumFrame {
            rms,
            magnitudes,
            maximum,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // Original arithmetic copied from the cited capture implementation.
    fn original(samples: &[f32; SAMPLES]) -> SpectrumFrame {
        let rms = (samples.iter().map(|v| v * v).sum::<f32>() / SAMPLES as f32).sqrt();
        let mut magnitudes = [0.0f32; BANDS];
        let mut maximum = 0.0f32;
        for (bi, magnitude) in magnitudes.iter_mut().enumerate() {
            let freq = 60.0 * (10000.0f32 / 60.0).powf(bi as f32 / (BANDS - 1) as f32);
            let n = samples.len();
            let mut k = (0.5 + (n as f32 * freq) / RATE) as i32;
            if k <= 0 || k >= n as i32 {
                k = k.clamp(1, n as i32 - 1);
            }
            let w = 2.0 * std::f32::consts::PI * k as f32 / n as f32;
            let coeff = 2.0 * w.cos();
            let (mut s_prev, mut s_prev2) = (0f32, 0f32);
            for &x in samples {
                let s = x + coeff * s_prev - s_prev2;
                s_prev2 = s_prev;
                s_prev = s;
            }
            let m = s_prev2 * s_prev2 + s_prev * s_prev - coeff * s_prev * s_prev2;
            *magnitude = m;
            if m > maximum {
                maximum = m;
            }
        }
        SpectrumFrame {
            rms,
            magnitudes,
            maximum,
        }
    }

    fn check(samples: &[f32; SAMPLES]) {
        let old = original(samples);
        let new = Spectrum::default().analyze(samples);
        assert_eq!(old.rms.to_bits(), new.rms.to_bits(), "RMS");
        assert_eq!(old.maximum.to_bits(), new.maximum.to_bits(), "maximum");
        for i in 0..BANDS {
            assert_eq!(
                old.magnitudes[i].to_bits(),
                new.magnitudes[i].to_bits(),
                "band {i}"
            );
        }
    }

    fn pcm_tone(amplitude: f32, frequency: f32) -> [f32; SAMPLES] {
        std::array::from_fn(|i| {
            let x = amplitude * (2.0 * std::f32::consts::PI * frequency * i as f32 / RATE).sin();
            (x * 32767.0).round() as i16 as f32 / 32768.0
        })
    }

    fn noise() -> [f32; SAMPLES] {
        let mut seed = 0x1a2b3c4du32;
        std::array::from_fn(|_| {
            seed = seed.wrapping_mul(1664525).wrapping_add(1013904223);
            (seed >> 16) as i16 as f32 / 32768.0
        })
    }

    #[test]
    fn exact_zero_pcm_matches_all_bits() {
        check(&[0.0; SAMPLES]);
    }

    #[test]
    fn tones_and_quiet_nonzero_pcm_match_all_bits() {
        for amp in [1.0 / 32767.0, 0.03, 0.25, 0.9] {
            for freq in [60.0, 70.3125, 440.0, 6000.0, 10007.8125] {
                check(&pcm_tone(amp, freq));
            }
        }
    }

    #[test]
    fn impulses_at_edges_and_middle_match_all_bits() {
        for pos in [0, 511, 1023] {
            for value in [1.0 / 32768.0, -1.0 / 32768.0, -1.0, 32767.0 / 32768.0] {
                let mut samples = [0.0; SAMPLES];
                samples[pos] = value;
                check(&samples);
            }
        }
    }

    #[test]
    fn deterministic_pcm_noise_matches_all_bits() {
        check(&noise());
    }

    #[derive(Default)]
    struct State {
        peak: f32,
        prev_rms: f32,
        beat_hold: u32,
        bpm: u32,
        last_onset_ms: Option<u64>,
    }
    impl State {
        fn publish(
            &mut self,
            frame: SpectrumFrame,
            clock_ms: u64,
        ) -> ([u32; BANDS], u32, u32, u32) {
            self.peak = (self.peak * 0.995).max(frame.maximum).max(1e-6);
            let bands = frame
                .magnitudes
                .map(|m| (m / self.peak).sqrt().min(1.0).to_bits());
            let volume = (frame.rms * 4.0).min(1.0).to_bits();
            let onset = frame.rms > self.prev_rms * 1.35 && frame.rms > 0.02;
            let beat = if onset {
                self.beat_hold = 3;
                1
            } else if self.beat_hold > 0 {
                self.beat_hold -= 1;
                1
            } else {
                0
            };
            if onset {
                if let Some(previous) = self.last_onset_ms {
                    let secs = (clock_ms - previous) as f32 / 1000.0;
                    if secs > 0.2 {
                        let raw = (60.0 / secs).clamp(40.0, 240.0);
                        let old = self.bpm as f32;
                        self.bpm = (if old == 0.0 {
                            raw
                        } else {
                            old * 0.7 + raw * 0.3
                        })
                        .round() as u32;
                    }
                }
                self.last_onset_ms = Some(clock_ms);
            }
            self.prev_rms = frame.rms;
            (bands, volume, beat, self.bpm)
        }
    }

    #[test]
    fn mixed_pcm_sequence_preserves_peak_decay_volume_beat_and_bpm_bits() {
        let spectrum = Spectrum::default();
        let mut old = State {
            peak: 1e-6,
            ..State::default()
        };
        let mut new = State {
            peak: 1e-6,
            ..State::default()
        };
        for frame in 0..144u64 {
            let samples = match frame % 12 {
                0 => pcm_tone(0.25, 70.3125),
                1 => noise(),
                2 => pcm_tone(1.0 / 32767.0, 10007.8125),
                _ => [0.0; SAMPLES],
            };
            assert_eq!(
                old.publish(original(&samples), frame * 43),
                new.publish(spectrum.analyze(&samples), frame * 43),
                "state frame {frame}"
            );
            assert_eq!(old.peak.to_bits(), new.peak.to_bits());
            assert_eq!(old.beat_hold, new.beat_hold);
        }
        assert!(new.bpm > 0);
        assert_eq!(new.beat_hold, 0);
        assert!(new.peak > 1e-6);
    }
}
