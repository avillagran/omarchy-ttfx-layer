//! Silicon Foundry: from sand to chip. A floor of silica sand round a chip
//! whose time is etched in copper. The cursor is a lithography laser: it
//! fuses the sand it crosses into circuit traces routed the way a board is,
//! straight, square and diagonal, with vias where they turn. Fresh copper is
//! white hot and cools to orange, then to copper. Beats send data along every
//! trace, treble sparks at the vias, and once the music stops long enough
//! the traces crumble back into sand. Left alone, a ghost router keeps
//! drawing so the board is never idle.
use super::{hash, Kit, Play, SandText};
use super::super::{mix, scale, Color, Rng};
use crate::interactive_tunnel::Raster;
use std::f32::consts::TAU;

const SAND: Color = (176, 160, 130);
const COPPER: Color = (178, 100, 48);
/// Copper the laser laid: brighter than the chip's own.
const FRESH: Color = (236, 138, 62);
const ORANGE: Color = (255, 124, 28);
const WHITE: Color = (255, 248, 222);
const GOLD: Color = (232, 186, 84);
const DATA: Color = (80, 232, 255);
const LASER: Color = (205, 120, 255);

/// One route of copper, cell by cell in the order it was drawn.
struct Path {
    cells: Vec<(i16, i16)>,
    glyphs: Vec<char>,
    born: Vec<f32>,
    /// Where the route turns or ends: vias, where treble sparks.
    nodes: Vec<usize>,
    /// The chip's own fan-out: it never crumbles.
    base: bool,
    /// 0 is sound copper, 1 is sand again.
    decay: f32,
    /// Too much copper on the board: the oldest route goes first.
    doomed: bool,
    /// Data on the way: position along the route and speed in cells per
    /// second, the sign giving the direction.
    pulses: Vec<(f32, f32)>,
    seed: u32,
}
impl Path {
    fn new(base: bool, seed: u32) -> Self {
        Self {
            cells: vec![],
            glyphs: vec![],
            born: vec![],
            nodes: vec![],
            base,
            decay: 0.,
            doomed: false,
            pulses: vec![],
            seed,
        }
    }
    fn push(&mut self, at: (i32, i32), glyph: char, time: f32) {
        if self.cells.last() == Some(&(at.0 as i16, at.1 as i16)) {
            return;
        }
        self.cells.push((at.0 as i16, at.1 as i16));
        self.glyphs.push(glyph);
        self.born.push(time);
    }
    /// The last cell becomes a via.
    fn node(&mut self) {
        if let Some(i) = self.cells.len().checked_sub(1) {
            if self.nodes.last() != Some(&i) {
                self.nodes.push(i);
                self.glyphs[i] = '•';
            }
        }
    }
    /// Whether cell `i` is still copper, and whether it is about to crumble.
    #[inline]
    fn state(&self, i: usize) -> (bool, bool) {
        if self.decay <= 0. {
            return (true, false);
        }
        let h = hash(self.seed ^ (i as u32).wrapping_mul(2654435761));
        (h >= self.decay, h < self.decay + 0.12)
    }
}
fn glyph(dir: (i32, i32)) -> char {
    match dir {
        (_, 0) => '━',
        (0, _) => '┃',
        (a, b) if a == b => '╲',
        _ => '╱',
    }
}
/// Lay `n` cells of fan-out from `at` along `dir`; false at the edge.
fn lay(p: &mut Path, at: &mut (i32, i32), dir: (i32, i32), n: i32, (cols, rows): (i32, i32)) -> bool {
    for _ in 0..n {
        at.0 += dir.0;
        at.1 += dir.1;
        if at.0 < 0 || at.1 < 0 || at.0 >= cols || at.1 >= rows {
            return false;
        }
        p.push(*at, fine(dir), -100.);
    }
    true
}
/// The chip's own routes are drawn finer than the laser's.
fn fine(dir: (i32, i32)) -> char {
    match dir {
        (_, 0) => '─',
        (0, _) => '│',
        (a, b) if a == b => '╲',
        _ => '╱',
    }
}
/// The nearest of the eight board directions towards `(dx, dy)`, in cells.
fn octant(dx: i32, dy: i32) -> (i32, i32) {
    let (ax, ay) = (dx.abs() as f32, dy.abs() as f32);
    if ay <= ax * 0.414 {
        (dx.signum(), 0)
    } else if ax <= ay * 0.414 {
        (0, dy.signum())
    } else {
        (dx.signum(), dy.signum())
    }
}

/// The laser's head: it routes towards a target one cell at a time.
struct Pen {
    at: (i32, i32),
    dir: (i32, i32),
    /// The route being drawn, as an index into the paths.
    path: Option<usize>,
}
/// The router that draws while nobody does.
struct Ghost {
    pen: Pen,
    waypoints: Vec<(i32, i32)>,
    budget: f32,
}
struct Spark {
    x: f32,
    y: f32,
    vx: f32,
    vy: f32,
    life: f32,
    /// 0 a hot spark, 1 a falling grain of sand, 2 a spark of data.
    kind: u8,
}

/// The chip in the middle: its package, pins and the time etched in it.
struct Chip {
    x0: i32,
    y0: i32,
    x1: i32,
    y1: i32,
    /// The time as copper sand: the laser blows it off, it pours back.
    sand: SandText,
    /// Per grain: 1 on a top edge of its digit, -1 on a bottom edge.
    bevel: Vec<i8>,
    /// Per grain: how hot it is, from flying off under the laser, and how
    /// far from home it was last frame.
    heat: Vec<f32>,
    last_away: Vec<f32>,
    /// Room for the label and the seconds bus.
    roomy: bool,
}
impl Default for Chip {
    fn default() -> Self {
        Self { x0: 0, y0: 0, x1: 0, y1: 0, sand: SandText::new(false, true), bevel: vec![], heat: vec![], last_away: vec![], roomy: false }
    }
}
impl Chip {
    /// Lit from above: a bevel reads as metal standing off the die. Worked
    /// out from where the grains belong, so it holds while they fly.
    fn bevel(&mut self) {
        let set: std::collections::HashSet<(i32, i32)> =
            self.sand.grains.iter().map(|g| (g.home.0.round() as i32, g.home.1.round() as i32)).collect();
        self.bevel = self
            .sand
            .grains
            .iter()
            .map(|g| {
                let (x, y) = (g.home.0.round() as i32, g.home.1.round() as i32);
                if !set.contains(&(x, y - 1)) {
                    1
                } else if !set.contains(&(x, y + 1)) {
                    -1
                } else {
                    0
                }
            })
            .collect();
        self.heat.resize(self.sand.grains.len(), 0.);
        self.last_away.resize(self.sand.grains.len(), 0.);
    }
    #[inline]
    fn covers(&self, x: i32, y: i32) -> bool {
        x >= self.x0 - 2 && x <= self.x1 + 2 && y >= self.y0 - 1 && y <= self.y1 + 1
    }
}

pub(super) struct Foundry {
    rng: Rng,
    sand: Vec<(i16, i16, f32)>,
    chip: Chip,
    paths: Vec<Path>,
    pen: Option<Pen>,
    ghost: Option<Ghost>,
    idle: f32,
    sparks: Vec<Spark>,
    /// Sand grains catching the treble this frame.
    glints: Vec<usize>,
    /// Light running over the sand, per column.
    sheen: Vec<f32>,
    surge: f32,
    shatter: f32,
    coil_cool: f32,
    /// When the second last turned: the etching light crosses the chip once a second.
    tick: f32,
}
impl Foundry {
    pub(super) fn new() -> Self {
        Self {
            rng: Rng(rand::random()),
            sand: vec![],
            chip: Chip::default(),
            paths: vec![],
            pen: None,
            ghost: None,
            idle: 0.,
            sparks: vec![],
            glints: vec![],
            sheen: vec![],
            surge: 0.,
            shatter: 0.,
            coil_cool: 0.,
            tick: 0.,
        }
    }
    fn seed(&mut self) -> u32 {
        (self.rng.next() * 4294967040.) as u32
    }
    /// The package, sized to the grid, and the time etched in it.
    fn etch(&mut self, kit: &Kit) {
        let (cols, rows) = (kit.cols as i32, kit.rows as i32);
        let w = ((cols as f32 * 0.44).round() as i32).clamp(1, cols);
        let h = ((rows as f32 * 0.42).round() as i32).clamp(1, rows);
        let c = &mut self.chip;
        c.x0 = (cols - w) / 2;
        c.x1 = c.x0 + w - 1;
        c.y0 = (rows - h) / 2;
        c.y1 = c.y0 + h - 1;
        c.roomy = h >= 14 && w >= 40;
        // Inside the border, less the label above and the bus below.
        let top = if c.roomy { 4 } else { 1 };
        let bottom = if c.roomy { 3 } else { 1 };
        let bw = (w - 6).max(1) as usize;
        let bh = (h - top - bottom).max(1) as usize;
        c.sand.place(kit, (c.x0 + 3, c.y0 + top, bw, bh));
        c.heat.clear();
        c.bevel();
    }
    /// A floor of silica, and the chip's pins fanned out to the edges.
    fn lay_board(&mut self, kit: &Kit) {
        let (cols, rows) = (kit.cols as i32, kit.rows as i32);
        self.sand.clear();
        for y in 0..rows {
            for x in 0..cols {
                let h = hash((x as u32).wrapping_mul(7919) ^ (y as u32).wrapping_mul(104729));
                if h < 0.3 && !self.chip.covers(x, y) {
                    self.sand.push((x as i16, y as i16, 0.45 + (h / 0.3) * 0.55));
                }
            }
        }
        self.paths.retain(|p| !p.base);
        let c = &self.chip;
        let (x0, x1, y0, y1) = (c.x0, c.x1, c.y0, c.y1);
        let (cx, cy) = ((x0 + x1) as f32 / 2., (y0 + y1) as f32 / 2.);
        let half_w = ((x1 - x0) as f32 / 2.).max(1.);
        let half_h = ((y1 - y0) as f32 / 2.).max(1.);
        let mut routes: Vec<Path> = vec![];
        // Top and bottom: up or down, then out on the diagonal, then straight.
        let room_v = (y0 - 3).max(0);
        for (side, start_y) in [(-1, y0 - 2), (1, y1 + 2)] {
            let mut x = x0 + 2;
            while x <= x1 - 2 {
                let out = ((x as f32 - cx) / half_w).clamp(-1., 1.);
                let lean = out.signum() as i32;
                let bend = (out.abs() * room_v as f32 * 0.8) as i32;
                let mut p = Path::new(true, (x as u32) * 31 + side as u32);
                let mut at = (x, start_y);
                p.push(at, '│', -100.);
                let ok = lay(&mut p, &mut at, (0, side), 2, (cols, rows)) && (bend == 0 || {
                    p.node();
                    lay(&mut p, &mut at, (lean, side), bend, (cols, rows))
                });
                if ok {
                    p.node();
                    let left = if side < 0 { at.1 } else { rows - 1 - at.1 };
                    // Some routes stop at a via short of the edge.
                    let n = if hash(x as u32 ^ 0x9e37) < 0.3 { left / 2 } else { left };
                    lay(&mut p, &mut at, (0, side), n, (cols, rows));
                    p.node();
                }
                routes.push(p);
                x += 6;
            }
        }
        // Left and right: out sideways, then away from the middle, then straight.
        let room_h = (x0 - 4).max(0);
        for (side, start_x) in [(-1, x0 - 3), (1, x1 + 3)] {
            let mut y = y0 + 1;
            while y <= y1 - 1 {
                let out = ((y as f32 - cy) / half_h).clamp(-1., 1.);
                let lean = out.signum() as i32;
                let bend = ((out.abs() * room_h as f32 * 0.35) as i32).min(y0.min(rows - 1 - y1));
                let mut p = Path::new(true, (y as u32) * 131 + side as u32 + 7);
                let mut at = (start_x, y);
                p.push(at, '─', -100.);
                let ok = lay(&mut p, &mut at, (side, 0), 3, (cols, rows)) && (bend == 0 || {
                    p.node();
                    lay(&mut p, &mut at, (side, lean), bend, (cols, rows))
                });
                if ok {
                    p.node();
                    let left = if side < 0 { at.0 } else { cols - 1 - at.0 };
                    let n = if hash(y as u32 ^ 0x51ed) < 0.3 { left / 2 } else { left };
                    lay(&mut p, &mut at, (side, 0), n, (cols, rows));
                    p.node();
                }
                routes.push(p);
                y += 2;
            }
        }
        routes.retain(|p| p.cells.len() > 1);
        // The fan-out sits under whatever was drawn by hand.
        routes.append(&mut self.paths);
        self.paths = routes;
        if let Some(p) = &mut self.pen {
            p.path = None;
        }
        self.ghost = None;
    }
    /// Route the pen towards `target`, at most `steps` cells, fusing sand.
    /// A `planned` route runs diagonal first, then straight: one turn, as an
    /// autorouter lays it. A free one follows the hand, holding its heading
    /// while the target stays near that line, so the copper runs in long
    /// straight segments instead of a staircase.
    fn walk(&mut self, mut pen: Pen, target: (i32, i32), steps: usize, kit: &Kit, planned: bool) -> Pen {
        for _ in 0..steps {
            let (dx, dy) = (target.0 - pen.at.0, target.1 - pen.at.1);
            if dx == 0 && dy == 0 {
                break;
            }
            let dir = if planned {
                (dx.signum(), dy.signum())
            } else {
                let (a, b) = pen.dir;
                let along = dx * a + dy * b;
                let off = (dx * b - dy * a).abs() as f32 / ((a * a + b * b) as f32).sqrt().max(1.);
                if (a, b) != (0, 0) && along > 0 && off <= 1.5 {
                    pen.dir
                } else {
                    octant(dx, dy)
                }
            };
            if dir != pen.dir && pen.dir != (0, 0) {
                if let Some(p) = pen.path.and_then(|i| self.paths.get_mut(i)) {
                    p.node();
                }
            }
            pen.dir = dir;
            pen.at = (pen.at.0 + dir.0, pen.at.1 + dir.1);
            if pen.at.0 < 0 || pen.at.1 < 0 || pen.at.0 >= kit.cols as i32 || pen.at.1 >= kit.rows as i32 {
                break;
            }
            let index = match pen.path {
                Some(i) if i < self.paths.len() => i,
                _ => {
                    let seed = self.seed();
                    self.paths.push(Path::new(false, seed));
                    self.paths.len() - 1
                }
            };
            pen.path = Some(index);
            let p = &mut self.paths[index];
            let first = p.cells.is_empty();
            p.push(pen.at, glyph(dir), kit.time);
            if first {
                p.node();
            }
            if self.rng.next() < 0.35 {
                let a = self.rng.next() * TAU;
                let v = 6. + self.rng.next() * 16.;
                self.sparks.push(Spark {
                    x: pen.at.0 as f32,
                    y: pen.at.1 as f32,
                    vx: a.cos() * v * kit.aspect,
                    vy: a.sin() * v - 8.,
                    life: 0.25 + self.rng.next() * 0.4,
                    kind: 0,
                });
            }
        }
        pen
    }
    fn splash(&mut self, at: (f32, f32), n: usize, power: f32, aspect: f32, kind: u8) {
        for _ in 0..n {
            let a = self.rng.next() * TAU;
            let v = power * (0.3 + self.rng.next() * 0.7);
            self.sparks.push(Spark {
                x: at.0,
                y: at.1,
                vx: a.cos() * v * aspect,
                vy: a.sin() * v - power * 0.2,
                life: 0.4 + self.rng.next() * 0.6,
                kind,
            });
        }
    }
    /// A square spiral inductor round `c`, the way a board lays one.
    fn coil(&mut self, c: (f32, f32), kit: &Kit) {
        let seed = self.seed();
        let mut p = Path::new(false, seed);
        let mut at = (c.0.round() as i32, c.1.round() as i32);
        let unit = (kit.reach.max(0.8) * 1.2).round().max(1.) as i32;
        let dirs = [(1, 0), (0, 1), (-1, 0), (0, -1)];
        let mut len = 1;
        for leg in 0..14 {
            let dir = dirs[leg % 4];
            let n = len * unit * if dir.1 == 0 { kit.aspect.round() as i32 } else { 1 };
            for _ in 0..n {
                at = (at.0 + dir.0, at.1 + dir.1);
                if at.0 < 0 || at.1 < 0 || at.0 >= kit.cols as i32 || at.1 >= kit.rows as i32 {
                    continue;
                }
                p.push(at, glyph(dir), kit.time);
            }
            p.node();
            if leg % 2 == 1 {
                len += 1;
            }
        }
        if p.cells.len() > 1 {
            p.pulses.push((0., 40.));
            self.paths.push(p);
        }
    }
    /// Data sent down a route, from whichever end.
    fn send(p: &mut Path, rng: &mut Rng, speed: f32) {
        if p.pulses.len() >= 4 || p.cells.len() < 2 {
            return;
        }
        let v = speed * (0.7 + rng.next() * 0.6);
        // The chip's own routes carry data outward.
        if p.base || rng.next() < 0.5 {
            p.pulses.push((0., v));
        } else {
            p.pulses.push(((p.cells.len() - 1) as f32, -v));
        }
    }
}
impl Play for Foundry {
    fn resize(&mut self, kit: &Kit) {
        self.paths.clear();
        self.sparks.clear();
        self.pen = None;
        self.ghost = None;
        self.etch(kit);
        self.lay_board(kit);
    }
    fn step(&mut self, kit: &Kit) {
        let (dt, time, s) = (kit.dt, kit.time, kit.sound);
        let chip = &mut self.chip;
        chip.sand.step(kit);
        if kit.minute || chip.bevel.len() != chip.sand.grains.len() {
            chip.bevel();
        }
        // Copper blown off by the laser is white hot as it flies, lies
        // orange where it lands and cools to copper on the way home.
        let cool = (-dt * 0.9).exp();
        for ((h, last), g) in chip.heat.iter_mut().zip(chip.last_away.iter_mut()).zip(&chip.sand.grains) {
            let away = g.away();
            *h = if away > *last + 0.25 { 1. } else { *h * cool };
            *last = away;
        }
        if kit.second {
            self.tick = time;
        }
        self.surge = (self.surge - dt * 0.8).max(0.);
        self.shatter = (self.shatter - dt).max(0.);
        self.coil_cool -= dt;
        // The laser follows the cursor.
        match kit.cursor {
            Some(c) => {
                self.idle = 0.;
                if let Some(g) = self.ghost.take() {
                    if let Some(p) = g.pen.path.and_then(|i| self.paths.get_mut(i)) {
                        p.node();
                    }
                }
                let target = (c.0.round() as i32, c.1.round() as i32);
                let pen = self.pen.take().unwrap_or(Pen { at: target, dir: (0, 0), path: None });
                let steps = (target.0 - pen.at.0).abs().max((target.1 - pen.at.1).abs()) as usize;
                let pen = self.walk(pen, target, steps.min(600), kit, false);
                self.pen = Some(pen);
            }
            None => {
                if let Some(p) = self.pen.take().and_then(|p| p.path).and_then(|i| self.paths.get_mut(i)) {
                    p.node();
                }
                self.idle += dt;
            }
        }
        // Left alone, a ghost router branches off the fan-out.
        if kit.cursor.is_none() && self.idle > 2.5 && self.ghost.is_none() && self.rng.next() < dt * (0.35 + s.bass * 1.2) {
            let bases: Vec<usize> = (0..self.paths.len()).filter(|&i| self.paths[i].base).collect();
            if !bases.is_empty() {
                let p = &self.paths[bases[(self.rng.next() * bases.len() as f32) as usize % bases.len()]];
                let (x, y) = p.cells[(self.rng.next() * p.cells.len() as f32) as usize % p.cells.len()];
                let mut waypoints = vec![];
                for _ in 0..2 + (self.rng.next() * 3.) as usize {
                    waypoints.push((
                        (self.rng.next() * kit.cols as f32) as i32,
                        (self.rng.next() * kit.rows as f32) as i32,
                    ));
                }
                self.ghost = Some(Ghost { pen: Pen { at: (x as i32, y as i32), dir: (0, 0), path: None }, waypoints, budget: 0. });
            }
        }
        if let Some(mut g) = self.ghost.take() {
            g.budget += dt * (22. + s.bass * 50.);
            let steps = g.budget as usize;
            g.budget -= steps as f32;
            if let Some(&target) = g.waypoints.first() {
                g.pen = self.walk(g.pen, target, steps, kit, true);
                if g.pen.at == target {
                    g.waypoints.remove(0);
                }
            }
            if g.waypoints.is_empty() {
                if let Some(p) = g.pen.path.and_then(|i| self.paths.get_mut(i)) {
                    p.node();
                }
            } else {
                self.ghost = Some(g);
            }
        }
        if let Some((c, _)) = kit.swirl {
            if self.coil_cool <= 0. {
                self.coil_cool = 2.5;
                self.coil(c, kit);
            }
        }
        if kit.shake {
            self.shatter = 0.9;
        }
        // Music: data on the beat, everything at once on a drop.
        if let Some(k) = kit.beat {
            let speed = 45. + k * 40.;
            let n = self.paths.len();
            if n > 0 {
                for _ in 0..(3. + k * 5.) as usize {
                    let i = (self.rng.next() * n as f32) as usize % n;
                    Self::send(&mut self.paths[i], &mut self.rng, speed);
                }
                for p in self.paths.iter_mut().filter(|p| !p.base) {
                    if self.rng.next() < 0.7 {
                        Self::send(p, &mut self.rng, speed);
                    }
                }
            }
        }
        if kit.drop {
            self.surge = 1.;
            for p in &mut self.paths {
                Self::send(p, &mut self.rng, 90.);
            }
        }
        // Treble sparks at the vias.
        let mut budget = s.treble * dt * 30. + self.rng.next();
        while budget >= 1. && !self.paths.is_empty() {
            budget -= 1.;
            let i = (self.rng.next() * self.paths.len() as f32) as usize % self.paths.len();
            let p = &self.paths[i];
            if p.nodes.is_empty() {
                continue;
            }
            let node = p.nodes[(self.rng.next() * p.nodes.len() as f32) as usize % p.nodes.len()];
            if !p.state(node).0 {
                continue;
            }
            let (x, y) = p.cells[node];
            self.splash((x as f32, y as f32), 3, 14. + s.treble * 18., kit.aspect, 2);
        }
        // Too much copper: the oldest hand-drawn route goes back to sand.
        let cap = (kit.cols * kit.rows / 9).clamp(200, 9000);
        let drawn: usize = self.paths.iter().filter(|p| !p.base).map(|p| p.cells.len()).sum();
        if drawn > cap {
            if let Some(p) = self.paths.iter_mut().find(|p| !p.base && !p.doomed) {
                p.doomed = true;
            }
        }
        // Crumbling: on a shake, when too much, or after a long silence.
        let mut crumbs = vec![];
        for p in &mut self.paths {
            let rate = if p.base {
                0.
            } else if self.shatter > 0. {
                1.4
            } else if p.doomed {
                0.6
            } else if kit.silence > 6. {
                0.14
            } else {
                0.
            };
            if rate > 0. && !p.cells.is_empty() {
                p.decay += rate * dt;
                let n = (rate * p.cells.len() as f32 * dt * 0.6) as usize + 1;
                for k in 0..n.min(30) {
                    let i = (hash(p.seed ^ (k as u32 + (time * 60.) as u32 * 977)) * p.cells.len() as f32) as usize % p.cells.len();
                    crumbs.push(p.cells[i]);
                }
            }
            for pulse in &mut p.pulses {
                pulse.0 += pulse.1 * dt;
            }
            let len = p.cells.len() as f32;
            p.pulses.retain(|q| q.0 >= 0. && q.0 < len);
        }
        for (x, y) in crumbs {
            let v = 2. + self.rng.next() * 4.;
            self.sparks.push(Spark {
                x: x as f32,
                y: y as f32,
                vx: (self.rng.next() - 0.5) * 6.,
                vy: v,
                life: 0.5 + self.rng.next() * 0.8,
                kind: 1,
            });
        }
        let before = self.paths.len();
        self.paths.retain(|p| p.decay < 1. && !p.cells.is_empty() || p.base);
        if self.paths.len() != before {
            if let Some(p) = &mut self.pen {
                p.path = None;
            }
            if let Some(g) = &mut self.ghost {
                g.pen.path = None;
            }
        }
        // Sparks fly and fall.
        let (w, h) = (kit.cols as f32, kit.rows as f32);
        for sp in &mut self.sparks {
            sp.x += sp.vx * dt;
            sp.y += sp.vy * dt;
            sp.vy += dt * if sp.kind == 1 { 18. } else { 34. };
            sp.vx *= (-dt * 2.).exp();
            sp.life -= dt;
        }
        self.sparks.retain(|sp| sp.life > 0. && sp.x >= 0. && sp.y >= 0. && sp.x < w && sp.y < h);
        if self.sparks.len() > 1600 {
            let extra = self.sparks.len() - 1600;
            self.sparks.drain(..extra);
        }
        // Light over the sand, and the grains the treble catches.
        self.sheen.clear();
        for x in 0..kit.cols {
            let x = x as f32;
            self.sheen.push(0.62 + (x * 0.045 - time * 0.35).sin() * 0.16 + (x * 0.011 + time * 0.13).sin() * 0.12);
        }
        self.glints.clear();
        if !self.sand.is_empty() {
            let n = (self.sand.len() as f32 * (0.002 + s.treble * 0.012)) as usize;
            for _ in 0..n {
                self.glints.push((self.rng.next() * self.sand.len() as f32) as usize % self.sand.len());
            }
        }
    }
    fn draw(&self, r: &mut Raster, kit: &Kit) {
        let s = kit.sound;
        let time = kit.time;
        // Silica.
        for &(x, y, shade) in &self.sand {
            let k = shade * self.sheen.get(x as usize).copied().unwrap_or(0.6);
            let ch = if shade > 0.85 { '∙' } else { '·' };
            r.dot(x as i32, y as i32, 9., ch, scale(SAND, k));
        }
        for &i in &self.glints {
            let (x, y, _) = self.sand[i];
            r.dot(x as i32, y as i32, 8.9, '+', mix(SAND, WHITE, 0.7 + s.treble * 0.3));
        }
        // Copper: white hot when fresh, cooling to copper; music keeps it lit.
        let quiet = (kit.silence / 4.).min(1.);
        let live = 0.55 + s.bass * 0.4 + self.surge * 0.7 - quiet * 0.25;
        for p in &self.paths {
            let z = if p.base { 6. } else { 5. };
            let base_light = if p.base { live * 0.95 } else { live * 1.1 };
            let cool = if p.base { COPPER } else { FRESH };
            for (i, &(x, y)) in p.cells.iter().enumerate() {
                let (alive, crumbling) = p.state(i);
                if !alive {
                    continue;
                }
                if crumbling {
                    r.dot(x as i32, y as i32, z, '∴', scale(SAND, 0.9));
                    continue;
                }
                let heat = ((time - p.born[i]) * -0.45).exp().max(self.surge * 0.8);
                let tone = if heat > 0.45 {
                    mix(ORANGE, WHITE, (heat - 0.45) / 0.55)
                } else {
                    mix(cool, ORANGE, heat / 0.45)
                };
                let light = if heat > 0.3 { 1. } else { base_light };
                let ch = p.glyphs[i];
                let tone = if ch == '•' || ch == '◉' { mix(tone, GOLD, 0.5) } else { tone };
                r.dot(x as i32, y as i32, z, ch, scale(tone, light));
            }
            // Data: a bright head and a fading tail behind it.
            for &(pos, v) in &p.pulses {
                let head = pos as i32;
                let back = if v > 0. { -1 } else { 1 };
                for t in 0..6 {
                    let i = head + back * t;
                    if i < 0 || i as usize >= p.cells.len() || !p.state(i as usize).0 {
                        continue;
                    }
                    let (x, y) = p.cells[i as usize];
                    let k = 1. - t as f32 / 6.;
                    let ch = if t == 0 { '■' } else { p.glyphs[i as usize] };
                    r.dot(x as i32, y as i32, 4., ch, scale(mix(DATA, WHITE, k * 0.5), 0.5 + k * 0.6));
                }
            }
        }
        self.draw_chip(r, kit);
        for sp in &self.sparks {
            let (ch, tone) = match sp.kind {
                0 => (if sp.life > 0.35 { '*' } else { '·' }, mix(ORANGE, WHITE, (sp.life * 2.).min(1.))),
                1 => ('·', scale(SAND, 0.8)),
                _ => ('+', mix(DATA, WHITE, (sp.life * 1.5).min(1.))),
            };
            r.dot(sp.x.round() as i32, sp.y.round() as i32, 1.5, ch, tone);
        }
        // The laser heads.
        let mut heads = vec![];
        if let Some(p) = &self.pen {
            heads.push((p.at, 1.));
        }
        if let Some(g) = &self.ghost {
            heads.push((g.pen.at, 0.7));
        }
        // A reticle round the spot, as on a lithography head.
        for ((x, y), k) in heads {
            let flicker = 0.85 + (time * 47.).sin() * 0.15;
            r.dot(x, y, 1., '◉', scale(mix(LASER, WHITE, 0.6), k * flicker));
            let open = 2 + ((time * 6.).sin() * 0.6).round() as i32;
            let tone = scale(LASER, k * (0.75 + s.treble * 0.25));
            for (dx, dy, ch) in [(-open, -1, '┌'), (open, -1, '┐'), (-open, 1, '└'), (open, 1, '┘')] {
                r.dot(x + dx, y + dy, 1.1, ch, tone);
            }
            r.dot(x - 1, y, 1.1, '─', scale(LASER, k * 0.5));
            r.dot(x + 1, y, 1.1, '─', scale(LASER, k * 0.5));
        }
        // A charge burns a pad under the laser.
        if let (Some(c), true) = (kit.cursor, kit.charge > 0.) {
            let radius = (0.8 + kit.charge * 2.6) * kit.reach.max(0.8);
            let rx = radius * kit.aspect;
            for dy in -(radius.ceil() as i32)..=radius.ceil() as i32 {
                for dx in -(rx.ceil() as i32)..=rx.ceil() as i32 {
                    let d = ((dx as f32 / kit.aspect).powi(2) + (dy as f32).powi(2)).sqrt();
                    if d <= radius {
                        let hot = 0.6 + 0.4 * (time * 30. + d * 3.).sin();
                        r.dot(c.0 as i32 + dx, c.1 as i32 + dy, 1.2, '●', mix(ORANGE, WHITE, hot * kit.charge));
                    }
                }
            }
        }
    }
}
impl Foundry {
    fn draw_chip(&self, r: &mut Raster, kit: &Kit) {
        let c = &self.chip;
        let s = kit.sound;
        let (x0, x1, y0, y1) = (c.x0, c.x1, c.y0, c.y1);
        if x1 <= x0 || y1 <= y0 {
            c.sand.draw(r, 2., |_| Some(('█', GOLD)));
            return;
        }
        // Package, pins, and the dark die.
        let rim = scale((120, 124, 140), 0.8 + s.bass * 0.5 + self.surge * 0.5);
        for x in x0..=x1 {
            let (top, bottom) = if x == x0 {
                ('╔', '╚')
            } else if x == x1 {
                ('╗', '╝')
            } else {
                ('═', '═')
            };
            r.dot(x, y0, 3., top, rim);
            r.dot(x, y1, 3., bottom, rim);
        }
        for y in y0 + 1..y1 {
            r.dot(x0, y, 3., '║', rim);
            r.dot(x1, y, 3., '║', rim);
        }
        let pin = scale(GOLD, 0.8 + s.bass * 0.3);
        let mut x = x0 + 2;
        while x <= x1 - 2 {
            r.dot(x, y0 - 1, 3., '┃', pin);
            r.dot(x, y1 + 1, 3., '┃', pin);
            x += 3;
        }
        let mut y = y0 + 1;
        while y <= y1 - 1 {
            for dx in 1..=2 {
                r.dot(x0 - dx, y, 3., '━', pin);
                r.dot(x1 + dx, y, 3., '━', pin);
            }
            y += 2;
        }
        for yy in y0 + 1..y1 {
            for xx in x0 + 1..x1 {
                if hash((xx as u32).wrapping_mul(3301) ^ (yy as u32).wrapping_mul(7177)) < 0.12 {
                    r.dot(xx, yy, 3.5, '·', (22, 46, 42));
                }
            }
        }
        r.dot(x0 + 2, y0 + 1, 3., '◉', scale(GOLD, 0.6));
        // Once a second the etching light crosses the digits.
        let sweep = x0 as f32 + ((kit.time - self.tick).min(1.)) * (x1 - x0) as f32;
        let glow = 0.75 + s.bass * 0.35 + self.surge * 0.5;
        let mut i = 0;
        c.sand.draw(r, 2., |g| {
            let (edge, heat) = (c.bevel.get(i).copied().unwrap_or(0), c.heat.get(i).copied().unwrap_or(0.));
            i += 1;
            let near = (-((g.x - sweep) / 3.).powi(2)).exp();
            let metal = match edge {
                1 => mix(GOLD, WHITE, 0.35),
                -1 => scale(COPPER, 0.7),
                _ => mix(COPPER, GOLD, 0.55),
            };
            // Hot copper sand: white in the air, orange as it lands.
            let hot = if heat > 0.5 { mix(ORANGE, WHITE, (heat - 0.5) * 2.) } else { mix(metal, ORANGE, heat * 2.) };
            let tone = mix(hot, WHITE, near * 0.6 * (1. - heat));
            let away = g.away();
            let ch = if away < 0.5 {
                '█'
            } else if away < 2. || g.shade > 0.8 {
                '●'
            } else {
                '•'
            };
            Some((ch, scale(tone, (glow + near * 0.3).max(heat * 1.1) * (0.85 + g.shade * 0.2))))
        });
        if c.roomy {
            let cx = (x0 + x1) as f32 / 2.;
            super::stamp(r, kit, "OMARCHY SI-45", 1, (cx, y0 + 1, 2.5), scale((150, 160, 175), 0.8 + s.treble * 0.3));
            // The seconds as a bus loading, one lamp per second.
            let (bx0, bx1) = (x0 + 3, x1 - 3);
            let width = (bx1 - bx0).max(1);
            let lit = (kit.hms.2 as f32 / 59. * width as f32).round() as i32;
            for i in 0..=width {
                let on = i <= lit;
                let tone = if on { scale(DATA, 0.6 + s.treble * 0.4) } else { (30, 50, 60) };
                r.dot(bx0 + i, y1 - 1, 2.5, if on { '■' } else { '·' }, tone);
            }
        }
    }
}

#[cfg(test)]
pub(super) mod tests {
    use crate::interactive_matrix::Sound;
    use std::f32::consts::TAU;
    /// Plays `name` with a scripted cursor and writes PPM stills at `snaps`:
    /// scribble, rest, move off, circle, leave, then shake.
    pub(in super::super) fn script_stills(name: &str, snaps: &[f32]) {
        use crate::neon_scenes::{Scene, Style};
        use gtk4::cairo::{Context, Format, ImageSurface};
        let dir = std::path::PathBuf::from(std::env::var("POSTER_DIR").unwrap_or_else(|_| "stills".into()));
        std::fs::create_dir_all(&dir).unwrap();
        let (cols, rows) = (240usize, 68usize);
        let (cw, ch) = (8., 17.);
        let mut scene = Scene::new(Style::parse(name).unwrap());
        let mut cells = vec![];
        scene.draw(cols, rows, ch as f32 / cw as f32, &mut cells);
        let dt = 1. / 30.;
        let mut t = 0f32;
        for &snap in snaps {
            while t < snap {
                let beat = (t * TAU * 2.).sin().max(0.).powi(6);
                let quiet = std::env::var("QUIET").is_ok();
                let sound = if quiet { Sound::default() } else { Sound { bass: 0.25 + beat * 0.6, treble: 0.3 + 0.3 * (t * 7.).sin().abs() } };
                // SCRIPT=sweep crosses the digit row once and leaves.
                let sweep = std::env::var("SCRIPT").is_ok_and(|v| v == "sweep");
                let pointer = match t {
                    t if sweep => (t > 1. && t < 2.6).then(|| (0.2 + (t - 1.) * 0.4, 0.52)),
                    t if t < 4. => Some((0.08 + t * 0.05, 0.2 + (t * 3.).sin() * 0.12 + t * 0.04)),
                    t if t < 7. => Some((0.3, 0.45)),
                    t if t < 7.5 => Some((0.3 + (t - 7.) * 0.4, 0.45 + (t - 7.) * 0.3)),
                    t if t < 10. => Some((0.8 + (t * 7.).cos() * 0.08, 0.3 + (t * 7.).sin() * 0.12)),
                    t if t < 16. => None,
                    t if t < 17. => Some((0.5 + if ((t * 12.) as i32) % 2 == 0 { 0.1 } else { -0.1 }, 0.8)),
                    _ => None,
                };
                scene.step(dt, pointer, sound);
                scene.draw(cols, rows, ch as f32 / cw as f32, &mut cells);
                t += dt;
            }
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
            std::fs::write(dir.join(format!("script-{name}-{snap:04.1}s.ppm")), ppm).unwrap();
        }
    }
    #[test]
    #[ignore = "manual: writes PPM stills"]
    fn foundry_script_stills() {
        let snaps: Vec<f32> = std::env::var("SNAPS").ok().filter(|s| !s.is_empty()).unwrap_or_else(|| "4,7.2,10,15,17.3".into()).split(',').filter_map(|v| v.parse().ok()).collect();
        script_stills("sand-clock-foundry", &snaps);
    }
}
