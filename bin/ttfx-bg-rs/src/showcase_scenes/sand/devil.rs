//! Dust devil: a whirlwind of sand in a neon desert at night, under a
//! striped synthwave sun and a billboard that keeps the time. The devil is
//! a column of motes orbiting a leaning axis: a dusty skirt where it touches
//! the ground, a funnel that widens and fans out at the top.
//!
//! It walks to the cursor with a lag, resting the cursor feeds it and grows
//! it (rest far away and it collapses and is born again there), circling
//! spins it faster. Bass widens it, treble lights it from within, a drop
//! sends a wall of dust across the desert and a shake bursts it into a cloud
//! that settles before it forms again. Without a cursor it wanders.
use super::super::{mix, noise, scale, Rng};
use super::{hash, Kit, Play, SandText};
use crate::interactive_tunnel::Raster;
use std::f32::consts::TAU;

const COLUMN: u8 = 0;
const SKIRT: u8 = 1;
const LOOSE: u8 = 2;

struct Mote {
    kind: u8,
    /// Column: height up the funnel (0 at the ground) and angle round it.
    t: f32,
    a: f32,
    /// How far out from the axis, as a share of the funnel's radius.
    out: f32,
    rise: f32,
    shade: f32,
    /// Loose dust and the skirt: screen position, velocity, seconds left.
    x: f32,
    y: f32,
    vx: f32,
    vy: f32,
    life: f32,
    age: f32,
}

struct Bolt {
    points: Vec<(f32, f32)>,
    life: f32,
}

pub(super) struct Devil {
    cols: usize,
    rows: usize,
    motes: Vec<Mote>,
    bolts: Vec<Bolt>,
    /// Where the devil touches the ground, and the top of its axis.
    base: (f32, f32),
    top: f32,
    /// 0 is gone, 1 full grown; it grows towards `want`.
    grow: f32,
    spin: f32,
    whirl: f32,
    /// Seconds since the last drop sent the storm wall across.
    storm: f32,
    /// Ground shock rings from a charged rest: centre and age.
    rings: Vec<(f32, f32, f32)>,
    rng: Rng,
    /// The sign's digits, grain by grain.
    text: SandText,
    /// Digit grains the devil has caught: seconds left, angle round the
    /// axis, share of the radius and height on screen.
    caught: Vec<(f32, f32, f32, f32)>,
    /// The billboard frame round the digits: x0, y0, x1, y1.
    frame: (i32, i32, i32, i32),
}

impl Devil {
    pub(super) fn new() -> Self {
        Self {
            cols: 0,
            rows: 0,
            motes: vec![],
            bolts: vec![],
            base: (0., 0.),
            top: 0.,
            grow: 0.,
            spin: 1.,
            whirl: 0.,
            storm: f32::INFINITY,
            rings: vec![],
            rng: Rng(rand::random()),
            text: SandText::new(true, true),
            caught: vec![],
            frame: (0, 0, 0, 0),
        }
    }
    fn horizon(&self) -> f32 {
        self.rows as f32 * 0.56
    }
    /// Nearer ground is lower on screen and makes everything on it bigger.
    fn size(&self) -> f32 {
        let h = self.horizon();
        let ground = (self.rows as f32 - h).max(1.);
        0.35 + 0.75 * ((self.base.1 - h) / ground).clamp(0.15, 1.)
    }
    fn height(&self, kit: &Kit) -> f32 {
        self.rows as f32 * 0.88 * self.size() * self.grow * (1. + kit.sound.bass * 0.08)
    }
    /// Funnel radius in rows at height `t`.
    fn radius(&self, kit: &Kit, t: f32) -> f32 {
        self.rows as f32 * self.size() * self.grow.max(0.2) * (0.02 + 0.14 * t.powf(1.6)) * (1. + kit.sound.bass * 0.75)
    }
    /// The axis leans from the base towards the lagging top and sways.
    fn axis(&self, kit: &Kit, t: f32) -> f32 {
        let lean = (self.top - self.base.0) * t.powf(1.3);
        let sway = (kit.time * 1.7 + t * 4.).sin() * self.radius(kit, t) * 0.35 * kit.aspect;
        self.base.0 + lean + sway
    }
    fn column_at(&self, kit: &Kit, m: &Mote, height: f32) -> (f32, f32) {
        let rad = self.radius(kit, m.t) * m.out;
        (
            self.axis(kit, m.t) + m.a.cos() * rad * kit.aspect,
            self.base.1 - m.t * height + m.a.sin() * rad * 0.22,
        )
    }
    fn reborn(&mut self, i: usize) {
        let r = &mut self.rng;
        let m = &mut self.motes[i];
        m.kind = if r.next() < 0.3 { SKIRT } else { COLUMN };
        m.t = if m.kind == SKIRT { r.next() * 0.06 } else { r.next().powi(2) * 0.15 };
        m.a = r.next() * TAU;
        m.out = 0.45 + r.next() * 0.7;
        m.age = 0.;
        m.life = 0.5 + r.next() * 0.9;
    }
    /// Every mote flies off as loose dust, and the devil starts over small.
    fn burst(&mut self, kit: &Kit, power: f32) {
        let height = self.height(kit);
        for i in 0..self.motes.len() {
            if self.motes[i].kind == LOOSE {
                continue;
            }
            let (x, y) = self.column_at(kit, &self.motes[i], height);
            let speed = (8. + self.rng.next() * 22.) * power * kit.reach;
            let lift = self.rng.next();
            let m = &mut self.motes[i];
            m.kind = LOOSE;
            m.x = x;
            m.y = y;
            m.vx = m.a.cos() * speed * kit.aspect;
            m.vy = m.a.sin() * speed * 0.35 - lift * speed * 0.5;
            m.life = 1.2 + lift * 2.2;
        }
        self.grow = 0.;
    }
}

impl Play for Devil {
    fn resize(&mut self, kit: &Kit) {
        self.cols = kit.cols;
        self.rows = kit.rows;
        let h = self.horizon();
        self.base = (kit.cols as f32 * 0.62, h + (kit.rows as f32 - h) * 0.7);
        self.top = self.base.0;
        self.grow = 0.8;
        let n = (kit.cols * kit.rows / 8).clamp(150, 8000);
        self.motes = (0..n)
            .map(|_| Mote { kind: COLUMN, t: 0., a: 0., out: 1., rise: 0., shade: 0., x: 0., y: 0., vx: 0., vy: 0., life: 0., age: 0. })
            .collect();
        for i in 0..n {
            self.reborn(i);
            let m = &mut self.motes[i];
            m.t = self.rng.next();
            m.rise = 0.1 + self.rng.next() * 0.25;
            m.shade = 0.5 + self.rng.next() * 0.5;
            // A share starts as dust drifting in the air.
            if self.rng.next() < 0.18 {
                m.kind = LOOSE;
                m.x = self.rng.next() * kit.cols as f32;
                m.y = h + self.rng.next() * (kit.rows as f32 - h);
                m.life = self.rng.next() * 4.;
            }
        }
        self.bolts.clear();
        self.rings.clear();
        // The digits fill a box in the sky and the frame hugs them.
        let (cols, rows) = (kit.cols as f32, kit.rows as f32);
        let top = (rows * 0.06).round() + 1.;
        let tall = (rows * 0.3).min(self.horizon() - top - 3.).max(1.);
        self.text.place(kit, ((cols * 0.15) as i32, top as i32, ((cols * 0.7) as usize).max(1), tall as usize));
        let pad = (2. * kit.aspect).round() as i32;
        let (x0, y0, x1, y1) = self.text.extent();
        self.frame = if self.text.grains.is_empty() {
            (0, 0, 0, 0)
        } else {
            ((x0.floor() as i32 - pad).max(0), (y0.floor() as i32 - 1).max(0), (x1.ceil() as i32 + pad).min(kit.cols as i32 - 1), y1.ceil() as i32 + 1)
        };
        self.caught.clear();
    }

    fn step(&mut self, kit: &Kit) {
        if self.cols == 0 || self.rows == 0 {
            return;
        }
        let (dt, aspect, reach) = (kit.dt, kit.aspect, kit.reach);
        let (cols, rows) = (self.cols as f32, self.rows as f32);
        let h = self.horizon();
        let ground = (rows - h).max(1.);
        // Where it wants to stand: under the cursor, or wandering.
        let t = kit.time;
        let target = match kit.cursor {
            // The whole height of the screen maps onto the ground, so the
            // devil follows a cursor in the sky as well, far away.
            Some((x, y)) => (x, h + ground * (0.22 + 0.75 * ((y / rows - 0.25) / 0.75).clamp(0., 1.))),
            None => (
                cols * (0.5 + 0.38 * (t * 0.11 + 0.7 * (t * 0.05).sin()).sin()),
                h + ground * (0.55 + 0.3 * (t * 0.083 + 2.).sin()),
            ),
        };
        // Rest far from it and it collapses and is born again there.
        let far = kit.dist(self.base, target) > cols * 0.22 / aspect;
        if kit.cursor.is_some() && kit.charge > 0.3 && far && self.grow > 0.3 {
            self.burst(kit, 0.6);
            self.base = target;
            self.top = target.0;
        }
        let k = 1. - (-dt * 1.4).exp();
        let most = cols * 0.3 * dt;
        self.base.0 += ((target.0 - self.base.0) * k).clamp(-most, most);
        self.base.1 += ((target.1 - self.base.1) * k).clamp(-most, most);
        self.top += (self.base.0 - self.top) * (1. - (-dt * 0.8).exp());
        // Circling winds it up; the boost fades when the circling stops.
        if let Some((_, rate)) = kit.swirl {
            self.whirl = (self.whirl + dt * rate.abs().min(3.) * 1.5).min(2.5);
        }
        self.whirl = (self.whirl - dt * 0.5).max(0.);
        let want = 0.9 + kit.charge * 0.45 + self.whirl * 0.18;
        self.grow += (want - self.grow) * (1. - (-dt * if want > self.grow { 0.6 } else { 1.2 }).exp());
        self.spin = (1.3 + kit.sound.bass * 0.9) * (1. + self.whirl * 0.6 + kit.charge * 0.4);
        if kit.shake {
            self.burst(kit, 1.);
        }
        if kit.drop {
            self.storm = 0.;
        }
        self.storm += dt;
        self.text.step(kit);
        self.catch(kit);
        for ring in &mut self.rings {
            ring.2 += dt;
        }
        self.rings.retain(|r| r.2 < 1.4);
        // Motes: the column climbs and turns, the skirt spreads, dust drifts.
        let height = self.height(kit);
        let turn = dt * TAU * self.spin;
        let wind = (4. + kit.sound.bass * 10.) * reach * aspect;
        let drag = (-dt * 1.2).exp();
        let calm = self.grow > 0.15;
        for i in 0..self.motes.len() {
            let kind = self.motes[i].kind;
            match kind {
                COLUMN => {
                    let m = &mut self.motes[i];
                    m.t += m.rise * dt * (1. + self.whirl * 0.4);
                    m.a += turn * (1.35 - 0.6 * m.t);
                    if m.t > 1. {
                        // Over the top: fanned out as loose dust, or round again.
                        if self.rng.next() < 0.45 {
                            m.t = 1.;
                            let (x, y) = self.column_at(kit, &self.motes[i], height);
                            let rad = self.radius(kit, 1.);
                            let m = &mut self.motes[i];
                            m.kind = LOOSE;
                            m.x = x;
                            m.y = y;
                            m.vx = -m.a.sin() * rad * aspect * TAU * self.spin * 0.5 + m.a.cos() * 4. * aspect;
                            m.vy = -3. * reach;
                            m.life = 1. + self.rng.next() * 2.;
                        } else {
                            self.reborn(i);
                        }
                    }
                }
                SKIRT => {
                    let m = &mut self.motes[i];
                    m.age += dt;
                    m.a += turn * 0.8;
                    if m.age > m.life {
                        self.reborn(i);
                    }
                }
                _ => {
                    let m = &mut self.motes[i];
                    m.vx = m.vx * drag + wind * dt * 0.8;
                    m.vy = m.vy * drag + 2.5 * reach * dt;
                    m.x += m.vx * dt;
                    m.y += m.vy * dt;
                    m.life -= dt;
                    if m.x > cols + 2. {
                        m.x -= cols + 4.;
                    }
                    if m.life <= 0. || m.y > rows + 1. {
                        if calm && self.rng.next() < 0.85 {
                            self.reborn(i);
                        } else {
                            let m = &mut self.motes[i];
                            m.x = self.rng.next() * cols;
                            m.y = h + self.rng.next() * ground;
                            m.vx = 0.;
                            m.vy = 0.;
                            m.life = 0.5 + self.rng.next() * 3.;
                        }
                    }
                }
            }
        }
        // Treble arcs lightning through the dust.
        for b in &mut self.bolts {
            b.life -= dt;
        }
        self.bolts.retain(|b| b.life > 0.);
        let chance = kit.sound.treble * 3.5 * dt + kit.beat.map_or(0., |k| k * 0.25);
        if self.grow > 0.3 && self.rng.next() < chance && self.bolts.len() < 4 {
            let t0 = 0.45 + self.rng.next() * 0.5;
            let t1 = (t0 - 0.2 - self.rng.next() * 0.3).max(0.05);
            let steps = 6 + (self.rng.next() * 4.) as usize;
            let mut points = Vec::with_capacity(steps + 1);
            for s in 0..=steps {
                let tt = t0 + (t1 - t0) * s as f32 / steps as f32;
                let jag = if s == 0 || s == steps { 0. } else { (self.rng.next() - 0.5) * 1.3 };
                let x = self.axis(kit, tt) + jag * self.radius(kit, tt) * aspect;
                points.push((x, self.base.1 - tt * height));
            }
            self.bolts.push(Bolt { points, life: 0.1 + self.rng.next() * 0.12 });
        }
    }

    fn draw(&self, r: &mut Raster, kit: &Kit) {
        if self.cols == 0 || self.rows == 0 {
            return;
        }
        let (cols, rows, aspect, time) = (self.cols, self.rows, kit.aspect, kit.time);
        let s = kit.sound;
        let h = self.horizon();
        let ground = (rows as f32 - h).max(1.);
        // Stars.
        let stars = cols * rows / 70;
        for i in 0..stars as u32 {
            let x = (hash(i.wrapping_mul(7919)) * cols as f32) as i32;
            let y = (hash(i.wrapping_mul(104729) ^ 0x9e37) * h * 0.95) as i32;
            let tw = (time * (0.7 + hash(i) * 2.) + i as f32).sin() * 0.5 + 0.5;
            let ch = if tw > 0.93 { '+' } else { '·' };
            r.dot(x, y, 70., ch, scale((160, 150, 220), 0.25 + tw * 0.5));
        }
        // The striped sun sinking behind the dunes.
        let sun_r = rows as f32 * 0.31;
        let sun_x = cols as f32 * 0.5;
        let y0 = (h - sun_r).floor().max(0.) as i32;
        for y in y0..h.ceil() as i32 {
            let dy = h - (y as f32 + 0.5);
            if dy < 0. || dy > sun_r {
                continue;
            }
            let up = dy / sun_r;
            let period = (rows as f32 * 0.055).max(2.);
            if up < 0.6 && ((dy + time * 0.5) % period) < period * (0.6 - up) * 1.25 {
                continue;
            }
            let half = (sun_r * sun_r - dy * dy).sqrt() * aspect;
            let tone = scale(mix((255, 55, 150), (255, 215, 95), up), 0.8 + s.bass * 0.2);
            for x in (sun_x - half).round() as i32..=(sun_x + half).round() as i32 {
                r.dot(x, y, 60., '█', tone);
            }
        }
        // A haze on the horizon line.
        let hy = h.floor() as i32;
        for x in 0..cols as i32 {
            let glow = 0.55 + 0.45 * (1. - ((x as f32 - sun_x) / (cols as f32 * 0.5)).abs()).max(0.);
            r.dot(x, hy, 50., '─', scale((255, 80, 185), glow));
        }
        self.draw_dunes(r, kit, h, ground);
        self.draw_sign(r, kit, h);
        self.draw_storm(r, kit, h);
        // Dust over the sign passes behind its letters and in front of its
        // dark panel: it veils the time without cutting holes in it.
        let (sx0, sy0, sx1, sy1) = self.frame;
        let veil = |x: i32, y: i32, z: f32| if x >= sx0 && x <= sx1 && y >= sy0 && y <= sy1 { 19.5 } else { z };
        // The devil: a faint body behind the motes so it reads as a volume.
        let height = self.height(kit);
        if self.grow > 0.05 && height > 1. {
            let top = (self.base.1 - height).floor().max(0.) as i32;
            let frame = (time * 12.) as u32;
            for y in top..=self.base.1.round() as i32 {
                let t = ((self.base.1 - y as f32) / height).clamp(0., 1.);
                let w = self.radius(kit, t) * aspect * 0.75;
                let cx = self.axis(kit, t);
                for x in (cx - w).round() as i32..=(cx + w).round() as i32 {
                    let core = 1. - ((x as f32 - cx).abs() / w.max(0.5)).min(1.);
                    let seed = (x as u32).wrapping_mul(73856093) ^ (y as u32).wrapping_mul(19349663) ^ frame.wrapping_mul(83492791);
                    if hash(seed) < 0.2 + core * 0.45 {
                        let tone = mix((205, 135, 95), (160, 100, 200), t);
                        let ch = if core > 0.55 && t < 0.5 { '▒' } else { '░' };
                        // Behind the sign: dust may veil the time, never cut it.
                        r.dot(x, y, veil(x, y, 22.), ch, scale(tone, 0.6 + core * 0.5 + s.bass * 0.3));
                    }
                }
            }
        }
        // Lightning lights the dust round it.
        let flashes: Vec<(f32, f32, f32)> = self
            .bolts
            .iter()
            .map(|b| {
                let n = b.points.len().max(1) as f32;
                let (cx, cy) = b.points.iter().fold((0., 0.), |(a, c), p| (a + p.0, c + p.1));
                let span = b.points.iter().map(|p| ((p.0 - cx / n) / aspect).abs().max((p.1 - cy / n).abs())).fold(0., f32::max);
                (cx / n, cy / n, span + rows as f32 * 0.05)
            })
            .collect();
        for m in &self.motes {
            let (x, y, z, ch, mut tone) = match m.kind {
                COLUMN => {
                    let (x, y) = self.column_at(kit, m, height);
                    let front = m.a.sin();
                    let lit = 0.5 + 0.8 * (front * 0.5 + 0.5);
                    let tone = mix((255, 195, 135), (215, 130, 240), m.t * 0.8);
                    let ch = if front > 0.5 && m.out < 0.75 { '●' } else if front > -0.1 { '•' } else { '·' };
                    // The near side passes in front of the sign, the far side behind it.
                    let z = if front > -0.2 { 10. - front } else { 23. - front };
                    (x, y, z, ch, scale(tone, lit * m.shade * (1.1 + s.treble * 0.3)))
                }
                SKIRT => {
                    let spread = 1.8 + m.age / m.life.max(0.1) * 3.2;
                    let rad = self.radius(kit, 0.02) * m.out * spread * 2.6;
                    let fade = (1. - m.age / m.life.max(0.1)).max(0.);
                    let x = self.base.0 + m.a.cos() * rad * aspect;
                    let y = self.base.1 + m.a.sin() * rad * 0.3 - m.t * rows as f32 * 0.6;
                    let ch = if fade > 0.6 { '•' } else { '·' };
                    (x, y, 10.5 - m.a.sin(), ch, scale((215, 150, 105), (0.35 + fade * 0.7) * m.shade))
                }
                _ => {
                    let fade = m.life.min(1.).max(0.);
                    (m.x, m.y, 23., '·', scale((200, 140, 125), (0.3 + fade * 0.55) * m.shade))
                }
            };
            for &(cx, cy, span) in &flashes {
                let d = (((x - cx) / aspect).powi(2) + (y - cy).powi(2)).sqrt();
                if d < span {
                    tone = mix(tone, (215, 230, 255), (1. - d / span) * 0.85);
                }
            }
            let (x, y) = (x.round() as i32, y.round() as i32);
            // Drifting dust over a dune glints on it instead of cutting a hole.
            let inside = x >= 0 && y >= 0 && (x as usize) < cols && (y as usize) < rows;
            if m.kind != COLUMN && inside && y as f32 > h && r.hidden(x as usize, y as usize, 1000.) {
                let glint = if m.kind == SKIRT { 0.55 } else { 0.3 };
                r.lift((x as usize, x as usize + 1), (y as usize, y as usize + 1), |_, _| glint * m.shade);
                continue;
            }
            r.dot(x, y, veil(x, y, z), ch, tone);
        }
        // Lightning, behind the sign so a flash never cuts the time.
        for b in &self.bolts {
            let flick = if (time * 60.) as i32 % 3 == 0 { 0.7 } else { 1. };
            let tone = scale((205, 228, 255), flick);
            for w in b.points.windows(2) {
                let (dx, dy) = ((w[1].0 - w[0].0) / aspect, w[1].1 - w[0].1);
                let ch = if dx.abs() < dy.abs() * 0.4 {
                    '│'
                } else if dy.abs() < dx.abs() * 0.3 {
                    '─'
                } else if dx * dy > 0. {
                    '╲'
                } else {
                    '╱'
                };
                r.line_with_glyph(w[0], w[1], 19.6, tone, false, Some(ch));
            }
        }
        // Shock rings along the ground.
        for &(x, y, age) in &self.rings {
            let radius = age * rows as f32 * 0.5 * self.size();
            let light = (1. - age / 1.4).max(0.);
            let n = (radius * aspect * TAU).clamp(8., 600.) as usize;
            for i in 0..n {
                let a = i as f32 * TAU / n as f32;
                let px = x + a.cos() * radius * aspect;
                let py = y + a.sin() * radius * 0.28;
                r.dot(px.round() as i32, py.round() as i32, 9., '∘', scale((255, 190, 130), light));
            }
        }
    }
}

impl Devil {
    /// Dune crests in perspective, near to far: each column keeps how high
    /// the nearer dunes already reach, so farther crests show only above them.
    fn draw_dunes(&self, r: &mut Raster, kit: &Kit, h: f32, ground: f32) {
        const DEPTHS: [f32; 7] = [1., 1.45, 2.1, 3., 4.4, 6.4, 9.3];
        let (cols, rows) = (self.cols, self.rows);
        let drift = kit.time * 0.015;
        let light = 0.85 + kit.sound.treble * 0.25;
        let sun_x = cols as f32 * 0.5;
        // A ridge and how steeply it rises to the right at `w`.
        let dune = |w: f32| {
            let v = (w * 2.1).sin() * 0.6 + (w * 4.7 + 1.3).sin() * 0.28 + (w * 9.3).sin() * 0.07;
            let dv = (w * 2.1).cos() * 1.26 + (w * 4.7 + 1.3).cos() * 1.32 + (w * 9.3).cos() * 0.65;
            (v, dv)
        };
        for x in 0..cols {
            let u = (x as f32 - sun_x) / cols as f32;
            // The sun lays a warm path across the crests in front of it.
            let path = (-(u * 3.2).powi(2)).exp();
            let mut covered = rows as f32;
            for (k, &d) in DEPTHS.iter().enumerate() {
                let w = u * d * 4. + drift / d + k as f32 * 1.7;
                let (v, dv) = dune(w);
                let amp = ground * 0.18 / d;
                let y = h + ground * 0.94 / d - amp * 0.6 + v * amp;
                if y >= covered || y < h {
                    covered = covered.min(y.max(h));
                    continue;
                }
                let far = k as f32 / (DEPTHS.len() - 1) as f32;
                // Faces turned to the sun catch its light; the others fall dark.
                let face: f32 = if dv < 0. { 1.15 } else { 0.6 };
                let warm = path * (1. - far * 0.4) * 0.75 * face.min(1.);
                let crest = mix(mix((255, 95, 200), (120, 75, 205), far), (255, 180, 120), warm);
                let body = mix(mix((92, 30, 92), (48, 28, 88), far), (110, 50, 70), warm * 0.6);
                // The crest edge in half blocks, then a face fading into the trough.
                let yi = y.floor() as i32;
                let frac = y - y.floor();
                let (first, edge) = if frac < 0.25 { (yi, '█') } else if frac < 0.75 { (yi, '▄') } else { (yi + 1, '█') };
                let lit = ((amp * 1.6).ceil() as i32 + 2).max(2);
                let end = (covered.ceil() as i32).min(first + lit);
                let z = 30. + k as f32;
                for yy in first..end {
                    let depth = (yy - first) as f32 / lit as f32;
                    let (ch, tone) = if yy == first {
                        (edge, scale(crest, light * face.max(0.8)))
                    } else {
                        let ch = if depth < 0.45 { '▓' } else if depth < 0.75 { '▒' } else { '░' };
                        (ch, scale(mix(crest, body, (depth * 1.6).min(1.)), light * face * (1. - depth * 0.55)))
                    };
                    r.dot(x as i32, yy, z, ch, tone);
                }
                covered = y;
            }
        }
    }
    /// Digit grains the funnel sweeps over are sucked off the sign and spun
    /// round the axis, climbing a little, until they drop out and pour home.
    fn catch(&mut self, kit: &Kit) {
        let (dt, aspect) = (kit.dt, kit.aspect);
        let n = self.text.grains.len();
        self.caught.resize(n, (0., 0., 0., 0.));
        let height = self.height(kit);
        let live = self.grow > 0.3 && height > 1.;
        let frame = (kit.time * 60.) as u32;
        let mut moves = vec![];
        for i in 0..n {
            let g = &self.text.grains[i];
            let (mut left, mut a, mut out, mut y) = self.caught[i];
            if !live {
                self.caught[i].0 = 0.;
                continue;
            }
            if left <= 0. {
                if g.y > self.base.1 || g.y < self.base.1 - height {
                    continue;
                }
                let t = (self.base.1 - g.y) / height;
                let reach = self.radius(kit, t) * aspect * 1.3;
                let off = g.x - self.axis(kit, t);
                if off.abs() > reach || hash((i as u32).wrapping_mul(2654435761) ^ frame) > dt * 8. {
                    continue;
                }
                left = 1.5 + hash(i as u32 ^ frame.wrapping_mul(97)) * 2.;
                a = (off / reach.max(0.1)).clamp(-1., 1.).acos();
                out = 0.55 + hash(i as u32 ^ 0x5bd1) * 0.55;
                y = g.y;
            }
            left -= dt;
            y -= dt * height * 0.06;
            let t = ((self.base.1 - y) / height).clamp(0., 1.);
            a += dt * TAU * self.spin * (1.35 - 0.6 * t);
            let rad = self.radius(kit, t) * out;
            moves.push((i, self.axis(kit, t) + a.cos() * rad * aspect, y + a.sin() * rad * 0.22));
            self.caught[i] = (left, a, out, y);
        }
        let (w, h) = (self.cols as f32 - 1., self.rows as f32 - 1.);
        for (i, x, y) in moves {
            let g = &mut self.text.grains[i];
            g.x = x.clamp(0., w);
            g.y = y.clamp(0., h);
        }
    }
    /// The billboard on the horizon: a dark panel framed in neon with the
    /// time in the pixel font. A digit now and then flickers like a tube.
    fn draw_sign(&self, r: &mut Raster, kit: &Kit, h: f32) {
        let (x0, y0, x1, y1) = self.frame;
        let w = x1 - x0 + 1;
        let time = kit.time;
        let buzz = 0.85 + 0.15 * (time * 31.).sin() * (time * 7.).sin();
        let frame = scale((255, 60, 200), buzz * (0.85 + kit.sound.bass * 0.3));
        // Posts down to the horizon.
        for px in [x0 + w / 5, x1 - w / 5] {
            for y in y1 + 1..=h as i32 {
                r.dot(px, y, 21., '║', (95, 70, 140));
            }
        }
        for y in y0..=y1 {
            for x in x0..=x1 {
                let edge_x = x == x0 || x == x1;
                let edge_y = y == y0 || y == y1;
                let ch = match (edge_x, edge_y) {
                    (true, true) => match (x == x0, y == y0) {
                        (true, true) => '╔',
                        (false, true) => '╗',
                        (true, false) => '╚',
                        (false, false) => '╝',
                    },
                    (true, false) => '║',
                    (false, true) => '═',
                    _ => ' ',
                };
                let tone = if ch == ' ' { (0, 0, 0) } else { frame };
                r.dot(x, y, 20., ch, tone);
            }
        }
        // Digits one by one, so a single tube can flicker.
        let flicker = (time * 0.7) as u32;
        let bad = (hash(flicker.wrapping_mul(2246822519)) * 8.) as usize;
        let dim = (time * 0.7).fract() < 0.12 && hash(flicker ^ 0x51ed) < 0.6;
        let neon = mix((80, 230, 255), (230, 250, 255), kit.sound.treble * 0.4);
        let off = dim && (time * 40.).sin() > -0.3;
        // Each lit cell of a digit is a grain: solid tube light at home,
        // loose glowing sand once the cursor or the devil has it.
        for g in &self.text.grains {
            let away = g.away();
            let mut tone = neon;
            if off && g.slot == bad {
                tone = scale(neon, 0.25);
            }
            let ch = if away < 0.5 {
                '█'
            } else {
                tone = scale(mix(tone, (255, 255, 255), 0.3), 0.85 + g.shade * 0.4);
                if away < 2. { '▪' } else { '•' }
            };
            let (x, y) = g.cell();
            let inside = x >= x0 && x <= x1 && y >= y0 && y <= y1;
            r.dot(x, y, if inside { 19. } else { 9. }, ch, tone);
        }
    }
    /// After a drop a wall of dust rolls across the desert behind the sign:
    /// a soft billowing front, churning inside, leaving a haze that clears.
    fn draw_storm(&self, r: &mut Raster, kit: &Kit, h: f32) {
        const CROSS: f32 = 7.;
        if self.storm > CROSS {
            return;
        }
        let (cols, rows, t) = (self.cols as f32, self.rows as f32, kit.time);
        let p = self.storm / CROSS;
        let front = -cols * 0.1 + p * cols * 1.25;
        let fade = 1. - ((p - 0.7) / 0.3).clamp(0., 1.);
        let lead = (cols * 0.07).max(2.);
        let soft = (rows * 0.07).max(1.);
        for x in 0..(front.min(cols).max(0.) as i32) {
            let behind = front - x as f32;
            let head = (behind / lead).min(1.);
            let head = head * head * (3. - 2. * head);
            let tail = (1. - behind / (cols * 0.55)).max(0.);
            let dense = (head * (0.3 + 0.7 * tail)).max(0.16 * head) * fade;
            if dense < 0.03 {
                continue;
            }
            // The haze left behind is thinner: grains on even columns only.
            let churning = tail > 0.15;
            if !churning && x % 2 == 1 {
                continue;
            }
            let tall = 0.16 + 0.34 * tail * head;
            let top = h - rows * tall * (0.7 + 0.6 * noise(x as f32 * 0.035 - t * 0.25, t * 0.15));
            for y in top.max(0.) as i32..rows as i32 {
                let edge = ((y as f32 - top) / soft).clamp(0., 1.);
                let turb = if churning { noise(x as f32 * 0.07 - t * 0.9, y as f32 * 0.18 + t * 0.35) } else { 0.5 };
                let d = dense * edge * (0.55 + 0.9 * turb);
                let n = hash((x as u32).wrapping_mul(2654435761) ^ (y as u32).wrapping_mul(40503));
                if n > d {
                    continue;
                }
                let ch = if d > 0.8 { '▓' } else if d > 0.5 { '▒' } else { '░' };
                let tone = mix((85, 42, 62), (235, 145, 115), (d * 0.9 + turb * 0.2).min(1.));
                r.dot(x, y, 25., ch, scale(tone, 0.75 + n * 0.25));
            }
        }
    }
}


#[cfg(test)]
mod tests {
    use super::*;
    fn kit() -> Kit {
        Kit { cols: 174, rows: 50, aspect: 2., dt: 0.05, reach: 1., ..Kit::default() }
    }
    fn run(d: &mut Devil, k: &mut Kit, frames: usize) {
        for _ in 0..frames {
            k.time += k.dt;
            d.step(k);
        }
    }
    #[test]
    fn a_shake_bursts_the_devil_it_forms_again_and_a_drop_raises_the_storm() {
        let (mut d, mut k) = (Devil::new(), kit());
        d.resize(&k);
        run(&mut d, &mut k, 40);
        assert!(d.grow > 0.5);
        k.shake = true;
        run(&mut d, &mut k, 1);
        k.shake = false;
        assert!(d.grow < 0.05);
        assert!(d.motes.iter().all(|m| m.kind == LOOSE));
        run(&mut d, &mut k, 100);
        assert!(d.grow > 0.6);
        assert!(d.motes.iter().filter(|m| m.kind == COLUMN).count() > d.motes.len() / 3);
        assert!(d.storm.is_infinite());
        k.drop = true;
        run(&mut d, &mut k, 1);
        assert!(d.storm < 0.1);
    }
    #[test]
    fn resting_far_away_gives_birth_there_and_it_follows_the_cursor() {
        let (mut d, mut k) = (Devil::new(), kit());
        d.resize(&k);
        run(&mut d, &mut k, 10);
        k.cursor = Some((20., 45.));
        k.charge = 0.5;
        run(&mut d, &mut k, 1);
        assert!((d.base.0 - 20.).abs() < 1., "{:?}", d.base);
        k.charge = 0.;
        k.cursor = Some((150., 45.));
        run(&mut d, &mut k, 120);
        assert!((d.base.0 - 150.).abs() < 3., "{:?}", d.base);
    }
}
