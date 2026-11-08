//! Sand toys: clocks of sand to play with. Every toy reads the same kit each
//! frame: the grid, the wall clock, the music read as beats, drops and
//! silence, and the cursor read as gestures. The layer only ever learns where
//! the pointer is, never a click, so gestures are made of motion: resting
//! slowly grows what a toy grows, circling stirs, a side to side shake bursts.
use super::{local_hms, music_reach, SandFont};
use crate::{interactive_matrix::Sound, interactive_tunnel::Raster};
use std::collections::VecDeque;
use std::f32::consts::{PI, TAU};

mod devil;
mod excavation;
mod ferro;
mod foundry;
mod glass;
mod mandala;
mod shore;
mod spectrum;
mod swarm;
mod text;
mod zen;
#[allow(unused_imports)]
use text::{Grain, SandText};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Toy {
    Ferro,
    Excavation,
    Zen,
    Foundry,
    Swarm,
    Shore,
    Glass,
    Devil,
    Spectrum,
    Mandala,
}

/// One sand toy. `resize` always runs before the first `step`.
pub(super) trait Play {
    /// The grid is new: the first frame, or a resize.
    fn resize(&mut self, kit: &Kit);
    fn step(&mut self, kit: &Kit);
    fn draw(&self, r: &mut Raster, kit: &Kit);
}

/// What every toy reads each frame. Positions are in cells (x in columns, y
/// in rows). Distances meant to look round on screen are in rows: a column
/// is `aspect` times narrower than a row is tall.
#[derive(Clone, Default)]
pub(super) struct Kit {
    pub cols: usize,
    pub rows: usize,
    /// Cell height over width.
    pub aspect: f32,
    pub dt: f32,
    pub time: f32,
    pub sound: Sound,
    /// Music displacements are in cells: scale them by this to look alike
    /// at any grid size.
    pub reach: f32,
    pub cursor: Option<(f32, f32)>,
    /// Where the cursor was the frame before; None right after it appears.
    pub last: Option<(f32, f32)>,
    /// Smoothed cursor velocity in rows per second, x already divided by the aspect.
    pub velocity: (f32, f32),
    pub speed: f32,
    /// Seconds the cursor has rested within a small circle.
    pub dwell: f32,
    /// How long the cursor has rested, eased from 0 to 1 over a few seconds.
    pub charge: f32,
    /// While the cursor circles: the centre of the circle and turns per
    /// second, positive clockwise on screen.
    pub swirl: Option<((f32, f32), f32)>,
    /// The frame the cursor is shaken side to side.
    pub shake: bool,
    /// The frame a beat lands, with its strength (up to 1.5).
    pub beat: Option<f32>,
    /// The frame a drop lands: bass back after a build-up, or loud music
    /// straight out of a long silence.
    pub drop: bool,
    /// Seconds without sound.
    pub silence: f32,
    pub hms: (u32, u32, u32),
    /// The second, or the minute, turned this frame.
    pub second: bool,
    pub minute: bool,
}
impl Kit {
    /// Screen distance between two cells, in rows.
    pub fn dist(&self, a: (f32, f32), b: (f32, f32)) -> f32 {
        let dx = (a.0 - b.0) / self.aspect;
        let dy = a.1 - b.1;
        (dx * dx + dy * dy).sqrt()
    }
    /// How close `p` is to the path the cursor took this frame, within
    /// `radius` rows: a weight from 1 on the path to 0 at the edge, and the
    /// unit direction away from the path (in rows, x already divided by the
    /// aspect). None out of reach or without a cursor.
    pub fn touch(&self, p: (f32, f32), radius: f32) -> Option<(f32, (f32, f32))> {
        let b = self.cursor?;
        let a = self.last.unwrap_or(b);
        let k = 1. / self.aspect;
        let (sx, sy) = ((b.0 - a.0) * k, b.1 - a.1);
        let (px, py) = ((p.0 - a.0) * k, p.1 - a.1);
        let len2 = sx * sx + sy * sy;
        let t = if len2 > 1e-6 { ((px * sx + py * sy) / len2).clamp(0., 1.) } else { 0. };
        let (dx, dy) = (px - sx * t, py - sy * t);
        let d = (dx * dx + dy * dy).sqrt();
        (d < radius).then(|| {
            let n = if d > 1e-4 { (dx / d, dy / d) } else { (0., -1.) };
            (1. - d / radius, n)
        })
    }
    /// Visit every cell within `radius` rows of the cursor's path this
    /// frame, with its weight and the direction away from the path.
    pub fn sweep(&self, radius: f32, mut f: impl FnMut(usize, usize, f32, (f32, f32))) {
        let Some(b) = self.cursor else { return };
        let a = self.last.unwrap_or(b);
        let rx = radius * self.aspect;
        let x0 = (a.0.min(b.0) - rx).floor().max(0.) as usize;
        let x1 = ((a.0.max(b.0) + rx).ceil().max(0.) as usize).min(self.cols.saturating_sub(1));
        let y0 = (a.1.min(b.1) - radius).floor().max(0.) as usize;
        let y1 = ((a.1.max(b.1) + radius).ceil().max(0.) as usize).min(self.rows.saturating_sub(1));
        for y in y0..=y1 {
            for x in x0..=x1 {
                if let Some((w, n)) = self.touch((x as f32, y as f32), radius) {
                    f(x, y, w, n);
                }
            }
        }
    }
    /// "HH:MM:SS", or "HH:MM".
    pub fn clock(&self, seconds: bool) -> String {
        let (h, m, s) = self.hms;
        if seconds {
            format!("{h:02}:{m:02}:{s:02}")
        } else {
            format!("{h:02}:{m:02}")
        }
    }
}

// ------------------------------------------------------------------ helpers
/// A circle `radius` rows across the screen, drawn with `ch`; `phase` turns it.
pub(super) fn ring(r: &mut Raster, kit: &Kit, c: (f32, f32), radius: f32, phase: f32, z: f32, ch: char, color: (u8, u8, u8)) {
    if radius <= 0. || !radius.is_finite() {
        return;
    }
    let n = (radius * kit.aspect * TAU).ceil().clamp(6., 900.) as usize;
    for i in 0..n {
        let a = phase + i as f32 * TAU / n as f32;
        let x = c.0 + a.cos() * radius * kit.aspect;
        let y = c.1 + a.sin() * radius;
        r.dot(x.round() as i32, y.round() as i32, z, ch, color);
    }
}
/// The pixel font of the 3D scenes, `k` cells per font pixel, centred on `cx`.
pub(super) fn stamp(r: &mut Raster, kit: &Kit, s: &str, k: usize, (cx, y, z): (f32, i32, f32), color: (u8, u8, u8)) {
    let (w, _) = crate::scenes3d::cyber::type_size(s, k);
    let x = (cx - w as f32 / 2.).round() as i32;
    crate::scenes3d::cyber::pixel_text(r, (kit.cols, kit.rows), (x, y, z), s, k.max(1), color);
}
/// The sand clock's bold face for `n` characters, fitted and centred in the
/// box `(x, y, w, h)` of cells.
pub(super) fn face(kit: &Kit, (x, y, w, h): (i32, i32, usize, usize), n: usize) -> SandFont {
    let (w, h, n) = (w.max(1), h.max(1), n.max(1));
    let wide = (w * 8 / n).max(1);
    let mut f = SandFont::cairo(wide, h, kit.aspect).unwrap_or_else(|| SandFont::blocks(wide, h));
    f.x0 = x + (w as i32 - f.advance * n as i32) / 2;
    f.y0 += y;
    f
}
/// A weightless uniform random number in [0, 1) from an integer.
pub(super) fn hash(n: u32) -> f32 {
    crate::neon_scenes::variant(n)
}

/// A scalar per cell, row by row: sand depth, dust.
#[derive(Clone, Default)]
pub(super) struct Field {
    pub v: Vec<f32>,
}
impl Field {
    pub fn new(w: usize, h: usize, fill: f32) -> Self {
        Self { v: vec![fill; w * h] }
    }
}

// ----------------------------------------------------------------- gestures
/// The cursor read as gestures.
#[derive(Default)]
pub(super) struct Hand {
    /// Recent positions: time, x in rows (divided by the aspect), y.
    trail: VecDeque<(f32, f32, f32)>,
    anchor: Option<(f32, f32)>,
    dwell: f32,
    velocity: (f32, f32),
    last: Option<(f32, f32)>,
    /// When the cursor last turned back on itself at speed, sideways.
    flips: VecDeque<f32>,
    heading: f32,
    shake_cool: f32,
}
/// Rest within this many rows to charge.
const DWELL_RADIUS: f32 = 1.2;
const CHARGE_FROM: f32 = 0.5;
const CHARGE_FULL: f32 = 2.5;
impl Hand {
    pub fn read(&mut self, kit: &mut Kit, cursor: Option<(f32, f32)>) {
        let (dt, time, aspect) = (kit.dt.max(1e-4), kit.time, kit.aspect.max(0.1));
        kit.swirl = None;
        kit.shake = false;
        kit.last = self.last;
        kit.cursor = cursor;
        self.shake_cool -= kit.dt;
        let Some(c) = cursor else {
            *self = Self { shake_cool: self.shake_cool, ..Self::default() };
            kit.velocity = (0., 0.);
            kit.speed = 0.;
            kit.dwell = 0.;
            kit.charge = 0.;
            return;
        };
        // Velocity, in rows per second.
        let raw = match self.last {
            Some(l) => ((c.0 - l.0) / aspect / dt, (c.1 - l.1) / dt),
            None => (0., 0.),
        };
        let k = 1. - (-kit.dt * 12.).exp();
        self.velocity.0 += (raw.0 - self.velocity.0) * k;
        self.velocity.1 += (raw.1 - self.velocity.1) * k;
        kit.velocity = self.velocity;
        kit.speed = (self.velocity.0.powi(2) + self.velocity.1.powi(2)).sqrt();
        // Rest: how long the cursor has stayed put; moving on starts over.
        let anchor = *self.anchor.get_or_insert(c);
        if kit.dist(anchor, c) > DWELL_RADIUS * kit.reach.max(0.8) {
            self.anchor = Some(c);
            self.dwell = 0.;
        } else {
            self.dwell += kit.dt;
        }
        kit.dwell = self.dwell;
        kit.charge = ((self.dwell - CHARGE_FROM) / (CHARGE_FULL - CHARGE_FROM)).clamp(0., 1.);
        // Shake: the cursor turning back on itself sideways, fast, again and again.
        let fast = 14. * kit.reach.max(0.8);
        if raw.0.abs() > fast {
            let sign = raw.0.signum();
            if self.heading != 0. && sign != self.heading {
                self.flips.push_back(time);
            }
            self.heading = sign;
        }
        while self.flips.front().is_some_and(|&t| time - t > 0.7) {
            self.flips.pop_front();
        }
        if self.flips.len() >= 4 && self.shake_cool <= 0. {
            kit.shake = true;
            self.shake_cool = 1.2;
            self.flips.clear();
        }
        // Swirl: the angle swept round the centre of the recent path.
        self.trail.push_back((time, c.0 / aspect, c.1));
        while self.trail.front().is_some_and(|&(t, _, _)| time - t > 1.4) {
            self.trail.pop_front();
        }
        let n = self.trail.len();
        if n >= 8 && self.trail.back().unwrap().0 - self.trail.front().unwrap().0 > 0.35 {
            let (sx, sy) = self.trail.iter().fold((0., 0.), |(a, b), &(_, x, y)| (a + x, b + y));
            let (mx, my) = (sx / n as f32, sy / n as f32);
            let (mut radius, mut near, mut far) = (0., f32::MAX, 0f32);
            let mut swept = 0.;
            let mut prev: Option<f32> = None;
            for &(_, x, y) in &self.trail {
                let (dx, dy) = (x - mx, y - my);
                let d = (dx * dx + dy * dy).sqrt();
                radius += d;
                near = near.min(d);
                far = far.max(d);
                let a = dy.atan2(dx);
                if let Some(p) = prev {
                    let mut d = a - p;
                    if d > PI {
                        d -= TAU;
                    } else if d < -PI {
                        d += TAU;
                    }
                    swept += d;
                }
                prev = Some(a);
            }
            radius /= n as f32;
            let span = self.trail.back().unwrap().0 - self.trail.front().unwrap().0;
            let turns = swept / TAU;
            // An out and back stroke also sweeps a turn round its middle,
            // but passes through it: a circle keeps its distance.
            let round = near > radius * 0.45 && far < radius * 1.7;
            if radius > 1.5 * kit.reach.max(0.8) && turns.abs() >= 0.9 && round {
                kit.swirl = Some(((mx * aspect, my), turns / span.max(0.1)));
            }
        }
        self.last = Some(c);
    }
}

/// The music read as events.
#[derive(Default)]
pub(super) struct Ear {
    slow: f32,
    cool: f32,
    /// Seconds of music with the bass held back: a build-up.
    lull: f32,
    drop_cool: f32,
    silence: f32,
    /// How long the last silence lasted, and how long ago it ended.
    was_quiet: f32,
    since_quiet: f32,
}
impl Ear {
    pub fn hear(&mut self, kit: &mut Kit) {
        let (s, dt) = (kit.sound, kit.dt);
        // A beat is a rising edge of the bass, not a loud passage.
        self.cool -= dt;
        let rise = s.bass - self.slow;
        self.slow += (s.bass - self.slow) * (1. - (-dt * 2.5).exp());
        kit.beat = (rise > 0.1 && s.bass > 0.18 && self.cool <= 0.).then(|| {
            self.cool = 0.2;
            (rise * 2. + s.bass * 0.4).min(1.5)
        });
        // A drop: the bass back after a build-up without it, or loud music
        // straight out of a long silence.
        self.drop_cool -= dt;
        if s.treble > 0.12 && s.bass < 0.22 {
            self.lull += dt;
        } else if s.bass >= 0.22 {
            self.lull = (self.lull - dt * 0.5).max(0.);
        }
        if s.bass + s.treble < 0.03 {
            self.silence += dt;
        } else {
            if self.silence > 0. {
                self.was_quiet = self.silence;
                self.since_quiet = 0.;
            }
            self.silence = 0.;
            self.since_quiet += dt;
        }
        let built = self.lull > 1.5;
        let woke = self.was_quiet > 3. && self.since_quiet < 0.6;
        kit.drop = s.bass > 0.5 && (built || woke) && self.drop_cool <= 0.;
        if kit.drop {
            self.drop_cool = 4.;
            self.lull = 0.;
            self.was_quiet = 0.;
        }
        kit.silence = self.silence;
    }
}

// ---------------------------------------------------------------- the box
/// A toy with its kit, as the showcase runs it.
pub(super) struct Sandbox {
    toy: Box<dyn Play>,
    kit: Kit,
    hand: Hand,
    ear: Ear,
    sized: (usize, usize),
    clock_key: u32,
    #[cfg(test)]
    pub(super) fake_hms: Option<(u32, u32, u32)>,
}
impl Sandbox {
    pub(super) fn new(toy: Toy) -> Self {
        let toy: Box<dyn Play> = match toy {
            Toy::Ferro => Box::new(ferro::Ferro::new()),
            Toy::Excavation => Box::new(excavation::Excavation::new()),
            Toy::Zen => Box::new(zen::Zen::new()),
            Toy::Foundry => Box::new(foundry::Foundry::new()),
            Toy::Swarm => Box::new(swarm::Swarm::new()),
            Toy::Shore => Box::new(shore::Shore::new()),
            Toy::Glass => Box::new(glass::Glass::new()),
            Toy::Devil => Box::new(devil::Devil::new()),
            Toy::Spectrum => Box::new(spectrum::Spectrum::new()),
            Toy::Mandala => Box::new(mandala::Mandala::new()),
        };
        Self {
            toy,
            kit: Kit { aspect: 2., reach: 1., ..Kit::default() },
            hand: Hand::default(),
            ear: Ear::default(),
            sized: (0, 0),
            clock_key: u32::MAX,
            #[cfg(test)]
            fake_hms: None,
        }
    }
    pub(super) fn step(&mut self, dt: f32, sound: &Sound, pointer: Option<(f32, f32)>, cols: usize, rows: usize, aspect: f32) {
        let kit = &mut self.kit;
        kit.cols = cols;
        kit.rows = rows;
        kit.aspect = aspect.max(0.5);
        kit.dt = dt;
        kit.time += dt;
        kit.sound = *sound;
        kit.reach = music_reach(rows);
        #[cfg(test)]
        let hms = self.fake_hms.unwrap_or_else(local_hms);
        #[cfg(not(test))]
        let hms = local_hms();
        let key = hms.0 * 3600 + hms.1 * 60 + hms.2;
        kit.second = key != self.clock_key;
        kit.minute = key / 60 != self.clock_key / 60;
        self.clock_key = key;
        kit.hms = hms;
        let cursor = pointer.map(|(x, y)| {
            (
                (x * cols as f32).clamp(0., cols.saturating_sub(1) as f32),
                (y * rows as f32).clamp(0., rows.saturating_sub(1) as f32),
            )
        });
        self.hand.read(kit, cursor);
        self.ear.hear(kit);
        if self.sized != (cols, rows) {
            self.sized = (cols, rows);
            self.toy.resize(kit);
        }
        self.toy.step(kit);
    }
    pub(super) fn draw(&self, r: &mut Raster) {
        if self.sized == (0, 0) {
            return;
        }
        self.toy.draw(r, &self.kit);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn kit() -> Kit {
        Kit { cols: 174, rows: 50, aspect: 2., dt: 1. / 60., reach: 1., ..Kit::default() }
    }
    #[test]
    fn resting_charges_without_bursting() {
        let (mut hand, mut k) = (Hand::default(), kit());
        for _ in 0..240 {
            k.time += k.dt;
            hand.read(&mut k, Some((80., 25.)));
        }
        assert!(k.charge > 0.99);
        k.time += k.dt;
        hand.read(&mut k, Some((100., 25.)));
        assert_eq!(k.charge, 0.);
        assert!(!k.shake);
    }
    #[test]
    fn circling_swirls_and_shaking_bursts() {
        let (mut hand, mut k) = (Hand::default(), kit());
        let mut swirled = false;
        for i in 0..120 {
            k.time += k.dt;
            let a = i as f32 * 0.12;
            hand.read(&mut k, Some((87. + a.cos() * 16., 25. + a.sin() * 8.)));
            if let Some((c, rate)) = k.swirl {
                swirled = true;
                assert!(rate > 0.);
                assert!((c.0 - 87.).abs() < 8. && (c.1 - 25.).abs() < 4., "{c:?}");
            }
            assert!(!k.shake);
        }
        assert!(swirled);
        let (mut hand, mut k) = (Hand::default(), kit());
        let mut shook = false;
        for i in 0..60 {
            k.time += k.dt;
            let x = if (i / 6) % 2 == 0 { 60. + (i % 6) as f32 * 6. } else { 96. - (i % 6) as f32 * 6. };
            hand.read(&mut k, Some((x, 25.)));
            shook |= k.shake;
            assert!(k.swirl.is_none());
        }
        assert!(shook);
        // Brushing back and forth is no circle.
        let (mut hand, mut k) = (Hand::default(), kit());
        for i in 0..120 {
            k.time += k.dt;
            let a = (i as f32 * 0.15).sin();
            hand.read(&mut k, Some((87. + a * 30., 25. + a * 6.)));
            assert!(k.swirl.is_none(), "frame {i}");
        }
        // A straight sweep is neither.
        let (mut hand, mut k) = (Hand::default(), kit());
        for i in 0..90 {
            k.time += k.dt;
            hand.read(&mut k, Some((10. + i as f32 * 1.5, 20. + (i as f32 * 0.7).sin() * 11.)));
            assert!(!k.shake && k.swirl.is_none());
        }
    }
    #[test]
    fn a_build_up_and_a_silence_both_drop() {
        let (mut ear, mut k) = (Ear::default(), kit());
        let hear = |ear: &mut Ear, k: &mut Kit, bass: f32, treble: f32, secs: f32| {
            let mut dropped = 0;
            for _ in 0..(secs * 60.) as usize {
                k.sound = Sound { bass, treble };
                ear.hear(k);
                dropped += k.drop as usize;
            }
            dropped
        };
        assert_eq!(hear(&mut ear, &mut k, 0.6, 0.4, 3.), 0, "steady music is no drop");
        assert_eq!(hear(&mut ear, &mut k, 0.1, 0.5, 2.5), 0);
        assert_eq!(hear(&mut ear, &mut k, 0.8, 0.5, 1.), 1, "bass back after the build-up");
        assert_eq!(hear(&mut ear, &mut k, 0., 0., 4.), 0);
        assert!(k.silence > 3.9);
        assert_eq!(hear(&mut ear, &mut k, 0.8, 0.5, 1.), 1, "loud out of silence");
    }
    #[test]
    fn the_touch_follows_the_path() {
        let mut k = kit();
        k.last = Some((40., 20.));
        k.cursor = Some((80., 20.));
        assert!(k.touch((60., 20.5), 2.).is_some());
        assert!(k.touch((60., 26.), 2.).is_none());
        let (w, n) = k.touch((60., 21.), 2.).unwrap();
        assert!(w > 0.4 && n.1 > 0.9);
        let mut cells = 0;
        k.sweep(2., |x, y, _, _| {
            assert!(x < 174 && y < 50);
            cells += 1;
        });
        assert!(cells > 100);
    }
}
