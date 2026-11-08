//! Nanoswarm clock: every grain a nanobot. In silence the swarm holds the
//! time; with music most of it peels off into a live waveform under a small
//! clock, and a hard beat blows it apart to rebuild. An escort follows the
//! cursor as a hexagonal shield; sweep the cursor fast and the swarm chases
//! it like a school of fish.
use super::super::{mix, scale, Rng, SandFont};
use super::{face, hash, Kit, Play};
use crate::interactive_tunnel::Raster;
use std::f32::consts::{PI, TAU};

type Color = (u8, u8, u8);
const BOT: Color = (70, 245, 200);
const HOT: Color = (215, 255, 245);
const ESCORT: Color = (255, 90, 210);
const SLOTS: usize = 8;

#[derive(Clone, Copy, PartialEq)]
enum Role {
    Clock,
    Escort,
}
struct Bot {
    x: f32,
    y: f32,
    vx: f32,
    vy: f32,
    role: Role,
    slot: usize,
    index: usize,
    jitter: (f32, f32),
    shade: f32,
    phase: f32,
    /// Free flight until then: thrown by a beat, a burst or the chase.
    loose_until: f32,
}

pub(super) struct Swarm {
    bots: Vec<Bot>,
    big: SandFont,
    small: SandFont,
    rng: Rng,
    time: f32,
    /// 0 holds the big clock, 1 is the music layout.
    music: f32,
    loud_for: f32,
    /// The time each slot currently shows.
    text: Vec<char>,
    beat_cool: f32,
    flash: f32,
    /// Shield: where it is, its spin and how charged it is.
    shield: Option<(f32, f32)>,
    spin: f32,
    /// Hex shock rings: centre, birth, strength.
    pulses: Vec<(f32, f32, f32, f32)>,
    wave_phase: f32,
    swell: f32,
}
impl Swarm {
    pub(super) fn new() -> Self {
        Self {
            bots: vec![],
            big: SandFont::default(),
            small: SandFont::default(),
            rng: Rng(rand::random()),
            time: 0.,
            music: 0.,
            loud_for: 0.,
            text: vec![],
            beat_cool: 0.,
            flash: 0.,
            shield: None,
            spin: 0.,
            pulses: vec![],
            wave_phase: 0.,
            swell: 0.,
        }
    }
    fn escorts(rows: usize) -> usize {
        (24. + rows as f32 * 0.9).min(160.) as usize
    }
    /// The live waveform: bass rolls long swells, treble rides them as ripples.
    fn wave(&self, kit: &Kit, u: f32) -> f32 {
        let (b, t) = (kit.sound.bass, kit.sound.treble);
        let p = self.wave_phase;
        let long = (u * TAU * 1.5 + p).sin() * (0.25 + b * 0.9);
        let mid = (u * TAU * 4.3 - p * 1.7).sin() * (0.1 + t * 0.45);
        let fine = (u * TAU * 11. + p * 3.1).sin() * t * 0.25;
        // The ends are pinned, like a scope trace across the screen.
        // sin(pi) rounds to a hair below zero, and its power to NaN.
        let pin = (u * PI).sin().max(0.).powf(0.6);
        (long + mid + fine) * pin * (1. + self.swell)
    }
    fn hex_point(&self, kit: &Kit, c: (f32, f32), radius: f32, u: f32) -> (f32, f32) {
        let side = (u * 6.).floor();
        let f = u * 6. - side;
        let a0 = self.spin + side * TAU / 6.;
        let a1 = a0 + TAU / 6.;
        let (x0, y0) = (a0.cos(), a0.sin());
        let (x1, y1) = (a1.cos(), a1.sin());
        let (x, y) = (x0 + (x1 - x0) * f, y0 + (y1 - y0) * f);
        (c.0 + x * radius * kit.aspect, c.1 + y * radius)
    }
    fn shield_radius(&self, kit: &Kit) -> f32 {
        (3.2 - kit.charge * 1.2 + kit.sound.bass * 0.6) * kit.reach.max(0.7)
    }
}
impl Play for Swarm {
    fn resize(&mut self, kit: &Kit) {
        let (cols, rows) = (kit.cols, kit.rows);
        self.big = face(kit, (0, (rows as f32 * 0.05) as i32, cols, (rows as f32 * 0.9) as usize), SLOTS);
        self.small = face(kit, ((cols as f32 * 0.2) as i32, (rows as f32 * 0.03) as i32, (cols as f32 * 0.6) as usize, (rows as f32 * 0.3) as usize), SLOTS);
        self.text = kit.clock(true).chars().collect();
        self.bots.clear();
        // Each slot keeps enough bots for its widest glyph.
        for slot in 0..SLOTS {
            let chars: &[char] = if slot == 2 || slot == 5 { &[':'] } else { &['0', '1', '2', '3', '4', '5', '6', '7', '8', '9'] };
            let most = chars.iter().filter_map(|c| self.big.glyphs.get(c)).map(Vec::len).max().unwrap_or(0);
            for index in 0..most {
                self.bots.push(Bot {
                    x: self.rng.next() * cols as f32,
                    y: self.rng.next() * rows as f32,
                    vx: 0.,
                    vy: 0.,
                    role: Role::Clock,
                    slot,
                    index,
                    jitter: ((self.rng.next() - 0.5) * 0.6, (self.rng.next() - 0.5) * 0.6),
                    shade: 0.55 + self.rng.next() * 0.45,
                    phase: self.rng.next() * TAU,
                    loose_until: 0.,
                });
            }
        }
        for index in 0..Self::escorts(rows) {
            self.bots.push(Bot {
                x: self.rng.next() * cols as f32,
                y: self.rng.next() * rows as f32,
                vx: 0.,
                vy: 0.,
                role: Role::Escort,
                slot: 0,
                index,
                jitter: (0., 0.),
                shade: 0.7 + self.rng.next() * 0.3,
                phase: self.rng.next() * TAU,
                loose_until: 0.,
            });
        }
    }
    fn step(&mut self, kit: &Kit) {
        let (dt, cols, rows) = (kit.dt, kit.cols, kit.rows);
        self.time = kit.time;
        let time = self.time;
        self.text = kit.clock(true).chars().collect();
        let level = kit.sound.bass.max(kit.sound.treble);
        // Music takes over after half a second of it and lets go after a
        // second and a half of silence.
        if level > 0.08 {
            self.loud_for += dt;
        } else {
            self.loud_for = 0.;
        }
        let goal = if self.loud_for > 0.5 { 1. } else if kit.silence > 1.5 || self.loud_for == 0. && self.music < 0.5 { 0. } else { self.music };
        self.music += (goal - self.music) * (1. - (-dt * 2.5).exp());
        self.wave_phase += dt * (1.2 + kit.sound.bass * 3.);
        self.swell = (self.swell - dt * 0.5).max(0.);
        self.flash = (self.flash - dt * 1.5).max(0.);
        self.beat_cool -= dt;
        self.spin += dt * (0.6 + kit.charge * 4. + kit.sound.treble);
        self.pulses.retain(|p| time - p.2 < 1.);
        let ease = 1. - (-dt * 10.).exp();
        self.shield = kit.cursor.map(|c| match self.shield {
            Some(s) => (s.0 + (c.0 - s.0) * ease, s.1 + (c.1 - s.1) * ease),
            None => c,
        });
        // Bursts: a hard beat, a shake or a drop throw the swarm off its
        // places; a charged rest fires a hex shock from the shield.
        let centre = (cols as f32 * 0.5, rows as f32 * 0.5);
        let mut burst: Option<((f32, f32), f32, f32)> = None;
        if let Some(k) = kit.beat {
            if k > 0.9 && self.beat_cool <= 0. {
                self.beat_cool = 1.4;
                burst = Some((centre, 18. * k, 0.25));
            }
        }
        if kit.shake {
            burst = Some((centre, 60., 0.9));
        }
        if kit.drop {
            burst = Some((centre, 85., 1.1));
            self.flash = 1.;
            self.swell = 1.;
        }
        let waves: Vec<(f32, f32, f32, f32)> = self.pulses.iter().filter(|p| p.2 == time).copied().collect();
        // The music layout: the small clock keeps a share of each slot, the
        // rest trace the waveform across the screen.
        let small_n: Vec<usize> = (0..SLOTS)
            .map(|s| self.text.get(s).and_then(|c| self.small.glyphs.get(c)).map_or(0, Vec::len))
            .collect();
        let tracers = self
            .bots
            .iter()
            .filter(|b| b.role == Role::Clock && b.index >= small_n[b.slot])
            .count()
            .max(1);
        let big_cells: Vec<Vec<(f32, f32)>> = (0..SLOTS)
            .map(|s| self.text.get(s).filter(|c| self.big.glyphs.contains_key(c)).map_or(vec![], |&c| self.big.cells(c, s)))
            .collect();
        let small_cells: Vec<Vec<(f32, f32)>> = (0..SLOTS)
            .map(|s| self.text.get(s).filter(|c| self.small.glyphs.contains_key(c)).map_or(vec![], |&c| self.small.cells(c, s)))
            .collect();
        let shield_r = self.shield_radius(kit);
        let escorts = self.bots.iter().filter(|b| b.role == Role::Escort).count().max(1);
        let chase = kit.speed > 32. * kit.reach;
        let (stiff, damp, friction) = (55., (-dt * 7.).exp(), (-dt * 2.2).exp());
        let wave_y = rows as f32 * 0.64;
        let amp = rows as f32 * 0.17;
        // The trace sampled with its running length, so the tracers spread
        // evenly along it and steep stretches don't break up.
        let samples = (cols / 2).clamp(16, 400);
        let mut trace = Vec::with_capacity(samples);
        let mut run = Vec::with_capacity(samples);
        let mut total = 0.;
        for i in 0..samples {
            let u = i as f32 / (samples - 1) as f32;
            let p = (u * (cols as f32 - 1.), wave_y + self.wave(kit, u) * amp);
            if let Some(&q) = trace.last() {
                total += kit.dist(p, q);
            }
            trace.push(p);
            run.push(total);
        }
        let along = |u: f32| {
            let s = u * total;
            let i = run.partition_point(|&l| l < s).clamp(1, samples - 1);
            let (a, b) = (trace[i - 1], trace[i]);
            let f = ((s - run[i - 1]) / (run[i] - run[i - 1]).max(1e-4)).clamp(0., 1.);
            (a.0 + (b.0 - a.0) * f, a.1 + (b.1 - a.1) * f)
        };
        let mut rank = 0;
        for i in 0..self.bots.len() {
            let b = &self.bots[i];
            let target = match b.role {
                Role::Clock => {
                    let big = &big_cells[b.slot];
                    let home = if big.is_empty() { centre } else { big[b.index % big.len()] };
                    let home = (home.0 + b.jitter.0, home.1 + b.jitter.1);
                    if self.music < 0.01 {
                        home
                    } else {
                        let tune = if b.index < small_n[b.slot] {
                            let sc = &small_cells[b.slot];
                            sc[b.index % sc.len().max(1)]
                        } else {
                            let u = (rank as f32 + 0.5) / tracers as f32;
                            rank += 1;
                            let thick = (hash(b.index as u32 * 13 + b.slot as u32) - 0.5) * 2.4 * kit.reach.max(0.8);
                            let p = along(u);
                            (p.0, p.1 + thick)
                        };
                        (home.0 + (tune.0 - home.0) * self.music, home.1 + (tune.1 - home.1) * self.music)
                    }
                }
                Role::Escort => {
                    let u = (b.index as f32 + 0.5) / escorts as f32;
                    match self.shield {
                        Some(c) => self.hex_point(kit, c, shield_r, u),
                        // Without a cursor the escort patrols an orbit round the clock.
                        None => {
                            let a = u * TAU + time * 0.15;
                            let wob = (time * 0.7 + b.phase).sin() * 0.04;
                            (centre.0 + a.cos() * cols as f32 * (0.47 + wob), centre.1 + a.sin() * rows as f32 * (0.47 + wob))
                        }
                    }
                }
            };
            let b = &mut self.bots[i];
            let mut kick = (0., 0.);
            if let Some((c, power, hold)) = burst {
                if b.role == Role::Clock {
                    let (dx, dy) = ((b.x - c.0) / kit.aspect, b.y - c.1);
                    let d = (dx * dx + dy * dy).sqrt().max(0.5);
                    let a = self.rng.next() * TAU;
                    let v = power * kit.reach * (0.4 + self.rng.next());
                    kick = ((dx / d + a.cos() * 0.6) * v * kit.aspect, (dy / d + a.sin() * 0.6) * v);
                    b.loose_until = time + hold * (0.6 + self.rng.next() * 0.8);
                }
            }
            for &(cx, cy, _, k) in &waves {
                let (dx, dy) = ((b.x - cx) / kit.aspect, b.y - cy);
                let d = (dx * dx + dy * dy).sqrt().max(0.5);
                let radius = rows as f32 * (0.2 + k * 0.4);
                if d < radius && b.role == Role::Clock {
                    let v = (30. + 60. * k) * kit.reach * (1. - d / radius);
                    kick.0 += dx / d * v * kit.aspect;
                    kick.1 += dy / d * v;
                    b.loose_until = time + 0.6 + self.rng.next() * 0.5;
                }
            }
            if let Some((c, turns)) = kit.swirl {
                let (dx, dy) = ((b.x - c.0) / kit.aspect, b.y - c.1);
                let d = (dx * dx + dy * dy).sqrt().max(0.5);
                let radius = rows as f32 * 0.35;
                if d < radius && b.role == Role::Clock {
                    let v = (20. + turns.abs().min(3.) * 18.) * kit.reach * turns.signum() * (1. - d / radius) * dt * 10.;
                    kick.0 += (-dy / d * v - dx / d * 4. * dt) * kit.aspect;
                    kick.1 += dx / d * v - dy / d * 4. * dt;
                    b.loose_until = b.loose_until.max(time + 0.5);
                }
            }
            // A fast sweep: the bots it passes near give chase.
            if chase && b.role == Role::Clock {
                if let (Some(c), Some((w, _))) = (kit.cursor, kit.touch((b.x, b.y), rows as f32 * 0.1)) {
                    let (dx, dy) = ((c.0 - b.x) / kit.aspect, c.1 - b.y);
                    let d = (dx * dx + dy * dy).sqrt().max(0.5);
                    // A nudge, not a yank: the bots trail the cursor a little
                    // and are soon back at their posts.
                    let v = 14. * kit.reach * w;
                    let pull = (kit.velocity.0.clamp(-40., 40.) * 0.2 * w, kit.velocity.1.clamp(-40., 40.) * 0.2 * w);
                    kick.0 += (dx / d * v + pull.0) * kit.aspect;
                    kick.1 += dy / d * v + pull.1;
                    b.loose_until = b.loose_until.max(time + 0.25 + self.rng.next() * 0.3);
                }
            }
            b.vx += kick.0;
            b.vy += kick.1;
            if time < b.loose_until {
                b.vx *= friction;
                b.vy *= friction;
            } else {
                let k = if b.role == Role::Escort { stiff * 1.6 } else { stiff };
                b.vx = (b.vx + (target.0 - b.x) * k * dt) * damp;
                b.vy = (b.vy + (target.1 - b.y) * k * dt) * damp;
            }
            b.x = (b.x + b.vx * dt).clamp(0., cols as f32 - 1.);
            b.y = (b.y + b.vy * dt).clamp(0., rows as f32 - 1.);
        }
    }
    fn draw(&self, r: &mut Raster, kit: &Kit) {
        let (cols, rows) = (kit.cols, kit.rows);
        let (bass, treble) = (kit.sound.bass, kit.sound.treble);
        // A dim honeycomb; cells near the cursor wake up.
        let (hx, hy) = ((6. * kit.reach.max(0.7)).round().max(3.) as usize, (3. * kit.reach.max(0.7)).round().max(2.) as usize);
        for (j, y) in (0..rows).step_by(hy).enumerate() {
            let shift = if j % 2 == 1 { hx / 2 } else { 0 };
            for x in (shift..cols).step_by(hx) {
                let near = kit.cursor.map_or(0., |c| (1. - kit.dist(c, (x as f32, y as f32)) / (rows as f32 * 0.25)).max(0.));
                let light = 0.1 + near * 0.5 + bass * 0.06;
                r.dot(x as i32, y as i32, 9., if near > 0.3 { '◇' } else { '·' }, scale((60, 110, 170), light));
            }
        }
        let tick = (kit.time * 10.) as u32;
        for b in &self.bots {
            let (x, y) = (b.x.round() as i32, b.y.round() as i32);
            // Only bots in free flight streak; the ones riding the wave keep still glyphs.
            let v = ((b.vx / kit.aspect).powi(2) + b.vy.powi(2)).sqrt();
            let hot = if kit.time < b.loose_until { (v / (40. * kit.reach)).min(1.) } else { 0. };
            let blink = hash((b.index as u32).wrapping_mul(747796405) ^ (b.slot as u32 * 31) ^ tick) < 0.02 + treble * 0.08;
            match b.role {
                Role::Clock => {
                    // In flight a bot streaks along its heading.
                    let ch = if hot > 0.35 {
                        let a = (b.vy).atan2(b.vx / kit.aspect);
                        let o = ((a + PI) / (PI / 4.)).round() as i32 % 4;
                        ['-', '\\', '|', '/'][o as usize]
                    } else if blink {
                        '◆'
                    } else if b.shade > 0.8 {
                        '●'
                    } else {
                        '•'
                    };
                    let tone = mix(BOT, HOT, hot * 0.7 + if blink { 0.6 } else { 0. } + self.flash * 0.5);
                    let light = b.shade * (1.05 + bass * 0.3 + self.flash * 0.6);
                    r.dot(x, y, 2. - hot, ch, scale(tone, light));
                }
                Role::Escort => {
                    let light = b.shade * (0.75 + kit.charge * 0.5 + treble * 0.3);
                    r.dot(x, y, 1., if kit.charge > 0.6 { '◆' } else { '•' }, scale(mix(ESCORT, (255, 220, 250), kit.charge * 0.6), light));
                }
            }
        }
        if let Some(c) = self.shield {
            // The shield's frame between its bots, faint, brighter as it charges.
            let radius = self.shield_radius(kit);
            for side in 0..6 {
                let a = self.hex_point(kit, c, radius, side as f32 / 6.);
                let b = self.hex_point(kit, c, radius, (side + 1) as f32 / 6.);
                r.line_with_glyph(a, b, 1.5, scale(ESCORT, 0.25 + kit.charge * 0.5), false, Some('·'));
            }
        }
        for &(x, y, born, k) in &self.pulses {
            let age = kit.time - born;
            let radius = age * rows as f32 * (0.5 + k * 0.6);
            let light = (1. - age).max(0.) * (0.5 + k * 0.5);
            for side in 0..6 {
                let a = self.hex_point(kit, (x, y), radius, side as f32 / 6.);
                let b = self.hex_point(kit, (x, y), radius, (side + 1) as f32 / 6.);
                r.line_with_glyph(a, b, 0.7, scale(ESCORT, light), false, Some('•'));
            }
        }
    }
}
#[cfg(test)]
mod tests {
    use super::super::{Kit, Play};
    use super::Swarm;
    use crate::interactive_matrix::Sound;
    #[test]
    fn the_wave_stays_finite_and_on_screen() {
        for (cols, rows) in [(192, 56), (240, 68), (17, 13)] {
            let mut kit = Kit { cols, rows, aspect: 2.125, dt: 1. / 30., reach: 1., ..Kit::default() };
            let mut s = Swarm::new();
            s.resize(&kit);
            for _ in 0..120 {
                kit.time += kit.dt;
                kit.sound = Sound { bass: 0.4, treble: 0.4 };
                s.step(&kit);
            }
            assert!(s.music > 0.9);
            assert!(s.bots.iter().all(|b| b.x.is_finite() && b.y.is_finite()));
            // On a real screen most of the swarm leaves the clock for the wave.
            let low = s.bots.iter().filter(|b| b.y > rows as f32 * 0.45).count();
            assert!(cols < 100 || low > s.bots.len() / 3, "{cols}x{rows}: {low} of {}", s.bots.len());
        }
    }
}
