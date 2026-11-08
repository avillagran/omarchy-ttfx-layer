//! Dusty glass: a pane in front of a neon city at night, the time up on a
//! billboard. Dust settles on the glass, faster with music, and the cursor
//! wipes it off like a cloth, leaving clumps at the edges of each stroke.
//! Frosted dust scatters light rather than hiding it, so the neon still
//! glows through a thick coat and the time stays readable; only the dark
//! city is lost. Bass shakes the pane and heavy dust runs down in streaks.
//! The billboard's digits are neon sand: wiped across, they blow off the sign
//! in front of the glass, drift, and pour back into place.
use super::super::{mix, scale, Color, Rng};
use super::excavation::Circling;
use super::{hash, ring, stamp, Field, Kit, Play, SandText};
use crate::interactive_tunnel::Raster;

const DUST: Color = (176, 160, 136);
const CYAN: Color = (80, 235, 255);
const PINK: Color = (255, 70, 200);
const AMBER: Color = (255, 185, 90);
const BODY: Color = (16, 15, 34);
/// An ordered dither: dust fills a cell at a time in a fixed pattern, so a
/// thickening coat reads as a smooth film instead of television static.
const BAYER: [f32; 16] = [0., 8., 2., 10., 12., 4., 14., 6., 3., 11., 1., 9., 15., 7., 13., 5.];

/// What a cell of the city shows. `glow` is how much light it sends
/// through dust: none for walls, all of it for neon.
#[derive(Clone, Copy)]
struct Px {
    ch: char,
    color: Color,
    glow: f32,
    kind: u8,
    id: u32,
}
const SKY: u8 = 0;
const WALL: u8 = 1;
const WINDOW: u8 = 2;
const NEON: u8 = 3;
const BEACON: u8 = 4;
const VOID: Px = Px { ch: '\0', color: (0, 0, 0), glow: 0., kind: SKY, id: 0 };

struct Car {
    x: f32,
    y: f32,
    v: f32,
    color: Color,
}
struct Flake {
    x: f32,
    y: f32,
    vy: f32,
    life: f32,
}

pub(super) struct Glass {
    rng: Rng,
    cols: usize,
    rows: usize,
    city: Vec<Px>,
    /// Billboard cells holding a digit grain at home, lit as neon.
    bill: Vec<char>,
    bill_cells: Vec<usize>,
    digits: SandText,
    placed: bool,
    board: (i32, i32, i32, i32),
    dust: Field,
    grain: Vec<f32>,
    /// Columns where heavy dust runs down when the pane shakes.
    drips: Vec<bool>,
    cars: Vec<Car>,
    flakes: Vec<Flake>,
    /// Rings of shine where a polished spot was left.
    shines: Vec<(f32, f32, f32, f32)>,
    moves: Vec<(usize, usize, f32)>,
    scratch: Option<Raster>,
    letters: Vec<crate::asm_direct_bridge::DirectCell>,
    circling: Circling,
    pulse: f32,
    tremor: f32,
    jolt: i32,
    phase: usize,
    time: f32,
}

impl Glass {
    pub(super) fn new() -> Self {
        Self {
            rng: Rng(rand::random()),
            cols: 0,
            rows: 0,
            city: vec![],
            bill: vec![],
            bill_cells: vec![],
            digits: SandText::new(true, true),
            placed: false,
            board: (0, 0, 0, 0),
            dust: Field::default(),
            grain: vec![],
            drips: vec![],
            cars: vec![],
            flakes: vec![],
            shines: vec![],
            moves: vec![],
            scratch: None,
            letters: vec![],
            circling: Circling::default(),
            pulse: 0.,
            tremor: 0.,
            jolt: 0,
            phase: 0,
            time: 0.,
        }
    }
    fn put(&mut self, x: i32, y: i32, px: Px) {
        if x >= 0 && y >= 0 && (x as usize) < self.cols && (y as usize) < self.rows {
            self.city[y as usize * self.cols + x as usize] = px;
        }
    }
    /// Draw pixel lettering off screen and keep the cells it covered.
    fn letter(&mut self, kit: &Kit, s: &str, k: usize, (cx, y): (f32, i32)) -> Vec<crate::asm_direct_bridge::DirectCell> {
        let mut r = match self.scratch.take() {
            Some(r) if r.fits(self.cols, self.rows) => r,
            _ => Raster::new(self.cols, self.rows),
        };
        stamp(&mut r, kit, s, k, (cx, y, 1.), (255, 255, 255));
        let mut out = std::mem::take(&mut self.letters);
        r.recycle(&mut out);
        self.scratch = Some(r);
        out
    }
    /// The skyline, once per grid: haze, far towers, near towers with their
    /// windows, neon signs, and the billboard tower in the middle.
    fn build(&mut self, kit: &Kit) {
        let (cols, rows) = (self.cols as i32, self.rows as i32);
        let s = (self.rows as f32 / 68.).max(0.4);
        self.city.clear();
        self.city.resize(self.cols * self.rows, VOID);
        for y in 0..rows {
            let t = y as f32 / rows.max(1) as f32;
            for x in 0..cols {
                let g = hash((y * 9973 + x * 31) as u32);
                let px = if t < 0.45 && g < 0.012 {
                    Px { ch: '·', color: scale((200, 200, 255), 0.4 + g * 30.), glow: 0.2, kind: SKY, id: 0 }
                } else if g < t * t * 0.9 {
                    // Haze thickening towards the street glow.
                    Px { ch: '░', color: mix((30, 12, 52), (120, 30, 95), t * t), glow: 0.15, kind: SKY, id: 0 }
                } else {
                    VOID
                };
                self.city[(y * cols + x) as usize] = px;
            }
        }
        let bw = ((cols as f32 * 0.5) as i32).max(3);
        let bh = ((rows as f32 * 0.3) as i32).max(3);
        self.board = ((cols - bw) / 2, ((rows as f32 * 0.08) as i32).max(0), bw, bh);
        let mut id = 1u32;
        // Far towers, then near ones over them.
        for layer in 0..2 {
            let mut x = -(self.rng.next() * 6.) as i32;
            while x < cols {
                let w = ((5. + self.rng.next() * 12.) * s * kit.aspect * 0.5).max(3.) as i32;
                let top = if layer == 0 {
                    rows as f32 * (0.3 + self.rng.next() * 0.3)
                } else {
                    rows as f32 * (0.45 + self.rng.next() * 0.35)
                } as i32;
                let lit = 0.35 + self.rng.next() * 0.4;
                let tint = [AMBER, CYAN, PINK, (255, 230, 170)][(self.rng.next() * 4.) as usize % 4];
                let (sx, sy) = ((2. * s).round().max(2.) as i32, (2. * s).round().max(2.) as i32);
                for y in top..rows {
                    for xx in x..x + w {
                        let edge = xx == x || xx == x + w - 1;
                        let window = !edge && (xx - x) % sx == 1 && (y - top) % sy == 1 && y < rows - 1;
                        let px = if window {
                            id += 1;
                            let on = hash(id) < lit;
                            let c = if layer == 0 { scale(tint, 0.45) } else { tint };
                            Px { ch: if layer == 0 { '▪' } else { '▮' }, color: c, glow: if on { 0.5 } else { 0. }, kind: WINDOW, id: id ^ ((!on as u32) << 31) }
                        } else {
                            Px { ch: '█', color: if layer == 0 { (20, 18, 38) } else { BODY }, glow: 0., kind: WALL, id: 0 }
                        };
                        self.put(xx, y, px);
                    }
                }
                if layer == 1 && self.rng.next() < 0.45 {
                    let c = if self.rng.next() < 0.5 { CYAN } else { PINK };
                    for xx in x..x + w {
                        self.put(xx, top, Px { ch: '▀', color: c, glow: 0.9, kind: NEON, id });
                    }
                    id += 1;
                }
                if self.rng.next() < 0.35 {
                    let ax = x + w / 2;
                    let tall = (2. + self.rng.next() * 3. * s) as i32;
                    for y in top - tall..top {
                        self.put(ax, y, Px { ch: '│', color: (70, 70, 90), glow: 0., kind: WALL, id: 0 });
                    }
                    self.put(ax, top - tall - 1, Px { ch: '•', color: (255, 50, 60), glow: 0.8, kind: BEACON, id });
                    id += 1;
                }
                x += w + (self.rng.next() * 3. * s) as i32;
            }
        }
        // Vertical neon signs on the near towers, in the pixel font.
        let words = ["BAR", "HOTEL", "RAMEN", "24H", "CYBER", "ARCADE", "NOODLE", "OMARCHY"];
        let k = ((self.rows as f32 / 90.).round() as usize).max(1);
        let (_, lh) = crate::scenes3d::cyber::type_size("A", k);
        let lw = 3 * k as i32;
        let (bx, by, bw, bh) = self.board;
        let mut placed: Vec<i32> = vec![];
        for _ in 0..(cols / 25).clamp(1, 9) {
            let word = words[(self.rng.next() * words.len() as f32) as usize % words.len()];
            let tall = word.len() as i32 * (lh as i32 + 1);
            let x = 2 + (self.rng.next() * (cols - lw - 4).max(1) as f32) as i32;
            let y = (rows as f32 * (0.4 + self.rng.next() * 0.2)) as i32;
            let over_board = x + lw >= bx - 2 && x <= bx + bw + 2 && y <= by + bh + 2;
            if y + tall >= rows - 1 || over_board || placed.iter().any(|&p| (p - x).abs() < lw + 6) {
                continue;
            }
            placed.push(x);
            let c = [PINK, CYAN, AMBER, (150, 255, 120)][(self.rng.next() * 4.) as usize % 4];
            let cells = {
                let mut all = vec![];
                for (i, ch) in word.chars().enumerate() {
                    let s = ch.to_string();
                    let got = self.letter(kit, &s, k, (x as f32 + lw as f32 / 2., y + i as i32 * (lh as i32 + 1)));
                    all.extend(got.iter().map(|c| (c.x, c.y, c.ch)));
                    self.letters = got;
                }
                all
            };
            for (cx, cy, ch) in cells {
                self.put(cx as i32, cy as i32, Px { ch, color: c, glow: 1., kind: NEON, id });
            }
            // A dark backing plate so the letters read against the haze.
            for yy in y - 1..y + tall {
                for xx in [x - 1, x + lw] {
                    self.put(xx, yy, Px { ch: '│', color: scale(c, 0.35), glow: 0.3, kind: NEON, id });
                }
            }
            id += 1;
        }
        // The billboard: a tower, a framed board.
        let (tx0, tx1) = (bx + bw / 2 - (bw / 6).max(1), bx + bw / 2 + (bw / 6).max(1));
        for y in by + bh..rows {
            for x in tx0..=tx1 {
                let px = if (x - tx0) % 3 == 1 && (y - by) % 2 == 0 && x < tx1 {
                    id += 1;
                    Px { ch: '▪', color: CYAN, glow: 0.5, kind: WINDOW, id }
                } else {
                    Px { ch: '█', color: BODY, glow: 0., kind: WALL, id: 0 }
                };
                self.put(x, y, px);
            }
        }
        for y in by..by + bh {
            for x in bx..bx + bw {
                let (l, r, t, b) = (x == bx, x == bx + bw - 1, y == by, y == by + bh - 1);
                let ch = match (l, r, t, b) {
                    (true, _, true, _) => '╔',
                    (_, true, true, _) => '╗',
                    (true, _, _, true) => '╚',
                    (_, true, _, true) => '╝',
                    (_, _, true, _) | (_, _, _, true) => '═',
                    (true, _, _, _) | (_, true, _, _) => '║',
                    _ => ' ',
                };
                let px = if ch == ' ' {
                    Px { ch: '█', color: (9, 6, 20), glow: 0., kind: WALL, id: 0 }
                } else {
                    Px { ch, color: PINK, glow: 0.9, kind: NEON, id: 0 }
                };
                self.put(x, y, px);
            }
        }
    }
    /// Light the board where digit grains sit at home; the ones blown off
    /// are drawn loose, in front of the glass.
    fn seat(&mut self) {
        for &i in &self.bill_cells {
            self.bill[i] = '\0';
        }
        self.bill_cells.clear();
        for g in &self.digits.grains {
            if g.away() >= 0.6 {
                continue;
            }
            let (x, y) = g.cell();
            if x >= 0 && y >= 0 && (x as usize) < self.cols && (y as usize) < self.rows {
                let i = y as usize * self.cols + x as usize;
                if self.bill[i] == '\0' {
                    self.bill[i] = '█';
                    self.bill_cells.push(i);
                }
            }
        }
    }
    fn landing(&self, kit: &Kit, x: usize, y: usize, n: (f32, f32), push: f32) -> usize {
        let tx = (x as f32 + n.0 * kit.aspect * push).round();
        let ty = (y as f32 + n.1 * push).round();
        if tx < 0. || ty < 0. || tx >= self.cols as f32 || ty >= self.rows as f32 {
            usize::MAX
        } else {
            ty as usize * self.cols + tx as usize
        }
    }
    fn apply_moves(&mut self) {
        for &(from, to, take) in &self.moves {
            self.dust.v[from] -= take;
            if to != usize::MAX {
                self.dust.v[to] = (self.dust.v[to] + take).min(1.8);
            }
        }
    }
    /// The cloth: dust along the stroke is pushed out to both edges.
    fn wipe(&mut self, kit: &Kit) {
        let Some(c) = kit.cursor else { return };
        let reach = kit.reach.max(0.8);
        let radius = (2.4 + kit.charge * 2.) * reach;
        let moved = kit.last.map_or(0., |l| kit.dist(l, c));
        let pass = (moved / radius).min(1.) * 0.9;
        // Resting polishes: past clean, to a shine that takes a while to dull.
        let polish = kit.dt * (0.6 + kit.charge * 2.5);
        let mut moves = std::mem::take(&mut self.moves);
        moves.clear();
        kit.sweep(radius, |x, y, w, n| {
            let i = y * self.cols + x;
            let d = self.dust.v[i];
            let lift = w.sqrt() * pass + w * polish;
            if d > 0. {
                let take = lift.min(d);
                let push = radius * w + 0.3 + self.grain[i] * 0.6;
                moves.push((i, self.landing(kit, x, y, n, push), take));
            } else if pass == 0. {
                moves.push((i, usize::MAX, (lift * 0.5).min(d + 0.6).max(0.)));
            }
        });
        self.moves = moves;
        self.apply_moves();
    }
}

impl Play for Glass {
    fn resize(&mut self, kit: &Kit) {
        self.cols = kit.cols;
        self.rows = kit.rows;
        let n = kit.cols * kit.rows;
        self.grain = (0..n).map(|i| hash(i as u32 ^ 0x51ed_270b)).collect();
        self.drips = (0..kit.cols).map(|x| hash(x as u32 ^ 0x2545_f491) < 0.3).collect();
        self.dust = Field::new(kit.cols, kit.rows, 0.);
        for (i, v) in self.dust.v.iter_mut().enumerate() {
            let (x, y) = ((i % kit.cols) as f32, (i / kit.cols) as f32);
            *v = 0.3 + super::super::noise(x * 0.04, y * 0.09) * 0.45 + self.grain[i] * 0.08;
        }
        #[cfg(test)]
        if std::env::var("GLASS_CLEAN").is_ok() {
            self.dust.v.fill(-0.6);
        }
        self.bill = vec!['\0'; n];
        self.bill_cells.clear();
        self.scratch = None;
        self.build(kit);
        let (bx, by, bw, bh) = self.board;
        self.placed = kit.cols >= 8 && kit.rows >= 4;
        self.digits = SandText::new(true, true);
        if self.placed {
            let (mx, my) = ((bw / 14).max(1) + 1, (bh / 6).max(1));
            self.digits.place(kit, (bx + mx, by + my, (bw - 2 * mx).max(1) as usize, (bh - 2 * my).max(1) as usize));
        }
        self.seat();
        self.flakes.clear();
        self.cars.clear();
        let lanes = (kit.rows / 14).clamp(1, 8);
        for i in 0..lanes {
            let y = kit.rows as f32 * (0.06 + 0.4 * self.rng.next());
            let v = (6. + self.rng.next() * 18.) * if i % 2 == 0 { 1. } else { -1. } * kit.reach.max(0.8);
            let color = if v > 0. { (255, 80, 70) } else { (230, 240, 255) };
            self.cars.push(Car { x: self.rng.next() * kit.cols as f32, y, v, color });
        }
    }

    fn step(&mut self, kit: &Kit) {
        if self.dust.v.len() != kit.cols * kit.rows {
            return;
        }
        let (dt, reach, s) = (kit.dt, kit.reach.max(0.8), kit.sound);
        self.time = kit.time;
        if self.placed {
            self.digits.step(kit);
            self.seat();
        }
        self.pulse = (self.pulse - dt * 4.).max(0.);
        if let Some(b) = kit.beat {
            self.pulse = self.pulse.max(b.min(1.));
            // The pane jumps a cell on a hard beat.
            if b > 0.6 {
                self.jolt = if self.rng.next() < 0.5 { 1 } else { -1 };
            }
        } else if self.rng.next() < dt * 12. {
            self.jolt = 0;
        }
        self.tremor = (self.tremor - dt).max(0.);
        if kit.drop {
            self.tremor = 1.4;
            self.pulse = 1.;
        }
        if self.tremor > 0. {
            self.jolt = ((self.rng.next() - 0.5) * 5. * self.tremor.min(1.)).round() as i32;
        }

        self.wipe(kit);
        self.shines.retain(|s| kit.time - s.2 < 1.);
        if let (Some((c, rate)), Some(at)) = (self.circling.read(kit), kit.cursor) {
            // Round and round: a clean ring where the cloth goes.
            let radius = kit.dist(c, at).clamp(2., self.rows as f32 * 0.45);
            let band = 1.8 * reach;
            let fade = (dt * (4. + rate.abs() * 2.)).min(1.);
            let rx = (radius + band) * kit.aspect;
            let (x0, x1) = ((c.0 - rx).max(0.) as usize, ((c.0 + rx).max(0.) as usize).min(self.cols - 1));
            let (y0, y1) = ((c.1 - radius - band).max(0.) as usize, ((c.1 + radius + band).max(0.) as usize).min(self.rows - 1));
            for y in y0..=y1 {
                for x in x0..=x1 {
                    let d = kit.dist((x as f32, y as f32), c);
                    if d < radius + band {
                        let v = &mut self.dust.v[y * self.cols + x];
                        *v -= v.max(0.) * fade * if d > radius - band { 1. } else { 0.4 };
                    }
                }
            }
        }
        if kit.shake {
            // Shaken off: most of the coat falls away in flakes.
            for i in 0..self.dust.v.len() {
                let v = &mut self.dust.v[i];
                if *v > 0.3 && self.grain[i] < 0.02 && self.flakes.len() < 4000 {
                    self.flakes.push(Flake { x: (i % self.cols) as f32, y: (i / self.cols) as f32, vy: 0., life: 1.5 });
                }
                if *v > 0. {
                    *v *= 0.3;
                }
            }
        }

        // Dust settles, a quarter of the rows a frame; heavy dust runs down
        // when the bass shakes the pane.
        let settle = (1. / 120. + (s.bass + s.treble) / 45.) * dt * 4.;
        let shake = if self.tremor > 0. { 1.5 } else { ((s.bass - 0.45) / 0.55).max(0.) };
        let run = shake * dt * 4. * 1.5;
        let phase = self.phase % 4;
        self.phase += 1;
        let (w, h) = (self.cols, self.rows);
        // Bottom up, so a streak moves one row a pass rather than all the way.
        let count = if h > phase { (h - phase).div_ceil(4) } else { 0 };
        for k in (0..count).rev() {
            let y = phase + k * 4;
            for x in 0..w {
                let i = y * w + x;
                let g = self.grain[i];
                let v = self.dust.v[i];
                if v < 1. {
                    self.dust.v[i] = (v + settle * (0.4 + g * 1.2)).min(1.);
                }
                // Only some columns run, so the dust leaves streaks, not a curtain.
                if run > 0. && v > 0.85 && y + 1 < h && self.drips[x] {
                    let m = ((v - 0.65) * run * (0.5 + g)).min(v - 0.65);
                    let below = i + w + if g < 0.15 && x + 1 < w { 1 } else { 0 };
                    self.dust.v[i] -= m;
                    self.dust.v[below] = (self.dust.v[below] + m).min(1.8);
                }
            }
        }
        for f in &mut self.flakes {
            f.vy += 40. * reach * dt;
            f.y += f.vy * dt;
            f.life -= dt;
        }
        let rows = self.rows as f32;
        self.flakes.retain(|f| f.life > 0. && f.y < rows);
        let cols = self.cols as f32;
        for c in &mut self.cars {
            c.x += c.v * dt * (1. + s.bass * 0.8);
            if c.x > cols + 4. {
                c.x = -4.;
            } else if c.x < -4. {
                c.x = cols + 4.;
            }
        }
    }

    fn draw(&self, r: &mut Raster, kit: &Kit) {
        let (cols, rows) = (self.cols, self.rows);
        if cols == 0 || self.city.len() != cols * rows || kit.cols != cols || kit.rows != rows {
            return;
        }
        let s = kit.sound;
        let time = kit.time;
        // Windows come and go every several seconds; treble flickers the neon.
        let epoch = time * 0.12;
        let flick = (time * 23.) as u32;
        let (bx, _, bw, _) = self.board;
        let bright = 1. + self.pulse * 0.6;
        let jolt = self.jolt;
        let v = &self.dust.v;
        for y in 0..rows {
            let row = y * cols;
            for x in 0..cols {
                let i = row + x;
                // The dust rides on the pane, which jolts; the city does not.
                let dx = (x as i32 + jolt).clamp(0, cols as i32 - 1) as usize;
                let d = v[row + dx];
                let mut px = self.city[i];
                let letter = self.bill[i];
                if letter != '\0' {
                    let t = (x as f32 - bx as f32) / bw.max(1) as f32;
                    px = Px { ch: letter, color: scale(mix(CYAN, PINK, t), bright), glow: 1., kind: NEON, id: 0 };
                } else {
                    match px.kind {
                        WINDOW => {
                            let off = px.id >> 31 == 1;
                            let id = px.id & 0x7fff_ffff;
                            let turn = (epoch + (id & 63) as f32 * 0.14) as u32;
                            let flip = hash(id.wrapping_mul(0x9e37_79b9) ^ turn.wrapping_mul(2_654_435_761)) < 0.25;
                            if off != flip {
                                px.color = (28, 28, 44);
                                px.glow = 0.;
                            }
                        }
                        NEON if px.id != 0 => {
                            if hash(px.id ^ flick.wrapping_mul(97)) < s.treble * 0.12 {
                                px.color = scale(px.color, 0.25);
                                px.glow = 0.2;
                            } else if px.id % 5 == 0 {
                                px.color = scale(px.color, bright.min(1.3));
                            }
                        }
                        NEON => px.color = scale(px.color, 0.8 + self.pulse * 0.5),
                        BEACON => {
                            if (time * 2.2 + px.id as f32).sin() < 0.3 {
                                px.color = (60, 10, 14);
                                px.glow = 0.;
                            }
                        }
                        _ => {}
                    }
                }
                let (xi, yi) = (x as i32, y as i32);
                if d <= 0.06 {
                    if px.ch != '\0' {
                        let polished = (-d).clamp(0., 0.6);
                        r.dot(xi, yi, 6., px.ch, scale(px.color, 1. + polished * 0.3));
                    }
                    // Polished glass catches a glint now and then.
                    let g = self.grain[i];
                    if d < -0.05 && g < 0.003 + s.treble * 0.004 && (time * 5. + g * 900.).sin() > 0.5 {
                        r.dot(xi, yi, 5., '+', (235, 245, 255));
                    }
                    continue;
                }
                let dd = d.min(1.);
                let g = (BAYER[(y & 3) * 4 + (x & 3)] + 0.5) / 16. * 0.85 + self.grain[i] * 0.15;
                if g < dd * 0.82 {
                    let ch = if d > 1.15 {
                        '▓'
                    } else if d > 0.82 {
                        '▒'
                    } else if d > 0.38 {
                        '░'
                    } else {
                        '·'
                    };
                    // Frosted dust lit from behind takes on the light it scatters.
                    let film = scale(DUST, 0.2 + 0.2 * dd + (d - 1.).max(0.) * 0.45);
                    let lit = if px.ch != '\0' { px.glow } else { 0. };
                    let color = mix(film, scale(px.color, 0.9), lit * (0.85 - dd * 0.15));
                    r.dot(xi, yi, 5., ch, color);
                } else if px.ch != '\0' {
                    let keep = 1. - 0.8 * dd * (1. - px.glow * 0.5);
                    r.dot(xi, yi, 6., px.ch, scale(px.color, keep));
                }
            }
        }
        for c in &self.cars {
            let (x, y) = (c.x.round() as i32, c.y.round() as i32);
            if x < 0 || y < 0 || x as usize >= cols || y as usize >= rows {
                continue;
            }
            let d = v[y as usize * cols + x as usize].clamp(0., 1.);
            let k = 1. - d * 0.7;
            r.dot(x, y, 5.5, '▬', scale((150, 150, 175), k));
            let back = if c.v > 0. { -1 } else { 1 };
            for t in 1..4 {
                r.dot(x + back * t, y, 5.6, '·', scale(c.color, k * (1. - t as f32 * 0.25)));
            }
        }
        for f in &self.flakes {
            r.dot(f.x as i32, f.y as i32, 2., '·', scale(DUST, 0.3 + f.life * 0.3));
        }
        for &(x, y, born, k) in &self.shines {
            let age = time - born;
            let light = (1. - age).max(0.);
            ring(r, kit, (x, y), (2.5 + 4. * k) * kit.reach.max(0.8) * (0.6 + age), age * 3., 1., '•', scale((215, 240, 255), light));
        }
        // Neon sand blown off the sign rides in front of the dusty pane.
        let span = bw.max(1) as f32;
        self.digits.draw(r, 0.8, |g| {
            if g.away() < 0.6 {
                return None;
            }
            let tone = mix(CYAN, PINK, (g.home.0 - bx as f32) / span);
            let glint = (time * 6. + g.phase).sin().max(0.).powi(4);
            let ch = if glint > 0.5 { '●' } else { '•' };
            Some((ch, scale(mix(tone, (255, 255, 255), glint * 0.45), (1. + glint * 0.3) * bright.min(1.3))))
        });
        if let (Some(c), true) = (kit.cursor, kit.speed > 2.) {
            // The cloth: a soft pale edge round the stroke.
            let radius = (2.4 + kit.charge * 2.) * kit.reach.max(0.8);
            ring(r, kit, c, radius, time, 1.5, '·', (95, 95, 115));
        }
    }
}
