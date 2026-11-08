//! Sand Mandala: a neon Tibetan sand mandala round the time. Bands of
//! coloured sand pour outward from the clock, each with its own motif:
//! lotus petals, teeth, beads, waves, lattices, a ring of fire. Everything
//! repeats round the centre with mirror symmetry, and so does whatever the
//! cursor draws: it pours sand like a chak-pur. Bass turns the mandala,
//! beats send light rings out through it, treble makes it glint. Each minute
//! the mandala is swept into the centre and that sand forms the new time.
use super::{hash, Kit, Play, SandText};
use super::super::{mix, scale, Color, Rng};
use crate::interactive_tunnel::Raster;
use std::f32::consts::{PI, TAU};

const PALETTE: [Color; 8] = [
    (255, 62, 190),
    (40, 222, 255),
    (255, 212, 58),
    (152, 92, 255),
    (110, 255, 122),
    (255, 126, 40),
    (255, 82, 118),
    (62, 124, 255),
];
const GOLD: Color = (255, 204, 96);
const SAND: Color = (255, 240, 212);
/// The mandala's rotational orders, in the order circling steps through them.
const FOLDS: [u32; 4] = [8, 12, 6, 16];
/// Angles kept in steps per turn, so folding is integer work. Divisible by
/// every fold and by twice it.
const TURN: u32 = 3840;
/// Angular bins of the drawn sand, across half a fold.
const BINS: usize = 64;
const SWEEP: f32 = 2.6;
/// Seconds hand-drawn sand holds, then takes to fade.
const HOLD: f32 = 7.;
const FADE: f32 = 6.;

/// A cell of the disc: its radius in rows and its angle in steps.
#[derive(Clone, Copy)]
struct Spot {
    x: i16,
    y: i16,
    r: f32,
    th: u16,
}
/// One ring of the pattern.
struct Band {
    r0: f32,
    r1: f32,
    motif: u8,
    ink: Color,
    ground: Color,
    edge: Color,
    /// Motif repeats per fold.
    count: f32,
}
/// Loose sand in the air: bursts, scatters, sparkles at the pouring edge.
struct Mote {
    x: f32,
    y: f32,
    vx: f32,
    vy: f32,
    life: f32,
    color: Color,
}

pub(super) struct Mandala {
    rng: Rng,
    spots: Vec<Spot>,
    /// Stray grains on the table round the disc.
    table: Vec<(i16, i16, u8, f32)>,
    center: (f32, f32),
    rmax: f32,
    hole: f32,
    size: f32,
    bands: Vec<Band>,
    /// Band per half row of radius, or 255.
    band_at: Vec<u8>,
    /// Light per half row of radius this frame: rings sent by beats.
    boost: Vec<f32>,
    /// Sand drawn by hand, per half row of radius and angular bin: palette
    /// index plus one.
    canvas: Vec<u8>,
    /// When each bin was poured: hand-drawn sand holds a while, then fades
    /// into the pattern so play never buries the mandala.
    poured: Vec<f32>,
    /// When the cursor last poured anything.
    painted_at: f32,
    painted: bool,
    /// The mandala's two hues and accent; the cursor pours these, gold and white.
    scheme: [Color; 3],
    fold: usize,
    rot: f32,
    front: f32,
    waves: Vec<f32>,
    flash: f32,
    sweep: Option<f32>,
    /// The time in sand: the cursor scatters it, it pours back home.
    digits: SandText,
    shown: String,
    motes: Vec<Mote>,
    glints: Vec<u32>,
    ink: usize,
    ink_clock: f32,
    fold_cool: f32,
    drawing: bool,
}
fn ease(p: f32) -> f32 {
    let p = p.clamp(0., 1.);
    p * p * (3. - 2. * p)
}
impl Mandala {
    pub(super) fn new() -> Self {
        Self {
            rng: Rng(rand::random()),
            spots: vec![],
            table: vec![],
            center: (0., 0.),
            rmax: 1.,
            hole: 0.4,
            size: 1.,
            bands: vec![],
            band_at: vec![],
            boost: vec![],
            canvas: vec![],
            poured: vec![],
            painted_at: f32::NEG_INFINITY,
            painted: false,
            scheme: [PALETTE[0], PALETTE[3], PALETTE[5]],
            fold: 0,
            rot: 0.,
            front: 0.,
            waves: vec![],
            flash: 0.,
            sweep: None,
            digits: SandText::new(false, true),
            shown: String::new(),
            motes: vec![],
            glints: vec![],
            ink: 0,
            ink_clock: 0.,
            fold_cool: 0.,
            drawing: false,
        }
    }
    fn folds(&self) -> u32 {
        FOLDS[self.fold % FOLDS.len()]
    }
    /// A fresh pattern: bands from the clock outward, each a new motif.
    fn lay_bands(&mut self) {
        self.bands.clear();
        // Two hues and an accent per mandala, so the motifs read as forms.
        let base = (self.rng.next() * 8.) as usize;
        let scheme = [PALETTE[base], PALETTE[(base + 3) % 8], PALETTE[(base + 5) % 8]];
        self.scheme = scheme;
        let mut r = self.hole + 0.5 * self.size;
        let mut i = 0;
        while r < self.rmax - 0.6 {
            let w = ((2.4 + self.rng.next() * 2.8) * self.size).min(self.rmax - r);
            let last = r + w >= self.rmax - 0.8 * self.size;
            let motif = if last {
                8
            } else if i == 0 {
                7
            } else {
                (self.rng.next() * 7.) as u8
            };
            let mid = r + w * 0.5;
            // Motifs about as wide as the band is deep, never smaller.
            let arc = TAU * mid / self.folds() as f32;
            let count = (arc / (w * 2.2)).round().clamp(1., 4.);
            self.bands.push(Band {
                r0: r,
                r1: r + w,
                motif,
                ink: scheme[i % 2],
                ground: scheme[(i + 1) % 2],
                edge: if i % 2 == 0 { GOLD } else { scheme[2] },
                count,
            });
            r += w;
            i += 1;
        }
        let bins = self.band_at.len();
        for (bin, slot) in self.band_at.iter_mut().enumerate().take(bins) {
            let r = bin as f32 * 0.5;
            *slot = self.bands.iter().position(|b| r >= b.r0 && r < b.r1).map_or(255, |i| i as u8);
        }
    }
    /// The time, placed in a box inside the clock's hole.
    fn place_time(&mut self, kit: &Kit) {
        let bh = self.hole * 0.38;
        let bw = self.hole * 0.9 * kit.aspect;
        let bounds = (
            (self.center.0 - bw).round() as i32,
            (self.center.1 - bh).round() as i32,
            (bw * 2.).max(1.) as usize,
            (bh * 2.).max(1.) as usize,
        );
        self.digits.place(kit, bounds);
        self.shown = kit.clock(false);
    }
    /// The polar position of a cell: radius in rows and angle in steps.
    fn polar(&self, kit: &Kit, x: f32, y: f32) -> (f32, u32) {
        let (dx, dy) = ((x - self.center.0) / kit.aspect, y - self.center.1);
        let r = (dx * dx + dy * dy).sqrt();
        let a = dy.atan2(dx).rem_euclid(TAU);
        (r, ((a / TAU * TURN as f32) as u32).min(TURN - 1))
    }
    /// Where an angle lands once folded: across one fold from 0 to 1.
    #[inline]
    fn fold_at(&self, th: u32, rot: u32) -> f32 {
        let sector = TURN / self.folds();
        let local = (th + rot) % TURN % sector;
        local as f32 / sector as f32
    }
    #[inline]
    fn rot_steps(&self) -> u32 {
        ((self.rot / TAU).rem_euclid(1.) * TURN as f32) as u32 % TURN
    }
    /// The pattern's sand at radius `r` and fold position `v`: colour,
    /// glyph and how bright, or nothing.
    #[inline]
    fn pattern(&self, r: f32, v: f32, time: f32) -> Option<(Color, char, f32)> {
        let band = *self.band_at.get((r * 2.) as usize)?;
        let b = self.bands.get(band as usize)?;
        let w = b.r1 - b.r0;
        let rr = (r - b.r0) / w;
        // A thin gold rim between bands.
        if rr < 0.5 / w {
            return Some((GOLD, '█', 0.9));
        }
        // Across one repeat: 0 on its axis, 1 on the mirror line.
        let q = (v * b.count).fract();
        let d = (q * 2. - 1.).abs();
        let (ink, ground, edge) = (Some((b.ink, '█', 1.)), Some((b.ground, '░', 0.42)), Some((b.edge, '█', 0.95)));
        match b.motif {
            // Lotus petals.
            0 => {
                let width = (1. - rr).sqrt() * (rr * 5.).min(1.) * 0.95;
                if d < width - 0.12 {
                    ink
                } else if d < width {
                    edge
                } else {
                    ground
                }
            }
            // Teeth.
            1 => {
                if d < rr - 0.08 {
                    ink
                } else if d < rr {
                    edge
                } else {
                    ground
                }
            }
            // Beads with a bright core.
            2 => {
                let arc = (TAU * (b.r0 + w * 0.5) / (self.folds() as f32 * 2. * b.count)) / w;
                let e = ((d * arc).powi(2) + ((rr - 0.5) * 2.).powi(2)).sqrt();
                if e < 0.3 {
                    edge
                } else if e < 0.75 {
                    ink
                } else if e < 0.9 {
                    edge
                } else {
                    ground
                }
            }
            // Waves.
            3 => {
                let off = (rr - 0.5 - 0.3 * (d * PI).cos()).abs();
                if off < 0.12 {
                    ink
                } else if off < 0.2 {
                    edge
                } else {
                    ground
                }
            }
            // Lattice.
            4 => {
                if ((d * 3.) as i32 + (rr * 3.) as i32) % 2 == 0 {
                    ink
                } else {
                    ground
                }
            }
            // Vajra diamonds.
            5 => {
                let e = d + (rr - 0.5).abs() * 2.;
                if e < 0.7 {
                    ink
                } else if e < 0.9 {
                    edge
                } else {
                    ground
                }
            }
            // Rays.
            6 => {
                if d > 0.82 - rr * 0.1 {
                    edge
                } else if d < 0.25 {
                    ink
                } else {
                    ground
                }
            }
            // A solid band threaded with beads.
            7 => {
                if (rr - 0.5).abs() < 0.16 && d > 0.55 {
                    edge
                } else {
                    ink
                }
            }
            // The ring of fire round the outside: flames that flicker.
            _ => {
                let flame = 0.25 + 0.7 * (1. - d) * (0.8 + 0.2 * (time * 6. + v * 40.).sin());
                if rr < flame * 0.6 {
                    Some(((255, 210, 70), '█', 1.))
                } else if rr < flame {
                    Some(((255, 96, 30), '▓', 0.9))
                } else {
                    None
                }
            }
        }
    }
    fn inks(&self) -> [Color; 5] {
        [self.scheme[0], self.scheme[1], self.scheme[2], GOLD, SAND]
    }
    /// Sand drawn by hand at radius `r` and fold position `v`, and how much
    /// of it is left.
    #[inline]
    fn drawn(&self, r: f32, v: f32, time: f32) -> Option<(Color, f32)> {
        let mirrored = 1. - (v * 2. - 1.).abs();
        let a = ((mirrored * (BINS - 1) as f32).round() as usize).min(BINS - 1);
        let i = (r * 2.) as usize * BINS + a;
        let c = *self.canvas.get(i)?;
        if c == 0 {
            return None;
        }
        let left = 1. - ((time - self.poured[i] - HOLD) / FADE).clamp(0., 1.);
        (left > 0.).then(|| (self.inks()[(c - 1) as usize % 5], left))
    }
    fn paint(&mut self, r: f32, v: f32, ink: u8, time: f32) {
        if r < self.hole || r > self.rmax {
            return;
        }
        let mirrored = 1. - (v * 2. - 1.).abs();
        let a = (mirrored * (BINS - 1) as f32).round() as i32;
        let rb = (r * 2.) as i32;
        // A little wider than a cell, so every repeat stays unbroken.
        for da in -1..=1 {
            {
                let (b, a) = (rb, a + da);
                if b < 0 || a < 0 || a >= BINS as i32 {
                    continue;
                }
                let i = b as usize * BINS + a as usize;
                if let Some(c) = self.canvas.get_mut(i) {
                    *c = ink;
                    self.poured[i] = time;
                }
            }
        }
        self.painted = true;
        self.painted_at = time;
    }
    fn burst(&mut self, at: (f32, f32), n: usize, power: f32, aspect: f32) {
        for _ in 0..n {
            let a = self.rng.next() * TAU;
            let v = power * (0.3 + self.rng.next() * 0.7);
            let color = PALETTE[(self.rng.next() * 8.) as usize % 8];
            self.motes.push(Mote {
                x: at.0,
                y: at.1,
                vx: a.cos() * v * aspect,
                vy: a.sin() * v,
                life: 0.6 + self.rng.next() * 0.8,
                color,
            });
        }
    }
    /// The whole pattern lifts off as loose sand.
    fn scatter(&mut self, kit: &Kit) {
        let rot = self.rot_steps();
        let reach = self.front.min(self.rmax);
        let n = self.spots.partition_point(|s| s.r <= reach);
        for _ in 0..600.min(n) {
            let s = self.spots[(self.rng.next() * n as f32) as usize % n];
            let v = self.fold_at(s.th as u32, rot);
            let color = self.drawn(s.r, v, kit.time).map(|d| d.0).or_else(|| self.pattern(s.r, v, kit.time).map(|p| p.0));
            if let Some(color) = color {
                let (dx, dy) = ((s.x as f32 - self.center.0) / kit.aspect, s.y as f32 - self.center.1);
                let d = (dx * dx + dy * dy).sqrt().max(0.5);
                let v = (12. + self.rng.next() * 30.) * self.size;
                self.motes.push(Mote {
                    x: s.x as f32,
                    y: s.y as f32,
                    vx: dx / d * v * kit.aspect,
                    vy: dy / d * v,
                    life: 0.6 + self.rng.next() * 0.9,
                    color,
                });
            }
        }
        self.canvas.iter_mut().for_each(|c| *c = 0);
        self.painted = false;
        self.front = self.hole;
    }
}
impl Play for Mandala {
    fn resize(&mut self, kit: &Kit) {
        let (cols, rows) = (kit.cols, kit.rows);
        self.center = ((cols as f32 - 1.) * 0.5, (rows as f32 - 1.) * 0.5);
        self.rmax = ((rows as f32 * 0.48).min(cols as f32 / kit.aspect * 0.48)).max(0.5);
        self.hole = self.rmax * 0.47;
        self.size = kit.reach * 1.6;
        self.spots.clear();
        self.table.clear();
        for y in 0..rows {
            for x in 0..cols {
                let (r, th) = self.polar(kit, x as f32, y as f32);
                if r <= self.rmax {
                    if r >= self.hole - 0.5 {
                        self.spots.push(Spot { x: x as i16, y: y as i16, r, th: th as u16 });
                    }
                } else {
                    let h = hash((x as u32).wrapping_mul(9277) ^ (y as u32).wrapping_mul(26699));
                    if h < 0.035 {
                        self.table.push((x as i16, y as i16, (h * 1000.) as u8 % 8, h * 180.));
                    }
                }
            }
        }
        self.spots.sort_by(|a, b| a.r.total_cmp(&b.r));
        let bins = (self.rmax * 2.) as usize + 3;
        self.band_at = vec![255; bins];
        self.boost = vec![0.; bins];
        self.canvas = vec![0; bins * BINS];
        self.poured = vec![0.; bins * BINS];
        self.painted = false;
        self.lay_bands();
        self.front = self.hole;
        self.sweep = None;
        self.motes.clear();
        self.place_time(kit);
    }
    fn step(&mut self, kit: &Kit) {
        let (dt, time, s) = (kit.dt, kit.time, kit.sound);
        // Bass turns the mandala.
        self.rot += dt * (0.035 + s.bass * 0.45);
        self.flash = (self.flash - dt * 1.4).max(0.);
        self.fold_cool -= dt;
        // Sand pours outward, faster with music.
        if self.sweep.is_none() {
            let before = self.front;
            self.front = (self.front + dt * (0.35 + s.bass * 1.3 + s.treble * 0.3) * self.size).min(self.rmax + 1.);
            // A few grains bounce off the pouring edge.
            if self.front < self.rmax && self.rng.next() < dt * 30. && self.front > before {
                let a = self.rng.next() * TAU;
                let p = (self.center.0 + a.cos() * self.front * kit.aspect, self.center.1 + a.sin() * self.front);
                let color = PALETTE[(self.rng.next() * 8.) as usize % 8];
                self.motes.push(Mote { x: p.0, y: p.1, vx: a.cos() * 3. * kit.aspect, vy: a.sin() * 3. - 2., life: 0.5, color });
            }
        }
        // Each minute: sweep it all into the centre, then pour the new time.
        let text = kit.clock(false);
        if text != self.shown && self.sweep.is_none() {
            self.sweep = Some(time);
            self.flash = 0.6;
        }
        if let Some(start) = self.sweep {
            if time - start >= SWEEP {
                self.sweep = None;
                self.canvas.iter_mut().for_each(|c| *c = 0);
                self.painted = false;
                self.fold = (self.fold + 1 + (self.rng.next() * 2.) as usize) % FOLDS.len();
                self.lay_bands();
                self.front = self.hole;
                // The swept sand bursts out of the centre and pours into the new time.
                let c = self.center;
                for g in &mut self.digits.grains {
                    let a = self.rng.next() * TAU;
                    let d = self.rng.next() * 1.5;
                    (g.x, g.y) = (c.0 + a.cos() * d * kit.aspect, c.1 + a.sin() * d);
                }
                self.shown = text.clone();
                self.burst(c, 60, 14. * self.size, kit.aspect);
            }
        }
        // The cursor pours sand; it repeats round the whole mandala.
        let rot = self.rot_steps();
        match kit.cursor {
            Some(_) if self.sweep.is_none() => {
                if !self.drawing {
                    self.drawing = true;
                    self.ink = (self.ink + 1 + (self.rng.next() * 2.) as usize) % 5;
                    self.ink_clock = 0.;
                }
                self.ink_clock += dt;
                if self.ink_clock > 2.2 {
                    self.ink_clock = 0.;
                    self.ink = (self.ink + 2) % 5;
                }
                let ink = self.ink as u8 + 1;
                let mut cells = vec![];
                kit.sweep(0.5 * kit.reach.max(0.8), |x, y, _, _| cells.push((x as f32, y as f32)));
                for (x, y) in cells {
                    let (r, th) = self.polar(kit, x, y);
                    let v = self.fold_at(th, rot);
                    self.paint(r, v, ink, time);
                }
            }
            _ => self.drawing = false,
        }
        if self.painted && time - self.painted_at > HOLD + FADE {
            self.canvas.iter_mut().for_each(|c| *c = 0);
            self.painted = false;
        }
        if kit.swirl.is_some() && self.fold_cool <= 0. {
            self.fold_cool = 2.;
            self.fold = (self.fold + 1) % FOLDS.len();
            self.flash = self.flash.max(0.5);
        }
        if kit.shake {
            self.scatter(kit);
        }
        if kit.drop {
            self.front = self.rmax + 1.;
            self.flash = 1.;
        }
        // Beats send a ring of light out through the sand.
        if let Some(k) = kit.beat {
            if self.waves.len() < 6 {
                self.waves.push(time - (1. - k.min(1.)) * 0.1);
            }
        }
        let speed = 22. * self.size;
        let (hole, rmax) = (self.hole, self.rmax);
        self.waves.retain(|&born| hole + (time - born) * speed < rmax + 3.);
        let width = 1.4 * self.size;
        for (bin, b) in self.boost.iter_mut().enumerate() {
            let r = bin as f32 * 0.5;
            let mut k = 0.;
            for &born in &self.waves {
                let at = hole + (time - born) * speed;
                let d = (r - at) / width;
                if d.abs() < 3. {
                    k += (-d * d).exp();
                }
            }
            *b = k.min(1.5);
        }
        // The time: scattered by the cursor and the gestures, pouring home.
        self.digits.step(kit);
        for m in &mut self.motes {
            m.x += m.vx * dt;
            m.y += m.vy * dt;
            m.vx *= (-dt * 2.2).exp();
            m.vy = m.vy * (-dt * 2.2).exp() + dt * 6.;
            m.life -= dt;
        }
        let (w, h) = (kit.cols as f32, kit.rows as f32);
        self.motes.retain(|m| m.life > 0. && m.x >= 0. && m.y >= 0. && m.x < w && m.y < h);
        if self.motes.len() > 1500 {
            let extra = self.motes.len() - 1500;
            self.motes.drain(..extra);
        }
        // Treble glints on the poured sand.
        self.glints.clear();
        let n = self.spots.partition_point(|sp| sp.r <= self.front.min(self.rmax));
        if n > 0 {
            let count = (n as f32 * (0.001 + s.treble * 0.006)) as usize;
            for _ in 0..count {
                self.glints.push((self.rng.next() * n as f32) as u32 % n as u32);
            }
        }
    }
    fn draw(&self, r: &mut Raster, kit: &Kit) {
        let s = kit.sound;
        let time = kit.time;
        let rot = self.rot_steps();
        // During the sweep the pattern shrinks into the centre, twisting.
        let squeeze = self.sweep.map(|start| ease((time - start) / SWEEP));
        let keep = squeeze.map_or(1., |p| (1. - p).max(0.02));
        let twist = squeeze.unwrap_or(0.) * 2.4;
        let reach = if self.painted || squeeze.is_some() { self.rmax } else { self.front.min(self.rmax) };
        let limit = reach * keep + 0.5;
        let feather = 1.2 * self.size;
        let light = 0.62 + s.bass * 0.22 + self.flash * 0.5;
        for (i, sp) in self.spots.iter().enumerate() {
            if sp.r > limit {
                break;
            }
            let (radius, th) = if squeeze.is_some() {
                let src = sp.r / keep;
                if src > self.rmax {
                    continue;
                }
                let turn = twist * (1.3 - src / self.rmax);
                let back = ((turn / TAU).rem_euclid(1.) * TURN as f32) as u32;
                (src, (sp.th as u32 + TURN - back % TURN) % TURN)
            } else {
                (sp.r, sp.th as u32)
            };
            if radius < self.hole {
                continue;
            }
            let v = self.fold_at(th, rot);
            let grain = hash(i as u32 ^ 0x5bd1e995);
            let boost = self.boost.get((radius * 2.) as usize).copied().unwrap_or(0.);
            let hand = self.drawn(radius, v, time);
            let (color, ch, k) = match hand {
                Some((c, left)) if left >= 1. => (c, '█', 1.05),
                _ => {
                    // Not poured yet, or only just: the edge is grainy.
                    if squeeze.is_none() && radius > self.front - feather {
                        let t = (self.front - radius) / feather;
                        if t <= 0. || grain > t {
                            continue;
                        }
                    }
                    match (self.pattern(radius, v, time), hand) {
                        // Fading hand-drawn sand sinks into the pattern.
                        (Some(p), Some((c, left))) => (mix(p.0, c, left), p.1, p.2 + (1.05 - p.2) * left),
                        (Some(p), None) => p,
                        (None, Some((c, left))) => (c, '█', 1.05 * left),
                        (None, None) => continue,
                    }
                }
            };
            let k = k * light * (0.93 + grain * 0.12) * (1. + boost * 0.7);
            let tone = if boost > 0.4 { mix(color, (255, 255, 255), (boost - 0.4) * 0.4) } else { color };
            r.dot(sp.x as i32, sp.y as i32, 5., ch, scale(tone, k));
        }
        for &g in &self.glints {
            if let Some(sp) = self.spots.get(g as usize) {
                if squeeze.is_none() {
                    r.dot(sp.x as i32, sp.y as i32, 4., '+', mix(SAND, (255, 255, 255), 0.5 + s.treble * 0.5));
                }
            }
        }
        // Stray grains on the table, twinkling slowly.
        for &(x, y, c, phase) in &self.table {
            let k = 0.18 + 0.14 * (time * 0.7 + phase).sin() + s.treble * 0.1;
            r.dot(x as i32, y as i32, 8., '·', scale(PALETTE[c as usize], k.max(0.05)));
        }
        // The clock's own ring: a gold rim and a bead for every second.
        let (cx, cy) = self.center;
        let rim = (self.hole - 0.6) * keep;
        if rim > 1. {
            let lit = kit.hms.2 as usize;
            for k in 0..60 {
                let a = -PI / 2. + k as f32 / 60. * TAU + twist;
                let x = cx + a.cos() * rim * kit.aspect;
                let y = cy + a.sin() * rim;
                let (ch, tone) = if k <= lit {
                    ('●', scale(GOLD, 0.75 + s.treble * 0.3 + if k == lit { 0.3 } else { 0. }))
                } else {
                    ('·', scale(GOLD, 0.35))
                };
                r.dot(x.round() as i32, y.round() as i32, 3., ch, tone);
            }
        }
        // The time, in sand; swept round and in with everything else.
        let bright = 1. + s.bass * 0.25 + self.flash * 0.5;
        let tints = self.scheme;
        for g in &self.digits.grains {
            let (x, y) = match squeeze {
                Some(p) => {
                    let (dx, dy) = ((g.x - cx) / kit.aspect, g.y - cy);
                    let (sa, ca) = (p * 2.4).sin_cos();
                    let (rx, ry) = (dx * ca - dy * sa, dx * sa + dy * ca);
                    (cx + rx * keep * kit.aspect, cy + ry * keep)
                }
                None => (g.x, g.y),
            };
            // Packed sand once home; loose grains while they fly, taking
            // on the mandala's colours the further they are thrown.
            let away = g.away();
            let ch = if away < 0.5 && squeeze.is_none() {
                '█'
            } else if g.shade > 0.78 {
                '●'
            } else {
                '•'
            };
            let glint = (time * (1.1 + g.shade * 1.5) + g.phase).sin().max(0.).powi(8) * (0.6 + s.treble);
            let tint = tints[(g.phase * 3.) as usize % 3];
            let tone = mix(mix(SAND, tint, (away / 6.).min(0.7)), (255, 255, 255), glint);
            r.dot(x.round() as i32, y.round() as i32, 2., ch, scale(tone, bright * (0.85 + g.shade * 0.15 + glint * 0.3)));
        }
        for m in &self.motes {
            r.dot(m.x.round() as i32, m.y.round() as i32, 1.5, if m.life > 0.4 { '•' } else { '·' }, scale(m.color, (m.life * 1.6).min(1.)));
        }
        // The chak-pur: sand pouring from the cursor.
        if let (Some(c), None) = (kit.cursor, self.sweep) {
            let ink = self.inks()[self.ink % 5];
            r.dot(c.0 as i32, c.1 as i32 - 1, 1., '▼', mix(ink, (255, 255, 255), 0.3));
            for k in 0..3 {
                let fall = ((time * 3. + k as f32 / 3.).fract() * 2.) as i32;
                r.dot(c.0 as i32 + (k - 1), c.1 as i32 + fall, 1.1, '·', ink);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    #[test]
    #[ignore = "manual: writes PPM stills"]
    fn mandala_script_stills() {
        let snaps: Vec<f32> = std::env::var("SNAPS").ok().filter(|s| !s.is_empty()).unwrap_or_else(|| "4,7.2,10,15,17.3".into()).split(',').filter_map(|v| v.parse().ok()).collect();
        super::super::foundry::tests::script_stills("sand-clock-mandala", &snaps);
    }
}
