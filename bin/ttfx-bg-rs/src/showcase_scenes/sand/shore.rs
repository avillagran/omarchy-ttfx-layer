//! A bioluminescent shore at night, seen from above. Waves glowing electric
//! blue roll up the sand and leave it wet; the cursor walks the beach and its
//! footprints light up in the wet sand. The time, in the sand clocks' bold
//! face, is pressed into the sand; each minute a big wave washes it away and
//! it is written again digit by digit. The strokes are glowing grains: walking through them
//! kicks the sand out, and it settles back into the groove a few seconds
//! later. Bass makes the waves bigger, treble sparkles the foam and a beat
//! sends a pulse of light through the water.
use super::super::{mix, scale, Color};
use super::{face, hash, ring, Kit, Play};
use crate::interactive_tunnel::Raster;
use std::f32::consts::{PI, TAU};

/// Seconds between waves reaching the shore.
const PERIOD: f32 = 4.2;
const GLOW: Color = (70, 190, 255);
const FOAM: Color = (190, 245, 255);

struct Print {
    x: f32,
    y: f32,
    /// Walking direction, in rows.
    dir: (f32, f32),
    born: f32,
    washed: Option<f32>,
}
struct Spark {
    x: f32,
    y: f32,
    /// Rows per second.
    vx: f32,
    vy: f32,
    born: f32,
    life: f32,
    ch: char,
    hue: Color,
}
/// Water running up the beach and back.
struct Swash {
    born: f32,
    len: f32,
    /// How far up it runs, in rows.
    reach: f32,
    big: bool,
    peaked: bool,
    seed: f32,
}
impl Swash {
    /// How far up it is, 0 to 1: fast up, slow back.
    fn run(&self, time: f32) -> f32 {
        let u = ((time - self.born) / self.len).clamp(0., 1.);
        if u < 0.33 {
            (u / 0.33 * PI * 0.5).sin()
        } else {
            ((u - 0.33) / 0.67 * PI * 0.5).cos().powi(2)
        }
    }
}
/// One cell of the stick-written time; `order` says when the stick passes.
struct Groove {
    x: i32,
    y: i32,
    ch: char,
    order: f32,
    /// The grain of glowing sand that fills the cell, where it is now.
    px: f32,
    py: f32,
    vx: f32,
    vy: f32,
    /// Kicked out until then; afterwards it settles back.
    loose: f32,
    shade: f32,
}
impl Groove {
    fn home(&self) -> bool {
        (self.px - self.x as f32).abs() + (self.py - self.y as f32).abs() < 0.6
    }
}
/// A line across the screen through `ys[x]` rows (NAN where there is
/// none), each cell taking the glyph of its slope, with steep steps filled so
/// the line never breaks. `paint` picks the glyph and colour per cell.
fn curve(r: &mut Raster, ys: &[f32], z: f32, mut paint: impl FnMut(usize, i32, char) -> Option<(char, Color)>) {
    let n = ys.len();
    for x in 0..n {
        let y = ys[x];
        if !y.is_finite() {
            continue;
        }
        let prev = ys[x.saturating_sub(1)];
        let next = ys[(x + 1).min(n - 1)];
        let prev = if prev.is_finite() { prev } else { y };
        let next = if next.is_finite() { next } else { y };
        let slope = (next - prev) * 0.5;
        let row = y.round();
        let ch = if slope.abs() < 0.41 {
            let f = y - row;
            if f < -0.25 {
                '¯'
            } else if f > 0.25 {
                '_'
            } else {
                '─'
            }
        } else if slope.abs() > 2.4 {
            '│'
        } else if slope > 0. {
            '╲'
        } else {
            '╱'
        };
        if let Some((ch, c)) = paint(x, row as i32, ch) {
            r.dot(x as i32, row as i32, z, ch, c);
        }
        let to = next.round();
        if (to - row).abs() > 1. {
            let (a, b) = if to > row { (row + 1., to) } else { (to + 1., row) };
            let mut yy = a;
            while yy < b {
                if let Some((_, c)) = paint(x, yy as i32, '│') {
                    r.dot(x as i32, yy as i32, z, '│', c);
                }
                yy += 1.;
            }
        }
    }
}

pub(super) struct Shore {
    cols: usize,
    rows: usize,
    aspect: f32,
    /// Footprint size, in rows per unit.
    size: f32,
    /// Per column: where the sea begins at rest, where the water runs to
    /// now, how far up the sand is wet, and the foam left at the last peak.
    shore: Vec<f32>,
    edge: Vec<f32>,
    wet: Vec<f32>,
    mark: Vec<f32>,
    mark_born: f32,
    /// Sand grains: cell and shade. Sea sparkles: column, depth 0..1, seed.
    grains: Vec<(i32, i32, f32)>,
    stars: Vec<(i32, f32, u32)>,
    swash: Option<Swash>,
    next_wave: f32,
    prints: Vec<Print>,
    travel: f32,
    side: f32,
    sparks: Vec<Spark>,
    /// A circle drawn by circling: points and when they were drawn.
    trail: Vec<(f32, f32, f32)>,
    /// Splash rings: centre, birth and strength.
    rings: Vec<(f32, f32, f32, f32)>,
    /// The written time: its box (left, top, width, height), its grooves
    /// in stick order and how far the stick has got.
    frame: (f32, f32, f32, f32),
    text: String,
    grooves: Vec<Groove>,
    /// The glow round the grooves, wider on finer grids: cell, when the
    /// stick passes, and how far out, 0 at the groove to 1 at the rim.
    /// The groove nearest each glow cell goes dark with it when kicked out.
    halo: Vec<(i32, i32, f32, f32, usize)>,
    ink: f32,
    strokes: f32,
    pulse: f32,
    time: f32,
    seed: u32,
}
impl Shore {
    pub(super) fn new() -> Self {
        Self {
            cols: 0,
            rows: 0,
            aspect: 2.,
            size: 1.,
            shore: vec![],
            edge: vec![],
            wet: vec![],
            mark: vec![],
            mark_born: -10.,
            grains: vec![],
            stars: vec![],
            swash: None,
            next_wave: 0.,
            prints: vec![],
            travel: 0.,
            side: 1.,
            sparks: vec![],
            trail: vec![],
            rings: vec![],
            frame: (0., 0., 1., 1.),
            text: String::new(),
            grooves: vec![],
            halo: vec![],
            ink: 0.,
            strokes: 0.,
            pulse: 0.,
            time: 0.,
            seed: rand::random(),
        }
    }
    fn next(&mut self) -> f32 {
        self.seed = self.seed.wrapping_mul(1664525).wrapping_add(1013904223);
        (self.seed >> 8) as f32 / 16777216.
    }
    /// The time in the sand clocks' bold face, pressed into the wet sand
    /// cell by cell. The stick writes each digit from left to right, so a
    /// cell's order is its slot plus how far across the digit it lies.
    fn write(&mut self, text: &str) {
        let (left, top, w, h) = self.frame;
        let aspect = self.aspect;
        let probe = Kit { aspect, ..Kit::default() };
        let n = text.chars().count().max(1);
        let font = face(&probe, (left as i32, top as i32, w.max(1.) as usize, h.max(1.) as usize), n);
        let step = (aspect.round() as i32).max(1);
        let advance = font.advance.max(1) as f32;
        let mut cells: std::collections::HashMap<(i32, i32), (char, f32)> = std::collections::HashMap::new();
        // A fine grid would leave a one-cell groove a hairline: glow round it.
        let band = (self.rows as f32 / 80.).max(0.75);
        for (slot, ch) in text.chars().enumerate() {
            if !font.glyphs.contains_key(&ch) {
                continue;
            }
            let x0 = (font.x0 + slot as i32 * font.advance) as f32;
            for (px, py) in font.cells(ch, slot) {
                for d in 0..step {
                    let cx = px + d as f32;
                    let across = ((cx - x0) / advance).clamp(0., 1.);
                    let down = ((py - font.y0 as f32) / h.max(1.)).clamp(0., 1.);
                    let order = slot as f32 + (across * 0.85 + down * 0.1).min(0.99);
                    cells.entry((cx as i32, py as i32)).or_insert(('•', order));
                }
            }
        }
        self.grooves = cells
            .into_iter()
            .filter(|&((x, y), _)| x >= 0 && y >= 0 && (x as usize) < self.cols && (y as usize) < self.rows)
            .map(|((x, y), (ch, order))| {
                let shade = 0.6 + hash((x as u32).wrapping_mul(7919) ^ (y as u32).wrapping_mul(104729)) * 0.4;
                // Grains like the sand clock's: some big, most small.
                let ch = if shade > 0.88 && ch == '•' { '●' } else { ch };
                Groove { x, y, ch, order, px: x as f32, py: y as f32, vx: 0., vy: 0., loose: 0., shade }
            })
            .collect();
        self.grooves.sort_by(|a, b| a.order.total_cmp(&b.order));
        let (bx, by) = ((band * aspect).ceil() as i32, band.ceil() as i32);
        let mut glow: std::collections::HashMap<(i32, i32), (f32, f32, usize)> = std::collections::HashMap::new();
        let core: std::collections::HashSet<(i32, i32)> = self.grooves.iter().map(|g| (g.x, g.y)).collect();
        for (owner, g) in self.grooves.iter().enumerate() {
            for dy in -by..=by {
                for dx in -bx..=bx {
                    let d = ((dx as f32 / aspect).powi(2) + (dy * dy) as f32).sqrt() / band;
                    let cell = (g.x + dx, g.y + dy);
                    if d > 1. || core.contains(&cell) {
                        continue;
                    }
                    let e = glow.entry(cell).or_insert((g.order, d, owner));
                    e.0 = e.0.min(g.order);
                    if d < e.1 {
                        e.1 = d;
                        e.2 = owner;
                    }
                }
            }
        }
        self.halo = glow
            .into_iter()
            .filter(|&((x, y), _)| x >= 0 && y >= 0 && (x as usize) < self.cols && (y as usize) < self.rows)
            .map(|((x, y), (order, d, owner))| (x, y, order, d, owner))
            .collect();
        self.strokes = text.chars().count() as f32;
        self.text = text.to_owned();
    }
    /// The written sand is loose: the cursor kicks the grains on its path
    /// out of the groove, a let-go charge bursts them, circling stirs them, a
    /// shake or a drop scatters them all; then each settles back home.
    fn kick(&mut self, kit: &Kit) {
        let (dt, time, aspect) = (kit.dt, kit.time, self.aspect);
        let reach = kit.reach.max(0.8);
        let moved = matches!((kit.last, kit.cursor), (Some(a), Some(b)) if kit.dist(a, b) > 0.15);
        let radius = (self.rows as f32 * 0.05).max(1.6);
        let (cvx, cvy) = ((kit.velocity.0 * aspect).clamp(-110., 110.), kit.velocity.1.clamp(-55., 55.));
        let (x0, y0, w, h) = self.frame;
        let centre = (x0 + w / 2., y0 + h / 2.);
        let mut seed = self.seed;
        let mut next = || {
            seed = seed.wrapping_mul(1664525).wrapping_add(1013904223);
            (seed >> 8) as f32 / 16777216.
        };
        let away = |c: (f32, f32), x: f32, y: f32| {
            let (dx, dy) = ((x - c.0) / aspect, y - c.1);
            let d = (dx * dx + dy * dy).sqrt().max(0.3);
            (dx / d, dy / d, d)
        };
        for g in &mut self.grooves {
            if moved {
                if let Some((wt, n)) = kit.touch((g.px, g.py), radius) {
                    // Some kick of its own, so the sand sprays rather than moves as one.
                    g.vx += (cvx * 0.5 + n.0 * aspect * 16. + (next() - 0.5) * 40. * aspect) * wt;
                    g.vy += (cvy * 0.5 + n.1 * 16. + (next() - 0.5) * 24.) * wt;
                    g.loose = time + 2.5 + next();
                }
            }
            if let Some((c, rate)) = kit.swirl {
                let r = (self.rows as f32 * 0.32).max(4.);
                let (ux, uy, d) = away(c, g.px, g.py);
                if d < r {
                    let spin = (25. + rate.abs().min(3.) * 20.) * reach * rate.signum();
                    let f = (1. - d / r) * dt * 9.;
                    g.vx += (-uy * spin - ux * 6. * reach) * aspect * f;
                    g.vy += (ux * spin - uy * 6. * reach) * f;
                    g.loose = g.loose.max(time + 0.9);
                }
            }
            if kit.shake || kit.drop {
                let (ux, uy, _) = away(centre, g.px, g.py);
                let a = next() * TAU;
                let v = (30. + next() * 70.) * reach * if kit.drop { 1.7 } else { 1. };
                g.vx += (ux * 0.7 + a.cos() * 0.5) * v * aspect;
                g.vy += (uy * 0.7 + a.sin() * 0.5) * v;
                g.loose = time + 1.1 + next() * 1.1;
            }
        }
        self.seed = seed;
        let friction = (-dt * 3.5).exp();
        let pull = 1. - 0.86f32.powf(dt * 60.);
        let (wmax, hmax) = (self.cols as f32 - 1., self.rows as f32 - 1.);
        for g in &mut self.grooves {
            if time < g.loose {
                g.vx *= friction;
                g.vy *= friction;
                g.px += g.vx * dt;
                g.py += g.vy * dt;
            } else {
                g.vx = 0.;
                g.vy = 0.;
                let (ex, ey) = (g.x as f32 - g.px, g.y as f32 - g.py);
                if ex.abs() + ey.abs() > 0.3 {
                    g.px += ex * pull;
                    g.py += ey * pull;
                } else {
                    (g.px, g.py) = (g.x as f32, g.y as f32);
                }
            }
            g.px = g.px.clamp(0., wmax.max(0.));
            g.py = g.py.clamp(0., hmax.max(0.));
        }
    }
    fn wave(&mut self, big: bool) {
        let reach = if big {
            // Up past the top of the time, to wash it away.
            let mid = self.shore.get(self.cols / 2).copied().unwrap_or(self.rows as f32);
            mid - (self.frame.1 - 2.).max(0.)
        } else {
            self.rows as f32 * (0.075 + 0.04 * self.next())
        };
        let seed = self.next() * TAU;
        self.swash = Some(Swash { born: self.time, len: if big { 5.5 } else { PERIOD * 0.97 }, reach, big, peaked: false, seed });
    }
}
impl Play for Shore {
    fn resize(&mut self, kit: &Kit) {
        let (cols, rows, aspect) = (kit.cols, kit.rows, kit.aspect);
        self.cols = cols;
        self.rows = rows;
        self.aspect = aspect;
        self.size = (rows as f32 / 40.).max(1.);
        let (w, h) = (cols as f32, rows as f32);
        self.shore = (0..cols)
            .map(|x| h * (0.73 + 0.03 * (x as f32 / w.max(1.) * TAU * 0.7 + 1.).sin()))
            .collect();
        self.edge = self.shore.clone();
        self.wet = self.shore.clone();
        self.mark = self.shore.clone();
        self.grains.clear();
        self.stars.clear();
        for y in 0..rows {
            for x in 0..cols {
                let seed = (x as u32).wrapping_mul(73856093) ^ (y as u32).wrapping_mul(19349663);
                if (y as f32) < self.shore[x] {
                    if hash(seed) < 0.085 {
                        self.grains.push((x as i32, y as i32, 0.4 + hash(seed ^ 0xabc) * 0.6));
                    }
                } else if hash(seed ^ 0x5ea) < 0.08 {
                    let depth = (y as f32 - self.shore[x]) / (h - self.shore[x]).max(1.);
                    self.stars.push((x as i32, depth, seed));
                }
            }
        }
        self.frame = (w * 0.17, h * 0.1, w * 0.66, h * 0.36);
        let text = kit.clock(false);
        self.write(&text);
        self.ink = f32::MAX;
        self.prints.clear();
        self.trail.clear();
        self.sparks.clear();
        self.swash = None;
        self.next_wave = ((self.time / PERIOD).floor() + 1.) * PERIOD;
    }
    fn step(&mut self, kit: &Kit) {
        if self.shore.len() != kit.cols {
            return;
        }
        let (cols, aspect, dt) = (self.cols, self.aspect, kit.dt);
        self.time = kit.time;
        let time = self.time;
        let sc = self.size;
        // Waves arrive on a beat of their own; a new minute or a drop sends a
        // big one that washes the time away.
        let minute = kit.clock(false) != self.text;
        let busy = self.swash.as_ref().is_some_and(|s| s.big);
        if (minute || kit.drop) && !busy {
            self.wave(true);
        } else if time >= self.next_wave && !busy {
            self.wave(false);
        }
        if time >= self.next_wave {
            // In step with the surf lines, which reach the shore on the period.
            self.next_wave = ((time / PERIOD).floor() + 1.) * PERIOD;
        }
        let bottom = self.frame.1 + self.frame.3 + 2.;
        let bass = kit.sound.bass;
        let mut peaked = false;
        let mut washed_time = false;
        if let Some(s) = &mut self.swash {
            let run = s.run(time);
            let reach = if s.big { s.reach } else { s.reach * (1. + bass * 0.7) };
            // Ordinary waves ease to a stop short of the time instead of
            // flattening against it.
            let soft = 3. * self.size.sqrt();
            for x in 0..cols {
                let xf = x as f32;
                let shape = 1. + 0.22 * (xf * 0.07 + s.seed).sin() + 0.12 * (xf * 0.023 + s.seed * 2.).sin();
                let mut e = self.shore[x] - reach * run * shape;
                if !s.big && e < bottom + soft {
                    e = bottom + soft * ((e - bottom - soft) / soft).exp();
                }
                self.edge[x] = e;
            }
            if !s.peaked && time - s.born > s.len * 0.33 {
                s.peaked = true;
                peaked = true;
                washed_time = s.big;
            }
            if time - s.born >= s.len {
                self.swash = None;
            }
        } else {
            self.edge.copy_from_slice(&self.shore);
        }
        if peaked {
            self.mark.copy_from_slice(&self.edge);
            self.mark_born = time;
        }
        if washed_time {
            // The big wave took the time: write the new one once it is back.
            let text = kit.clock(false);
            self.write(&text);
            self.ink = 0.;
        }
        // The sand dries slowly back towards the sea.
        let dry = self.rows as f32 * 0.006 * dt;
        for x in 0..cols {
            self.wet[x] = (self.wet[x] + dry).min(self.shore[x]).min(self.edge[x]);
        }
        // The stick writes once the water is clear of the time.
        let clear = self.swash.as_ref().map_or(true, |s| !s.big || s.run(time) < 0.25);
        if clear && self.ink < self.strokes + 1. {
            self.ink += dt * 1.5;
        }
        self.kick(kit);
        // Footprints: one each stride, left and right of the path.
        if let (Some(c), Some(l)) = (kit.cursor, kit.last) {
            let d = kit.dist(l, c);
            self.travel += d;
            let stride = 2.2 * sc;
            if self.travel >= stride && d > 1e-3 {
                self.travel = 0.;
                self.side = -self.side;
                let dir = ((c.0 - l.0) / aspect / d, (c.1 - l.1) / d);
                let off = 0.55 * sc * self.side;
                let (x, y) = (c.0 - dir.1 * off * aspect, c.1 + dir.0 * off);
                let xi = (x.max(0.) as usize).min(cols - 1);
                if y >= self.edge[xi] {
                    // Stepping in the water only splashes.
                    self.rings.push((x, y, time, 0.5));
                } else {
                    self.prints.push(Print { x, y, dir, born: time, washed: None });
                }
            }
        }
        if self.prints.len() > 400 {
            let extra = self.prints.len() - 400;
            self.prints.drain(..extra);
        }
        // Water running over a print washes it out in a flash.
        let mut flashes = vec![];
        for p in &mut self.prints {
            let xi = (p.x.max(0.) as usize).min(cols - 1);
            if p.washed.is_none() && p.y >= self.edge[xi] {
                p.washed = Some(time);
                flashes.push((p.x, p.y));
            }
        }
        self.prints.retain(|p| p.washed.map_or(true, |t| time - t < 0.5));
        for (x, y) in flashes {
            for _ in 0..2 {
                let a = self.next() * TAU;
                let v = (1. + self.next() * 3.) * sc;
                let life = 0.4 + self.next() * 0.5;
                self.sparks.push(Spark { x, y, vx: a.cos() * v, vy: a.sin() * v, born: time, life, ch: '∙', hue: FOAM });
            }
        }
        // Circling draws a glowing ring in the sand.
        if let (Some(_), Some(c)) = (kit.swirl, kit.cursor) {
            self.trail.push((c.0, c.1, time));
        }
        let edge = &self.edge;
        self.trail.retain(|&(x, y, t)| time - t < 7. && y < edge[(x.max(0.) as usize).min(cols - 1)]);
        if let (true, Some(c)) = (kit.shake, kit.cursor) {
            for _ in 0..90 {
                let a = self.next() * TAU;
                let v = (3. + self.next() * 18.) * sc;
                let life = 0.5 + self.next() * 1.;
                let ch = ['*', '+', '·', '°'][(self.next() * 4.) as usize % 4];
                let hue = mix(FOAM, (255, 255, 255), self.next());
                self.sparks.push(Spark { x: c.0, y: c.1, vx: a.cos() * v, vy: a.sin() * v, born: time, life, ch, hue });
            }
        }
        if let Some(k) = kit.beat {
            self.pulse = self.pulse.max(k.min(1.2));
        }
        if kit.drop {
            self.pulse = 1.5;
        }
        self.pulse = (self.pulse - dt * 2.5).max(0.);
        let keep = (-dt * 2.5).exp();
        for s in &mut self.sparks {
            s.x += s.vx * aspect * dt;
            s.y += s.vy * dt;
            s.vx *= keep;
            s.vy *= keep;
        }
        self.sparks.retain(|s| time - s.born < s.life);
        if self.sparks.len() > 1500 {
            let extra = self.sparks.len() - 1500;
            self.sparks.drain(..extra);
        }
        self.rings.retain(|r| time - r.2 < 1.4);
    }
    fn draw(&self, r: &mut Raster, kit: &Kit) {
        let (cols, rows, aspect) = (self.cols, self.rows, self.aspect);
        if self.shore.len() != cols || cols * rows == 0 {
            return;
        }
        let (bass, treble, time) = (kit.sound.bass, kit.sound.treble, self.time);
        let sc = self.size;
        let tick = (time * 12.) as u32;
        // Sand: dry and moonlit up the beach, dark and glinting where wet.
        for &(x, y, shade) in &self.grains {
            let (xu, yf) = (x as usize, y as f32);
            if yf >= self.edge[xu] {
                continue;
            }
            if yf >= self.wet[xu] {
                let glint = hash((x as u32).wrapping_mul(31) ^ (y as u32).wrapping_mul(977) ^ tick.wrapping_mul(2246822519)) < 0.025 + self.pulse * 0.03;
                if glint {
                    r.dot(x, y, 8.5, '∙', scale(GLOW, 0.55 + treble * 0.3));
                } else {
                    r.dot(x, y, 9., '·', scale((55, 70, 100), shade));
                }
            } else {
                r.dot(x, y, 9., '·', scale((120, 110, 128), shade * 0.8));
            }
        }
        // Footprints, glowing where the sand is wet.
        for p in &self.prints {
            let xi = (p.x.max(0.) as usize).min(cols - 1);
            let wet = p.y >= self.wet[xi];
            let age = time - p.born;
            let glow = if let Some(t) = p.washed {
                1.4 - (time - t) * 2.
            } else if wet {
                0.28 + 0.72 * (-age / 4.).exp()
            } else {
                0.15 + 0.35 * (-age / 2.).exp()
            };
            let base = if wet { (35, 55, 85) } else { (100, 92, 112) };
            self.draw_print(r, p, mix(base, GLOW, glow.clamp(0., 1.4)), sc);
        }
        self.draw_time(r, kit);
        // A circle drawn by circling.
        for pair in self.trail.windows(2) {
            let (a, b) = (pair[0], pair[1]);
            if b.2 - a.2 > 0.15 {
                continue;
            }
            let fade = (1. - (time - b.2) / 7.).max(0.);
            r.line_with_glyph((a.0, a.1), (b.0, b.1), 5.5, scale(GLOW, 0.3 + fade * 0.8), false, Some('•'));
        }
        // The swash: a thin sheet of water hiding the sand, a glowing line of
        // foam along its edge.
        let light = 0.78 + self.pulse * 0.45 + bass * 0.25;
        let mut line = vec![0.; cols];
        for x in 0..cols {
            let (e, s) = (self.edge[x], self.shore[x]);
            line[x] = if e < s - 0.3 { e } else { f32::NAN };
            let top = (e.round() + 1.).max(0.) as usize;
            let end = (s.ceil() as usize).min(rows);
            for y in top..end {
                let seed = (x as u32).wrapping_mul(2654435761) ^ (y as u32).wrapping_mul(40503);
                let depth = y as f32 - e;
                let band = 2.2 * sc.sqrt();
                if depth < band && hash(seed ^ tick.wrapping_mul(0x2c1b_3c6d)) < 0.55 * (1. - depth / band) + 0.1 {
                    // Plankton lit by the rush of the water, thinning behind the edge.
                    let ch = if depth < band * 0.4 { '∙' } else { '·' };
                    r.dot(x as i32, y as i32, 3.5, ch, scale(mix(GLOW, FOAM, 0.3), (0.95 - depth / band * 0.5) * light));
                } else if hash(seed ^ ((time * 3.) as u32).wrapping_mul(0x632b_e5ab)) < 0.3 {
                    let ch = if (x + y) % 3 == 0 { '≈' } else { '~' };
                    r.dot(x as i32, y as i32, 4., ch, scale((45, 130, 195), light * 0.8));
                } else {
                    r.dot(x as i32, y as i32, 4., ' ', (0, 0, 0));
                }
            }
            // Foam left where the last wave turned, fading.
            let left = 1. - (time - self.mark_born) / 3.5;
            if left > 0. && self.mark[x] < e - 0.5 && hash((x as u32) ^ 0x7f4a) < 0.7 {
                r.dot(x as i32, self.mark[x].round() as i32, 6., '∙', scale(FOAM, 0.25 + left * 0.5 + treble * 0.2));
            }
        }
        let foam = scale(mix(GLOW, FOAM, 0.75), light * 1.15);
        curve(r, &line, 3., |x, y, ch| {
            let sparkle = hash((x as u32).wrapping_mul(7919) ^ (y as u32) ^ tick.wrapping_mul(97)) < 0.05 + treble * 0.25;
            Some(if sparkle { ('°', (235, 252, 255)) } else { (ch, foam) })
        });
        // A second, dimmer line of foam just behind the first.
        let behind: Vec<f32> = line.iter().map(|y| y + 1.).collect();
        let trail = scale(GLOW, light * 0.75);
        curve(r, &behind, 3.2, |_, _, ch| Some((ch, trail)));
        // The sea: lines of surf rolling in, glowing electric blue as they break.
        let roll = (-time / PERIOD).rem_euclid(1.);
        for j in 0..4 {
            let mut near = 0.;
            let mut breaking = false;
            for x in 0..cols {
                let s = self.shore[x];
                let spacing = ((rows as f32 - s) / 3.2).max(1.);
                let d = (roll + j as f32) * spacing;
                let wobble = (x as f32 * 0.05 + time * 0.8 + j as f32 * 1.7).sin() * (0.6 + bass * 1.2);
                let y = s + d + wobble;
                line[x] = if y > s && y < rows as f32 { y } else { f32::NAN };
                near = 1. - d / (4. * spacing);
                breaking = d < spacing * 0.45;
            }
            let k = (0.45 + near * 0.5 + if breaking { 0.3 } else { 0. }) * light;
            let tone = if breaking { scale(mix(GLOW, FOAM, 0.6), k) } else { scale(mix((30, 90, 190), GLOW, near), k) };
            curve(r, &line, 4., |_, _, ch| Some((ch, tone)));
            if breaking {
                // The breaking crest lights the plankton in the water behind it.
                let deep = (3. * sc.sqrt()).ceil() as i32;
                for x in 0..cols {
                    let y = line[x];
                    if !y.is_finite() {
                        continue;
                    }
                    let y = y.round() as i32;
                    for dy in 1..=deep {
                        let fall = 1. - dy as f32 / (deep + 1) as f32;
                        if hash((x as u32).wrapping_mul(2246822519) ^ (dy as u32).wrapping_mul(3266489917) ^ tick.wrapping_mul(668265263)) < fall * 0.75 {
                            r.dot(x as i32, y + dy, 4.2, if fall > 0.6 { '∙' } else { '·' }, scale(GLOW, k * (0.5 + fall * 0.7)));
                        }
                    }
                }
            }
        }
        for &(x, depth, seed) in &self.stars {
            let s = self.shore[x as usize];
            let y = (s + depth * (rows as f32 - s)).round() as i32;
            let on = hash(seed ^ ((time * (1.5 + treble * 6.)) as u32).wrapping_mul(2654435761)) < 0.2 + self.pulse * 0.5;
            if on {
                let k = 0.35 + (1. - depth) * 0.4 + self.pulse * 0.5;
                r.dot(x, y, 5., if self.pulse > 0.5 { '∙' } else { '·' }, scale(mix(GLOW, FOAM, self.pulse * 0.5), k));
            }
        }
        // A resting cursor presses a glowing puddle that radiates.
        if let (Some(c), true) = (kit.cursor, kit.charge > 0.) {
            let radius = (0.6 + 2.4 * kit.charge) * sc;
            let throb = 0.85 + 0.15 * (time * 6.).sin();
            let y0 = (c.1 - radius).floor().max(0.) as i32;
            let y1 = ((c.1 + radius).ceil() as i32).min(rows as i32 - 1);
            let x0 = (c.0 - radius * aspect).floor().max(0.) as i32;
            let x1 = ((c.0 + radius * aspect).ceil() as i32).min(cols as i32 - 1);
            for y in y0..=y1 {
                for x in x0..=x1 {
                    let d = kit.dist((x as f32, y as f32), c) / radius;
                    if d < 1. {
                        let ch = if d < 0.4 { '▓' } else if d < 0.75 { '▒' } else { '░' };
                        r.dot(x, y, 5., ch, scale(GLOW, (0.35 + kit.charge * 0.65) * throb * (1.2 - d * 0.6)));
                    }
                }
            }
            for k in 0..2 {
                let phase = (time * 1.6 + k as f32 * 0.5).fract();
                ring(r, kit, c, radius * (1. + phase * 1.4), time, 4.5, '·', scale(GLOW, (1. - phase) * (0.4 + kit.charge * 0.5)));
            }
        }
        for &(x, y, born, k) in &self.rings {
            let age = time - born;
            let fade = (1. - age / 1.4).max(0.);
            ring(r, kit, (x, y), (0.5 + age * 6.) * sc * k.max(0.4), age, 2.5, '∘', scale(FOAM, fade * (0.4 + k * 0.4)));
        }
        for s in &self.sparks {
            let k = 1. - (time - s.born) / s.life;
            if k > 0. {
                r.dot(s.x.round() as i32, s.y.round() as i32, 1.5, s.ch, scale(s.hue, k.max(0.) * 1.1));
            }
        }
    }
}
impl Shore {
    /// A footprint: an oval sole and a row of toes, turned along the walk.
    fn draw_print(&self, r: &mut Raster, p: &Print, colour: Color, sc: f32) {
        let aspect = self.aspect;
        let (half_l, half_w) = (0.9 * sc, 0.42 * sc);
        let reach = half_l + 0.6 * sc;
        let (x0, x1) = ((p.x - reach * aspect).floor() as i32, (p.x + reach * aspect).ceil() as i32);
        let (y0, y1) = ((p.y - reach).floor() as i32, (p.y + reach).ceil() as i32);
        let mut any = false;
        for y in y0..=y1 {
            for x in x0..=x1 {
                let (dx, dy) = ((x as f32 - p.x) / aspect, y as f32 - p.y);
                let u = dx * p.dir.0 + dy * p.dir.1;
                let v = -dx * p.dir.1 + dy * p.dir.0;
                let sole = (u / half_l).powi(2) + (v / half_w).powi(2);
                let toes = (u - half_l * 1.2).powi(2) / (0.3 * sc).powi(2) + (v / (half_w * 1.1)).powi(2);
                if sole < 1. {
                    // The ball of the foot presses deeper than the heel.
                    r.dot(x, y, 7., if sole < 0.5 && u > -half_l * 0.2 { '▓' } else { '▒' }, colour);
                    any = true;
                } else if toes < 1. {
                    r.dot(x, y, 7., '∙', colour);
                    any = true;
                }
            }
        }
        if !any {
            r.dot(p.x.round() as i32, p.y.round() as i32, 7., '▒', colour);
        }
    }
    /// The stick-written time, hidden where water covers it; the stick tip
    /// glows while it writes.
    fn draw_time(&self, r: &mut Raster, kit: &Kit) {
        let cols = self.cols;
        let (treble, time) = (kit.sound.treble, self.time);
        let base = 0.85 + treble * 0.2 + self.pulse * 0.3 + (time * 0.9).sin() * 0.05;
        let mut tip = None;
        for g in &self.grooves {
            if g.order >= self.ink {
                break;
            }
            let xi = (g.x.max(0) as usize).min(cols - 1);
            if g.y as f32 >= self.edge[xi] {
                continue;
            }
            let fresh = (1. - (self.ink - g.order) / 0.6).max(0.);
            tip = Some((g.x, g.y));
            if g.home() {
                let colour = mix(scale((105, 220, 255), base), FOAM, fresh * 0.7);
                r.dot(g.x, g.y, 6., g.ch, colour);
            } else {
                // Kicked out: a loose grain of glowing sand, sparkling as it tumbles.
                let (px, py) = (g.px.round() as i32, g.py.round() as i32);
                if g.py >= self.edge[(px.max(0) as usize).min(cols - 1)] {
                    continue;
                }
                let glint = (time * 9. + g.shade * 40.).sin().max(0.).powi(4) * (0.3 + treble * 0.7);
                let ch = if g.shade > 0.9 { '●' } else if g.shade > 0.7 { '•' } else { '∙' };
                let colour = scale(mix(GLOW, FOAM, 0.45 + glint * 0.5), 0.75 + g.shade * 0.45 + glint * 0.4);
                r.dot(px, py, 5.7, ch, colour);
                // A stroke cell holds more than one grain: the rest spray round it.
                let away = (g.px - g.x as f32).abs() + (g.py - g.y as f32).abs();
                let spread = (away * 0.3).min(3.5);
                for k in 1..3 {
                    let a = g.shade * 37. + k as f32 * 2.4;
                    let (sx, sy) = (g.px + a.cos() * spread * self.aspect * 0.6, g.py + a.sin() * spread * 0.6);
                    r.dot(sx.round() as i32, sy.round() as i32, 5.75, if k == 1 { '•' } else { '∙' }, scale(colour, 0.8));
                }
            }
        }
        let glow = 0.42 + treble * 0.12 + self.pulse * 0.25;
        for &(x, y, order, d, owner) in &self.halo {
            if order >= self.ink || !self.grooves[owner].home() || y as f32 >= self.edge[(x.max(0) as usize).min(cols - 1)] {
                continue;
            }
            let ch = if d < 0.5 { '▒' } else { '░' };
            r.dot(x, y, 6.5, ch, scale(GLOW, glow * (1.1 - d * 0.7)));
        }
        if self.ink < self.strokes {
            if let Some((x, y)) = tip {
                r.dot(x, y, 5.8, '●', (235, 250, 255));
                r.dot(x - 1, y, 5.9, '░', scale(GLOW, 0.6));
                r.dot(x + 1, y, 5.9, '░', scale(GLOW, 0.6));
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn kit() -> Kit {
        Kit { cols: 174, rows: 50, aspect: 2., dt: 0.05, reach: 1., hms: (10, 0, 0), ..Kit::default() }
    }
    #[test]
    fn walking_leaves_prints_that_the_water_washes() {
        let mut s = Shore::new();
        let mut k = kit();
        s.resize(&k);
        for i in 0..60 {
            k.time += k.dt;
            k.last = k.cursor;
            k.cursor = Some((20. + i as f32 * 2., 12.));
            s.step(&k);
        }
        let n = s.prints.len();
        let mut r = Raster::new(174, 50);
        s.draw(&mut r, &k);
        let mut cells = vec![];
        r.recycle(&mut cells);
        let printed = cells.iter().filter(|c| matches!(c.ch, '▓' | '▒') && c.y >= 10 && c.y <= 14).count();
        assert!(n > 10, "{n}");
        assert!(printed > 10, "{printed}");
        // A print down the beach is washed away by the next waves.
        s.prints.push(Print { x: 80., y: s.shore[80] - 1., dir: (1., 0.), born: k.time, washed: None });
        k.cursor = None;
        k.last = None;
        for _ in 0..200 {
            k.time += k.dt;
            s.step(&k);
        }
        assert!(s.prints.iter().all(|p| p.y < s.shore[80] - 2.));
    }
    #[test]
    fn a_new_minute_washes_the_time_and_writes_it_again() {
        let mut s = Shore::new();
        let mut k = kit();
        s.resize(&k);
        assert_eq!(s.text, "10:00");
        k.hms = (10, 1, 0);
        let mut erased = false;
        for _ in 0..400 {
            k.time += k.dt;
            s.step(&k);
            erased |= s.ink < 1.;
        }
        assert!(erased);
        assert_eq!(s.text, "10:01");
        assert!(s.ink > s.strokes);
        assert!(!s.grooves.is_empty());
    }
    #[test]
    fn walking_through_the_time_kicks_its_sand_out_and_it_settles_back() {
        let mut s = Shore::new();
        let mut k = kit();
        k.dt = 1. / 60.;
        s.resize(&k);
        let n = s.grooves.len();
        assert!(n > 100 && s.grooves.iter().all(|g| g.home()));
        let (x0, top, w, h) = s.frame;
        let y = top + h / 2.;
        for i in 0..=40 {
            k.time += k.dt;
            k.last = k.cursor;
            k.cursor = Some((x0 + w * i as f32 / 40., y));
            k.velocity = (w / 40. / 2. / k.dt, 0.);
            s.step(&k);
        }
        let loose = s.grooves.iter().filter(|g| !g.home()).count();
        assert!(loose > 30, "only {loose} of {n} grains kicked");
        k.cursor = None;
        k.last = None;
        for _ in 0..400 {
            k.time += k.dt;
            s.step(&k);
        }
        assert!(s.grooves.iter().all(|g| g.home()), "the sand never settled back");
    }
}
