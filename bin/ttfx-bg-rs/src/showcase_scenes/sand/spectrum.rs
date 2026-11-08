//! Sand equalizer: a glass tube per band, and sand pouring from the clock
//! into each until its pile stands as high as the band is loud. When a band
//! goes quiet its tube drains from below and the pile sinks and slumps.
//!
//! A pile is a stack of grains per column, so it never has holes and sand
//! only slides where a column stands two grains above its neighbour: the
//! angle of repose in one pass along the floor instead of a cellular
//! automaton over every cell. Columns are two grains per text row, drawn
//! with half blocks, so grains are square on screen. Grains in the air are
//! particles: the pour, and whatever the cursor, a hair dryer, blows off.
use super::super::{mix, noise, scale, Color, Rng};
use super::{hash, Kit, Play, SandText};
use crate::interactive_tunnel::Raster;
use std::f32::consts::TAU;

/// Band colours from bass to treble, mixed with sand.
const HUES: [Color; 6] = [
    (255, 70, 140),
    (255, 140, 70),
    (250, 215, 90),
    (100, 230, 160),
    (70, 175, 255),
    (175, 110, 255),
];
const SAND: Color = (235, 190, 130);

fn hue(f: f32) -> Color {
    let t = f.clamp(0., 1.) * (HUES.len() - 1) as f32;
    let i = (t as usize).min(HUES.len() - 2);
    mix(HUES[i], HUES[i + 1], t - i as f32)
}

struct Grain {
    /// Column, and height in half rows from the top of the screen.
    x: f32,
    y: f32,
    vx: f32,
    vy: f32,
    shade: u8,
    /// Grains it stands for: on a big grid one falling speck carries a few,
    /// so the tubes fill as fast as on a small one.
    mass: u8,
    /// Seconds left caught in a sand twister, and its angle round the axis.
    spin: f32,
    phase: f32,
}

struct Band {
    /// Inner columns, between the walls.
    x0: usize,
    x1: usize,
    level: f32,
    /// Grains still owed to the pour, and to the drain.
    pour: f32,
    drain: f32,
    /// The peak marker: height in grains and how long it holds.
    peak: f32,
    hold: f32,
    tint: Color,
    /// Share of the spectrum, 0 for the lowest band.
    f: f32,
}

pub(super) struct Spectrum {
    cols: usize,
    rows: usize,
    /// Sand per column, bottom grain first: a shade per grain.
    stacks: Vec<Vec<u8>>,
    /// Glass and margins: sand never rests there.
    wall: Vec<bool>,
    /// Which band a column belongs to, and the nearest open column.
    band_of: Vec<u16>,
    home: Vec<usize>,
    bands: Vec<Band>,
    air: Vec<Grain>,
    /// Half rows: the tube tops and the floor.
    top: usize,
    floor: usize,
    clock_rows: usize,
    twister: Option<(f32, f32, f32)>,
    slide: f32,
    sweep: bool,
    flash: f32,
    rng: Rng,
    /// Grains in flight towards each band, counted each step.
    owed: Vec<u32>,
    /// The clock, grain by grain.
    text: SandText,
    /// Per digit grain: seconds left falling, and its fall speed. Blown
    /// grains rain down onto the piles before they pour home.
    fall: Vec<(f32, f32)>,
}

impl Spectrum {
    pub(super) fn new() -> Self {
        Self {
            cols: 0,
            rows: 0,
            stacks: vec![],
            wall: vec![],
            band_of: vec![],
            home: vec![],
            bands: vec![],
            air: vec![],
            top: 0,
            floor: 0,
            clock_rows: 0,
            twister: None,
            slide: 0.,
            sweep: false,
            flash: 0.,
            rng: Rng(rand::random()),
            owed: vec![],
            text: SandText::new(true, true),
            fall: vec![],
        }
    }
    fn room(&self) -> usize {
        self.floor.saturating_sub(self.top).max(1)
    }
    /// Lift the top grain of column `x` into the air with velocity `v`
    /// (columns and half rows per second).
    fn lift(&mut self, x: usize, v: (f32, f32), spin: f32) -> bool {
        let Some(shade) = self.stacks.get_mut(x).and_then(|s| s.pop()) else {
            return false;
        };
        if self.air.len() >= self.air_cap() {
            self.stacks[x].push(shade);
            return false;
        }
        let y = (self.floor - self.stacks[x].len()) as f32 - 0.5;
        self.air.push(Grain {
            x: x as f32,
            y,
            vx: v.0,
            vy: v.1,
            shade,
            mass: 1,
            spin,
            phase: self.rng.next() * TAU,
        });
        true
    }
    fn air_cap(&self) -> usize {
        (self.cols * self.rows / 3).clamp(200, 9000)
    }
    /// The pseudo spectrum: only bass and treble arrive, so low bands follow
    /// the bass, high ones the treble, each with its own wobble. Silence
    /// keeps a slow swell so the sand never stops flowing.
    fn loudness(&self, kit: &Kit, band: &Band, i: usize) -> f32 {
        let (s, t, f) = (kit.sound, kit.time, band.f);
        // Each band wanders on its own so neighbours never move as one.
        let wobble = 0.3 + 0.7 * noise(t * 1.6 + i as f32 * 3.7, i as f32 * 1.3).powf(1.5);
        let low = s.bass * (1. - f).powf(1.4);
        let high = s.treble * f.powf(0.8) * 0.85;
        let mid = (s.bass * 0.4 + s.treble * 0.6) * (1. - (f - 0.4).abs() * 2.2).max(0.);
        let music = (low * 1.15 + high + mid * 0.6) * wobble;
        let idle = 0.09 + 0.06 * (t * 0.45 + i as f32 * 0.6).sin() + 0.03 * (t * 1.3 + i as f32 * 1.7).sin();
        idle + music * 1.05
    }
}

impl Spectrum {
    /// Digit grains knocked off the clock fall like the rest of the sand and
    /// settle on the piles until their time is up and they pour home.
    fn rain(&mut self, kit: &Kit) {
        self.text.step(kit);
        let n = self.text.grains.len();
        self.fall.resize(n, (0., 0.));
        let gravity = self.rows as f32 * 1.6;
        let floor = self.floor;
        for i in 0..n {
            let g = &mut self.text.grains[i];
            let f = &mut self.fall[i];
            let away = g.away();
            if f.0 <= 0. {
                // Spent: pouring home, free to fall again once it lands.
                if f.1 < 0. {
                    if away < 0.5 {
                        f.1 = 0.;
                    }
                    continue;
                }
                if away <= 1.2 {
                    continue;
                }
                // Just blown loose: fall for about as long as it stays loose.
                *f = (2.2 + g.shade, 0.);
            }
            f.0 -= kit.dt;
            f.1 += gravity * kit.dt;
            let x = (g.x.round().max(0.) as usize).min(self.cols - 1);
            let rest = ((floor - self.stacks[x].len().min(floor)) as f32 / 2. - 1.).max(0.);
            g.y = (g.y + f.1 * kit.dt).min(rest);
            if g.y >= rest {
                f.1 = 0.;
            }
            if f.0 <= 0. {
                f.1 = -1.;
            }
        }
    }
}

impl Play for Spectrum {
    fn resize(&mut self, kit: &Kit) {
        let (cols, rows) = (kit.cols, kit.rows);
        self.cols = cols;
        self.rows = rows;
        if cols == 0 || rows == 0 {
            self.bands.clear();
            return;
        }
        self.floor = rows * 2;
        self.clock_rows = ((rows as f32 * 0.3).round() as usize).clamp(1, rows);
        self.top = ((self.clock_rows + 1) * 2).min(self.floor.saturating_sub(2));
        self.air.clear();
        self.twister = None;
        // Tubes about eleven columns wide, glass between them and margins
        // either side.
        let margin = (cols as f32 * 0.025) as usize;
        let inner = cols.saturating_sub(margin * 2).max(1);
        let n = (inner / 11).clamp(1, 24);
        let width = inner as f32 / n as f32;
        let glass = width >= 4.;
        self.wall = vec![true; cols];
        self.band_of = vec![0; cols];
        self.bands.clear();
        for i in 0..n {
            let a = margin + (i as f32 * width) as usize;
            let b = (margin + ((i + 1) as f32 * width) as usize).min(cols);
            let (x0, x1) = if glass { (a + 1, b.saturating_sub(1)) } else { (a, b) };
            let x0 = x0.min(cols - 1);
            let x1 = x1.clamp(x0 + 1, cols);
            for x in x0..x1 {
                self.wall[x] = false;
                self.band_of[x] = i as u16;
            }
            let f = if n > 1 { i as f32 / (n - 1) as f32 } else { 0.5 };
            self.bands.push(Band {
                x0,
                x1,
                level: 0.12,
                pour: 0.,
                drain: 0.,
                peak: 0.,
                hold: 0.,
                tint: mix(hue(f), SAND, 0.3),
                f,
            });
        }
        if cols > 0 && self.wall.iter().all(|&w| w) {
            self.wall[0] = false;
        }
        // Where sand that lands on glass or a margin ends up.
        let mut left = vec![None; cols];
        let mut right = vec![None; cols];
        for x in 0..cols {
            left[x] = if self.wall[x] { x.checked_sub(1).and_then(|p| left[p]) } else { Some(x) };
        }
        for x in (0..cols).rev() {
            right[x] = if self.wall[x] { right.get(x + 1).copied().flatten() } else { Some(x) };
        }
        // Glass between two tubes sheds either way.
        self.home = (0..cols)
            .map(|x| match (left[x], right[x]) {
                (Some(l), Some(r)) => if x - l < r - x || (x - l == r - x && x % 2 == 0) { l } else { r },
                (Some(l), None) => l,
                (None, Some(r)) => r,
                (None, None) => 0,
            })
            .collect();
        let room = self.room();
        self.stacks = (0..cols)
            .map(|x| {
                if self.wall[x] {
                    return vec![];
                }
                let b = &self.bands[self.band_of[x] as usize];
                let mid = (b.x0 + b.x1) as f32 / 2.;
                let cone = ((b.x1 - b.x0) as f32 / 2. - (x as f32 - mid).abs()).max(0.);
                let h = ((room as f32 * 0.14) as usize + cone as usize).min(room);
                (0..h).map(|_| (self.rng.next() * 255.) as u8).collect()
            })
            .collect();
        self.owed = vec![0; self.bands.len()];
        let w = ((cols as f32 * 0.84) as usize).max(1);
        self.text.place(kit, ((cols - w) as i32 / 2, 0, w, self.clock_rows.saturating_sub(1).max(1)));
        self.fall.clear();
    }

    fn step(&mut self, kit: &Kit) {
        if self.cols == 0 || self.rows == 0 || self.bands.is_empty() {
            return;
        }
        let (dt, reach, aspect) = (kit.dt, kit.reach, kit.aspect);
        let (floor, top, room) = (self.floor, self.top, self.room());
        // Half rows per second squared: a fall from the clock takes about a second.
        let gravity = self.rows as f32 * 3.2;
        self.flash = (self.flash - dt * 1.5).max(0.);
        // In-flight sand counts towards its band, or the pour overshoots.
        self.owed.iter_mut().for_each(|o| *o = 0);
        for g in &self.air {
            let x = (g.x.round().max(0.) as usize).min(self.cols - 1);
            self.owed[self.band_of[self.home[x]] as usize] += g.mass as u32;
        }
        let beat = kit.beat;
        for i in 0..self.bands.len() {
            let target = self.loudness(kit, &self.bands[i], i).min(1.05);
            let target = target + beat.map_or(0., |k| k * 0.25 * (1. - self.bands[i].f * 2.).max(0.));
            let b = &mut self.bands[i];
            let k = if target > b.level { 1. - (-dt * 9.).exp() } else { 1. - (-dt * 2.2).exp() };
            b.level += (target - b.level) * k;
            let width = (b.x1 - b.x0) as f32;
            let held: usize = self.stacks[b.x0..b.x1].iter().map(|s| s.len()).sum();
            let want = (0.06 + 0.88 * b.level.min(1.)) * room as f32 * width;
            let have = held as f32 + self.owed[i] as f32;
            // A trickle always runs, like an hourglass; the drain answers it.
            b.pour += dt * ((want - have).max(0.) * 2.6 + 5. + width * 0.4);
            let over = held as f32 - want;
            b.drain += if over > 0. { dt * (over * 2.2 + 5. + width * 0.4) } else { 0. };
            let mean = held as f32 / width.max(1.);
            if mean >= b.peak {
                b.peak = mean;
                b.hold = 0.7;
            } else if b.hold > 0. {
                b.hold -= dt;
            } else {
                b.peak = (b.peak - room as f32 * 0.35 * dt).max(mean);
            }
        }
        // Pour from just under the clock, drain from the floor.
        let cap = self.air_cap();
        let mass = (room / 90).clamp(1, 8) as u8;
        for i in 0..self.bands.len() {
            let (x0, x1) = (self.bands[i].x0, self.bands[i].x1);
            let mid = (x0 + x1) as f32 / 2. - 0.5;
            let spread = ((x1 - x0) as f32 * 0.12).max(0.3);
            while self.bands[i].pour >= 1. {
                self.bands[i].pour -= mass as f32;
                if self.air.len() >= cap {
                    self.bands[i].pour = 0.;
                    break;
                }
                let x = mid + (self.rng.next() - 0.5) * spread;
                self.air.push(Grain {
                    x,
                    y: top as f32 - 1. - self.rng.next() * 2.,
                    vx: (self.rng.next() - 0.5) * 2.,
                    vy: self.rng.next() * 8.,
                    shade: (self.rng.next() * 255.) as u8,
                    mass,
                    spin: 0.,
                    phase: 0.,
                });
            }
            while self.bands[i].drain >= 1. {
                self.bands[i].drain -= 1.;
                let x = x0 + ((self.rng.next() * (x1 - x0) as f32) as usize).min(x1 - x0 - 1);
                if !self.stacks[x].is_empty() {
                    self.stacks[x].remove(0);
                }
            }
        }
        // The hair dryer blows the surface it passes over.
        let speed_boost = 1. + (kit.speed / (20. * reach.max(0.8))).min(2.);
        if kit.cursor.is_some() {
            let radius = (self.rows as f32 * 0.09).max(2.);
            let blow = (24. + 30. * speed_boost) * reach;
            for x in 0..self.cols {
                if self.wall[x] || self.stacks[x].is_empty() {
                    continue;
                }
                let surface = (floor - self.stacks[x].len()) as f32 / 2.;
                // Buried in a pile, it blows the sand above it up like a geyser.
                let c = kit.cursor.unwrap();
                let side = (x as f32 - c.0) / (radius * aspect);
                let buried = (c.1 > surface && side.abs() < 1.).then(|| (1. - side.abs(), (side * 0.45, -1.)));
                let Some((w, n)) = kit.touch((x as f32, surface), radius).or(buried) else { continue };
                let many = (w * 45. * speed_boost * dt + self.rng.next()) as usize;
                for _ in 0..many.min(4) {
                    let side = if n.1 > 0.3 { n.0.signum() * 0.8 } else { 0. };
                    let vx = (n.0 + side) * blow * aspect + kit.velocity.0 * aspect * 0.5 + (self.rng.next() - 0.5) * 6.;
                    let vy = ((n.1.min(-0.2)) * blow - 8. * reach + kit.velocity.1 * 0.4) * 2.;
                    self.lift(x, (vx, vy.min(-6.)), 0.);
                }
            }
        }
        // Circling raises a sand twister that drinks from the piles below it.
        if let Some((c, rate)) = kit.swirl {
            let strength = rate.abs().clamp(0.5, 3.);
            self.twister = Some((c.0, strength, 0.5));
        }
        if let Some((tx, strength, life)) = self.twister {
            let life = life - dt;
            self.twister = (life > 0.).then_some((tx, strength, life));
            let reach_x = (self.rows as f32 * 0.06 * aspect).max(1.5);
            let x0 = (tx - reach_x).floor().max(0.) as usize;
            let x1 = ((tx + reach_x).ceil().max(0.) as usize).min(self.cols.saturating_sub(1));
            for x in x0..=x1 {
                if self.wall[x] {
                    continue;
                }
                let many = ((25. + strength * 25.) * dt + self.rng.next()) as usize;
                for _ in 0..many {
                    let spin = 0.8 + self.rng.next() * 1.2;
                    self.lift(x, (0., -(18. + strength * 10.) * reach * 2.), spin);
                }
            }
        }
        if kit.shake || kit.drop {
            let power = if kit.drop { 1.9 } else { 1. };
            if kit.drop {
                self.flash = 1.;
            }
            for x in 0..self.cols {
                if self.wall[x] {
                    continue;
                }
                let many = if kit.drop {
                    (self.stacks[x].len() as f32 * (0.25 + self.rng.next() * 0.15)) as usize
                } else {
                    2 + (self.rng.next() * 3.) as usize
                }
                .min(40);
                for _ in 0..many {
                    let v = (22. + self.rng.next() * 45.) * power * reach;
                    let vx = (self.rng.next() - 0.5) * 14. * aspect * power;
                    if !self.lift(x, (vx, -v * 2.), 0.) {
                        break;
                    }
                }
            }
        }
        // Bass bands hop on the beat.
        if let Some(k) = beat {
            for i in 0..self.bands.len() {
                let f = self.bands[i].f;
                if f > 0.4 {
                    continue;
                }
                for x in self.bands[i].x0..self.bands[i].x1 {
                    if self.rng.next() < 0.7 * (1. - f * 2.) {
                        let v = (8. + self.rng.next() * 16.) * k * reach;
                        let vx = (self.rng.next() - 0.5) * 4.;
                        self.lift(x, (vx, -v * 2.), 0.);
                    }
                }
            }
        }
        // Grains in the air fall, drift and land on the column below them.
        let drag = (-dt * 0.9).exp();
        let twister = self.twister;
        let last = (self.cols - 1) as f32;
        let mut i = 0;
        while i < self.air.len() {
            let g = &mut self.air[i];
            if g.spin > 0. {
                g.spin -= dt;
                let (tx, strength) = twister.map_or((g.x, 1.), |t| (t.0, t.1));
                let height = ((floor as f32 - g.y) / floor as f32).clamp(0., 1.);
                let radius = (0.6 + height * 4.5) * reach;
                g.phase += dt * (6. + strength * 3.);
                g.y -= dt * (20. + strength * 8.) * reach * 2.;
                g.x = tx + g.phase.cos() * radius * aspect;
                if g.spin <= 0. || g.y < top as f32 * 0.4 {
                    g.spin = 0.;
                    g.vx = -g.phase.sin() * radius * aspect * (6. + strength * 3.) * 0.6;
                    g.vy = -10. * reach;
                }
            } else {
                g.vy += gravity * dt;
                g.vx *= drag;
                g.x += g.vx * dt;
                g.y += g.vy * dt;
            }
            if g.x < 0. || g.x > last {
                g.x = g.x.clamp(0., last);
                g.vx = -g.vx * 0.4;
            }
            let mut x = g.x.round() as usize;
            if self.wall[x] && g.y >= top as f32 {
                // Glass: slide off into the nearest tube.
                x = self.home[x];
                g.x = x as f32;
                g.vx *= -0.3;
            }
            let surface = (floor - self.stacks[x].len()) as f32;
            if g.spin <= 0. && g.y >= surface - 0.5 && g.vy >= 0. && !self.wall[x] {
                let (shade, mass) = (g.shade, g.mass);
                self.air.swap_remove(i);
                for m in 0..mass {
                    // Each grain rolls down the slope it lands on.
                    let mut at = x;
                    for step in 0..12 {
                        let left_first = (m as usize + step + at) % 2 == 0;
                        let (a, b) = if left_first { (at.wrapping_sub(1), at + 1) } else { (at + 1, at.wrapping_sub(1)) };
                        let h = self.stacks[at].len();
                        match [a, b].into_iter().find(|&o| o < self.cols && !self.wall[o] && self.stacks[o].len() + 1 < h) {
                            Some(o) => at = o,
                            None => break,
                        }
                    }
                    if self.stacks[at].len() < room {
                        self.stacks[at].push(shade.wrapping_add(m.wrapping_mul(37)));
                    }
                }
                continue;
            }
            if g.y > floor as f32 + 2. {
                self.air.swap_remove(i);
                continue;
            }
            i += 1;
        }
        // Avalanches: a column two grains above its neighbour sheds its top
        // grain, a few passes a second so the slide can be watched.
        self.slide += dt * 70.;
        let mut passes = 0;
        while self.slide >= 1. && passes < 4 {
            self.slide -= 1.;
            passes += 1;
            self.sweep = !self.sweep;
            let n = self.cols;
            for j in 0..n {
                let x = if self.sweep { j } else { n - 1 - j };
                if self.wall[x] {
                    continue;
                }
                let h = self.stacks[x].len();
                let rough = 1 + (hash((x as u32).wrapping_mul(2654435761) ^ (kit.time * 30.) as u32) < 0.25) as usize;
                let (first, second) = if self.sweep { (x + 1, x.wrapping_sub(1)) } else { (x.wrapping_sub(1), x + 1) };
                for o in [first, second] {
                    if o < n && !self.wall[o] && h > self.stacks[o].len() + rough {
                        // Half the excess slides at once, so a steep heap
                        // slumps as fast on a fine grid as on a coarse one.
                        let slide = ((h - self.stacks[o].len() - rough + 1) / 2).max(1);
                        for _ in 0..slide {
                            let g = self.stacks[x].pop().unwrap();
                            self.stacks[o].push(g);
                        }
                        break;
                    }
                }
            }
        }
        if self.slide > 4. {
            self.slide = 0.;
        }
        self.rain(kit);
    }

    fn draw(&self, r: &mut Raster, kit: &Kit) {
        if self.cols == 0 || self.rows == 0 || self.bands.is_empty() {
            return;
        }
        let (rows, floor) = (self.rows, self.floor);
        let s = kit.sound;
        // The clock, in sand, with a dark shadow so it reads over flying grains.
        let glint = (kit.time * 0.9).sin().max(0.).powi(8);
        let face = scale(mix((255, 200, 115), (255, 245, 220), glint * 0.6 + s.treble * 0.25 + self.flash * 0.5), 0.9 + s.bass * 0.2);
        let shift = kit.aspect.round() as i32;
        for g in &self.text.grains {
            let away = g.away();
            let (x, y) = g.cell();
            if away < 0.5 {
                // At rest: solid sand with its drop shadow.
                r.dot(x, y, 1., '█', scale(face, 0.92 + g.shade * 0.1));
                r.dot(x + shift, y + 1, 1.5, '█', (60, 30, 20));
            } else {
                let ch = if away < 2.5 { '▪' } else if y % 2 == 0 { '▀' } else { '▄' };
                r.dot(x, y, 2.5, ch, scale(mix(face, SAND, 0.4), 0.7 + g.shade * 0.35));
            }
        }
        let top_row = self.top / 2;
        // Nozzles under the clock and the glass between the tubes.
        for b in &self.bands {
            let mid = ((b.x0 + b.x1) / 2).min(self.cols - 1);
            r.dot(mid as i32, top_row as i32 - 1, 6., '▼', scale(b.tint, 0.35 + b.level * 0.5));
        }
        let glass = (78, 70, 128);
        let mut panes: Vec<usize> = self.bands.iter().filter_map(|b| b.x0.checked_sub(1)).collect();
        panes.extend(self.bands.last().map(|b| b.x1).filter(|&x| x < self.cols));
        for x in panes {
            if !self.wall[x] {
                continue;
            }
            for y in top_row..rows {
                let shine = if (y + x) % 9 == 0 { 1.5 } else { 1. };
                r.dot(x as i32, y as i32, 8., '│', scale(glass, shine));
            }
            r.dot(x as i32, top_row as i32, 8., '╷', scale(glass, 1.6));
        }
        // The piles: half blocks, two grains to a row, brightest at the surface.
        let flash = self.flash;
        for x in 0..self.cols {
            let stack = &self.stacks[x];
            let n = stack.len();
            if n == 0 {
                continue;
            }
            let b = &self.bands[self.band_of[x] as usize];
            let glow = 0.75 + b.level * 0.55 + flash * 0.5;
            let surface = floor - n;
            for y in surface / 2..rows {
                let (ht, hb) = (y * 2, y * 2 + 1);
                let top_full = ht >= surface;
                // Grain under the upper half, or the lower one at the surface.
                let h = if top_full { ht } else { hb };
                let idx = floor - 1 - h;
                let shade = stack[idx.min(n - 1)];
                let depth = (h - surface) as f32;
                let light = (1.2 - (depth / 26.).min(1.) * 0.6) * (0.82 + shade as f32 / 255. * 0.3) * glow;
                let lit = if depth < 2.5 { mix(b.tint, (255, 245, 225), 0.25 + s.treble * 0.2) } else { b.tint };
                let ch = if top_full { '█' } else { '▄' };
                r.dot(x as i32, y as i32, 5., ch, scale(lit, light));
            }
        }
        // Peak markers, as on a hardware equalizer.
        for b in &self.bands {
            let h = floor as f32 - b.peak - 2.;
            if b.peak < 1. || h < self.top as f32 {
                continue;
            }
            let y = (h / 2.).floor() as i32;
            let ch = if (h as i32) % 2 == 0 { '▀' } else { '▄' };
            for x in b.x0..b.x1 {
                r.dot(x as i32, y, 4., ch, scale(mix(b.tint, (255, 255, 255), 0.4), 0.9 + b.level * 0.4));
            }
        }
        // Sand in the air.
        for g in &self.air {
            let x = g.x.round() as i32;
            let y = g.y.floor() as i32;
            if y < 0 {
                continue;
            }
            let b = &self.bands[self.band_of[self.home[(x.max(0) as usize).min(self.cols - 1)]] as usize];
            let tone = scale(mix(b.tint, SAND, 0.4), 0.85 + g.shade as f32 / 255. * 0.35);
            let ch = if g.spin > 0. { '•' } else if y % 2 == 0 { '▀' } else { '▄' };
            r.dot(x, y / 2, 3., ch, tone);
        }
        // The dryer's breath round the cursor.
        if let Some(c) = kit.cursor {
            let radius = (rows as f32 * 0.09).max(2.);
            for i in 0..7 {
                let a = kit.time * 2.4 + i as f32 * TAU / 7.;
                let d = radius * (0.55 + ((kit.time * 3. + i as f32 * 0.37).fract()) * 0.6);
                let (x, y) = ((c.0 + a.cos() * d * kit.aspect).round() as i32, (c.1 + a.sin() * d).round() as i32);
                // Over sand the breath stirs it rather than cutting a hole.
                if x >= 0 && y >= 0 && (x as usize) < self.cols && (y as usize) < rows && r.hidden(x as usize, y as usize, 1000.) {
                    r.lift((x as usize, x as usize + 1), (y as usize, y as usize + 1), |_, _| 0.35);
                } else {
                    r.dot(x, y, 2., '~', (120, 200, 230));
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::interactive_matrix::Sound;
    use std::f32::consts::TAU;
    fn kit() -> Kit {
        Kit { cols: 174, rows: 50, aspect: 2., dt: 0.05, reach: 1., ..Kit::default() }
    }
    fn run(s: &mut Spectrum, k: &mut Kit, frames: usize) {
        for _ in 0..frames {
            k.time += k.dt;
            s.step(k);
        }
    }
    fn held(s: &Spectrum, b: usize) -> usize {
        s.stacks[s.bands[b].x0..s.bands[b].x1].iter().map(|c| c.len()).sum()
    }
    #[test]
    fn bass_fills_the_low_tubes_and_the_dryer_lifts_sand() {
        let (mut s, mut k) = (Spectrum::new(), kit());
        s.resize(&k);
        run(&mut s, &mut k, 80);
        let quiet = held(&s, 0);
        k.sound = Sound { bass: 1., treble: 0. };
        run(&mut s, &mut k, 80);
        let last = s.bands.len() - 1;
        assert!(held(&s, 0) > quiet * 3 / 2, "{} vs {quiet}", held(&s, 0));
        assert!(held(&s, 0) > held(&s, last) * 2);
        let x = (s.bands[0].x0 + s.bands[0].x1) / 2;
        let surface = (s.floor - s.stacks[x].len()) as f32 / 2.;
        k.cursor = Some((x as f32, surface));
        k.last = k.cursor;
        run(&mut s, &mut k, 1);
        assert!(s.air.iter().any(|g| g.vy < 0.), "the dryer blew nothing");
    }
    #[test]
    fn sand_rests_only_in_the_tubes_and_never_overflows() {
        let (mut s, mut k) = (Spectrum::new(), kit());
        s.resize(&k);
        for frame in 0..300 {
            k.sound = Sound { bass: (frame % 40) as f32 / 40., treble: 0.6 };
            k.shake = frame == 100;
            k.drop = frame == 200;
            k.cursor = Some((40. + (frame as f32 * 0.3).sin() * 30., 35.));
            k.swirl = (frame > 250).then_some(((90., 30.), 1.5));
            run(&mut s, &mut k, 1);
            let room = s.room();
            for x in 0..s.cols {
                assert!(s.stacks[x].len() <= room);
                assert!(!s.wall[x] || s.stacks[x].is_empty());
            }
        }
    }
    /// Stills of the sand clocks while the cursor sweeps the digit row:
    /// SCENES="sand-clock-spectrum" SWEEP_Y=0.15 POSTER_DIR=out \
    ///   cargo test --release --bin ttfx-bg-rs -- sweep_stills --ignored --nocapture
    #[test]
    #[ignore = "manual: writes PPM files"]
    fn sweep_stills() {
        use crate::neon_scenes::{Scene, Style};
        use gtk4::cairo::{Context, Format, ImageSurface};
        let names = std::env::var("SCENES").unwrap_or_else(|_| "sand-clock-spectrum sand-clock-dust-devil".into());
        let sweep_y: f32 = std::env::var("SWEEP_Y").ok().and_then(|v| v.parse().ok()).unwrap_or(0.15);
        let dir = std::path::PathBuf::from(std::env::var("POSTER_DIR").unwrap_or_else(|_| "stills".into()));
        std::fs::create_dir_all(&dir).unwrap();
        let (cols, rows, cw, ch) = (240usize, 68usize, 8., 17.);
        for name in names.split_whitespace() {
            let mut scene = Scene::new(Style::parse(name).unwrap());
            let mut cells = vec![];
            scene.draw(cols, rows, ch as f32 / cw as f32, &mut cells);
            let dt = 1. / 30.;
            // Rest 2 s, sweep left to right in 1.2 s, then watch the sand.
            for frame in 0..(30 * 9) {
                let t = frame as f32 * dt;
                let beat = (t * TAU * 2.).sin().max(0.).powi(6);
                let sound = Sound { bass: 0.25 + beat * 0.6, treble: 0.4 };
                let pointer = if t < 2. { None } else { Some((((t - 2.) / 1.2).min(1.) * 0.9 + 0.05, sweep_y)) };
                scene.step(dt, pointer, sound);
                scene.draw(cols, rows, ch as f32 / cw as f32, &mut cells);
                let snap = [2.6, 3.4, 4.5, 8.5].iter().find(|&&s| (t - s).abs() < dt / 2.);
                let Some(snap) = snap else { continue };
                let surface = ImageSurface::create(Format::Rgb24, (cols as f64 * cw) as i32, (rows as f64 * ch) as i32).unwrap();
                let cr = Context::new(&surface).unwrap();
                cr.set_source_rgb(0., 0., 0.);
                cr.paint().unwrap();
                let layout = pangocairo::functions::create_layout(&cr);
                layout.set_font_description(Some(&pangocairo::pango::FontDescription::from_string("monospace 10")));
                for c in &cells {
                    let (r, g, b) = c.color;
                    cr.set_source_rgb(r as f64 / 255., g as f64 / 255., b as f64 / 255.);
                    cr.move_to(c.x as f64 * cw, c.y as f64 * ch);
                    layout.set_text(&c.ch.to_string());
                    pangocairo::functions::show_layout(&cr, &layout);
                }
                drop(cr);
                let (w, h) = (surface.width() as usize, surface.height() as usize);
                let stride = surface.stride() as usize;
                let mut surface = surface;
                let data = surface.data().unwrap();
                let mut ppm = format!("P6 {w} {h} 255\n").into_bytes();
                for y in 0..h {
                    for x in 0..w {
                        let px = &data[y * stride + x * 4..][..4];
                        ppm.extend_from_slice(&[px[2], px[1], px[0]]);
                    }
                }
                let path = dir.join(format!("{name}-sweep-{snap:03.1}s.ppm"));
                std::fs::write(&path, ppm).unwrap();
                println!("STILL {}", path.display());
            }
        }
    }
}
