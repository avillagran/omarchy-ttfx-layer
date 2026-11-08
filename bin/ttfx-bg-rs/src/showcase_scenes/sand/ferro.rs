//! Ferrofluid clock: the time in black magnetic fluid with neon caught in its
//! chrome. The cursor is a magnet: near it the digits stretch towards it,
//! resting grows a hedgehog of spikes that stands taller with the bass, and
//! moving off flings the fluid home. The bass raises spikes along every edge,
//! treble sparks on their tips and a drop makes the whole clock bristle.
use super::super::{mix, scale, Rng, SandFont};
use super::{face, hash, Kit, Play};
use crate::interactive_tunnel::Raster;
use std::collections::HashMap;
use std::f32::consts::{PI, TAU};

type Color = (u8, u8, u8);
const STEEL: Color = (22, 22, 36);
const CYAN: Color = (60, 225, 255);
const MAGENTA: Color = (255, 60, 200);

struct Bit {
    x: f32,
    y: f32,
    vx: f32,
    vy: f32,
    slot: usize,
    index: usize,
    home: (f32, f32),
    /// Outward unit normal of the glyph edge at home (in rows), and how much
    /// of an edge it is: 0 deep inside a stroke.
    normal: (f32, f32),
    edge: f32,
    jitter: (f32, f32),
    /// Fixed per grain: how readily it stands up in a spike.
    tip: f32,
    phase: f32,
    /// How much the magnet holds it in the hedgehog, 0 to 1.
    held: f32,
    loose_until: f32,
}

pub(super) struct Ferro {
    bits: Vec<Bit>,
    font: SandFont,
    /// Edge normal and edge weight of every glyph point, per character.
    normals: HashMap<char, Vec<((f32, f32), f32)>>,
    shown: Vec<char>,
    rng: Rng,
    time: f32,
    /// The magnet trails the cursor a little, like a hand under the table.
    magnet: Option<(f32, f32)>,
    /// How grown the hedgehog is, and how far it has turned.
    urchin: f32,
    spin: f32,
    /// After a drop every edge bristles for a while.
    bristle: f32,
    flash: f32,
    /// The grains melted into one liquid: a density per cell, and the box
    /// of cells it covers this frame.
    density: Vec<f32>,
    span: (usize, usize, usize, usize),
}
/// A cell is fluid from this density, a faint film from about half of it.
const WET: f32 = 0.8;
impl Ferro {
    pub(super) fn new() -> Self {
        Self {
            bits: vec![],
            font: SandFont::default(),
            normals: HashMap::new(),
            shown: vec![],
            rng: Rng(rand::random()),
            time: 0.,
            magnet: None,
            urchin: 0.,
            spin: 0.,
            bristle: 0.,
            flash: 0.,
            density: vec![],
            span: (0, 0, 0, 0),
        }
    }
    /// Every grain spreads a soft blob; where blobs overlap they merge, so
    /// the sand reads as one liquid with spikes drawn out of it.
    fn melt(&mut self, kit: &Kit) {
        let (cols, rows) = (kit.cols, kit.rows);
        self.density.clear();
        self.density.resize(cols * rows, 0.);
        let (rx, ry) = ((kit.aspect * 1.5).max(2.), 1.6);
        let (mut x0, mut y0, mut x1, mut y1) = (cols, rows, 0, 0);
        for b in &self.bits {
            let xa = (b.x - rx).ceil().max(0.) as usize;
            let xb = ((b.x + rx).floor().max(0.) as usize).min(cols - 1);
            let ya = (b.y - ry).ceil().max(0.) as usize;
            let yb = ((b.y + ry).floor().max(0.) as usize).min(rows - 1);
            for y in ya..=yb {
                let dy = (y as f32 - b.y) / ry;
                let row = y * cols;
                for x in xa..=xb {
                    let dx = (x as f32 - b.x) / rx;
                    let q = 1. - dx * dx - dy * dy;
                    if q > 0. {
                        self.density[row + x] += q * q;
                    }
                }
            }
            x0 = x0.min(xa);
            y0 = y0.min(ya);
            x1 = x1.max(xb + 1);
            y1 = y1.max(yb + 1);
        }
        self.span = (x0, y0, x1.max(x0), y1.max(y0));
    }
    /// Which way each glyph point faces: away from the mass of its
    /// neighbours. Points inside a stroke have neighbours all round and no edge.
    fn bake_normals(&mut self, aspect: f32) {
        self.normals.clear();
        let reach = 3.;
        for (&ch, pts) in &self.font.glyphs {
            let mut out = Vec::with_capacity(pts.len());
            for &(x, y) in pts {
                let (mut ax, mut ay, mut n) = (0., 0., 0.);
                for &(qx, qy) in pts {
                    let dx = (qx - x) as f32 / aspect;
                    let dy = (qy - y) as f32;
                    if dx.abs() <= reach && dy.abs() <= reach && (dx != 0. || dy != 0.) {
                        ax += dx;
                        ay += dy;
                        n += 1.;
                    }
                }
                let len = (ax * ax + ay * ay).sqrt();
                let normal = if len > 1e-3 { (-ax / len, -ay / len) } else { (0., -1.) };
                out.push((normal, (len / (n + 1.) / 1.2).min(1.)));
            }
            self.normals.insert(ch, out);
        }
    }
    fn retarget(&mut self, slot: usize, ch: char) {
        let Some(normals) = self.normals.get(&ch) else { return };
        let cells = self.font.cells(ch, slot);
        if cells.is_empty() {
            return;
        }
        let have = self.bits.iter().filter(|b| b.slot == slot).count();
        for index in have..cells.len() {
            let jitter = ((self.rng.next() - 0.5) * 0.5, (self.rng.next() - 0.5) * 0.5);
            let (x, y) = cells[index];
            self.bits.push(Bit {
                x: x + jitter.0,
                y: y + jitter.1,
                vx: 0.,
                vy: 0.,
                slot,
                index,
                home: (x, y),
                normal: (0., -1.),
                edge: 0.,
                jitter,
                tip: self.rng.next(),
                phase: self.rng.next() * TAU,
                held: 0.,
                loose_until: 0.,
            });
        }
        for b in self.bits.iter_mut().filter(|b| b.slot == slot) {
            let i = b.index % cells.len();
            b.home = (cells[i].0 + b.jitter.0, cells[i].1 + b.jitter.1);
            (b.normal, b.edge) = normals[i];
        }
    }
    /// Spikes of the hedgehog: how many, and how long, in rows.
    fn urchin_shape(&self, kit: &Kit) -> (usize, f32) {
        let spikes = 9 + (kit.reach * 3.) as usize;
        let length = (2. + self.urchin * 7.) * (0.7 + kit.sound.bass * 0.9 + self.bristle * 0.4) * kit.reach;
        (spikes, length)
    }
    fn urchin_point(&self, kit: &Kit, m: (f32, f32), b: &Bit) -> ((f32, f32), f32) {
        let (spikes, length) = self.urchin_shape(kit);
        let k = b.index as u32 ^ (b.slot as u32).wrapping_mul(7919);
        let spike = (b.index + b.slot * 5) % spikes;
        let a = self.spin + spike as f32 * TAU / spikes as f32;
        let t = hash(k.wrapping_mul(31)).powf(0.8);
        let along = 1.5 * kit.reach + t * length;
        let side = (hash(k.wrapping_mul(17) ^ 0x51ed) - 0.5) * (1. - t) * 1.6 * kit.reach;
        let (c, s) = (a.cos(), a.sin());
        let x = m.0 + (c * along - s * side) * kit.aspect;
        let y = m.1 + s * along + c * side;
        ((x, y), t)
    }
    /// Spikes rise on a fixed lattice of sites, as on real ferrofluid, so
    /// the edge grows cones rather than fuzz.
    fn site(kit: &Kit, b: &Bit) -> f32 {
        let s = 2.8 * kit.reach.max(0.8);
        let v = (b.home.0 / kit.aspect / s * TAU).cos() * (b.home.1 / s * TAU).cos();
        b.edge * v.max(0.).powi(4)
    }
    fn reach_of_magnet(&self, kit: &Kit) -> f32 {
        kit.rows as f32 * (0.22 + self.urchin * 0.14)
    }
}
impl Play for Ferro {
    fn resize(&mut self, kit: &Kit) {
        let (cols, rows) = (kit.cols, kit.rows);
        let top = (rows as f32 * 0.06) as i32;
        self.font = face(kit, (0, top, cols, (rows as f32 * 0.88) as usize), 8);
        self.bake_normals(kit.aspect);
        self.bits.clear();
        self.shown = kit.clock(true).chars().collect();
        for (slot, ch) in self.shown.clone().into_iter().enumerate() {
            self.retarget(slot, ch);
        }
        for b in &mut self.bits {
            (b.x, b.y) = b.home;
        }
    }
    fn step(&mut self, kit: &Kit) {
        let dt = kit.dt;
        self.time = kit.time;
        let text: Vec<char> = kit.clock(true).chars().collect();
        for slot in 0..text.len().min(self.shown.len()) {
            if text[slot] != self.shown[slot] {
                self.retarget(slot, text[slot]);
            }
        }
        self.shown = text;
        // The magnet follows the cursor with a little lag; it fades without one.
        let ease = 1. - (-dt * 14.).exp();
        self.magnet = kit.cursor.map(|c| match self.magnet {
            Some(m) => (m.0 + (c.0 - m.0) * ease, m.1 + (c.1 - m.1) * ease),
            None => c,
        });
        let goal = if kit.cursor.is_some() { kit.charge } else { 0. };
        let rate = if goal > self.urchin { 2. } else { 1.2 };
        self.urchin += (goal - self.urchin) * (1. - (-dt * rate).exp());
        self.spin += dt * (0.25 + kit.sound.treble * 0.6);
        if let Some((_, turns)) = kit.swirl {
            self.spin += turns.clamp(-3., 3.) * TAU * dt * 0.6;
        }
        self.bristle = (self.bristle - dt * 0.7).max(0.);
        self.flash = (self.flash - dt * 1.5).max(0.);
        if kit.drop {
            self.bristle = 1.6;
            self.flash = 1.;
        }
        let time = self.time;
        if kit.shake {
            for b in &mut self.bits {
                let a = self.rng.next() * TAU;
                let v = (20. + self.rng.next() * 60.) * kit.reach;
                b.vx += a.cos() * v * kit.aspect;
                b.vy += a.sin() * v;
                b.held = 0.;
                b.loose_until = time + 0.4 + self.rng.next() * 0.8;
            }
        }
        // Every edge stands up in spikes on the bass.
        let spike = (kit.sound.bass * 3. + self.bristle * 4. + 0.2) * kit.reach;
        let reach = self.reach_of_magnet(kit);
        let (stiff, damp) = (90., (-dt * 9.).exp());
        let friction = (-dt * 3.).exp();
        let magnet = self.magnet;
        let urchin = self.urchin;
        for i in 0..self.bits.len() {
            let b = &self.bits[i];
            let out = Self::site(kit, b) * spike;
            let (mut tx, mut ty) = (b.home.0 + b.normal.0 * out * kit.aspect, b.home.1 + b.normal.1 * out);
            let mut held_goal = 0.;
            if let Some(m) = magnet {
                let d = kit.dist(b.home, m);
                if d < reach {
                    // Stretch towards the magnet, most where it is closest.
                    let f = (1. - d / reach).powi(2);
                    tx += (m.0 - tx) * 0.3 * f;
                    ty += (m.1 - ty) * 0.3 * f;
                    // The hedgehog draws only the fluid nearest the magnet, so
                    // the rest of the time stays readable.
                    let grab = kit.rows as f32 * (0.07 + urchin * 0.07);
                    if urchin > 0.02 && d < grab {
                        held_goal = (urchin * (1. - d / grab) * 2.).min(1.);
                    }
                }
            }
            let held = b.held + (held_goal - b.held) * (1. - (-dt * if held_goal > b.held { 2.5 } else { 1.5 }).exp());
            if let (Some(m), true) = (magnet, held > 0.01) {
                let (u, _) = self.urchin_point(kit, m, b);
                tx += (u.0 - tx) * held;
                ty += (u.1 - ty) * held;
            }
            let b = &mut self.bits[i];
            b.held = held;
            if time < b.loose_until {
                b.vx *= friction;
                b.vy *= friction;
            } else {
                // A soft spring: the fluid overshoots and wobbles as it lands.
                b.vx = (b.vx + (tx - b.x) * stiff * dt) * damp;
                b.vy = (b.vy + (ty - b.y) * stiff * dt) * damp;
            }
            b.x = (b.x + b.vx * dt).clamp(0., kit.cols as f32 - 1.);
            b.y = (b.y + b.vy * dt).clamp(0., kit.rows as f32 - 1.);
        }
        self.melt(kit);
    }
    fn draw(&self, r: &mut Raster, kit: &Kit) {
        let (cols, rows) = (kit.cols, kit.rows);
        let (bass, treble) = (kit.sound.bass, kit.sound.treble);
        let glow = 1. + bass * 0.35 + self.flash * 0.8;
        let tick = (self.time * 12.) as u32;
        // The hedgehog's spines, under the liquid they grow from.
        if let (Some(m), true) = (self.magnet, self.urchin > 0.05) {
            let (spikes, length) = self.urchin_shape(kit);
            for i in 0..spikes {
                let a = self.spin + i as f32 * TAU / spikes as f32;
                let (c, s) = (a.cos() * kit.aspect, a.sin());
                let (from, to) = (1.8 * kit.reach, 1.5 * kit.reach + length * self.urchin);
                let tone = mix(CYAN, MAGENTA, s * 0.5 + 0.5);
                // Over the liquid: the spines are its sharpest, brightest part.
                let spine = match ((a.rem_euclid(PI) / (PI / 4.)).round() as i32) % 4 {
                    0 => '─',
                    1 => '\\',
                    2 => '│',
                    _ => '/',
                };
                r.line_with_glyph((m.0 + c * from, m.1 + s * from), (m.0 + c * to, m.1 + s * to), 1.5, scale(tone, (0.5 + bass * 0.4) * glow * self.urchin), false, Some(spine));
            }
        }
        // Black chrome: flat tops stay dark steel, slopes mirror a neon room
        // (magenta above, cyan below) and the side facing the light from the
        // top left flares white. A sheen sweeps across now and then.
        let (x0, y0, x1, y1) = self.span;
        let d = &self.density;
        let at = |x: usize, y: usize| d[y * cols + x];
        let sheen_at = self.time * 9.;
        for y in y0..y1.min(rows) {
            for x in x0..x1.min(cols) {
                let v = at(x, y);
                if v < WET * 0.5 {
                    continue;
                }
                let gx = (at((x + 1).min(cols - 1), y) - at(x.saturating_sub(1), y)) * 0.5 * kit.aspect;
                let gy = (at(x, (y + 1).min(rows - 1)) - at(x, y.saturating_sub(1))) * 0.5;
                let g = (gx * gx + gy * gy).sqrt();
                let (nx, ny) = if g > 1e-4 { (-gx / g, -gy / g) } else { (0., 0.) };
                // Near the surface of the liquid, not deep in a stroke.
                let rim = (1. - (v - WET) / (WET * 1.6)).clamp(0., 1.);
                let slope = (g / (v * 0.5 + 0.3)).min(1.) * rim;
                let room = mix(MAGENTA, CYAN, ny * 0.5 + 0.5);
                let spec = (-(nx * 0.6 + ny * 0.8)).max(0.).powi(4) * slope;
                let band = (x as f32 / kit.aspect + y as f32 * 1.6 - sheen_at).rem_euclid(rows as f32 * 3. + 40.);
                let sheen = (1. - (band - 6.).abs() / 6.).max(0.) * 0.35;
                let mut tone = mix(STEEL, room, (slope * 1.3).min(1.) * (0.8 + bass * 0.2));
                tone = mix(tone, (245, 245, 255), spec * 0.7 + sheen);
                let (ch, light) = if v >= WET * 1.6 {
                    ('█', 1.)
                } else if v >= WET {
                    ('▓', 0.95)
                } else {
                    ('░', 0.7)
                };
                r.dot(x as i32, y as i32, 2., ch, scale(tone, light * glow));
            }
        }
        // Treble sparks on the tips of the spikes.
        for b in &self.bits {
            let stood = Self::site(kit, b) * (bass + self.bristle) * 1.5;
            if (stood > 0.6 || (b.held > 0.5 && b.tip > 0.8))
                && hash((b.index as u32).wrapping_mul(2654435761) ^ b.slot as u32 ^ tick.wrapping_mul(97)) < treble * 0.3
            {
                r.dot(b.x.round() as i32, b.y.round() as i32, 0.8, '*', mix((200, 250, 255), (255, 200, 245), b.phase / TAU));
            }
        }
        if let Some(m) = self.magnet {
            r.dot(m.0.round() as i32, m.1.round() as i32, 0.4, '◉', scale((235, 225, 255), 0.6 + self.urchin * 0.4));
        }
    }
}
