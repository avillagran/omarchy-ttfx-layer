//! Neon zen garden: raked sand, server blocks for stones and the time
//! engraved in the sand. The sand is drawn by its groove crests: each cell
//! knows the phase of the pattern raked into it, and a crest crossing the cell
//! picks a line glyph by its direction, so rings and rake strokes read as
//! drawn lines rather than noise. The cursor is a rake; its strokes fade back
//! into the combed pattern over minutes, faster when the bass shakes the sand.
//! The engraved time is itself sand: raking across it throws its grains out,
//! and the engraving fills in again as they pour home.
use super::super::{mix, scale, Color};
use super::{hash, Kit, Play, SandText};
use crate::interactive_tunnel::Raster;
use std::f32::consts::TAU;

const NONE: char = '\0';
/// Rake tines either side of the middle one.
const TINES: f32 = 2.;

/// The glyph of a groove crest crossing a cell, if one does. `phase` counts
/// grooves; `(gx, gy)` is its gradient in grooves per row on screen.
fn crest(phase: f32, gx: f32, gy: f32, aspect: f32) -> char {
    let g = (gx * gx + gy * gy).sqrt();
    if g < 1e-5 {
        return NONE;
    }
    let (nx, ny) = (gx / g, gy / g);
    // Rows from the nearest crest, along the gradient.
    let delta = (phase - phase.round()) / g;
    // Half the cell's extent along the gradient.
    let half = 0.5 * (ny.abs() + nx.abs() / aspect);
    if delta < -half || delta >= half {
        return NONE;
    }
    // The crest runs across the gradient; glyphs are drawn in cells.
    let (tx, ty) = (-ny * aspect, nx);
    let (ax, ay) = (tx.abs(), ty.abs());
    if ay < ax * 0.41 {
        // Where the crest sits in the cell keeps near-flat rings smooth.
        let off = -delta * ny;
        if off < -0.22 {
            '¯'
        } else if off > 0.22 {
            '_'
        } else {
            '─'
        }
    } else if ax < ay * 0.41 {
        '│'
    } else if (tx > 0.) == (ty > 0.) {
        '╲'
    } else {
        '╱'
    }
}

/// Cells whose screen distance from `c` lies in `[r0, r1)` rows, with that
/// distance and the unit direction away from `c` (in rows).
fn annulus(cols: usize, rows: usize, aspect: f32, c: (f32, f32), r0: f32, r1: f32, mut f: impl FnMut(usize, usize, f32, (f32, f32))) {
    if r1 <= 0. || cols == 0 || rows == 0 || !c.0.is_finite() || !c.1.is_finite() {
        return;
    }
    let y0 = (c.1 - r1).floor().max(0.) as i32;
    let y1 = ((c.1 + r1).ceil() as i32).min(rows as i32 - 1);
    for y in y0..=y1 {
        let dy = y as f32 - c.1;
        if dy.abs() >= r1 {
            continue;
        }
        let xo = (r1 * r1 - dy * dy).max(0.).sqrt() * aspect;
        let xi = if dy.abs() < r0 { (r0 * r0 - dy * dy).sqrt() * aspect } else { 0. };
        let left = ((c.0 - xo).ceil().max(0.) as i32, ((c.0 - xi).floor() as i32).min(cols as i32 - 1));
        let right = (((c.0 + xi).ceil() as i32).max(left.1 + 1).max(0), ((c.0 + xo).floor() as i32).min(cols as i32 - 1));
        for (a, b) in [left, right] {
            for x in a..=b {
                let dx = (x as f32 - c.0) / aspect;
                let d = (dx * dx + dy * dy).sqrt();
                if d >= r0 && d < r1 {
                    let n = if d > 1e-4 { (dx / d, dy / d) } else { (0., -1.) };
                    f(x as usize, y as usize, d, n);
                }
            }
        }
    }
}

/// A server block set in the sand like a garden stone.
struct Stone {
    /// Centre in cells; half sizes in rows.
    cx: f32,
    cy: f32,
    hw: f32,
    hh: f32,
    hue: Color,
    seed: u32,
}
impl Stone {
    /// Rows from the stone's rounded outline, and the way away from it.
    fn away(&self, x: f32, y: f32, aspect: f32) -> (f32, (f32, f32)) {
        let round = self.hw.min(self.hh) * 0.6;
        let (dx, dy) = ((x - self.cx) / aspect, y - self.cy);
        let (qx, qy) = (dx.abs() - (self.hw - round), dy.abs() - (self.hh - round));
        let (ox, oy) = (qx.max(0.), qy.max(0.));
        let out = (ox * ox + oy * oy).sqrt();
        let d = out + qx.max(qy).min(0.) - round;
        let n = if out > 1e-4 {
            (ox / out * dx.signum(), oy / out * dy.signum())
        } else if qx > qy {
            (dx.signum(), 0.)
        } else {
            (0., dy.signum())
        };
        (d, n)
    }
    /// Its cells: left, top, width and height.
    fn rect(&self, aspect: f32) -> (i32, i32, i32, i32) {
        let w = (self.hw * 2. * aspect).round().max(3.) as i32;
        let h = (self.hh * 2.).round().max(3.) as i32;
        ((self.cx - w as f32 / 2.).round() as i32, (self.cy - h as f32 / 2.).round() as i32, w, h)
    }
}

/// Light travelling out through the rings: round a stone on a beat, or from
/// where a charged cursor let go, raking new rings as it passes.
struct Ripple {
    stone: Option<usize>,
    at: (f32, f32),
    born: f32,
    strength: f32,
    /// How far it travels, in rows, and how fast.
    reach: f32,
    speed: f32,
}
impl Ripple {
    fn radius(&self, time: f32) -> f32 {
        (time - self.born) * self.speed
    }
}

pub(super) struct Zen {
    cols: usize,
    rows: usize,
    aspect: f32,
    /// Rows between grooves, and how far the rings round a stone reach.
    period: f32,
    zone: f32,
    stones: Vec<Stone>,
    /// The combed pattern: the crest glyph per cell, NONE between crests.
    rest: Vec<char>,
    /// Nearest stone and the distance from its outline, in rows.
    near: Vec<(u8, f32)>,
    /// Raked strokes: crest glyph and freshness, 1 when just raked.
    rake: Vec<char>,
    fresh: Vec<f32>,
    /// Sand pushed up beside a stroke.
    heap: Vec<f32>,
    /// The time as grains: where they have landed the sand is engraved.
    sand: SandText,
    /// Cells engraved this frame, and the list of them to clear next frame.
    cut: Vec<bool>,
    marked: Vec<usize>,
    /// Rows the time spans.
    band: (usize, usize),
    /// Columns re-combed since a shake smoothed the garden flat.
    comb: f32,
    ripples: Vec<Ripple>,
    /// The arm raking rings round a resting cursor.
    spiral: f32,
    /// Direction of the last stroke, in rows, to draw the rake head.
    heading: (f32, f32),
    flash: f32,
    strip: usize,
    time: f32,
}
impl Zen {
    pub(super) fn new() -> Self {
        Self {
            cols: 0,
            rows: 0,
            aspect: 2.,
            period: 3.,
            zone: 12.,
            stones: vec![],
            rest: vec![],
            near: vec![],
            rake: vec![],
            fresh: vec![],
            heap: vec![],
            sand: SandText::new(false, true),
            cut: vec![],
            marked: vec![],
            band: (1, 0),
            comb: f32::MAX,
            ripples: vec![],
            spiral: 0.,
            heading: (1., 0.),
            flash: 0.,
            strip: 0,
            time: 0.,
        }
    }
    /// Engrave the cells where grains of the time have landed; loose grains
    /// leave their cell open until they are back.
    fn engrave(&mut self) {
        for &i in &self.marked {
            self.cut[i] = false;
        }
        self.marked.clear();
        let (cols, rows) = (self.cols as i32, self.rows as i32);
        for g in &self.sand.grains {
            if g.away() < 0.6 {
                let (x, y) = (g.home.0.round() as i32, g.home.1.round() as i32);
                if x >= 0 && y >= 0 && x < cols && y < rows {
                    let i = (y * cols + x) as usize;
                    if !self.cut[i] {
                        self.cut[i] = true;
                        self.marked.push(i);
                    }
                }
            }
        }
    }
    /// Write a ring glyph round `c` into the raked layer.
    fn ring_cell(&mut self, x: usize, y: usize, d: f32, n: (f32, f32)) {
        let i = y * self.cols + x;
        let p = self.period;
        self.rake[i] = crest(d / p, n.0 / p, n.1 / p, self.aspect);
        self.fresh[i] = 1.;
        self.heap[i] *= 0.5;
    }
}
impl Play for Zen {
    fn resize(&mut self, kit: &Kit) {
        let (cols, rows, aspect) = (kit.cols, kit.rows, kit.aspect);
        self.cols = cols;
        self.rows = rows;
        self.aspect = aspect;
        self.period = (rows as f32 / 28.).clamp(2.5, 4.5);
        self.zone = self.period * 4.5;
        let (w, h) = (cols as f32, rows as f32);
        // Three stones off the time, in an uneven garden triangle.
        let place = [
            (0.12, 0.24, 0.075, (255, 70, 200)),
            (0.87, 0.74, 0.095, (70, 215, 255)),
            (0.7, 0.13, 0.055, (165, 115, 255)),
        ];
        self.stones = place
            .iter()
            .enumerate()
            .map(|(i, &(fx, fy, size, hue))| {
                let hh = (h * size).max(1.6);
                Stone { cx: w * fx, cy: h * fy, hw: hh * 1.75, hh, hue, seed: 977 * (i as u32 + 1) }
            })
            .collect();
        let n = cols * rows;
        self.rest = vec![NONE; n];
        self.near = vec![(0, f32::MAX); n];
        let p = self.period;
        for y in 0..rows {
            for x in 0..cols {
                let i = y * cols + x;
                let mut best = (0u8, f32::MAX, (0., 1.));
                for (s, stone) in self.stones.iter().enumerate() {
                    let (d, dir) = stone.away(x as f32, y as f32, aspect);
                    if d < best.1 {
                        best = (s as u8, d, dir);
                    }
                }
                self.near[i] = (best.0, best.1);
                // Rings follow each stone out to the zone; straight combing beyond.
                self.rest[i] = if best.1 < self.zone {
                    if best.1 > 0. { crest(best.1 / p, best.2 .0 / p, best.2 .1 / p, aspect) } else { NONE }
                } else {
                    crest(y as f32 / p, 0., 1. / p, aspect)
                };
            }
        }
        self.rake = vec![NONE; n];
        self.fresh = vec![0.; n];
        self.heap = vec![0.; n];
        self.sand.place(kit, ((w * 0.2) as i32, (h * 0.3) as i32, (w * 0.6).max(1.) as usize, (h * 0.42).max(1.) as usize));
        let (_, y0, _, y1) = self.sand.extent();
        self.band = if y0 <= y1 { ((y0.floor().max(0.)) as usize, (y1.ceil().max(0.) as usize).min(rows.saturating_sub(1))) } else { (1, 0) };
        self.cut = vec![false; n];
        self.marked.clear();
        self.engrave();
        self.comb = f32::MAX;
        self.ripples.clear();
    }
    fn step(&mut self, kit: &Kit) {
        if self.rest.len() != kit.cols * kit.rows {
            return;
        }
        self.time = kit.time;
        let (cols, rows, aspect, dt, time) = (self.cols, self.rows, self.aspect, kit.dt, kit.time);
        let p = self.period;
        // The minute turns as the old digit pours its sand into the new one.
        self.sand.step(kit);
        self.engrave();
        // The rake: tines along the stroke, sand heaped either side. Circling
        // rakes true rings round the circle's centre instead.
        if let Some((centre, _)) = kit.swirl {
            let mut cells = vec![];
            kit.sweep(p * (TINES + 0.5), |x, y, _, _| cells.push((x, y)));
            for (x, y) in cells {
                let (dx, dy) = ((x as f32 - centre.0) / aspect, y as f32 - centre.1);
                let d = (dx * dx + dy * dy).sqrt().max(1e-3);
                self.ring_cell(x, y, d, (dx / d, dy / d));
            }
        } else if let (Some(b), Some(a)) = (kit.cursor, kit.last) {
            let (sx, sy) = ((b.0 - a.0) / aspect, b.1 - a.1);
            let len = (sx * sx + sy * sy).sqrt();
            if len > 0.05 {
                let (ux, uy) = (sx / len, sy / len);
                self.heading = (ux, uy);
                let perp = (-uy, ux);
                let (tines, edge) = (p * (TINES + 0.5), p * (TINES + 1.3));
                let (rake, fresh, heap) = (&mut self.rake, &mut self.fresh, &mut self.heap);
                kit.sweep(edge, |x, y, _, _| {
                    let i = y * cols + x;
                    let off = (x as f32 - a.0) / aspect * perp.0 + (y as f32 - a.1) * perp.1;
                    if off.abs() < tines {
                        rake[i] = crest(off / p, perp.0 / p, perp.1 / p, aspect);
                        fresh[i] = 1.;
                        heap[i] *= 0.5;
                    } else {
                        heap[i] = (heap[i] + 0.2).min(1.);
                    }
                });
            }
        }
        // Resting: an arm rakes rings round the cursor, wider as it charges.
        if let (Some(c), true) = (kit.cursor, kit.charge > 0.) {
            let from = self.spiral;
            self.spiral += dt * (2.5 + kit.charge * 3.5);
            let swept = (self.spiral - from).min(TAU);
            let radius = (1.2 + kit.charge * 4.5) * p;
            let mut cells = vec![];
            annulus(cols, rows, aspect, c, 0., radius, |x, y, d, n| {
                let a = n.1.atan2(n.0);
                if (a - from).rem_euclid(TAU) <= swept {
                    cells.push((x, y, d, n));
                }
            });
            for (x, y, d, n) in cells {
                self.ring_cell(x, y, d, n);
            }
            // The arm drags the time's sand round with it, out to its tip.
            let spin = (14. + kit.charge * 22.) * kit.reach.max(0.8);
            self.sand.push(kit, |x, y| {
                let (dx, dy) = ((x - c.0) / aspect, y - c.1);
                let d = (dx * dx + dy * dy).sqrt();
                if d > radius || d < 0.3 || (dy.atan2(dx) - from).rem_euclid(TAU) > swept {
                    return None;
                }
                let (nx, ny) = (dx / d, dy / d);
                Some(((-ny * spin + nx * 4.) * aspect, nx * spin + ny * 4.))
            });
        }
        if kit.shake {
            // Smoothed flat; a comb then sweeps the pattern back in.
            self.fresh.iter_mut().for_each(|f| *f = 0.);
            self.heap.iter_mut().for_each(|h| *h = 0.);
            self.comb = 0.;
        }
        if self.comb < cols as f32 {
            self.comb += dt * cols as f32 / 4.5;
        }
        // Beats light the rings round every stone outward; a drop sends
        // them across the whole garden.
        if let Some(k) = kit.beat {
            for s in 0..self.stones.len() {
                self.ripples.push(Ripple { stone: Some(s), at: (0., 0.), born: time, strength: 0.25 + k * 0.5, reach: self.zone + p, speed: self.zone * 2.4 });
            }
        }
        if kit.drop {
            self.flash = 1.;
            for s in 0..self.stones.len() {
                self.ripples.push(Ripple { stone: Some(s), at: (0., 0.), born: time, strength: 1.2, reach: rows as f32 * 1.6, speed: rows as f32 * 0.7 });
            }
        }
        self.flash = (self.flash - dt * 1.4).max(0.);
        // Rings from a let-go charge are raked where the front passes.
        let fronts: Vec<_> = self
            .ripples
            .iter()
            .filter(|r| r.stone.is_none())
            .map(|r| (r.at, (r.radius(time - dt)).max(0.), r.radius(time).min(r.reach)))
            .filter(|&(_, a, b)| b > a)
            .collect();
        for (c, a, b) in fronts {
            let mut cells = vec![];
            annulus(cols, rows, aspect, c, a, b, |x, y, d, n| cells.push((x, y, d, n)));
            for (x, y, d, n) in cells {
                self.ring_cell(x, y, d, n);
            }
        }
        self.ripples.retain(|r| r.radius(time) < r.reach + p * 3.);
        if self.ripples.len() > 24 {
            let extra = self.ripples.len() - 24;
            self.ripples.drain(..extra);
        }
        // Raked strokes slump back into the combed pattern, a strip of rows
        // per frame: it takes minutes, sooner when the bass shakes the sand.
        const STRIPS: usize = 8;
        let rate = 1. / 80. + kit.sound.bass * 0.05;
        let keep = (-dt * STRIPS as f32 * rate).exp();
        let per = rows.div_ceil(STRIPS).max(1);
        let (y0, y1) = ((self.strip % STRIPS) * per, ((self.strip % STRIPS + 1) * per).min(rows));
        for i in y0 * cols..y1 * cols {
            self.fresh[i] *= keep;
            self.heap[i] *= keep;
        }
        self.strip += 1;
    }
    fn draw(&self, r: &mut Raster, kit: &Kit) {
        let (cols, rows, aspect) = (self.cols, self.rows, self.aspect);
        if self.rest.len() != cols * rows || cols * rows == 0 {
            return;
        }
        let (bass, treble, time) = (kit.sound.bass, kit.sound.treble, self.time);
        let p = self.period;
        let sand: Color = (168, 160, 214);
        let light = 0.82 + treble * 0.15 + self.flash * 0.3;
        let neon: Color = (110, 235, 255);
        // Each stone's LEDs tint the sand round it.
        let glow: Vec<f32> = self.stones.iter().map(|_| 0.22 + bass * 0.55 + self.flash * 0.5).collect();
        let frame = (time * 24.) as u32;
        let shimmer = bass * 0.45;
        let tinted: Vec<Color> = self.stones.iter().map(|s| mix(sand, s.hue, 0.85)).collect();
        for y in 0..rows {
            for x in 0..cols {
                let i = y * cols + x;
                if self.cut[i] {
                    continue;
                }
                let f = self.fresh[i];
                let (ch, k) = if f > 0.5 {
                    (self.rake[i], 0.45 + f * 0.55)
                } else if (x as f32) < self.comb {
                    (self.rest[i], 1. - f * 0.8)
                } else {
                    (NONE, 0.)
                };
                let (s, d) = self.near[i];
                let s = s as usize;
                // Beat ripples run out along the rings of their own stone.
                let mut boost = 0.;
                for rp in &self.ripples {
                    if rp.stone == Some(s) {
                        let off = (d - rp.radius(time)).abs();
                        if off < p {
                            boost += rp.strength * (1. - off / p);
                        }
                    }
                }
                if ch == NONE {
                    let seed = (i as u32).wrapping_mul(2654435761);
                    if self.heap[i] > 0.25 && hash(seed ^ 0x51ed) < self.heap[i] * 0.55 {
                        r.dot(x as i32, y as i32, 8., '∙', scale(sand, 0.3 + self.heap[i] * 0.45 + boost * 0.3));
                    } else if hash(seed) < 0.045 + boost * 0.15 {
                        r.dot(x as i32, y as i32, 9., '·', scale(sand, 0.28 + boost * 0.5));
                    }
                    continue;
                }
                let jitter = if shimmer > 0. { (hash((i as u32) ^ frame.wrapping_mul(0x9e37_79b9)) - 0.5) * shimmer } else { 0. };
                let near = (-d.max(0.) / self.zone).exp() * glow[s];
                let mut colour = mix(sand, tinted[s], near + boost * 0.6);
                // A fresh stroke glows, cooling to sand as it ages.
                if f > 0.5 {
                    colour = mix(colour, neon, ((f - 0.5) * 2.).powi(2) * 0.75);
                }
                r.dot(x as i32, y as i32, 8., ch, scale(colour, (k * light + boost * 0.7) * (1. + jitter)));
            }
        }
        // The light at a let-go ripple's front.
        for rp in self.ripples.iter().filter(|r| r.stone.is_none()) {
            let rad = rp.radius(time);
            let fade = (1. - rad / (rp.reach + p * 3.)).max(0.);
            annulus(cols, rows, aspect, rp.at, (rad - p * 0.6).max(0.), rad + p * 0.6, |x, y, _, _| {
                let i = y * cols + x;
                let ch = if self.rake[i] != NONE { self.rake[i] } else { '·' };
                r.dot(x as i32, y as i32, 7., ch, scale((120, 235, 255), (0.5 + rp.strength * 0.5) * fade + 0.2));
            });
        }
        self.draw_time(r, kit);
        self.draw_rakes(r, kit);
        for (i, s) in self.stones.iter().enumerate() {
            self.draw_stone(r, kit, s, i);
        }
    }
}
impl Zen {
    /// The engraving: a glowing floor, its top wall in shadow and a lit
    /// lip below, so the digits read as cut into the sand.
    fn draw_time(&self, r: &mut Raster, kit: &Kit) {
        let (cols, rows) = (self.cols, self.rows);
        if self.band.0 > self.band.1 {
            return;
        }
        let floor = scale((95, 225, 255), 0.62 + kit.sound.treble * 0.18 + kit.sound.bass * 0.12 + self.flash * 0.3);
        let deep = scale((40, 120, 170), 0.9 + kit.sound.bass * 0.2);
        let lip = scale((225, 220, 255), 0.85);
        let cut = |x: i32, y: i32| x >= 0 && y >= 0 && (x as usize) < cols && (y as usize) < rows && self.cut[y as usize * cols + x as usize];
        for y in self.band.0.saturating_sub(1)..=(self.band.1 + 1).min(rows - 1) {
            for x in 0..cols {
                let (xi, yi) = (x as i32, y as i32);
                if cut(xi, yi) {
                    let ch = if !cut(xi, yi - 1) { '▄' } else { '█' };
                    let colour = if !cut(xi - 1, yi) || !cut(xi, yi - 1) { deep } else { floor };
                    r.dot(xi, yi, 5., ch, colour);
                } else if cut(xi, yi - 1) {
                    r.dot(xi, yi, 5., '▀', lip);
                }
            }
        }
        // Sand thrown out of the engraving: glowing loose grains, cooling to
        // the garden's colour the further they fly.
        // Grains thrown together fly as a clump; spreading each by its own
        // phase, more the further it has flown, lets them read as a spray.
        let time = self.time;
        for g in &self.sand.grains {
            let away = g.away();
            if away < 0.6 {
                continue;
            }
            let spread = (away * 0.35).min(4.);
            let x = g.x + (g.phase * 7.).cos() * spread * self.aspect * 0.5;
            let y = g.y + (g.phase * 5.).sin() * spread * 0.5;
            let glint = (time * 2.3 + g.phase).sin().max(0.).powi(6) * (0.4 + kit.sound.treble * 0.6);
            let ch = if g.shade > 0.9 { '●' } else if g.shade > 0.68 { '•' } else { '∙' };
            let tone = mix((110, 230, 255), (215, 205, 255), (away / 20.).min(0.5));
            r.dot(x.round() as i32, y.round() as i32, 4.5, ch, scale(tone, 0.7 + g.shade * 0.45 + glint * 0.5 + self.flash * 0.3));
        }
    }
    /// The rake head under a moving cursor, the arm raking round a resting
    /// one, and the comb restoring the garden after a shake.
    fn draw_rakes(&self, r: &mut Raster, kit: &Kit) {
        let (aspect, p) = (self.aspect, self.period);
        let neon = (120, 240, 255);
        if let Some(c) = kit.cursor {
            if kit.charge > 0. {
                let radius = (1.2 + kit.charge * 4.5) * p;
                let (ca, sa) = (self.spiral.cos(), self.spiral.sin());
                let steps = (radius * 2.).ceil().max(2.) as usize;
                for j in 0..=steps {
                    let t = j as f32 / steps as f32 * radius;
                    r.dot((c.0 + ca * t * aspect).round() as i32, (c.1 + sa * t).round() as i32, 5.5, '•', scale(neon, 0.4 + kit.charge * 0.6));
                }
                r.dot(c.0.round() as i32, c.1.round() as i32, 5.5, '◆', scale(neon, 0.6 + kit.charge * 0.4));
            } else {
                let (ux, uy) = self.heading;
                let perp = (-uy, ux);
                let span = p * (TINES + 0.5);
                let steps = (span * 4.).ceil().max(2.) as i32;
                for j in -steps..=steps {
                    let t = j as f32 / steps as f32 * span;
                    let (x, y) = (c.0 + perp.0 * t * aspect, c.1 + perp.1 * t);
                    let tine = ((t / p).round() * p - t).abs() < 0.3 && (t / p).round().abs() <= TINES;
                    // Behind the engraving, so the time is never covered.
                    r.dot(x.round() as i32, y.round() as i32, 5.5, if tine { '●' } else { '·' }, scale(neon, if tine { 0.95 } else { 0.55 }));
                }
            }
        }
        if self.comb < self.cols as f32 {
            let x = self.comb.round() as i32;
            let mut y = 0.;
            while y < self.rows as f32 {
                r.dot(x, y.round() as i32, 3., '●', scale(neon, 0.85));
                r.dot(x - 1, y.round() as i32, 3.5, '─', scale(neon, 0.4));
                y += p;
            }
        }
    }
    /// A server block: a rounded case, activity LEDs, bass and treble meters
    /// and vents, with a shadow on the sand.
    fn draw_stone(&self, r: &mut Raster, kit: &Kit, s: &Stone, index: usize) {
        let (x0, y0, w, h) = s.rect(self.aspect);
        let (bass, treble, time) = (kit.sound.bass, kit.sound.treble, self.time);
        // The shadow falls down and to the right, hiding the sand there.
        let sx = (self.aspect * 1.2).round().max(1.) as i32;
        for y in y0 + 1..=y0 + h {
            for x in x0 + w..x0 + w + sx {
                r.dot(x, y, 6., ' ', (0, 0, 0));
            }
        }
        for x in x0 + sx..x0 + w {
            r.dot(x, y0 + h, 6., ' ', (0, 0, 0));
        }
        let metal = mix((105, 110, 135), (255, 255, 255), self.flash * 0.7);
        let rim = mix(metal, s.hue, 0.25 + bass * 0.35);
        let inner = (w - 2).max(0);
        for y in 0..h {
            for x in 0..w {
                let ch = match (x, y) {
                    (0, 0) => '╭',
                    (x, 0) if x == w - 1 => '╮',
                    (0, y) if y == h - 1 => '╰',
                    (x, y) if x == w - 1 && y == h - 1 => '╯',
                    (_, 0) => '─',
                    (_, y) if y == h - 1 => '─',
                    (0, _) => '│',
                    (x, _) if x == w - 1 => '│',
                    _ => ' ',
                };
                let colour = if ch == ' ' { (0, 0, 0) } else { rim };
                r.dot(x0 + x, y0 + y, 2., ch, colour);
            }
        }
        if inner <= 0 || h < 3 {
            return;
        }
        let rows_in = h - 2;
        let lit = |level: f32, j: i32| (j as f32) < level * inner as f32;
        for row in 0..rows_in {
            let y = y0 + 1 + row;
            for j in 0..inner {
                let x = x0 + 1 + j;
                let (ch, colour) = match row {
                    // Disk and network activity: busier with the music.
                    0 => {
                        if j % 2 == 1 {
                            continue;
                        }
                        let tick = (time * (3. + treble * 14. + index as f32)) as u32;
                        let on = hash(s.seed ^ (j as u32).wrapping_mul(7919) ^ tick.wrapping_mul(104729)) < 0.3 + bass * 0.45 + self.flash;
                        let amber = hash(s.seed ^ j as u32) < 0.25;
                        let hue = if amber { (255, 185, 60) } else { (90, 255, 150) };
                        if on { ('▪', hue) } else { ('·', scale(hue, 0.25)) }
                    }
                    1 => {
                        let level = (bass + self.flash).min(1.);
                        if lit(level, j) { ('▮', mix(s.hue, (255, 255, 255), j as f32 / inner as f32 * 0.5)) } else { ('▯', scale(s.hue, 0.22)) }
                    }
                    2 => {
                        let level = (treble + self.flash).min(1.);
                        if lit(level, j) { ('▮', mix((80, 220, 255), (255, 255, 255), j as f32 / inner as f32 * 0.5)) } else { ('▯', (25, 60, 75)) }
                    }
                    _ => ('═', scale(metal, 0.35)),
                };
                r.dot(x, y, 1., ch, colour);
            }
        }
        // The power light breathes even in silence.
        let breath = 0.45 + 0.4 * (time * 1.3 + index as f32).sin().max(0.);
        r.dot(x0 + w - 2, y0, 1., '●', scale(s.hue, breath + bass * 0.3));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn kit() -> Kit {
        Kit { cols: 174, rows: 50, aspect: 2., dt: 0.05, reach: 1., hms: (10, 0, 0), ..Kit::default() }
    }
    #[test]
    fn crests_follow_the_groove_direction() {
        // Horizontal grooves draw flat lines, vertical ones upright lines.
        assert_eq!(crest(0., 0., 0.4, 2.), '─');
        assert_eq!(crest(0., 0.4, 0., 2.), '│');
        assert_eq!(crest(0.5, 0., 0.4, 2.), NONE);
        assert!(matches!(crest(0., 0.3, 0.3, 2.), '╱' | '╲'));
    }
    #[test]
    fn the_rake_carves_strokes_and_the_minute_is_rewritten() {
        let mut z = Zen::new();
        let mut k = kit();
        z.resize(&k);
        let engraved = z.cut.iter().filter(|c| **c).count();
        assert!(engraved > 100, "{engraved}");
        // A stroke across the bottom of the garden.
        for i in 0..40 {
            k.time += k.dt;
            k.last = k.cursor;
            k.cursor = Some((10. + i as f32 * 3., 44.));
            z.step(&k);
        }
        let raked = z.fresh.iter().filter(|f| **f > 0.9).count();
        let lines = z.rake.iter().zip(&z.fresh).filter(|(c, f)| **f > 0.9 && **c != NONE).count();
        assert!(raked > 300 && lines > 100 && lines < raked, "{raked} {lines}");
        // A shake smooths it all; the comb then brings the pattern back.
        k.shake = true;
        z.step(&k);
        k.shake = false;
        assert!(z.fresh.iter().all(|f| *f == 0.) && z.comb < 5.);
        k.cursor = None;
        k.last = None;
        for _ in 0..120 {
            k.time += k.dt;
            z.step(&k);
        }
        let before = z.cut.clone();
        k.hms = (10, 1, 0);
        for _ in 0..120 {
            k.time += k.dt;
            z.step(&k);
        }
        assert!(z.comb >= 174.);
        assert!(z.sand.grains.iter().all(|g| g.away() < 0.6));
        assert_ne!(before, z.cut, "the new minute was never engraved");
    }
    #[test]
    fn raking_across_the_time_throws_its_sand_out_and_it_refills() {
        let mut z = Zen::new();
        let mut k = kit();
        k.dt = 1. / 60.;
        z.resize(&k);
        let full = z.cut.iter().filter(|c| **c).count();
        let (x0, y0, x1, y1) = z.sand.extent();
        let y = (y0 + y1) / 2.;
        for i in 0..=40 {
            k.time += k.dt;
            k.last = k.cursor;
            k.cursor = Some((x0 + (x1 - x0) * i as f32 / 40., y));
            k.velocity = ((x1 - x0) / 40. / 2. / k.dt, 0.);
            z.step(&k);
        }
        let open = z.cut.iter().filter(|c| **c).count();
        let loose = z.sand.grains.iter().filter(|g| g.away() > 1.5).count();
        assert!(open < full * 9 / 10 && loose > 50, "{open} of {full}, {loose} loose");
        k.cursor = None;
        k.last = None;
        for _ in 0..400 {
            k.time += k.dt;
            z.step(&k);
        }
        assert_eq!(z.cut.iter().filter(|c| **c).count(), full, "the engraving never refilled");
    }
    /// Sweeps the cursor along the digit row of the zen garden and the
    /// shore and writes PPM stills just after, while the sand pours back and
    /// once it has settled:
    ///   POSTER_DIR=/tmp/out cargo test --release -- sweep_stills --ignored
    #[test]
    #[ignore = "manual: writes PPM files"]
    fn sweep_stills() {
        use super::super::{Sandbox, Toy};
        use crate::interactive_matrix::Sound;
        use gtk4::cairo::{Context, Format, ImageSurface};
        let dir = std::path::PathBuf::from(std::env::var("POSTER_DIR").unwrap_or_else(|_| "stills".into()));
        std::fs::create_dir_all(&dir).unwrap();
        let (cols, rows, cw, chh) = (240usize, 68usize, 8., 17.);
        let aspect = chh as f32 / cw as f32;
        for (name, toy, row) in [("zen", Toy::Zen, 0.51), ("shore", Toy::Shore, 0.28)] {
            let mut sb = Sandbox::new(toy);
            sb.fake_hms = Some((12, 34, 56));
            let dt = 1. / 30.;
            let mut t = 0f32;
            let sound = Sound { bass: 0.3, treble: 0.4 };
            for &(snap, label) in &[(2.2f32, "a-swept"), (3.6, "b-pouring"), (8., "c-settled")] {
                while t < snap {
                    // Rest, then sweep across the digits from 1.0 s to 2.1 s.
                    let u = ((t - 1.) / 1.1).clamp(0., 1.);
                    let pointer = if t < 2.1 { Some((0.12 + u * 0.76, row + (t * 9.).sin() * 0.02)) } else { None };
                    sb.step(dt, &sound, pointer, cols, rows, aspect);
                    t += dt;
                }
                let mut r = Raster::new(cols, rows);
                sb.draw(&mut r);
                let mut cells = vec![];
                r.recycle_visible(&mut cells);
                let surface = ImageSurface::create(Format::Rgb24, (cols as f64 * cw) as i32, (rows as f64 * chh) as i32).unwrap();
                let cr = Context::new(&surface).unwrap();
                cr.set_source_rgb(0., 0., 0.);
                cr.paint().unwrap();
                let layout = pangocairo::functions::create_layout(&cr);
                layout.set_font_description(Some(&pangocairo::pango::FontDescription::from_string("monospace 10")));
                for c in &cells {
                    let (red, g, b) = c.color;
                    cr.set_source_rgb(red as f64 / 255., g as f64 / 255., b as f64 / 255.);
                    cr.move_to(c.x as f64 * cw, c.y as f64 * chh);
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
                std::fs::write(dir.join(format!("sweep-{name}-{label}.ppm")), ppm).unwrap();
            }
        }
    }
}
