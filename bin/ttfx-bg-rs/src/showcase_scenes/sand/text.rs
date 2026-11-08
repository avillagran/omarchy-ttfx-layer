//! The time in loose sand, for any toy. Every point of the digits is a grain:
//! the cursor scatters the grains along its path and they pour back home a
//! few seconds later, as in the classic sand clock. Resting and moving off
//! bursts them, circling stirs them, a shake or a drop blows the time apart.
//! A toy places it in a box, picks how dense it is and paints every grain in
//! its own look, so tablets, billboards and chips all break up into sand.
use super::super::{Rng, SandFont};
use super::{face, Kit};
use crate::interactive_tunnel::Raster;
use std::f32::consts::TAU;

type Color = (u8, u8, u8);

pub(super) struct Grain {
    pub x: f32,
    pub y: f32,
    vx: f32,
    vy: f32,
    /// Where it belongs in the digit.
    pub home: (f32, f32),
    pub slot: usize,
    index: usize,
    jitter: (f32, f32),
    loose_until: f32,
    /// Fixed per grain: 0.55 to 1, and a phase for glints.
    pub shade: f32,
    pub phase: f32,
}
impl Grain {
    /// How far it has been blown from home, in cells: 0 when it has landed.
    pub fn away(&self) -> f32 {
        (self.x - self.home.0).abs() + (self.y - self.home.1).abs()
    }
    pub fn cell(&self) -> (i32, i32) {
        (self.x.round() as i32, self.y.round() as i32)
    }
}

pub(super) struct SandText {
    pub grains: Vec<Grain>,
    pub font: SandFont,
    shown: Vec<char>,
    seconds: bool,
    dense: bool,
    rng: Rng,
    time: f32,
    /// Grains the cursor touched pour home after this long, plus up to a second.
    pub rest: f32,
}
impl SandText {
    /// `seconds` shows HH:MM:SS rather than HH:MM. `dense` puts a grain in
    /// every cell of a stroke, for digits that should read solid; otherwise
    /// grains sit on a square lattice where each can be seen moving.
    pub fn new(seconds: bool, dense: bool) -> Self {
        Self {
            grains: vec![],
            font: SandFont::default(),
            shown: vec![],
            seconds,
            dense,
            rng: Rng(rand::random()),
            time: 0.,
            rest: 2.5,
        }
    }
    /// Fit the time to a box of cells, centred in it. Call it on resize.
    pub fn place(&mut self, kit: &Kit, bounds: (i32, i32, usize, usize)) {
        let n = if self.seconds { 8 } else { 5 };
        self.font = face(kit, bounds, n);
        self.grains.clear();
        self.shown = kit.clock(self.seconds).chars().collect();
        for (slot, ch) in self.shown.clone().into_iter().enumerate() {
            self.retarget(slot, ch, kit.aspect);
        }
        for g in &mut self.grains {
            (g.x, g.y) = g.home;
        }
    }
    /// Columns and rows the digits cover once placed: x0, y0, x1, y1.
    pub fn extent(&self) -> (f32, f32, f32, f32) {
        self.grains.iter().fold((f32::MAX, f32::MAX, f32::MIN, f32::MIN), |(a, b, c, d), g| {
            (a.min(g.home.0), b.min(g.home.1), c.max(g.home.0), d.max(g.home.1))
        })
    }
    fn cells(&self, ch: char, slot: usize, aspect: f32) -> Vec<(f32, f32)> {
        if !self.font.glyphs.contains_key(&ch) {
            return vec![];
        }
        let base = self.font.cells(ch, slot);
        if !self.dense {
            return base;
        }
        let step = (aspect.round() as i32).max(1);
        base.into_iter().flat_map(|(x, y)| (0..step).map(move |d| (x + d as f32, y))).collect()
    }
    /// Pools only grow, and grain i follows glyph point i % n, so a changed
    /// digit pours its own sand into the new shape and no other digit moves.
    fn retarget(&mut self, slot: usize, ch: char, aspect: f32) {
        let cells = self.cells(ch, slot, aspect);
        if cells.is_empty() {
            return;
        }
        let spread = if self.dense { 0.15 } else { 0.35 };
        let have = self.grains.iter().filter(|g| g.slot == slot).count();
        for index in have..cells.len() {
            let jitter = ((self.rng.next() - 0.5) * spread, (self.rng.next() - 0.5) * spread);
            let (x, y) = cells[index];
            self.grains.push(Grain {
                x: x + jitter.0,
                y: y + jitter.1,
                vx: 0.,
                vy: 0.,
                home: (x, y),
                slot,
                index,
                jitter,
                loose_until: 0.,
                shade: 0.55 + self.rng.next() * 0.45,
                phase: self.rng.next() * TAU,
            });
        }
        for g in self.grains.iter_mut().filter(|g| g.slot == slot) {
            let (x, y) = cells[g.index % cells.len()];
            g.home = (x + g.jitter.0, y + g.jitter.1);
        }
    }
    pub fn step(&mut self, kit: &Kit) {
        let (dt, time, aspect) = (kit.dt, kit.time, kit.aspect);
        self.time = time;
        let text: Vec<char> = kit.clock(self.seconds).chars().collect();
        for slot in 0..text.len().min(self.shown.len()) {
            if text[slot] != self.shown[slot] {
                self.retarget(slot, text[slot], aspect);
            }
        }
        self.shown = text;
        let reach = kit.reach.max(0.8);
        // The cursor only touches the grains on the path it moved this frame.
        let moved = match (kit.last, kit.cursor) {
            (Some(a), Some(b)) => kit.dist(a, b) > 0.15,
            _ => false,
        };
        if moved {
            let radius = (kit.rows as f32 * 0.05).max(1.6);
            let (cvx, cvy) = ((kit.velocity.0 * aspect).clamp(-110., 110.), kit.velocity.1.clamp(-55., 55.));
            for g in &mut self.grains {
                if let Some((w, n)) = kit.touch((g.x, g.y), radius) {
                    g.vx += (cvx * 0.5 + n.0 * aspect * 16.) * w;
                    g.vy += (cvy * 0.5 + n.1 * 16.) * w;
                    g.loose_until = time + self.rest + self.rng.next();
                }
            }
        }
        let away = |c: (f32, f32), x: f32, y: f32| {
            let (dx, dy) = ((x - c.0) / aspect, y - c.1);
            let d = (dx * dx + dy * dy).sqrt().max(0.3);
            (dx / d, dy / d, d)
        };
        if let Some((c, rate)) = kit.swirl {
            let radius = (kit.rows as f32 * 0.32).max(4.);
            let spin = (25. + rate.abs().min(3.) * 20.) * reach * rate.signum();
            for g in &mut self.grains {
                let (ux, uy, d) = away(c, g.x, g.y);
                if d < radius {
                    let f = (1. - d / radius) * dt * 9.;
                    g.vx += (-uy * spin - ux * 6. * reach) * aspect * f;
                    g.vy += (ux * spin - uy * 6. * reach) * f;
                    g.loose_until = g.loose_until.max(time + 0.9);
                }
            }
        }
        if kit.shake || kit.drop {
            let power = if kit.drop { 1.7 } else { 1. } * reach;
            let (x0, y0, x1, y1) = self.extent();
            let c = ((x0 + x1) * 0.5, (y0 + y1) * 0.5);
            for g in &mut self.grains {
                let (ux, uy, _) = away(c, g.x, g.y);
                let a = self.rng.next() * TAU;
                let v = (30. + self.rng.next() * 70.) * power;
                g.vx += (ux * 0.7 + a.cos() * 0.5) * v * aspect;
                g.vy += (uy * 0.7 + a.sin() * 0.5) * v;
                g.loose_until = time + 1.1 + self.rng.next() * 1.1;
            }
        }
        let friction = (-dt * 3.5).exp();
        // 14% of the way per 60 Hz frame, the same curve at any frame rate.
        let frames = dt * 60.;
        let pull = 1. - 0.86f32.powf(frames);
        let (w, h) = (kit.cols as f32 - 1., kit.rows as f32 - 1.);
        for (i, g) in self.grains.iter_mut().enumerate() {
            if time < g.loose_until {
                g.vx *= friction;
                g.vy *= friction;
                g.x += g.vx * dt;
                g.y += g.vy * dt;
            } else {
                g.vx = 0.;
                g.vy = 0.;
                let (ex, ey) = (g.home.0 - g.x, g.home.1 - g.y);
                let d = (ex * ex + ey * ey).sqrt();
                if d > 0.3 {
                    // A sideways sway that fades as the grain lands.
                    let sway = (d / 3.).min(1.) * frames;
                    let speed = 0.6 + (i % 7) as f32 * 0.12;
                    g.x += ex * pull + (time * speed + g.phase).sin() * 0.14 * sway;
                    g.y += ey * pull + (time * speed * 0.9 + g.phase).sin() * 0.07 * sway;
                } else {
                    (g.x, g.y) = g.home;
                }
            }
            g.x = g.x.clamp(0., w.max(0.));
            g.y = g.y.clamp(0., h.max(0.));
        }
    }
    /// Throw the grains `hit` picks with the velocity it returns, in cells
    /// per second: they come loose and pour home later like any others.
    pub fn push(&mut self, kit: &Kit, mut hit: impl FnMut(f32, f32) -> Option<(f32, f32)>) {
        for g in &mut self.grains {
            if let Some((vx, vy)) = hit(g.x, g.y) {
                g.vx += vx;
                g.vy += vy;
                g.loose_until = g.loose_until.max(kit.time + self.rest * 0.6 + self.rng.next());
            }
        }
    }
    /// Paint each grain in the toy's look; None leaves it out.
    pub fn draw(&self, r: &mut Raster, z: f32, mut paint: impl FnMut(&Grain) -> Option<(char, Color)>) {
        for g in &self.grains {
            if let Some((ch, color)) = paint(g) {
                let (x, y) = g.cell();
                r.dot(x, y, z, ch, color);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::super::Kit;
    use super::SandText;
    #[test]
    fn the_cursor_scatters_the_time_and_it_pours_back() {
        let mut kit = Kit { cols: 174, rows: 50, aspect: 2., dt: 1. / 60., reach: 1., hms: (12, 34, 56), ..Kit::default() };
        let mut t = SandText::new(true, false);
        t.place(&kit, (0, 10, 174, 30));
        assert!(t.grains.len() > 100);
        assert!(t.grains.iter().all(|g| g.away() < 0.01));
        // Sweep across the middle of the digits.
        let (x0, y0, x1, y1) = t.extent();
        let y = (y0 + y1) / 2.;
        for i in 0..40 {
            kit.time += kit.dt;
            kit.last = kit.cursor;
            kit.cursor = Some((x0 + (x1 - x0) * i as f32 / 40., y));
            kit.velocity = (((x1 - x0) / 40.) / 2. / kit.dt, 0.);
            t.step(&kit);
        }
        let blown = t.grains.iter().filter(|g| g.away() > 1.5).count();
        assert!(blown > 30, "only {blown} grains moved");
        kit.cursor = None;
        kit.last = None;
        for _ in 0..360 {
            kit.time += kit.dt;
            t.step(&kit);
        }
        assert!(t.grains.iter().all(|g| g.away() < 0.6), "sand never returned");
    }
}
