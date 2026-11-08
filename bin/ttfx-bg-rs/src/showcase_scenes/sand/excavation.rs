//! Digital archaeology: a desert floor over the buried relics of old
//! computing. The cursor is a brush that sweeps the sand aside into little
//! ridges; a relic laid mostly bare is framed and named, as Neon City frames
//! its towers. The time lies in an engraving on a stone tablet in the middle:
//! glowing amber grains that glow through the sand however deep the drift.
//! Brushed across, they scatter over the floor and pour back into the groove. Bass winds
//! bring the sand back; every beat pings a sonar ring out of the tablet that
//! shows, as it passes, what still lies buried.
use super::super::{mix, noise, scale, Color, Rng};
use super::{hash, stamp, Field, Kit, Play, SandText};
use crate::interactive_tunnel::Raster;
use std::f32::consts::TAU;

/// A relic as pixel art: one letter per pixel, '.' clear, and its inks.
struct Art {
    name: &'static str,
    px: &'static [&'static str],
    ink: &'static [(char, Color)],
}
const ARTS: [Art; 8] = [
    Art {
        name: "FLOPPY DISK",
        px: &[
            "BBBBBBBBBBBBBBB.",
            "BBBSSSSSSSSSBBBB",
            "BBBSSSKKSSSSBBBB",
            "BBBSSSKKSSSSBBBB",
            "BBBSSSSSSSSSBBBB",
            "BBBBBBBBBBBBBBBB",
            "BBWWWWWWWWWWWWBB",
            "BBWRRRRRRRRRRWBB",
            "BBWWWWWWWWWWWWBB",
            "BBWKKKKKKKKKWWBB",
            "BBWWWWWWWWWWWWBB",
            "BBWWWWWWWWWWWWBB",
        ],
        ink: &[('B', (45, 80, 175)), ('S', (175, 180, 190)), ('K', (30, 30, 42)), ('W', (230, 225, 210)), ('R', (225, 65, 65))],
    },
    Art {
        name: "CPU",
        px: &[
            ".G.G.G.G.G.G.G.",
            "GPPPPPPPPPPPPPG",
            ".PPPPPPPPPPPPP.",
            "GPPDDDDDDDDDPPG",
            ".PPDDDHHDDDDPP.",
            "GPPDDDDDDDDDPPG",
            ".PPDDDDDDDDDPP.",
            "GPPDDDDDDDDDPPG",
            ".PPPPPPPPPPPPP.",
            "GPLPPPPPPPPPPPG",
            ".G.G.G.G.G.G.G.",
        ],
        ink: &[('G', (230, 185, 70)), ('P', (62, 64, 74)), ('D', (150, 156, 172)), ('H', (215, 220, 235)), ('L', (230, 185, 70))],
    },
    Art {
        name: "CARTRIDGE",
        px: &[
            "..CCCCCCCCCCCC..",
            ".CCCCCCCCCCCCCC.",
            ".CCLLLLLLLLLLCC.",
            ".CCLYYYYYYYYLCC.",
            ".CCLLLLLLLLLLCC.",
            ".CCLYYYYYLLLLCC.",
            ".CCLLLLLLLLLLCC.",
            ".CCCCCCCCCCCCCC.",
            ".CCKCKCKCKCKCCC.",
            ".CCCCCCCCCCCCCC.",
            "..CC........CC..",
        ],
        ink: &[('C', (125, 125, 138)), ('L', (230, 70, 95)), ('Y', (250, 220, 95)), ('K', (40, 40, 48))],
    },
    Art {
        name: "ROBOT SKULL",
        px: &[
            "....MMMMMMMM....",
            "..MMMMMMMMMMMM..",
            ".MMMMMMMMMMMMMM.",
            ".MMDDDMMMMDDDMM.",
            ".MMDRDMMMMDRDMM.",
            ".MMDDDMMMMDDDMM.",
            ".MMMMMMDDMMMMMM.",
            "..MMMMMMMMMMMM..",
            "...MTDTDTDTDM...",
            "...MTDTDTDTDM...",
            "....MMMMMMMM....",
        ],
        ink: &[('M', (178, 184, 200)), ('D', (36, 36, 48)), ('R', (255, 55, 65)), ('T', (215, 215, 225))],
    },
    Art {
        name: "CRT MONITOR",
        px: &[
            "EEEEEEEEEEEEEEE",
            "EKKKKKKKKKKKKKE",
            "EKSSSSSSSSSSSKE",
            "EKSGGGSSSSSSSKE",
            "EKSSSSSSSSSSSKE",
            "EKSGGGGGSSSSSKE",
            "EKSSSSSSSSSSSKE",
            "EKKKKKKKKKKKKKE",
            "EEEEEEEEEEEEEEE",
            "....EEEEEEE....",
            "..EEEEEEEEEEE..",
        ],
        ink: &[('E', (195, 185, 155)), ('K', (40, 40, 40)), ('S', (25, 95, 60)), ('G', (90, 255, 150))],
    },
    Art {
        name: "KEYBOARD",
        px: &[
            "CCCCCCCCCCCCCCCC",
            "CKCKCKCKCKCKCKAC",
            "CCCCCCCCCCCCCCCC",
            "CKCKCKCKCKCKCKKC",
            "CCCCCCCCCCCCCCCC",
            "CAKCKCKCKCKCKCKC",
            "CCCCCCCCCCCCCCCC",
            "CKCKAAAAAAAKCKCC",
            "CCCCCCCCCCCCCCCC",
        ],
        ink: &[('C', (60, 60, 72)), ('K', (205, 205, 215)), ('A', (255, 145, 60))],
    },
    Art {
        name: "CASSETTE",
        px: &[
            "CCCCCCCCCCCCCCCC",
            "CLLLLLLLLLLLLLLC",
            "CLLWWWWWWWWWWLLC",
            "CLLWKKWWWWKKWLLC",
            "CLLWKKWWWWKKWLLC",
            "CLLWWWWWWWWWWLLC",
            "CLLLLLLLLLLLLLLC",
            "CCCCCCCCCCCCCCCC",
            "CCCKCCCCCCCCKCCC",
            "..CCCCCCCCCCCC..",
        ],
        ink: &[('C', (52, 52, 64)), ('L', (240, 200, 80)), ('W', (150, 160, 172)), ('K', (30, 30, 36))],
    },
    Art {
        name: "JOYSTICK",
        px: &[
            "......RRR......",
            ".....RRRRR.....",
            ".....RRRRR.....",
            "......RRR......",
            ".......S.......",
            ".......S.......",
            ".......S.......",
            "..KKKKKKKKKKK..",
            ".KKYKKKKKKKKKK.",
            ".KKKKKKKKKKKKK.",
            "..KKKKKKKKKKK..",
        ],
        ink: &[('K', (44, 44, 54)), ('S', (165, 165, 178)), ('R', (232, 45, 55)), ('Y', (250, 205, 45))],
    },
];

const SAND: Color = (198, 158, 98);
const RIDGE: Color = (245, 208, 145);
const BEDROCK: Color = (88, 64, 46);
const GLOW: Color = (255, 190, 85);
const GLOW_HOT: Color = (255, 238, 175);
const SONAR: Color = (70, 225, 255);
const ACCENT: Color = (120, 235, 255);

/// The kit's swirl, kept only while the cursor really goes round: a
/// back and forth brush stroke turns about its own middle too, but its
/// distance from that middle swings from nothing to the stroke's length.
#[derive(Default)]
pub(super) struct Circling {
    recent: std::collections::VecDeque<(f32, (f32, f32))>,
}
impl Circling {
    pub(super) fn read(&mut self, kit: &Kit) -> Option<((f32, f32), f32)> {
        match kit.cursor {
            Some(c) => self.recent.push_back((kit.time, c)),
            None => self.recent.clear(),
        }
        while self.recent.front().is_some_and(|&(t, _)| kit.time - t > 0.8) {
            self.recent.pop_front();
        }
        let (centre, rate) = kit.swirl?;
        let (mut lo, mut hi, mut sum) = (f32::MAX, 0f32, 0.);
        for &(_, p) in &self.recent {
            let d = kit.dist(p, centre);
            lo = lo.min(d);
            hi = hi.max(d);
            sum += d;
        }
        let mean = sum / self.recent.len().max(1) as f32;
        (lo > mean * 0.5 && hi < mean * 1.6).then_some((centre, rate))
    }
}

/// What lies under the sand in one cell.
#[derive(Clone, Copy)]
struct Bed {
    ch: char,
    color: Color,
    owner: u8,
}
const NOBODY: u8 = 255;
const TABLET: u8 = 254;
const EMPTY: Bed = Bed { ch: '\0', color: (0, 0, 0), owner: NOBODY };

struct Relic {
    art: usize,
    /// Top left cell and size, in cells.
    x: i32,
    y: i32,
    w: i32,
    h: i32,
    /// Share of its cells laid bare, surveyed a few times a second.
    bare: f32,
    found: bool,
    /// The focus fades in and out with what is visible.
    lit: f32,
}
/// A grain in the air: wind, or a puff from the brush.
struct Mote {
    x: f32,
    y: f32,
    vx: f32,
    vy: f32,
    life: f32,
    span: f32,
}

pub(super) struct Excavation {
    rng: Rng,
    cols: usize,
    rows: usize,
    /// Sand depth: 1 is a full cover, more is a ridge.
    cover: Field,
    bed: Vec<Bed>,
    grain: Vec<f32>,
    ripple: Vec<f32>,
    /// Each cell's distance from the tablet centre, in rows, for the sonar.
    radial: Vec<f32>,
    /// The engraving: cells where a digit grain is at home.
    etched: Vec<bool>,
    etched_cells: Vec<usize>,
    digits: SandText,
    placed: bool,
    tablet: (i32, i32, i32, i32),
    relics: Vec<Relic>,
    motes: Vec<Mote>,
    /// Birth times of the sonar rings.
    pings: Vec<f32>,
    moves: Vec<(usize, usize, f32)>,
    scratch: Vec<f32>,
    circling: Circling,
    gust: f32,
    boost: f32,
    /// Seconds left of a drop's full sonar sweep.
    reveal: f32,
    /// Seconds left of the storm that buries a new dig site.
    storm: f32,
    cleared: f32,
    survey: f32,
    drift: f32,
    spawn: f32,
    phase: usize,
    time: f32,
}

impl Excavation {
    pub(super) fn new() -> Self {
        Self {
            rng: Rng(rand::random()),
            cols: 0,
            rows: 0,
            cover: Field::default(),
            bed: vec![],
            grain: vec![],
            ripple: vec![],
            radial: vec![],
            etched: vec![],
            etched_cells: vec![],
            digits: SandText::new(true, true),
            placed: false,
            tablet: (0, 0, 0, 0),
            relics: vec![],
            motes: vec![],
            pings: vec![],
            moves: vec![],
            scratch: vec![],
            circling: Circling::default(),
            gust: 0.,
            boost: 0.,
            reveal: 0.,
            storm: 0.,
            cleared: 0.,
            survey: 0.,
            drift: 0.,
            spawn: 0.,
            phase: 0,
            time: 0.,
        }
    }
    /// Pixel art is drawn in half blocks: a pixel is `k` columns by `k` half rows.
    fn pixel(&self) -> i32 {
        ((self.rows as f32 / 40.).round() as i32).max(1)
    }
    fn art_size(&self, art: &Art) -> (i32, i32) {
        let k = self.pixel();
        let w = art.px.iter().map(|r| r.len()).max().unwrap_or(0) as i32;
        let h = art.px.len() as i32;
        (w * k, (h * k + 1) / 2)
    }
    /// Clear the bed, lay the tablet and bury a new set at random places.
    fn bury(&mut self, kit: &Kit) {
        let (cols, rows) = (self.cols as i32, self.rows as i32);
        self.bed.clear();
        self.bed.resize(self.cols * self.rows, EMPTY);
        let (tx, ty, tw, th) = self.tablet;
        for y in ty.max(0)..(ty + th).min(rows) {
            for x in tx.max(0)..(tx + tw).min(cols) {
                let edge = x == tx || x == tx + tw - 1 || y == ty || y == ty + th - 1;
                let g = hash((x * 7919 + y * 104729) as u32);
                let (ch, color) = if edge {
                    ('█', scale((132, 126, 116), 0.8 + g * 0.3))
                } else if g < 0.06 {
                    ('╱', (64, 60, 56))
                } else {
                    ('░', scale((96, 92, 86), 0.75 + g * 0.4))
                };
                self.bed[(y * cols + x) as usize] = Bed { ch, color, owner: TABLET };
            }
        }
        self.relics.clear();
        let mut order: Vec<usize> = (0..ARTS.len()).collect();
        for i in (1..order.len()).rev() {
            let j = (self.rng.next() * (i + 1) as f32) as usize;
            order.swap(i, j.min(i));
        }
        let want = 6;
        let (gx, gy) = ((3. * kit.aspect) as i32, 3);
        for &art in &order {
            if self.relics.len() >= want {
                break;
            }
            let (w, h) = self.art_size(&ARTS[art]);
            if w + 4 >= cols || h + 4 >= rows {
                continue;
            }
            for _ in 0..80 {
                let x = 2 + (self.rng.next() * (cols - w - 4) as f32) as i32;
                let y = 2 + (self.rng.next() * (rows - h - 4) as f32) as i32;
                let clear = |ax: i32, ay: i32, aw: i32, ah: i32| x + w + gx <= ax || ax + aw + gx <= x || y + h + gy <= ay || ay + ah + gy <= y;
                if !clear(tx, ty, tw, th) || !self.relics.iter().all(|r| clear(r.x, r.y, r.w, r.h)) {
                    continue;
                }
                self.relics.push(Relic { art, x, y, w, h, bare: 0., found: false, lit: 0. });
                break;
            }
        }
        let k = self.pixel();
        for (id, relic) in self.relics.iter().enumerate() {
            let art = &ARTS[relic.art];
            let ink = |c: u8| art.ink.iter().find(|i| i.0 as u8 == c).map(|i| i.1);
            let px = |row: i32, col: i32| -> Option<Color> {
                let line = art.px.get(row as usize)?.as_bytes();
                ink(*line.get(col as usize)?)
            };
            for cy in 0..relic.h {
                for cx in 0..relic.w {
                    let (x, y) = (relic.x + cx, relic.y + cy);
                    if x < 0 || y < 0 || x >= cols || y >= rows {
                        continue;
                    }
                    let col = cx / k;
                    let (top, bottom) = (px(cy * 2 / k, col), px((cy * 2 + 1) / k, col));
                    let (ch, color) = match (top, bottom) {
                        (Some(t), Some(_)) => ('█', t),
                        (Some(t), None) => ('▀', t),
                        (None, Some(b)) => ('▄', b),
                        _ => continue,
                    };
                    self.bed[(y * cols + x) as usize] = Bed { ch, color, owner: id as u8 };
                }
            }
        }
        for (i, v) in self.cover.v.iter_mut().enumerate() {
            let tablet = self.bed[i].owner == TABLET;
            *v = if tablet { 0.3 } else { 0.92 + self.grain[i] * 0.16 };
        }
        self.cleared = 0.;
    }
    /// Cut the groove where the digit grains belong.
    fn etch(&mut self) {
        for &i in &self.etched_cells {
            self.etched[i] = false;
        }
        self.etched_cells.clear();
        for g in &self.digits.grains {
            let (x, y) = (g.home.0.round() as i32, g.home.1.round() as i32);
            if x >= 0 && y >= 0 && (x as usize) < self.cols && (y as usize) < self.rows {
                let i = y as usize * self.cols + x as usize;
                if !self.etched[i] {
                    self.etched[i] = true;
                    self.etched_cells.push(i);
                }
            }
        }
    }
    /// Sand pushed off `from` lands `push` rows further along `n`, past the rim.
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
            self.cover.v[from] -= take;
            if to != usize::MAX {
                self.cover.v[to] = (self.cover.v[to] + take).min(2.4);
            }
        }
    }
    /// The brush: sand along the cursor's path is swept to both sides.
    fn brush(&mut self, kit: &Kit) {
        let Some(c) = kit.cursor else { return };
        let reach = kit.reach.max(0.8);
        let radius = (2. + kit.charge * 2.5) * reach;
        // A cell stays under the brush for about its width of travel: taking
        // in step with the distance moved clears the same per pass at any
        // speed. Resting digs on, harder when charged.
        let moved = kit.last.map_or(0., |l| kit.dist(l, c));
        let pass = (moved / radius).min(1.) * 0.6;
        let rate = (1.1 + kit.charge * 3.5) * kit.dt;
        let mut moves = std::mem::take(&mut self.moves);
        moves.clear();
        kit.sweep(radius, |x, y, w, n| {
            let i = y * self.cols + x;
            let d = self.cover.v[i];
            if d <= 0.01 {
                return;
            }
            let take = (w.sqrt() * pass + w * rate).min(d);
            let push = radius * w + 0.4 + self.grain[i] * 0.7;
            moves.push((i, self.landing(kit, x, y, n, push), take));
        });
        self.moves = moves;
        self.apply_moves();
        let (lo, hi) = match kit.last {
            Some(l) => ((l.0.min(c.0), l.1.min(c.1)), (l.0.max(c.0), l.1.max(c.1))),
            None => (c, c),
        };
        let pad = radius * 2.2;
        self.soften((lo.0 - pad * kit.aspect, lo.1 - pad), (hi.0 + pad * kit.aspect, hi.1 + pad), 0.22);
    }
    /// Scoop a crater: sand inside `radius` rows of `c` lands round its rim.
    fn scoop(&mut self, kit: &Kit, c: (f32, f32), radius: f32, strength: f32) {
        let mut moves = std::mem::take(&mut self.moves);
        moves.clear();
        let rx = radius * kit.aspect;
        let (x0, x1) = ((c.0 - rx).max(0.) as usize, ((c.0 + rx).max(0.) as usize).min(self.cols.saturating_sub(1)));
        let (y0, y1) = ((c.1 - radius).max(0.) as usize, ((c.1 + radius).max(0.) as usize).min(self.rows.saturating_sub(1)));
        for y in y0..=y1 {
            for x in x0..=x1 {
                let (dx, dy) = ((x as f32 - c.0) / kit.aspect, y as f32 - c.1);
                let d = (dx * dx + dy * dy).sqrt();
                if d >= radius {
                    continue;
                }
                let i = y * self.cols + x;
                let take = (self.cover.v[i] * strength * (0.4 + 0.6 * (1. - d / radius))).max(0.);
                if take <= 0.001 {
                    continue;
                }
                let n = if d > 0.01 { (dx / d, dy / d) } else { (0., -1.) };
                let push = radius - d + 0.3 + self.grain[i] * 0.8;
                moves.push((i, self.landing(kit, x, y, n, push), take));
            }
        }
        self.moves = moves;
        self.apply_moves();
        let pad = radius + 2.;
        self.soften((c.0 - pad * kit.aspect, c.1 - pad), (c.0 + pad * kit.aspect, c.1 + pad), 0.2);
    }
    /// Fresh ridges slump into soft banks: peaks ease towards their
    /// neighbours, troughs are left alone so the brushed track stays open.
    fn soften(&mut self, lo: (f32, f32), hi: (f32, f32), k: f32) {
        let (w, h) = (self.cols, self.rows);
        if w < 3 || h < 3 {
            return;
        }
        let x0 = (lo.0.max(1.) as usize).min(w - 2);
        let x1 = (hi.0.max(1.) as usize).min(w - 2);
        let y0 = (lo.1.max(1.) as usize).min(h - 2);
        let y1 = (hi.1.max(1.) as usize).min(h - 2);
        if x0 > x1 || y0 > y1 {
            return;
        }
        let bw = x1 - x0 + 3;
        self.scratch.clear();
        for y in y0 - 1..=y1 + 1 {
            self.scratch.extend_from_slice(&self.cover.v[y * w + x0 - 1..y * w + x1 + 2]);
        }
        let s = &self.scratch;
        for y in y0..=y1 {
            for x in x0..=x1 {
                let (bx, by) = (x - x0 + 1, y - y0 + 1);
                let at = |dx: isize, dy: isize| s[(by as isize + dy) as usize * bw + (bx as isize + dx) as usize];
                let mean = (at(-1, 0) + at(1, 0) + at(0, -1) + at(0, 1)) * 0.25;
                let v = at(0, 0);
                if mean < v {
                    self.cover.v[y * w + x] = v + (mean - v) * k;
                }
            }
        }
    }
}

impl Play for Excavation {
    fn resize(&mut self, kit: &Kit) {
        let (cols, rows) = (kit.cols, kit.rows);
        self.cols = cols;
        self.rows = rows;
        let n = cols * rows;
        self.cover = Field::new(cols, rows, 1.);
        self.grain = (0..n).map(|i| hash(i as u32 ^ 0x9e37_79b9)).collect();
        let tw = ((cols as f32 * 0.58) as i32).max(1);
        let th = ((rows as f32 * 0.32) as i32).max(1);
        self.tablet = ((cols as i32 - tw) / 2, (rows as i32 - th) / 2, tw, th);
        let (cx, cy) = (cols as f32 * 0.5, rows as f32 * 0.5);
        self.radial.clear();
        self.ripple.clear();
        for y in 0..rows {
            for x in 0..cols {
                let u = x as f32 / kit.aspect;
                let (dx, dy) = (u - cx / kit.aspect, y as f32 - cy);
                self.radial.push((dx * dx + dy * dy).sqrt());
                // Wind ripples: long wavering crests a few rows apart, wide
                // enough apart that the grid cannot alias them into a check.
                let bend = noise(u * 0.05, y as f32 * 0.08) * 6.;
                let w = ((u * 0.3 + y as f32 * 0.95) * TAU / 4.2 + bend).sin();
                self.ripple.push(w * 0.5 + 0.5);
            }
        }
        self.etched = vec![false; n];
        self.etched_cells.clear();
        let (tx, ty, tw, th) = self.tablet;
        let (mx, my) = ((tw as f32 * 0.06) as i32 + 1, (th as f32 * 0.14) as i32 + 1);
        self.placed = cols >= 8 && rows >= 4;
        self.digits = SandText::new(true, true);
        if self.placed {
            self.digits.place(kit, (tx + mx, ty + my, (tw - 2 * mx).max(1) as usize, (th - 2 * my).max(1) as usize));
        }
        self.etch();
        self.motes.clear();
        self.bury(kit);
    }

    fn step(&mut self, kit: &Kit) {
        if self.cover.v.len() != kit.cols * kit.rows {
            return;
        }
        let (dt, reach) = (kit.dt, kit.reach.max(0.8));
        self.time = kit.time;
        if self.placed {
            self.digits.step(kit);
            if kit.second {
                self.etch();
            }
        }
        let s = kit.sound;
        self.gust += (s.bass - self.gust) * (1. - (-dt * 3.).exp());
        self.boost = (self.boost - dt * 0.6).max(0.);
        self.reveal = (self.reveal - dt).max(0.);
        self.drift += dt * (0.25 + self.gust * 5. + self.boost * 4.);
        if kit.shake {
            self.boost = 1.4;
        }
        // A ring on every beat would keep the whole site lit: one at most
        // every bar or so, so the buried stays a glimpse.
        if kit.beat.is_some() && self.pings.last().map_or(true, |&t| kit.time - t > 1.7) {
            self.pings.push(kit.time);
        }
        if kit.drop {
            self.reveal = 2.8;
            self.pings.push(kit.time);
        }
        let life = 2.4;
        self.pings.retain(|&t| kit.time - t < life);

        self.brush(kit);
        let swirl = self.circling.read(kit);
        if let (Some((c, rate)), Some(at)) = (swirl, kit.cursor) {
            let radius = kit.dist(c, at).clamp(2., self.rows as f32 * 0.4);
            self.scoop(kit, c, radius, (dt * (2. + rate.abs())).min(0.5));
            if self.rng.next() < 0.6 {
                let a = self.rng.next() * TAU;
                let (x, y) = (c.0 + a.cos() * radius * kit.aspect, c.1 + a.sin() * radius);
                let spin = rate.signum() * 20. * reach;
                self.motes.push(Mote { x, y, vx: -a.sin() * spin * kit.aspect, vy: a.cos() * spin, life: 0.8, span: 0.8 });
            }
        }

        // The wind brings the sand back, a quarter of the rows a frame.
        let storm = (self.storm > 0.) as u8 as f32;
        let base = 0.011 + self.gust * 0.09 + self.boost * 0.06 + storm * 0.9;
        let span = dt * 4.;
        let phase = self.phase % 4;
        self.phase += 1;
        for y in (phase..self.rows).step_by(4) {
            let row = y * self.cols;
            for i in row..row + self.cols {
                let target = if self.bed[i].owner == TABLET { 0.3 } else { 1. };
                let k = (base * (0.3 + 1.4 * self.grain[i]) * span).min(1.);
                let v = &mut self.cover.v[i];
                *v += (target - *v) * k;
            }
        }

        // Grains in the air.
        let wind = (7. + self.gust * 55. + self.boost * 45. + storm * 90.) * reach;
        let want = (self.cols * self.rows) as f32 / 110. * (0.2 + self.gust * 1.6 + self.boost + storm * 3.);
        self.spawn += want * dt / 2.5;
        while self.spawn >= 1. {
            self.spawn -= 1.;
            if self.motes.len() > 6000 {
                self.spawn = 0.;
                break;
            }
            let span = 1.5 + self.rng.next() * 2.5;
            let (x, y) = (self.rng.next() * self.cols as f32, self.rng.next() * self.rows as f32);
            self.motes.push(Mote { x, y, vx: wind, vy: 0., life: span, span });
        }
        let pull = 1. - (-dt * 1.8).exp();
        let (w, h) = (self.cols as f32, self.rows as f32);
        let time = kit.time;
        for m in &mut self.motes {
            let sway = ((m.y * 0.3 + time * 1.3).sin() + (m.x * 0.05).cos() * 0.5) * 3. * reach;
            m.vx += (wind * (0.7 + (m.span - 1.5) * 0.15) - m.vx) * pull;
            m.vy += (sway - m.vy) * pull;
            m.x += m.vx * dt;
            m.y += m.vy * dt;
            m.life -= dt;
            if m.x >= w {
                m.x -= w;
            }
        }
        self.motes.retain(|m| m.life > 0. && m.x >= 0. && m.y >= 0. && m.y < h);

        // Survey what is bare, and bury a new site once everything is found.
        self.survey -= dt;
        if self.survey <= 0. {
            self.survey = 0.25;
            let cols = self.cols as i32;
            for (id, relic) in self.relics.iter_mut().enumerate() {
                let (mut all, mut bare) = (0, 0);
                for y in relic.y..relic.y + relic.h {
                    for x in relic.x..relic.x + relic.w {
                        let i = (y * cols + x) as usize;
                        if self.bed[i].owner as usize == id {
                            all += 1;
                            bare += (self.cover.v[i] < 0.35) as usize;
                        }
                    }
                }
                relic.bare = bare as f32 / all.max(1) as f32;
                relic.found |= relic.bare > 0.55;
            }
            if !self.relics.is_empty() && self.relics.iter().all(|r| r.found) && self.storm <= 0. {
                self.cleared += 0.25;
                if self.cleared > 14. {
                    self.storm = 4.;
                }
            }
        }
        for relic in &mut self.relics {
            let goal = (relic.bare > 0.4) as u8 as f32;
            relic.lit += (goal - relic.lit) * (1. - (-dt * 4.).exp());
        }
        if self.storm > 0. {
            self.storm -= dt;
            if self.storm <= 0. {
                self.storm = 0.;
                self.bury(kit);
            }
        }
    }

    fn draw(&self, r: &mut Raster, kit: &Kit) {
        let (cols, rows) = (self.cols, self.rows);
        if cols == 0 || self.cover.v.len() != cols * rows || kit.cols != cols || kit.rows != rows {
            return;
        }
        let s = kit.sound;
        let reach = kit.reach.max(0.8);
        let speed = rows as f32 * 0.55;
        let width = 1.4 * reach;
        let trail = rows as f32 * 0.12;
        let rings: Vec<(f32, f32)> = self
            .pings
            .iter()
            .map(|&t| {
                let age = kit.time - t;
                (age * speed, (1. - age / 2.4).max(0.))
            })
            .collect();
        let everything = (self.reveal / 1.2).min(1.);
        let glow_beat = 0.88 + (kit.time * 1.7).sin() * 0.12 + s.treble * 0.25;
        let shift = self.drift as usize;
        let v = &self.cover.v;
        for y in 0..rows {
            let row = y * cols;
            // The ripples drift with the wind: a running index, no division per cell.
            let mut rx = shift % cols;
            for x in 0..cols {
                let i = row + x;
                let rip = self.ripple[row + rx];
                rx += 1;
                if rx == cols {
                    rx = 0;
                }
                let d = v[i];
                let mut sonar = 0.;
                if !rings.is_empty() {
                    let dist = self.radial[i];
                    for &(rr, st) in &rings {
                        let e = rr - dist;
                        if e > -width && e < trail {
                            sonar += st * if e < 0. { 1. + e / width } else { 1. - e / trail };
                        }
                    }
                }
                let sonar = sonar.min(1.);
                let (xi, yi) = (x as i32, y as i32);
                let g = self.grain[i];
                if self.etched[i] {
                    // The groove a grain left: a dark cut in the stone, or
                    // a dip in the sand over it. The grains at home cover it.
                    let t = (1. - d).clamp(0., 1.);
                    let ch = if d < 0.5 { '▓' } else { '▒' };
                    r.dot(xi, yi, 4., ch, mix(scale(SAND, 0.4), (40, 34, 28), t));
                    continue;
                }
                let b = self.bed[i];
                if b.ch != '\0' {
                    // The tablet's stone shows through more sand than a relic does.
                    let edge = if b.owner == TABLET { 0.75 } else { 0.6 };
                    let vis = ((edge - d) / 0.4).clamp(0., 1.);
                    if vis > 0. && g < vis * 1.3 {
                        r.dot(xi, yi, 5., b.ch, scale(b.color, (0.3 + 0.7 * vis) * (1. + sonar * 0.3)));
                        continue;
                    }
                    let seen = if b.owner == TABLET { 0. } else { (sonar + everything).min(1.) };
                    if seen > 0.05 {
                        r.dot(xi, yi, 5., '▒', scale(SONAR, 0.15 + seen * 0.6));
                        continue;
                    }
                }
                if d < 0.45 {
                    // Bare ground, or a last few grains on it.
                    if d >= 0.12 && g < d * 1.5 {
                        let ch = if g < d * 0.5 { '∙' } else { '·' };
                        r.dot(xi, yi, 5., ch, mix(scale(SAND, 0.4 + d), SONAR, sonar * 0.5));
                    } else if g < 0.5 {
                        // Packed earth, with a pebble here and there.
                        r.dot(xi, yi, 5., '░', mix(scale(BEDROCK, 0.5 + g * 0.6), SONAR, sonar * 0.5));
                    } else if g < 0.56 {
                        r.dot(xi, yi, 5., '•', mix(scale(BEDROCK, 1.3), SONAR, sonar * 0.5));
                    }
                    continue;
                }
                let (sx, sy) = if x > 0 && y > 0 && x + 1 < cols && y + 1 < rows {
                    ((v[i + 1] - v[i - 1]) * 0.5, (v[i + cols] - v[i - cols]) * 0.5)
                } else {
                    (0., 0.)
                };
                // Lit from the upper left: banks facing it shine, the far side
                // falls dark. Shaded blocks read as a surface where dots read
                // as lines of text.
                let shade = (0.75 - sx * 2.4 - sy * 3.).clamp(0.25, 1.45);
                let heap = (d - 1.).max(0.);
                // Ripple troughs fall into shadow: gaps that draw the crests out.
                if rip < 0.25 && g > 0.6 && heap < 0.1 && sonar < 0.05 {
                    continue;
                }
                let ch = if heap > 0.5 {
                    '▓'
                } else if heap > 0.15 || rip > 0.9 {
                    '▒'
                } else if g < 0.03 {
                    '•'
                } else {
                    '░'
                };
                let glint = if g < 0.03 { 0.5 + (kit.time * 2.5 + g * 900.).sin() * 0.5 } else { 0. };
                // The sand tone and its light in one pass: this runs for most
                // of the screen, every frame.
                let t = (heap * 1.4 + rip * 0.2 + glint * 0.5).min(1.);
                let light = shade * (0.42 + rip * 0.32 + g * 0.16 + glint * 0.4) * (0.55 + 0.45 * d.min(1.));
                let lit = |a: u8, b: u8| ((a as f32 + (b as f32 - a as f32) * t) * light).min(255.) as u8;
                let tone = (lit(SAND.0, RIDGE.0), lit(SAND.1, RIDGE.1), lit(SAND.2, RIDGE.2));
                let color = if sonar > 0. { mix(tone, SONAR, sonar * 0.55) } else { tone };
                r.dot(xi, yi, 5., ch, color);
            }
        }
        for m in &self.motes {
            let fade = (m.life / m.span).min(1.) * ((m.span - m.life) * 4.).min(1.);
            let fast = m.vx.abs() > 45. * reach;
            r.dot(m.x as i32, m.y as i32, 2., if fast { '-' } else { '·' }, scale(RIDGE, 0.35 + fade * 0.6));
        }
        // Relics laid bare: lit, framed by breathing brackets, and named.
        for relic in &self.relics {
            if relic.lit < 0.05 {
                continue;
            }
            let (x0, y0) = (relic.x as f32, relic.y as f32);
            let (x1, y1) = ((relic.x + relic.w - 1) as f32, (relic.y + relic.h - 1) as f32);
            let glow = (0.3 + s.bass * 0.3) * relic.lit;
            r.lift((relic.x.max(0) as usize, (relic.x + relic.w).max(0) as usize), (relic.y.max(0) as usize, (relic.y + relic.h).max(0) as usize), |_, _| glow);
            let accent = scale(ACCENT, 0.4 + relic.lit * 0.6);
            let breathe = 1. + (kit.time * 4.).sin().abs() * 0.6 + s.bass;
            let (bx, by) = (breathe * kit.aspect, breathe * 0.5);
            for (x, y, ch) in [(x0 - bx, y0 - by, '┌'), (x1 + bx, y0 - by, '┐'), (x0 - bx, y1 + by, '└'), (x1 + bx, y1 + by, '┘')] {
                r.dot(x.round() as i32, y.round() as i32, 0.5, ch, accent);
            }
            let name = ARTS[relic.art].name;
            let room = ((x1 - x0) * 1.6).max(cols as f32 * 0.12);
            let most = (rows / 40).max(1);
            let k = (1..=most).rev().find(|&k| crate::scenes3d::cyber::type_size(name, k).0 as f32 <= room).unwrap_or(1);
            let (_, h) = crate::scenes3d::cyber::type_size(name, k);
            let over = y0 - by - h as f32 - 1.;
            let y = if over >= 0. { over } else { (y1 + by + 1.).min(rows as f32 - h as f32) };
            stamp(r, kit, name, k, ((x0 + x1) * 0.5, y.round() as i32, 0.6), scale(accent, 0.9 + s.treble * 0.2));
        }
        // The digit grains: at home they glow through the sand over them;
        // brushed loose they lie on top of it, a scatter of sparks.
        let hot = mix(GLOW, GLOW_HOT, s.bass * 0.3 + everything * 0.4);
        self.digits.draw(r, 3.5, |g| {
            let (x, y) = g.cell();
            let d = if x >= 0 && y >= 0 && (x as usize) < cols && (y as usize) < rows { v[y as usize * cols + x as usize] } else { 0. };
            if g.away() < 0.6 {
                let t = (1.15 - d).clamp(0.32, 1.);
                let ch = if d < 0.35 { '█' } else if d < 0.8 { '▓' } else { '▒' };
                Some((ch, scale(mix(scale(SAND, 0.55), hot, t), glow_beat * (0.92 + g.shade * 0.12))))
            } else {
                // Loose on warm sand they must out-shine it: hot and sparkling.
                let glint = (kit.time * 5. + g.phase).sin().max(0.).powi(4);
                let ch = if glint > 0.5 { '●' } else { '•' };
                Some((ch, scale(mix(GLOW, GLOW_HOT, 0.4 + glint * 0.6), 1. + glint * 0.25 + s.treble * 0.1)))
            }
        });
        if let Some(c) = kit.cursor {
            // The brush head: four bristle tufts round the cursor.
            let rad = (2. + kit.charge * 2.5) * reach;
            let tone = scale(RIDGE, 0.55);
            for (dx, dy, ch) in [(-1., 0., '('), (1., 0., ')')] {
                r.dot((c.0 + dx * rad * kit.aspect).round() as i32, (c.1 + dy).round() as i32, 0.4, ch, tone);
            }
        }
        if !self.relics.is_empty() && rows >= 24 && cols >= 48 {
            let found = self.relics.iter().filter(|r| r.found).count();
            let label = if self.storm > 0. {
                "SANDSTORM".to_string()
            } else if found == self.relics.len() {
                "SITE CLEARED".to_string()
            } else {
                format!("RELICS {found}/{}", self.relics.len())
            };
            let k = (rows / 70).max(1);
            let (w, _) = crate::scenes3d::cyber::type_size(&label, k);
            stamp(r, kit, &label, k, (2. + w as f32 / 2., 1, 0.2), scale(ACCENT, 0.55 + everything * 0.4));
        }
    }
}

/// Stills along a scripted cursor path, for looking at strokes the stock
/// harness never makes:
///   TOY=sand-clock-excavation PATH_KIND=sweep SNAPS=3,8 POSTER_DIR=out \
///   cargo test --release --bin ttfx-bg-rs -- toy_stills --ignored --nocapture
#[cfg(test)]
pub(super) mod stills {
    use crate::interactive_matrix::Sound;
    use gtk4::cairo::{Context, Format, ImageSurface};

    /// The cursor at time `t` for a named path, in 0..1 screen units.
    pub fn path(kind: &str, t: f32) -> Option<(f32, f32)> {
        match kind {
            // Long back and forth strokes across the lower half, then a rest.
            "sweep" => {
                if t > 7. {
                    return Some((0.22, 0.78));
                }
                let lane = (t / 1.6).floor();
                let u = (t / 1.6).fract();
                let x = if lane as i32 % 2 == 0 { 0.05 + u * 0.9 } else { 0.95 - u * 0.9 };
                Some((x, 0.62 + lane * 0.07 + (u * 9.).sin() * 0.02))
            }
            // "row0.5": one brisk wavy pass across that row, then a rest at its end.
            k if k.starts_with("row") => {
                let y: f32 = k[3..].parse().unwrap_or(0.5);
                let u = (t / 1.2).min(1.);
                Some((0.08 + u * 0.89, y + (u * 17.).sin() * 0.03))
            }
            // Brisk circles round the upper left.
            "circle" => Some((0.25 + (t * 5.).cos() * 0.1, 0.3 + (t * 5.).sin() * 0.14)),
            // A side to side shake mid screen at 2 s, otherwise still.
            "shake" => {
                if (2. ..2.6).contains(&t) {
                    Some((0.5 + if ((t * 20.) as i32) % 2 == 0 { 0.15 } else { -0.15 }, 0.5))
                } else {
                    Some((0.5, 0.5))
                }
            }
            "none" => None,
            _ => Some((0.5 + (t * 0.21).sin() * 0.25, 0.55 + (t * 0.17).cos() * 0.15)),
        }
    }

    #[test]
    #[ignore = "manual: writes image files"]
    fn toy_stills() {
        use crate::showcase_scenes::{Kind, Showcase};
        let toy = std::env::var("TOY").unwrap_or_else(|_| "sand-clock-excavation".into());
        let kind = std::env::var("PATH_KIND").unwrap_or_else(|_| "sweep".into());
        let quiet = std::env::var("QUIET").is_ok();
        let snaps: Vec<f32> = std::env::var("SNAPS").unwrap_or_else(|_| "8".into()).split(',').filter_map(|v| v.trim().parse().ok()).collect();
        let dir = std::path::PathBuf::from(std::env::var("POSTER_DIR").unwrap_or_else(|_| "stills".into()));
        std::fs::create_dir_all(&dir).unwrap();
        let (cols, rows): (usize, usize) = std::env::var("GRID").ok().and_then(|g| {
            let (c, r) = g.split_once('x')?;
            Some((c.parse().ok()?, r.parse().ok()?))
        }).unwrap_or((240, 68));
        let (cw, ch) = (8., 17.);
        let mut scene = Showcase::new(Kind::parse(&toy).unwrap());
        let mut cells = vec![];
        scene.draw(cols, rows, ch as f32 / cw as f32, &mut cells);
        let dt = 1. / 30.;
        let mut t = 0f32;
        for &snap in &snaps {
            while t < snap {
                let beat = (t * std::f32::consts::TAU * 2.).sin().max(0.).powi(6);
                let sound = if quiet { Sound::default() } else { Sound { bass: 0.25 + beat * 0.6, treble: 0.3 + 0.3 * (t * 7.).sin().abs() } };
                scene.step(dt, path(&kind, t), sound);
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
            let path = dir.join(format!("{toy}-{kind}-{snap:04.1}s.ppm"));
            std::fs::write(&path, ppm).unwrap();
            println!("STILL {} cells {}", path.display(), cells.len());
        }
    }
}
