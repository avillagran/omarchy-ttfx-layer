//! Cyberpunk scenes on the same 3D raster.
//!
//! - `neon-avenue`: a flight down a rainy avenue of towers, shop neon, vertical
//!   signs, billboards that show the machine's load, flying cars and
//!   holograms over the crossings. Busy cores light windows and fill the
//!   sky lanes; bass pulses the neon and sends waves of light down the
//!   windows, hard beats strike lightning, treble brings the rain. The
//!   cursor steers the flight and scans the facade it points at.
//! - `packet-grid`: a synthwave grid towards a striped sun. Every pulse of
//!   light is real network traffic: downloads run at you, uploads away, and
//!   towers on either side keep the recent history. Beats roll rings of
//!   light down the grid; the cursor marks the floor and draws packets in.
//! - `silicon`: a processor on its board, one core per CPU that rises and
//!   heats with its load, memory chips that fill with use, and traces that
//!   carry disk and network traffic; the clock ring and the copper beat
//!   with the bass. The cursor turns the board and names what it rests on.
//!
//! Text is drawn in a pixel font sized to the room it has, so labels and
//! status lines grow with the grid instead of shrinking to specks.

use super::*;

// ---------------------------------------------------------------- helpers
/// Bytes per second as a short label.
fn rate(bps: f32) -> String {
    let (v, u) = match bps {
        b if b >= 1e9 => (b / 1e9, "GB"),
        b if b >= 1e6 => (b / 1e6, "MB"),
        b if b >= 1e3 => (b / 1e3, "KB"),
        b => (b, "B"),
    };
    if v >= 100. || u == "B" {
        format!("{v:.0} {u}/s")
    } else {
        format!("{v:.1} {u}/s")
    }
}
/// 0 to 1 on a log scale: 1 KB/s is a trickle, 100 MB/s is flat out.
fn level(bps: f32) -> f32 {
    ((1. + bps.max(0.) / 1000.).log10() / 5.).clamp(0., 1.)
}

/// Fills a flat polygon, clipped to the near plane, with attributes that
/// stay true under perspective: windows and grids do not swim.
fn poly(cv: &mut Canvas, pts: &[(V, [f32; 3])], paint: &mut impl FnMut([f32; 3], f32) -> Option<(char, Color)>) {
    const CLIP: f32 = NEAR * 2.;
    let mut out: Vec<(V, [f32; 3])> = Vec::with_capacity(pts.len() + 2);
    for i in 0..pts.len() {
        let (a, aa) = (cv.cam.view(pts[i].0), pts[i].1);
        let j = (i + 1) % pts.len();
        let (b, ba) = (cv.cam.view(pts[j].0), pts[j].1);
        let (ina, inb) = (a[2] >= CLIP, b[2] >= CLIP);
        if ina {
            out.push((a, aa));
        }
        if ina != inb {
            let t = (CLIP - a[2]) / (b[2] - a[2]);
            let l = |x: f32, y: f32| x + (y - x) * t;
            out.push(([l(a[0], b[0]), l(a[1], b[1]), CLIP], [l(aa[0], ba[0]), l(aa[1], ba[1]), l(aa[2], ba[2])]));
        }
    }
    if out.len() < 3 {
        return;
    }
    let mut vx = Vec::with_capacity(out.len());
    for &(v, a) in &out {
        let Some((x, y, z)) = cv.cam.screen(v) else { return };
        let w = 1. / z;
        vx.push(Vx { x, y, w, a: [a[0] * w, a[1] * w, a[2] * w] });
    }
    let mut wrap = |a: [f32; 3], d: f32| paint([a[0] * d, a[1] * d, a[2] * d], d);
    for i in 1..vx.len() - 1 {
        cv.tri(vx[0], vx[i], vx[i + 1], &mut wrap);
    }
}

/// The faces of a box that face the camera. `paint` gets (u, v, face):
/// face 0 is the top (u, v = x, z), 1 and 2 face -z and +z (x, y), 3 and 4
/// face -x and +x (z, y).
fn solid_box(cv: &mut Canvas, lo: V, hi: V, paint: &mut impl FnMut([f32; 3], f32) -> Option<(char, Color)>) {
    let c = cv.cam.pos;
    let [x0, y0, z0] = lo;
    let [x1, y1, z1] = hi;
    if c[1] > y1 {
        poly(cv, &[([x0, y1, z0], [x0, z0, 0.]), ([x1, y1, z0], [x1, z0, 0.]), ([x1, y1, z1], [x1, z1, 0.]), ([x0, y1, z1], [x0, z1, 0.])], paint);
    }
    if c[2] < z0 {
        poly(cv, &[([x0, y0, z0], [x0, y0, 1.]), ([x1, y0, z0], [x1, y0, 1.]), ([x1, y1, z0], [x1, y1, 1.]), ([x0, y1, z0], [x0, y1, 1.])], paint);
    }
    if c[2] > z1 {
        poly(cv, &[([x0, y0, z1], [x0, y0, 2.]), ([x1, y0, z1], [x1, y0, 2.]), ([x1, y1, z1], [x1, y1, 2.]), ([x0, y1, z1], [x0, y1, 2.])], paint);
    }
    if c[0] < x0 {
        poly(cv, &[([x0, y0, z0], [z0, y0, 3.]), ([x0, y0, z1], [z1, y0, 3.]), ([x0, y1, z1], [z1, y1, 3.]), ([x0, y1, z0], [z0, y1, 3.])], paint);
    }
    if c[0] > x1 {
        poly(cv, &[([x1, y0, z0], [z0, y0, 4.]), ([x1, y0, z1], [z1, y0, 4.]), ([x1, y1, z1], [z1, y1, 4.]), ([x1, y1, z0], [z0, y1, 4.])], paint);
    }
}

/// A line split into short pieces, so each keeps its own depth.
fn long_line(cv: &mut Canvas, a: V, b: V, piece: f32, color: impl Fn(f32) -> Option<Color>) {
    let n = (len(sub(b, a)) / piece).ceil().max(1.) as usize;
    for i in 0..n {
        let (t0, t1) = (i as f32 / n as f32, (i + 1) as f32 / n as f32);
        let (p, q) = (add(a, mul(sub(b, a), t0)), add(a, mul(sub(b, a), t1)));
        let d = len(sub(mul(add(p, q), 0.5), cv.cam.pos));
        if let Some(c) = color(d) {
            cv.line(p, q, None, c);
        }
    }
}

// ----------------------------------------------------------- cell maths
// The per-cell shaders run tens of thousands of times a frame: these keep
// libm out of them (on baseline x86-64 even `floor` is a call; `flo` is
// the parent module's).
fn fract(x: f32) -> f32 {
    x - flo(x)
}
/// e^-x for x >= 0, within about 0.2 %: the fog of every cell.
fn fade(x: f32) -> f32 {
    let y = (-x * std::f32::consts::LOG2_E).max(-126.);
    let i = flo(y);
    let f = y - i;
    f32::from_bits(((i as i32 + 127) as u32) << 23) * (1. + f * (0.6565 + f * 0.3435))
}

/// Value noise in [0, 1) on a plane: half the lookups of `noise3`.
fn noise2(x: f32, y: f32) -> f32 {
    let (xi, yi) = (flo(x), flo(y));
    let (fx, fy) = (s3(x - xi), s3(y - yi));
    let (i, j) = (xi as i32, yi as i32);
    let l = |a: f32, b: f32, t: f32| a + (b - a) * t;
    l(l(hash3(i, j, 3), hash3(i + 1, j, 3), fx), l(hash3(i, j + 1, 3), hash3(i + 1, j + 1, 3), fx), fy)
}

// ------------------------------------------------------------------- type
/// A 3x5 pixel font for what the scenes print, one row of three bits per
/// line. Drawn in half blocks it grows with the grid, where plain
/// characters would shrink to specks on a dense one.
fn glyph(c: char) -> [u8; 5] {
    match c.to_ascii_uppercase() {
        '0' => [7, 5, 5, 5, 7],
        '1' => [2, 6, 2, 2, 7],
        '2' => [7, 1, 7, 4, 7],
        '3' => [7, 1, 3, 1, 7],
        '4' => [5, 5, 7, 1, 1],
        '5' => [7, 4, 7, 1, 7],
        '6' => [7, 4, 7, 5, 7],
        '7' => [7, 1, 1, 2, 2],
        '8' => [7, 5, 7, 5, 7],
        '9' => [7, 5, 7, 1, 7],
        'A' => [2, 5, 7, 5, 5],
        'B' => [6, 5, 6, 5, 6],
        'C' => [3, 4, 4, 4, 3],
        'D' => [6, 5, 5, 5, 6],
        'E' => [7, 4, 6, 4, 7],
        'F' => [7, 4, 6, 4, 4],
        'G' => [3, 4, 5, 5, 3],
        'H' => [5, 5, 7, 5, 5],
        'I' => [7, 2, 2, 2, 7],
        'J' => [1, 1, 1, 5, 2],
        'K' => [5, 5, 6, 5, 5],
        'L' => [4, 4, 4, 4, 7],
        'M' => [5, 7, 7, 5, 5],
        'N' => [6, 5, 5, 5, 5],
        'O' => [2, 5, 5, 5, 2],
        'P' => [6, 5, 6, 4, 4],
        'Q' => [2, 5, 5, 6, 3],
        'R' => [6, 5, 6, 5, 5],
        'S' => [3, 4, 2, 1, 6],
        'T' => [7, 2, 2, 2, 2],
        'U' => [5, 5, 5, 5, 7],
        'V' => [5, 5, 5, 5, 2],
        'W' => [5, 5, 7, 7, 5],
        'X' => [5, 5, 2, 5, 5],
        'Y' => [5, 5, 2, 2, 2],
        'Z' => [7, 1, 2, 4, 7],
        '%' => [5, 1, 2, 4, 5],
        '.' | '·' => [0, 0, 0, 0, 2],
        '/' => [1, 1, 2, 4, 4],
        ':' => [0, 2, 0, 2, 0],
        '-' => [0, 0, 7, 0, 0],
        '+' => [0, 2, 7, 2, 0],
        '▲' => [0, 2, 7, 7, 0],
        '▼' => [0, 7, 7, 2, 0],
        _ => [0; 5],
    }
}

/// Columns and rows of `s` at `k` cells per font pixel; 0 is plain text.
pub(crate) fn type_size(s: &str, k: usize) -> (usize, usize) {
    let n = s.chars().count();
    if k == 0 {
        return (n, 1);
    }
    ((n * 4).saturating_sub(1) * k, (5 * k).div_ceil(2))
}

fn type_at(cv: &mut Canvas, x0: i32, y0: i32, z: f32, s: &str, k: usize, c: Color) {
    if k == 0 {
        cv.text(x0, y0, z, s, c);
    } else {
        pixel_text(cv.r, (cv.cols, cv.rows), (x0, y0, z), s, k, c);
    }
}

/// `s` in the pixel font at `k` cells per font pixel (k >= 1), in half
/// blocks, with its top left corner on a cell of a `cols` by `rows` raster.
pub(crate) fn pixel_text(r: &mut Raster, (cols, rows): (usize, usize), (x0, y0, z): (i32, i32, f32), s: &str, k: usize, c: Color) {
    let (_, h) = type_size(s, k);
    for (i, ch) in s.chars().enumerate() {
        let g = glyph(ch);
        if g == [0; 5] {
            continue;
        }
        let gx = x0 + (i * 4 * k) as i32;
        let bit = |half: usize, px: usize| {
            let r = half / k;
            r < 5 && (g[r] >> (2 - px / k)) & 1 == 1
        };
        for row in 0..h {
            let y = y0 + row as i32;
            if y < 0 || y >= rows as i32 {
                continue;
            }
            for px in 0..3 * k {
                let x = gx + px as i32;
                if x < 0 || x >= cols as i32 {
                    continue;
                }
                let ch = match (bit(row * 2, px), bit(row * 2 + 1, px)) {
                    (true, true) => '█',
                    (true, false) => '▀',
                    (false, true) => '▄',
                    _ => continue,
                };
                r.dot(x, y, z, ch, c);
            }
        }
    }
}

/// The largest size, up to `most`, at which `s` fits `w` by `h` cells.
fn type_fit(s: &str, w: f32, h: f32, most: usize) -> usize {
    (1..=most)
        .rev()
        .find(|&k| {
            let (tw, th) = type_size(s, k);
            tw as f32 <= w && th as f32 <= h
        })
        .unwrap_or(0)
}

/// Text centred on a screen point at depth `z`, sized to a `w` by `h` cell
/// box: big where there is room, plain when small, gone when the plain
/// text would swamp what it labels.
fn tag_px(cv: &mut Canvas, (x, y, z): (f32, f32, f32), w: f32, h: f32, s: &str, c: Color) {
    // Never more than a twentieth of the screen tall, however near.
    let k = type_fit(s, w, h.min(cv.rows as f32 / 20.), 4);
    if k == 0 && (cv.cols < 90 || w < s.chars().count() as f32 * 0.5) {
        return;
    }
    let (tw, th) = type_size(s, k);
    // Halfway to the camera: in front of the parts round what it labels
    // (a sign, the core's own top), still behind what is truly nearer.
    type_at(cv, (x - tw as f32 * 0.5).round() as i32, (y - th as f32 * 0.5).round() as i32, z * 0.5, s, k, c);
}

/// Text on a world point, sized to the `w` by `h` world units it labels,
/// so it scales with distance and with the density of the grid.
fn tag(cv: &mut Canvas, p: V, (w, h): (f32, f32), s: &str, c: Color) {
    let Some(at) = cv.cam.project(p) else { return };
    let (pw, ph) = (w / at.2 * cv.cam.f * cv.cam.aspect, h / at.2 * cv.cam.f);
    tag_px(cv, at, pw, ph, s, c);
}

/// Status lines at the foot of the screen, all at one size that grows with
/// the grid and still fits its width.
fn hud(cv: &mut Canvas, lines: &[(&str, Color)]) {
    if cv.cols < 60 || lines.is_empty() {
        return;
    }
    let room = cv.cols as f32 * 0.92;
    let k = (1..=cv.rows / 60).rev().find(|&k| lines.iter().all(|l| type_size(l.0, k).0 as f32 <= room)).unwrap_or(0);
    let step = type_size("", k).1 + (k > 0) as usize;
    // One row of margin below the last line.
    let total = step * lines.len() - (k > 0) as usize;
    let mut y = cv.rows as i32 - 1 - total as i32;
    for &(s, c) in lines {
        let (w, _) = type_size(s, k);
        type_at(cv, (cv.cols as i32 - w as i32) / 2, y, 0.01, s, k, c);
        y += step as i32;
    }
}

/// Corner marks round the cursor, so it is clear what it touches.
fn reticle(cv: &mut Canvas, (x, y): (f32, f32), r: f32, c: Color) {
    let (dx, dy) = (r * cv.cam.aspect, r);
    for (sx, sy, ch) in [(-1., -1., '┌'), (1., -1., '┐'), (-1., 1., '└'), (1., 1., '┘')] {
        cv.dot(x + sx * dx, y + sy * dy, 0.02, ch, c);
    }
}

/// The screen cell under the smoothed cursor.
fn under(cv: &Canvas, gaze: (f32, f32)) -> (f32, f32) {
    ((gaze.0 + 1.) * 0.5 * cv.cols as f32, (gaze.1 + 1.) * 0.5 * cv.rows as f32)
}

/// Where a ray enters a box, if it does.
fn hit_box(o: V, d: V, lo: V, hi: V) -> Option<f32> {
    let (mut t0, mut t1) = (0f32, f32::MAX);
    for i in 0..3 {
        if d[i].abs() < 1e-6 {
            if o[i] < lo[i] || o[i] > hi[i] {
                return None;
            }
            continue;
        }
        let (a, b) = ((lo[i] - o[i]) / d[i], (hi[i] - o[i]) / d[i]);
        t0 = t0.max(a.min(b));
        t1 = t1.min(a.max(b));
    }
    (t0 <= t1).then_some(t0)
}

/// Two byte counters from /proc turned into smoothed rates, with a short
/// history. Where /proc cannot be read a believable wobble stands in.
#[derive(Clone, Copy, PartialEq)]
enum Feed {
    Net,
    Disk,
}
struct Counter {
    feed: Feed,
    prev: Option<(u64, u64)>,
    now: (f32, f32),
    smooth: (f32, f32),
    hist: VecDeque<(f32, f32)>,
    clock: f32,
    time: f32,
    label: String,
    /// Whole disks, listed once: partitions would count twice.
    disks: Vec<String>,
}
const COUNT_TICK: f32 = 0.5;
impl Counter {
    fn new(feed: Feed) -> Self {
        let disks = match feed {
            Feed::Disk => std::fs::read_dir("/sys/block")
                .map(|d| d.flatten().filter_map(|e| e.file_name().into_string().ok()).filter(|n| !["loop", "ram", "zram", "dm-", "md", "sr"].iter().any(|p| n.starts_with(p))).collect())
                .unwrap_or_default(),
            Feed::Net => vec![],
        };
        let mut c = Self { feed, prev: None, now: (0., 0.), smooth: (0., 0.), hist: VecDeque::new(), clock: 0., time: 0., label: String::new(), disks };
        c.sample(COUNT_TICK);
        c
    }
    fn read(&self) -> Option<(u64, u64, String)> {
        match self.feed {
            Feed::Net => {
                let s = std::fs::read_to_string("/proc/net/dev").ok()?;
                let (mut rx, mut tx, mut best, mut name) = (0u64, 0u64, 0u64, String::new());
                for line in s.lines().skip(2) {
                    let Some((n, rest)) = line.split_once(':') else { continue };
                    let n = n.trim();
                    if n == "lo" {
                        continue;
                    }
                    let f: Vec<u64> = rest.split_whitespace().filter_map(|v| v.parse().ok()).collect();
                    if f.len() < 9 {
                        continue;
                    }
                    rx += f[0];
                    tx += f[8];
                    if f[0] + f[8] >= best {
                        best = f[0] + f[8];
                        name = n.chars().filter(|c| c.is_ascii_alphanumeric() || *c == '-' || *c == '_').take(16).collect();
                    }
                }
                Some((rx, tx, name))
            }
            Feed::Disk => {
                let s = std::fs::read_to_string("/proc/diskstats").ok()?;
                let (mut r, mut w, mut n) = (0u64, 0u64, 0);
                for line in s.lines() {
                    let f: Vec<&str> = line.split_whitespace().collect();
                    if f.len() < 10 {
                        continue;
                    }
                    if !self.disks.iter().any(|d| d == f[2]) {
                        continue;
                    }
                    r += f[5].parse::<u64>().unwrap_or(0) * 512;
                    w += f[9].parse::<u64>().unwrap_or(0) * 512;
                    n += 1;
                }
                (n > 0).then(|| (r, w, "disk".into()))
            }
        }
    }
    fn sample(&mut self, span: f32) {
        match self.read() {
            Some((a, b, label)) => {
                if let Some((a0, b0)) = self.prev {
                    self.now = (a.saturating_sub(a0) as f32 / span, b.saturating_sub(b0) as f32 / span);
                }
                self.prev = Some((a, b));
                self.label = label;
            }
            None => {
                let k = if self.feed == Feed::Net { 1. } else { 5. };
                let t = self.time;
                self.now = (2e3 + 3e6 * noise3([t * 0.15, k, 0.]).powi(4), 1e3 + 8e5 * noise3([t * 0.2, k, 3.]).powi(4));
                self.label = if self.feed == Feed::Net { "net".into() } else { "disk".into() };
            }
        }
        self.hist.push_front(self.now);
        self.hist.truncate(64);
    }
    fn step(&mut self, dt: f32) {
        self.time += dt;
        self.clock += dt;
        if self.clock >= COUNT_TICK {
            let span = self.clock;
            self.clock = 0.;
            self.sample(span);
        }
        let k = 1. - (-dt * 3.).exp();
        self.smooth.0 += (self.now.0 - self.smooth.0) * k;
        self.smooth.1 += (self.now.1 - self.smooth.1) * k;
    }
}

// ============================================================= neon city
const STREET: f32 = 6.;
const LOT: f32 = 7.;
const LOTS: i64 = 4;
const CROSS: f32 = 7.;
const BLOCK: f32 = LOT * LOTS as f32 + CROSS;
/// The city repeats after this many blocks, so the flight can wrap.
const BLOCKS: i64 = 1024;
const NEON: [Color; 5] = [(255, 60, 200), (60, 230, 255), (255, 170, 60), (160, 100, 255), (80, 255, 170)];
const SIGNS: [&str; 10] = ["BAR", "HOTEL", "RAMEN", "OPEN", "24H", "DATA", "SYNTH", "NOODLE", "ARCADE", "CLINIC"];
const WINDOWS: [Color; 4] = [(255, 190, 100), (110, 220, 255), (255, 110, 210), (220, 230, 255)];
const HACK: Color = (80, 240, 255);

struct Lot {
    i: i64,
    side: f32,
    z0: f32,
    z1: f32,
    face: f32,
    back: f32,
    h: f32,
    neon: Color,
    sign: Option<&'static str>,
    board: Option<usize>,
    seed: i32,
}
fn lot(i: i64, side: f32) -> Lot {
    let block = i.div_euclid(LOTS);
    let j = i.rem_euclid(LOTS);
    let z0 = block as f32 * BLOCK + j as f32 * LOT;
    let seed = (i.rem_euclid(LOTS * BLOCKS) as i32) * 2 + (side > 0.) as i32;
    let h = |n: i32| hash3(seed, n, 11);
    let tall = h(2);
    let height = 9. + tall * tall * 46.;
    let set = if h(3) < 0.3 { 0.6 + h(4) * 2. } else { 0. };
    let face = side * (STREET + set);
    Lot {
        i,
        side,
        z0: z0 + 0.35,
        z1: z0 + LOT - 0.35,
        face,
        back: face + side * (6. + h(5) * 8.),
        h: height,
        neon: NEON[(h(6) * 5.) as usize % 5],
        sign: (h(7) < 0.45).then(|| SIGNS[(h(8) * 10.) as usize % 10]),
        board: (height > 20. && h(9) < 0.45).then(|| (h(10) * 6.) as usize),
        seed,
    }
}
struct Car {
    p: V,
    v: f32,
}
pub(crate) struct NeonCity {
    load: SysLoad,
    z: f32,
    time: f32,
    cars: Vec<Car>,
    rain: Vec<V>,
    stars: Vec<Star>,
    edges: Vec<(usize, usize)>,
    glitch: f32,
    /// Lightning on a hard beat: its light, where it falls, and a rest
    /// before the next.
    bolt: f32,
    bolt_seed: i32,
    bolt_rest: f32,
    /// Seconds since each recent beat: a wave of windows runs down the
    /// avenue for each.
    waves: Vec<f32>,
    gaze: (f32, f32),
    cursor: bool,
    bass: f32,
    pulse: Pulse,
    rng: Rng,
}
const ICO: [V; 12] = {
    const G: f32 = 1.618034;
    [
        [-1., G, 0.], [1., G, 0.], [-1., -G, 0.], [1., -G, 0.],
        [0., -1., G], [0., 1., G], [0., -1., -G], [0., 1., -G],
        [G, 0., -1.], [G, 0., 1.], [-G, 0., -1.], [-G, 0., 1.],
    ]
};
fn ico_edges() -> Vec<(usize, usize)> {
    let mut e = vec![];
    for i in 0..12 {
        for j in i + 1..12 {
            if (len(sub(ICO[i], ICO[j])) - 2.).abs() < 0.01 {
                e.push((i, j));
            }
        }
    }
    e
}
/// What the cursor touches on a facade: the lot and the world point.
struct Hack {
    i: i64,
    side: f32,
    p: V,
}
impl NeonCity {
    pub(crate) fn new() -> Self {
        let mut rng = Rng::new();
        let stars = starfield(500, &mut rng).into_iter().filter(|s| s.dir[1] > 0.12).collect();
        let mut s = Self {
            load: SysLoad::new(),
            z: rng.next() * BLOCK * 100.,
            time: 0.,
            cars: vec![],
            rain: vec![],
            stars,
            edges: ico_edges(),
            glitch: 0.,
            bolt: 0.,
            bolt_seed: 0,
            bolt_rest: 0.,
            waves: vec![],
            gaze: (0., 0.),
            cursor: false,
            bass: 0.,
            pulse: Pulse::default(),
            rng,
        };
        for _ in 0..30 {
            let car = s.car(true);
            s.cars.push(car);
        }
        s
    }
    fn car(&mut self, anywhere: bool) -> Car {
        let away = self.rng.next() < 0.5;
        let lanes = [0.9, 2.3, 3.6];
        let x = lanes[(self.rng.next() * 3.) as usize % 3] * if away { 1. } else { -1. };
        let y = [7.5, 10.5, 14., 18.][(self.rng.next() * 4.) as usize % 4] + self.rng.signed() * 0.4;
        let v = (12. + self.rng.next() * 14.) * if away { 1. } else { -1. };
        let z = if anywhere || away { self.z + 4. + self.rng.next() * 150. } else { self.z + 150. + self.rng.next() * 20. };
        Car { p: [x, y, z], v }
    }
    /// The camera: the cursor steers the flight across the avenue and up
    /// the towers, and turns the view towards its side.
    fn eye(&self) -> (V, V) {
        let (t, g) = (self.time, self.gaze);
        let x = ((t * 0.11).sin() * 1.3 + g.0 * 3.4).clamp(-(STREET - 0.8), STREET - 0.8);
        let y = (4.6 + (t * 0.07).sin() * 1.5 - g.1 * 4.).clamp(1.2, 11.) + (t * 9.).sin() * self.bass * 0.15;
        ([x, y, self.z], [x * 0.3 + g.0 * 7., y + 1.6 - g.1 * 3., self.z + 24.])
    }
    fn step(&mut self, dt: f32, sound: &Sound, cursor: Option<(f32, f32)>, gaze: (f32, f32)) {
        self.time += dt;
        self.load.step(dt);
        self.gaze = gaze;
        self.cursor = cursor.is_some();
        self.bass = sound.bass;
        let speed = 5. + sound.bass * 4. + self.load.total * 3.;
        self.z += speed * dt;
        let period = BLOCK * BLOCKS as f32;
        if self.z >= period {
            self.z -= period;
            for c in &mut self.cars {
                c.p[2] -= period;
            }
            for r in &mut self.rain {
                r[2] -= period;
            }
        }
        // A beat glitches the signs and sends light down the avenue; a hard
        // one now and then strikes lightning.
        self.bolt_rest -= dt;
        if let Some(k) = self.pulse.hit(sound.bass, dt) {
            self.glitch = self.glitch.max(k.min(1.));
            if self.waves.len() < 6 {
                self.waves.push(0.);
            }
            if k > 0.6 && self.bolt_rest <= 0. && self.rng.next() < 0.4 {
                self.bolt = 1.;
                self.bolt_seed = (self.rng.next() * 1e6) as i32;
                self.bolt_rest = 2.5;
            }
        }
        for w in &mut self.waves {
            *w += dt;
        }
        self.waves.retain(|w| *w < 3.5);
        self.glitch *= (-dt * 5.).exp();
        self.bolt *= (-dt * 5.).exp();
        // Busy cores fill the sky lanes; the bass hurries them.
        let want = 14 + (self.load.total * 80.) as usize;
        let hurry = 1. + sound.bass * 0.8;
        for c in &mut self.cars {
            c.p[2] += c.v * hurry * dt;
        }
        let z = self.z;
        self.cars.retain(|c| c.p[2] > z - 4. && c.p[2] < z + 175.);
        self.cars.truncate(want + 10);
        if self.cars.len() < want {
            let car = self.car(false);
            self.cars.push(car);
        }
        // Treble brings the rain, round wherever the camera flies.
        let want = 140 + (sound.treble * 700.) as usize;
        let eye_x = self.eye().0[0];
        while self.rain.len() < want {
            let p = [eye_x + self.rng.signed() * 12., self.rng.next() * 18., self.z + 1. + self.rng.next() * 25.];
            self.rain.push(p);
        }
        self.rain.truncate(want);
        for i in 0..self.rain.len() {
            let r = &mut self.rain[i];
            r[1] -= dt * 26.;
            r[0] += dt * 2.;
            if r[1] < 0. || r[2] < z + 0.5 {
                *r = [eye_x + self.rng.signed() * 12., 18., self.z + 1. + self.rng.next() * 25.];
            }
        }
    }
    fn draw(&self, cv: &mut Canvas, sound: &Sound) {
        let t = self.time;
        let (eye, target) = self.eye();
        cv.cam = Cam::look(eye, target, 1.15, cv.cols, cv.rows, cv.cam.aspect);
        draw_sky(cv, &self.stars, t, sound.treble * 0.4);
        if self.bolt > 0.05 {
            self.lightning(cv);
        }
        let far = self.z + 190.;
        let first = ((self.z - LOT) / BLOCK).floor() as i64 * LOTS;
        let last = (far / BLOCK).ceil() as i64 * LOTS;
        // Every lot in range, two per index: the street looks its neon up.
        let lots: Vec<Lot> = (first..last).flat_map(|i| [lot(i, -1.), lot(i, 1.)]).collect();
        let spot = under(cv, self.gaze);
        let hack = if self.cursor { Self::pick(&lots, eye, cv.cam.ray(spot.0, spot.1)) } else { None };
        // Where each beat's wave of light has got to, and how bright.
        let mut fronts = [(f32::MIN, 0f32); 6];
        for (f, &age) in fronts.iter_mut().zip(&self.waves) {
            *f = (self.z + 4. + age * 48., 1. - age / 3.5);
        }
        // Nearest first: the depth test then spares the shading of
        // everything they hide.
        for l in lots.iter().filter(|l| l.z1 > self.z - 1. && l.z0 < far) {
            let touched = hack.as_ref().filter(|h| h.i == l.i && h.side == l.side).map(|h| h.p);
            self.building(cv, l, sound, &fronts, touched);
        }
        self.street(cv, &lots, first, sound);
        self.holograms(cv, sound, far);
        let fog = |d: f32| (-d / 80.).exp();
        for c in &self.cars {
            let d = c.p[2] - self.z;
            let k = fog(d);
            if k < 0.05 {
                continue;
            }
            let (head, tail): (Color, Color) = if c.v > 0. { ((255, 60, 60), (200, 40, 60)) } else { ((230, 240, 255), (120, 170, 255)) };
            let trail = add(c.p, [0., 0., -c.v.signum() * (1.5 + c.v.abs() * 0.12) * (1. + sound.bass)]);
            cv.line(c.p, trail, None, scale(tail, 0.6 * k));
            cv.point(c.p, if d < 25. { '█' } else { '*' }, scale(head, 0.4 + 0.7 * k));
        }
        for r in &self.rain {
            if r[2] - self.z > 26. {
                continue;
            }
            let k = fog(r[2] - self.z);
            cv.line(*r, add(*r, [0.06, 0.55, 0.]), None, scale((120, 150, 220), 0.12 + 0.25 * k + self.bolt * 0.3));
        }
        if self.cursor {
            let on = hack.is_some();
            reticle(cv, spot, if on { 2. } else { 1.2 }, scale(HACK, if on { 1. } else { 0.5 }));
            if let Some(h) = &hack {
                let id = (hash3(h.i as i32, h.side as i32, 9) * 65535.) as u32;
                let at = add(h.p, [-h.side * 0.4, 1.6, 0.]);
                tag(cv, at, (6., 1.6), &format!("NODE {id:04X}"), (200, 255, 255));
            }
        }
    }
    /// The facade the cursor's ray meets first.
    fn pick(lots: &[Lot], o: V, d: V) -> Option<Hack> {
        let mut best: Option<(f32, Hack)> = None;
        for l in lots {
            if d[0] * l.side <= 1e-4 {
                continue;
            }
            let t = (l.face - o[0]) / d[0];
            if t <= 0. || best.as_ref().is_some_and(|b| b.0 <= t) {
                continue;
            }
            let p = add(o, mul(d, t));
            if (l.z0..=l.z1).contains(&p[2]) && (0. ..=l.h).contains(&p[1]) {
                best = Some((t, Hack { i: l.i, side: l.side, p }));
            }
        }
        best.map(|b| b.1)
    }
    /// A jagged bolt in the sky, behind the towers.
    fn lightning(&self, cv: &mut Canvas) {
        let seed = self.bolt_seed;
        let (cols, rows) = (cv.cols as f32, cv.rows as f32);
        let mut x = cols * (0.15 + 0.7 * hash3(seed, 1, 3));
        let bottom = (rows * (0.3 + 0.2 * hash3(seed, 2, 3))) as i32;
        let c = scale((215, 195, 255), 0.4 + self.bolt * 0.8);
        let mut fork = None;
        for y in 0..bottom {
            let nx = x + (hash3(seed, y, 5) - 0.5) * 3. * cv.cam.aspect * 0.5;
            let ch = if nx > x + 0.5 { '\\' } else if nx < x - 0.5 { '/' } else { '│' };
            x = nx;
            cv.dot(x, y as f32, 9.9e4, ch, c);
            if fork.is_none() && y > bottom / 3 && hash3(seed, y, 7) < 0.15 {
                fork = Some(x);
            }
            if let Some(f) = fork.as_mut() {
                *f += 0.8 + hash3(seed, y, 8);
                cv.dot(*f, y as f32, 9.9e4, '\\', scale(c, 0.6));
            }
        }
    }
    fn street(&self, cv: &mut Canvas, lots: &[Lot], first: i64, sound: &Sound) {
        let (z0, z1) = (self.z - 1., self.z + 190.);
        let t = self.time;
        let neon_at = |x: f32, z: f32| -> Color {
            let block = flo(z * (1. / BLOCK));
            let j = flo((z - block * BLOCK) * (1. / LOT)).min(3.);
            let i = block as i64 * LOTS + j as i64;
            let k = (i - first) * 2 + (x > 0.) as i64;
            usize::try_from(k).ok().and_then(|k| lots.get(k)).map_or(NEON[0], |l| l.neon)
        };
        let glow = 0.7 + sound.bass * 0.6 + self.bolt;
        let ground = mix((40, 36, 70), (110, 100, 170), self.bolt * 0.6);
        let mut paint = |a: [f32; 3], d: f32| -> Option<(char, Color)> {
            let (x, z) = (a[0], a[1]);
            let fog = fade(d * (1. / 70.));
            if fog < 0.03 {
                return Some((' ', (0, 0, 0)));
            }
            let ax = x.abs();
            let zb = fract(z * (1. / BLOCK)) * BLOCK;
            if zb > LOT * LOTS as f32 + 0.6 && zb < LOT * LOTS as f32 + 2.4 && ax < STREET && fract(x * (1. / 0.9)) < 0.5 {
                return Some(('▬', scale((170, 170, 190), 0.45 * fog)));
            }
            if ax < 0.1 && fract(z * 0.25) < 0.45 {
                return Some(('-', scale((255, 190, 80), 0.7 * fog)));
            }
            if (ax - (STREET - 1.4)).abs() < 0.07 {
                return Some(('_', scale((100, 100, 150), 0.6 * fog)));
            }
            // Wet asphalt mirrors the neon of the nearest side. The noise
            // is only worth sampling where a reflection could show.
            let dry = Some(('·', scale(ground, fog)));
            let edge = (ax / STREET).min(1.).powi(3) * 0.5;
            let gain = fog * glow;
            if (1.4 + edge) * gain < 0.07 {
                return dry;
            }
            let puddle = smoothstep(0.42, 0.7, noise2(x * 0.25 + 7., z * 0.12));
            let gain = (puddle * 1.4 + edge) * gain;
            if gain < 0.07 {
                return dry;
            }
            let refl = noise2(x * 0.9 + t * 0.35, z * 0.3 - t * 0.25).powi(2) * gain;
            if refl > 0.07 {
                let ch = if refl > 0.5 { '≈' } else if refl > 0.25 { '~' } else { '-' };
                Some((ch, scale(neon_at(x, z), refl.min(1.))))
            } else {
                dry
            }
        };
        let w = 80.;
        poly(cv, &[([-w, 0., z0], [-w, z0, 0.]), ([w, 0., z0], [w, z0, 0.]), ([w, 0., z1], [w, z1, 0.]), ([-w, 0., z1], [-w, z1, 0.])], &mut paint);
    }
    fn building(&self, cv: &mut Canvas, l: &Lot, sound: &Sound, fronts: &[(f32, f32)], touched: Option<V>) {
        let t = self.time;
        let lit = 0.18 + self.load.total * 0.6 + self.load.mem * 0.1;
        let pulse = 0.55 + sound.bass * 0.7;
        let f = cv.cam.f * cv.cam.aspect;
        let neon = l.neon;
        let h = l.h;
        let seed = l.seed;
        let (x0, x1) = (l.face, l.back);
        let (z0, z1) = (l.z0, l.z1);
        let board = l.board.map(|k| (1.0f32, LOT - 1.7, h - 9., h - 4., k));
        let glitch = self.glitch;
        let treble = sound.treble;
        let wall_tone = mix((8, 7, 17), (60, 52, 105), self.bolt * 0.6);
        // Beyond this a window is under 1.3 cells wide.
        let far_windows = 1.1 * f / 1.3;
        // Only the waves passing this lot matter to its windows.
        let mut near = [(0f32, 0f32); 6];
        let mut n_near = 0;
        for &(front, k) in fronts {
            if front > z0 - 3. && front < z1 + 3. {
                near[n_near] = (front, k);
                n_near += 1;
            }
        }
        let near = &near[..n_near];
        // The cursor's spot, in the facade's own (u, v).
        let spot = touched.map(|p| (p[2] - z0, p[1]));
        let mut paint = |a: [f32; 3], d: f32| -> Option<(char, Color)> {
            let (u, v) = (a[0], a[1]);
            let fog = fade(d * (1. / 80.));
            if fog < 0.03 {
                return Some((' ', (0, 0, 0)));
            }
            let wall = || Some(('█', scale(wall_tone, fog.max(0.5))));
            // The shop front: a neon band at street level.
            if (2.3..2.7).contains(&v) {
                return Some(('▀', scale(neon, (0.45 + 0.55 * pulse) * fog.max(0.2))));
            }
            if v < 2.3 {
                let glass = fract(u * 1.3) < 0.85 && v > 0.3;
                return if glass { Some(('▒', scale(mix(neon, (255, 230, 200), 0.5), (0.3 + sound.bass * 0.3) * fog))) } else { wall() };
            }
            let front = a[2] < 0.5;
            // Under the cursor the facade is being scanned: rings of light
            // sweep out from the spot and every window wakes up.
            let scan = spot.filter(|_| front).map(|(su, sv)| ((u - su).powi(2) + ((v - sv) * 0.8).powi(2)).sqrt()).filter(|r| *r < 7.);
            if let Some((u0, u1, v0, v1, k)) = board {
                if front && (u0..u1).contains(&u) && (v0..v1).contains(&v) {
                    let (fu, fv) = ((u - u0) / (u1 - u0), (v - v0) / (v1 - v0));
                    if fu < 0.04 || fu > 0.96 || fv < 0.06 || fv > 0.94 {
                        return Some(('█', scale(neon, (0.5 + 0.5 * pulse) * fog.max(0.3))));
                    }
                    // An equalizer that follows the music, or a scan, under
                    // a strip kept for the text.
                    let bar = flo(fu * 14.);
                    let level = if k % 2 == 0 {
                        noise3([bar * 0.7, t * 2.5, k as f32]) * (0.35 + sound.bass * 0.9) + treble * 0.2
                    } else {
                        0.5 + 0.5 * (bar * 0.5 - t * 3.).sin()
                    };
                    let tint = NEON[(k + 1) % 5];
                    return if fv < level.min(1.) * 0.62 {
                        Some(('█', scale(mix(tint, (255, 255, 255), fv * 0.5), (0.4 + 0.5 * fv) * fog.max(0.3))))
                    } else {
                        Some(('░', scale(tint, 0.18 * fog.max(0.3))))
                    };
                }
            }
            let (cu, cw) = (u * (1. / 1.1), v * (1. / 1.45));
            let (col, row) = (flo(cu) as i32, flo(cw) as i32);
            let far = d > far_windows;
            // Near, a cell between windows is plain wall: skip the rest.
            let (fu, fv) = (cu - col as f32, cw - row as f32);
            let pane = (0.2..0.8).contains(&fu) && (0.22..0.78).contains(&fv);
            if !far && !pane && scan.is_none() {
                return wall();
            }
            let hsh = hash3(col, row, seed);
            // Windows switch on and off now and then; a beat's wave lights
            // the ones it passes.
            let wz = if front { z0 + u } else { z0 };
            let wave = near.iter().map(|&(fz, k)| (1. - (wz - fz).abs() / 3.).max(0.) * k).fold(0., f32::max);
            let on = wave > 0.2 || scan.is_some() || {
                let epoch = flo(t * 0.08 + hsh * 13.) as i32;
                hash3(col, row + epoch * 977, seed + 5) < lit
            };
            let base = WINDOWS[(hsh * 4.) as usize % 4];
            let tint = match scan {
                Some(r) => mix(HACK, (255, 80, 220), r * (1. / 7.)),
                None if wave > 0. => mix(base, (255, 255, 255), wave * 0.5),
                None => base,
            };
            if let Some(r) = scan {
                if fract((r - t * 9.) * (1. / 3.)) < 0.35 / 3. {
                    return Some(('░', scale(HACK, 0.8 * (1. - r / 7.) + 0.2)));
                }
            }
            if far {
                // Far: one cell is several windows, so light it by chance.
                return if on && hsh < 0.75 { Some(('▪', scale(tint, (0.18 + hsh * 0.4 + wave * 0.5) * fog))) } else { wall() };
            }
            if !pane {
                return wall();
            }
            if on {
                let bright = (0.2 + 0.25 * hsh + wave * 0.6 + scan.map_or(0., |r| 0.5 * (1. - r / 7.))).min(1.);
                let flick = if glitch > 0.3 && hsh > 0.9 { 0.3 } else { 1. };
                Some(('█', scale(tint, bright * fog * flick)))
            } else {
                Some(('▒', scale((24, 20, 44), fog)))
            }
        };
        // The face along the street, then the end that faces the flight.
        poly(cv, &[([x0, 0., z0], [0., 0., 0.]), ([x0, 0., z1], [z1 - z0, 0., 0.]), ([x0, h, z1], [z1 - z0, h, 0.]), ([x0, h, z0], [0., h, 0.])], &mut paint);
        if cv.cam.pos[2] < z0 {
            poly(cv, &[([x0, 0., z0], [0., 0., 1.]), ([x1, 0., z0], [(x1 - x0).abs(), 0., 1.]), ([x1, h, z0], [(x1 - x0).abs(), h, 1.]), ([x0, h, z0], [0., h, 1.])], &mut paint);
        }
        let d = (z0 - self.z).max(1.);
        let fog = (-d / 80.).exp();
        // Far edges and signs only crowd the vanishing point.
        if d > 70. {
            return;
        }
        // Neon edges, pulled a hair towards the street so they stay on top;
        // white on the building the cursor scans.
        let s = l.side;
        let edge = if touched.is_some() { scale((200, 255, 255), 0.7 + 0.3 * pulse) } else { scale(neon, (0.35 + 0.4 * pulse) * fog) };
        let e = |p: V| add(p, [-s * 0.06, 0., -0.06]);
        cv.line(e([x0, 2.7, z0]), e([x0, h, z0]), None, edge);
        cv.line(e([x0, h, z0]), e([x0, h, z1]), None, edge);
        cv.line(e([x0, h, z0]), e([x1, h, z0]), None, edge);
        if touched.is_some() {
            cv.line(e([x0, 2.7, z1]), e([x0, h, z1]), None, edge);
        }
        if h > 30. {
            let base = [x0 + s * 2., h, z0 + 3.];
            let top = add(base, [0., 4. + (h - 30.) * 0.1, 0.]);
            cv.line(base, top, None, scale((120, 120, 140), fog));
            if (t * 2.5 + seed as f32).sin() > 0.4 {
                cv.point(top, '●', scale((255, 50, 40), 0.5 + fog * 0.5));
            }
        }
        if let Some(word) = l.sign {
            let n = word.chars().count();
            let (y0, step) = (4.2, 1.35);
            let zs = z0 + 0.9;
            let lo = [x0.min(x0 - s * 1.3), y0 - 0.3, zs - 0.15];
            let hi = [x0.max(x0 - s * 1.3), y0 + n as f32 * step + 0.2, zs + 0.15];
            let mut box_paint = |_: [f32; 3], _: f32| Some(('█', scale((20, 14, 30), fog.max(0.3))));
            solid_box(cv, lo, hi, &mut box_paint);
            let blink = |k: usize| !(self.glitch > 0.25 && hash3(k as i32, (t * 12.) as i32, seed) < 0.5);
            let shine = (0.55 + 0.5 * pulse + treble * 0.3 * hash3(seed, (t * 20.) as i32, 4)) * fog.max(0.4);
            let color = scale(mix(neon, (255, 255, 255), 0.3), shine);
            let mut buf = [0u8; 4];
            for (k, ch) in word.chars().enumerate() {
                if !blink(k) {
                    continue;
                }
                let p = [x0 - s * 0.65, y0 + (n - 1 - k) as f32 * step + 0.6, zs - 0.2];
                let Some((x, y, z)) = cv.cam.project(p) else { continue };
                // Each letter as big as its slot on the sign allows.
                let letter = ch.encode_utf8(&mut buf);
                let k = type_fit(letter, 1.0 / z * f, step * 0.85 / z * cv.cam.f, 4);
                if k == 0 {
                    cv.dot(x, y, z - 0.1, ch, color);
                } else {
                    let (tw, th) = type_size(letter, k);
                    type_at(cv, (x - tw as f32 * 0.5).round() as i32, (y - th as f32 * 0.5).round() as i32, z - 0.1, letter, k, color);
                }
            }
            cv.line(add(lo, [0., 0., -0.2]), add([lo[0], hi[1], lo[2]], [0., 0., -0.2]), None, edge);
            cv.line(add([hi[0], lo[1], lo[2]], [0., 0., -0.2]), add([hi[0], hi[1], lo[2]], [0., 0., -0.2]), None, edge);
        }
        if let Some((u0, u1, _, v1, k)) = board {
            let text = match k % 3 {
                0 => format!("CPU {:.0}%", self.load.total * 100.),
                1 => format!("RAM {:.0}%", self.load.mem * 100.),
                _ => ["AMBIENT", "LIVE", "NO SIGNAL", "SYNC"][(seed as usize / 2) % 4].to_string(),
            };
            // The strip above the equalizer, as wide as the board looks
            // from here.
            let x = x0 - s * 0.1;
            let (a, b) = (cv.cam.project([x, v1 - 1., z0 + u0]), cv.cam.project([x, v1 - 1., z0 + u1]));
            if let (Some(a), Some(b)) = (a, b) {
                let mid = ((a.0 + b.0) * 0.5, (a.1 + b.1) * 0.5, a.2.min(b.2));
                let tall = 1.5 / mid.2 * cv.cam.f;
                tag_px(cv, mid, (a.0 - b.0).abs() * 0.9, tall, &text, scale((255, 255, 255), 0.6 + fog * 0.4));
            }
        }
    }
    fn holograms(&self, cv: &mut Canvas, sound: &Sound, far: f32) {
        let t = self.time;
        let first = (self.z / BLOCK).floor() as i64;
        for b in first..=(far / BLOCK).ceil() as i64 {
            let zc = b as f32 * BLOCK + LOT * LOTS as f32 + CROSS * 0.5 - 0.35;
            if zc < self.z + 3. || zc > far {
                continue;
            }
            let fog = (-(zc - self.z) / 90.).exp();
            let c = [0., 14. + (t * 0.8 + b as f32).sin() * 0.6, zc];
            let size = 2.3 * (1. + sound.bass * 0.5);
            let spin = t * (0.5 + sound.treble * 1.5) + b as f32;
            let place = |p: V, dx: f32| add(add(c, [dx, 0., 0.]), mul(yaw(tilt(p, 0.5), spin), size / 1.9));
            let color = scale((80, 240, 255), (0.45 + 0.5 * sound.bass) * fog);
            for &(i, j) in &self.edges {
                cv.line(place(ICO[i], 0.), place(ICO[j], 0.), None, color);
            }
            if self.glitch > 0.2 {
                let ghost = scale((255, 60, 200), self.glitch * fog);
                for &(i, j) in self.edges.iter().step_by(2) {
                    cv.line(place(ICO[i], 0.35), place(ICO[j], 0.35), None, ghost);
                }
            }
            // The projector's beam from the crossing.
            cv.line([0., 0.2, zc], add(c, [0., -size, 0.]), Some(':'), scale((60, 160, 200), 0.4 * fog));
        }
    }
}

// =========================================================== packet grid
const HORIZON: f32 = 150.;
const TOWERS: usize = 24;
/// How far round the cursor's mark packets are drawn in.
const SNARE: f32 = 7.;
struct Packet {
    x: f32,
    z: f32,
    v: f32,
    size: f32,
    /// How strongly the cursor holds it, 0 to 1.
    held: f32,
}
pub(crate) struct PacketGrid {
    net: Counter,
    time: f32,
    z: f32,
    packets: Vec<Packet>,
    budget: (f32, f32),
    flash: f32,
    /// Rings of light the beats roll down the grid towards the camera.
    ripples: Vec<f32>,
    stripes: f32,
    gaze: (f32, f32),
    cursor: bool,
    /// Where the cursor meets the floor, found while drawing.
    aim: Cell<Option<(f32, f32)>>,
    stars: Vec<Star>,
    pulse: Pulse,
    rng: Rng,
}
impl PacketGrid {
    pub(crate) fn new() -> Self {
        let mut rng = Rng::new();
        let stars = starfield(700, &mut rng).into_iter().filter(|s| s.dir[1] > 0.03).collect();
        Self {
            net: Counter::new(Feed::Net),
            time: 0.,
            z: 0.,
            packets: vec![],
            budget: (0., 0.),
            flash: 0.,
            ripples: vec![],
            stripes: 0.,
            gaze: (0., 0.),
            cursor: false,
            aim: Cell::new(None),
            stars,
            pulse: Pulse::default(),
            rng,
        }
    }
    fn eye(&self) -> (V, V) {
        let (t, g) = (self.time, self.gaze);
        let eye = [g.0 * 5. + (t * 0.1).sin() * 0.8, (2.6 - g.1 * 1.3).clamp(1.3, 4.2), self.z];
        (eye, [g.0 * 3., 2. - g.1 * 0.6, self.z + 30.])
    }
    fn step(&mut self, dt: f32, sound: &Sound, cursor: Option<(f32, f32)>, gaze: (f32, f32)) {
        self.time += dt;
        self.gaze = gaze;
        self.cursor = cursor.is_some();
        self.net.step(dt);
        let (rx, tx) = self.net.smooth;
        let speed = 3. + (level(rx) + level(tx)) * 5. + sound.bass * 5.;
        self.z += speed * dt;
        self.stripes += dt * (0.6 + sound.bass * 2.5);
        // The grid repeats every 2 units: wrapping keeps f32 precise.
        if self.z > 4096. {
            self.z -= 4096.;
            for p in &mut self.packets {
                p.z -= 4096.;
            }
            for r in &mut self.ripples {
                *r -= 4096.;
            }
        }
        if let Some(k) = self.pulse.hit(sound.bass, dt) {
            self.flash = self.flash.max(k.min(1.));
            if self.ripples.len() < 6 {
                self.ripples.push(self.z + HORIZON * 0.8);
            }
        }
        self.flash *= (-dt * 4.).exp();
        for r in &mut self.ripples {
            *r -= dt * 55.;
        }
        let z = self.z;
        self.ripples.retain(|r| *r > z);
        // Downloads come at you, uploads run away: one pulse per slice of
        // traffic, a few always, a storm when the link is busy.
        self.budget.0 += dt * (1.5 + level(rx).powf(1.5) * 110.);
        self.budget.1 += dt * (1.5 + level(tx).powf(1.5) * 110.);
        while self.budget.0 >= 1. {
            self.budget.0 -= 1.;
            let x = (self.rng.signed() * 11.).round() * 2.;
            self.packets.push(Packet { x, z: self.z + HORIZON * (0.6 + self.rng.next() * 0.35), v: -(12. + self.rng.next() * 16.), size: level(rx), held: 0. });
        }
        while self.budget.1 >= 1. {
            self.budget.1 -= 1.;
            let x = (self.rng.signed() * 11.).round() * 2.;
            self.packets.push(Packet { x, z: self.z + 1., v: speed + 10. + self.rng.next() * 16., size: level(tx), held: 0. });
        }
        // The cursor's mark draws passing packets in.
        let aim = self.aim.get().filter(|_| self.cursor);
        let pull = 1. - (-dt * 5.).exp();
        for p in &mut self.packets {
            p.z += p.v * dt;
            let want = aim.map_or(0., |(ax, az)| {
                let d = ((p.x - ax).powi(2) + (p.z - az).powi(2) * 0.25).sqrt();
                (1. - d / SNARE).max(0.)
            });
            if let Some((ax, _)) = aim {
                p.x += (ax - p.x) * pull * want;
            }
            p.held += (want - p.held) * pull;
        }
        self.packets.retain(|p| p.z > z - 1. && p.z < z + HORIZON);
        if self.packets.len() > 3000 {
            let n = self.packets.len() - 3000;
            self.packets.drain(..n);
        }
    }
    fn draw(&self, cv: &mut Canvas, sound: &Sound) {
        let t = self.time;
        let (eye, target) = self.eye();
        cv.cam = Cam::look(eye, target, 1.2, cv.cols, cv.rows, cv.cam.aspect);
        let spot = under(cv, self.gaze);
        let aim = self.cursor.then(|| cv.cam.ray(spot.0, spot.1)).filter(|d| d[1] < -1e-3).map(|d| add(eye, mul(d, -eye[1] / d[1]))).filter(|p| p[2] < self.z + HORIZON * 0.9).map(|p| (p[0], p[2]));
        self.aim.set(aim);
        draw_sky(cv, &self.stars, t, sound.treble);
        self.sun(cv, sound);
        self.mountains(cv, sound);
        // The floor is the grid, drawn per cell: lines a cell wide at any
        // distance, fading to haze where they would crowd together. Beats
        // roll rings of light over it; the cursor's mark glows.
        let (z0, z1) = (self.z - 1., self.z + HORIZON);
        let flash = self.flash;
        let (f, aspect, ex, ey) = (cv.cam.f, cv.cam.aspect, eye[0], eye[1].max(0.3));
        let rings = &self.ripples[..];
        let base = mix((170, 50, 255), (255, 200, 255), flash * 0.6);
        let mut floor = |a: [f32; 3], d: f32| -> Option<(char, Color)> {
            let (x, z) = (a[0], a[1]);
            let fog = fade((z - self.z) * (1. / 55.));
            let ring = rings.iter().map(|&r| (1. - (z - r).abs() / (0.8 + (r - self.z) * 0.03)).max(0.)).fold(0., f32::max);
            let glow = aim.map_or(0., |(ax, az)| (1. - ((x - ax).powi(2) + (z - az).powi(2)).sqrt() / 3.5).max(0.));
            let lit = ring.max(glow);
            let color = scale(if lit > 0. { mix(base, (120, 255, 255), lit) } else { base }, (0.25 + 0.75 * fog) * (0.8 + flash * 0.5 + ring * 0.8 + glow));
            // One cell's footprint on the floor, across and along.
            let (wx, wz) = (d / (f * aspect), d * d / (f * ey));
            // Far off the cross lines thin out, doubling their spacing.
            let mut spacing = 2.;
            while wz * 2.5 > spacing && spacing < 64. {
                spacing *= 2.;
            }
            let (gx, gz) = (flo(x * 0.5 + 0.5) * 2., flo(z / spacing + 0.5) * spacing);
            let on_x = (x - gx).abs() < wx * 0.5 && gx.abs() <= 32.;
            let on_z = (z - gz).abs() < wz * 0.5;
            match (on_x, on_z) {
                (true, true) => Some(('+', color)),
                (false, true) => Some((if wz < 0.35 { '─' } else { '-' }, color)),
                (true, false) => {
                    let slope = (gx - ex) * aspect / ey;
                    let ch = if slope.abs() < 0.6 { '│' } else if slope.abs() > 3. { '_' } else if slope > 0. { '\\' } else { '/' };
                    Some((ch, color))
                }
                _ if ring > 0.3 || glow > 0.2 => Some(('·', scale(color, 0.5 * lit))),
                _ => Some((' ', (0, 0, 0))),
            }
        };
        poly(cv, &[([-300., 0., z0], [-300., z0, 0.]), ([300., 0., z0], [300., z0, 0.]), ([300., 0., z1], [300., z1, 0.]), ([-300., 0., z1], [-300., z1, 0.])], &mut floor);
        self.towers(cv);
        let mut caught = 0;
        for p in &self.packets {
            let d = p.z - self.z;
            let k = (-d / 70.).exp();
            if k < 0.04 {
                continue;
            }
            caught += (p.held > 0.3) as usize;
            let color: Color = mix(if p.v < 0. { (70, 240, 255) } else { (255, 80, 220) }, (255, 255, 255), p.held * 0.7);
            let tail = (2. + p.size * 6.) * -p.v.signum() * (1. + sound.bass * 0.6);
            let lift = 0.05 + p.held * 0.5;
            cv.line([p.x, lift, p.z], [p.x, lift, p.z + tail], None, scale(color, 0.5 * k));
            let head = if (d < 30. && p.size > 0.5) || p.held > 0.5 { '█' } else if d < 50. { '■' } else { '▪' };
            let sparkle = if sound.treble > 0.3 && hash3(p.x as i32, (p.z * 3.) as i32, (t * 8.) as i32) < sound.treble * 0.3 { 0.5 } else { 0. };
            cv.point([p.x, lift + 0.03, p.z], head, scale(mix(color, (255, 255, 255), 0.4 + sparkle), 0.6 + 0.5 * k));
        }
        if let Some((ax, az)) = aim {
            // The mark: a ring on the floor and a beam above it.
            let spin = t * 2.;
            let n = 40;
            for i in 0..n {
                let (a, b) = (i as f32 / n as f32 * TAU + spin, (i as f32 + 0.6) / n as f32 * TAU + spin);
                if i % 5 == 4 {
                    continue;
                }
                cv.line([ax + a.cos() * 3., 0.03, az + a.sin() * 3.], [ax + b.cos() * 3., 0.03, az + b.sin() * 3.], None, (120, 255, 255));
            }
            cv.line([ax, 0.1, az], [ax, 6., az], Some(':'), scale((120, 255, 255), 0.6 + flash * 0.4));
            tag(cv, [ax, 6.8, az], (7., 1.6), &format!("{caught} PKT"), (200, 255, 255));
        }
        let (rx, tx) = self.net.smooth;
        let text = format!("{}   ▼ {}   ▲ {}", self.net.label.to_uppercase(), rate(rx), rate(tx));
        hud(cv, &[(&text, (200, 190, 255))]);
    }
    fn sun(&self, cv: &mut Canvas, sound: &Sound) {
        let Some((sx, sy)) = cv.cam.sky(norm([0., 0.09, 1.])) else { return };
        let r = cv.cam.f * 0.3 * (1. + sound.bass * 0.14);
        let aspect = cv.cam.aspect;
        for y in (sy - r).floor().max(0.) as i32..=((sy + r).ceil() as i32).min(cv.rows as i32 - 1) {
            let dy = (y as f32 + 0.5 - sy) / r;
            if dy.abs() > 1. {
                continue;
            }
            // Stripes widen towards the bottom and drift down, faster with
            // the bass.
            if dy > 0. {
                let band = fract(dy * 7. - self.stripes);
                if band < 0.12 + dy * 0.45 {
                    continue;
                }
            }
            let half = (1. - dy * dy).sqrt() * r * aspect;
            let color = grad(&[(0., (255, 230, 120)), (0.45, (255, 140, 70)), (1., (255, 40, 160))], (dy + 1.) * 0.5);
            for x in (sx - half).max(0.) as i32..=((sx + half) as i32).min(cv.cols as i32 - 1) {
                cv.r.dot(x, y, 9e4, '█', scale(color, 0.8 + sound.bass * 0.3));
            }
        }
        halo(cv, (sx, sy), r, 9.5e4, self.time, (255, 90, 180), &[(1.12, 0.35 + self.flash * 0.3), (1.3, 0.18 + self.flash * 0.2)], 0.35);
    }
    fn mountains(&self, cv: &mut Canvas, sound: &Sound) {
        let z = self.z + HORIZON;
        let ridge = |x: f32| {
            let valley = smoothstep(10., 40., x.abs());
            (noise3([x * 0.018 + 3., 0., 1.]).powf(1.4) * 34. + noise3([x * 0.07, 2., 0.]) * 8.) * valley * (1. + sound.bass * 0.35)
        };
        let step = 6.;
        let mut dark = |_: [f32; 3], _: f32| Some(('█', (14, 4, 26)));
        let line = scale((255, 60, 200), 0.6 + sound.bass * 0.4 + self.flash * 0.3);
        let dim = (110, 30, 150);
        let mut x = -260.;
        let mut h0 = ridge(x);
        while x < 260. {
            let h1 = ridge(x + step);
            let (a, b) = ([x, 0., z], [x + step, 0., z]);
            let (ta, tb) = ([x, h0, z], [x + step, h1, z]);
            poly(cv, &[(a, [0.; 3]), (b, [0.; 3]), (tb, [0.; 3]), (ta, [0.; 3])], &mut dark);
            cv.line(add(ta, [0., 0., -0.5]), add(tb, [0., 0., -0.5]), None, line);
            if h0 > 1. {
                cv.line(add(ta, [0., 0., -0.4]), add(b, [0., 0., -0.4]), None, dim);
            }
            x += step;
            h0 = h1;
        }
    }
    fn towers(&self, cv: &mut Canvas) {
        let hist = &self.net.hist;
        let scroll = self.net.clock / COUNT_TICK;
        for (k, &(rx, tx)) in hist.iter().take(TOWERS).enumerate() {
            let z = self.z + 7. + (k as f32 + scroll) * 5.;
            let fog = (-(z - self.z) / 70.).exp();
            for (side, v, tint) in [(-1f32, rx, (70, 240, 255)), (1., tx, (255, 80, 220))] {
                let h = 0.3 + level(v) * 13.;
                let x = side * 17.;
                let lo = [x - 1., 0., z];
                let hi = [x + 1., h, z + 2.5];
                let mut paint = |a: [f32; 3], _: f32| {
                    let l = if a[2] < 0.5 { 0.9 } else { 0.25 + 0.55 * (a[1] / h.max(0.1)) };
                    Some((block(l), scale(tint, (0.25 + 0.6 * l) * fog)))
                };
                solid_box(cv, lo, hi, &mut paint);
                cv.line([x - 1.05, h + 0.05, z - 0.05], [x + 1.05, h + 0.05, z - 0.05], None, scale(mix(tint, (255, 255, 255), 0.5), fog));
                if k == 0 {
                    tag(cv, [x, h + 1.2, z], (5., 1.4), &rate(v), scale(tint, 0.9));
                }
            }
        }
    }
}

// =============================================================== silicon
const BOARD: f32 = 17.;
const PKG: f32 = 6.;
const DIE: f32 = 4.6;
/// The cursor's pick when it rests on the memory.
const PICK_RAM: usize = usize::MAX;
struct Signal {
    trace: usize,
    t: f32,
    v: f32,
    color: Color,
}
/// One core on the die: footprint and height, load, and its name.
struct Core {
    lo: V,
    hi: V,
    v: f32,
    name: String,
}
pub(crate) struct Silicon {
    load: SysLoad,
    disk: Counter,
    net: Counter,
    time: f32,
    traces: Vec<(Vec<V>, f32)>,
    signals: Vec<Signal>,
    budget: [f32; 4],
    sparks: Vec<Mote>,
    rings: Vec<f32>,
    cores: Vec<Core>,
    /// The copper lights up on a beat and fades.
    zap: f32,
    clock: f32,
    bass: f32,
    gaze: (f32, f32),
    cursor: bool,
    /// What the cursor rests on, found while drawing.
    pick: Cell<Option<usize>>,
    pulse: Pulse,
    rng: Rng,
}
/// Lays the cores out on a square on the die; the bass lifts them a little.
fn arrange(groups: &[(f32, String)], bass: f32) -> Vec<Core> {
    let n = groups.len().max(1);
    let cols = (n as f32).sqrt().ceil() as usize;
    let rows = n.div_ceil(cols);
    let cell = (DIE * 2. - 1.6) / cols.max(rows) as f32;
    let (w, d) = (cols as f32 * cell, rows as f32 * cell);
    groups
        .iter()
        .enumerate()
        .map(|(i, (v, name))| {
            let v = *v;
            let (cx, cz) = ((i % cols) as f32 * cell - w * 0.5 + cell * 0.5, (i / cols) as f32 * cell - d * 0.5 + cell * 0.5);
            let half = cell * 0.39;
            let h = 0.15 + v * 2.6 + bass * (0.2 + v * 0.5);
            Core { lo: [cx - half, 0.55, cz - half], hi: [cx + half, 0.55 + h, cz + half], v, name: name.to_uppercase() }
        })
        .collect()
}
impl Silicon {
    pub(crate) fn new() -> Self {
        let mut rng = Rng::new();
        let mut traces = vec![];
        for side in 0..4 {
            for k in 0..11 {
                let s = -5. + k as f32 * 1.0;
                let out = 0.8 + rng.next() * 3.;
                let jog = (rng.signed() * 2.5).round();
                let local = [
                    [s, PKG + 0.1],
                    [s, PKG + 0.1 + out],
                    [s + jog, PKG + 0.1 + out + jog.abs()],
                    [s + jog, BOARD - 0.3],
                ];
                let pts: Vec<V> = local
                    .iter()
                    .map(|&[a, b]| match side {
                        0 => [a, 0.03, b],
                        1 => [b, 0.03, -a],
                        2 => [-a, 0.03, -b],
                        _ => [-b, 0.03, a],
                    })
                    .collect();
                let length = pts.windows(2).map(|w| len(sub(w[1], w[0]))).sum();
                traces.push((pts, length));
            }
        }
        let load = SysLoad::new();
        let cores = arrange(&SysLoad::groups(&load.cores, 16), 0.);
        Self {
            load,
            disk: Counter::new(Feed::Disk),
            net: Counter::new(Feed::Net),
            time: 0.,
            traces,
            signals: vec![],
            budget: [0.; 4],
            sparks: vec![],
            rings: vec![],
            cores,
            zap: 0.,
            clock: 0.,
            bass: 0.,
            gaze: (0., 0.),
            cursor: false,
            pick: Cell::new(None),
            pulse: Pulse::default(),
            rng,
        }
    }
    fn step(&mut self, dt: f32, sound: &Sound, cursor: Option<(f32, f32)>, gaze: (f32, f32)) {
        self.time += dt;
        self.gaze = gaze;
        self.cursor = cursor.is_some();
        self.bass = sound.bass;
        self.load.step(dt);
        self.disk.step(dt);
        self.net.step(dt);
        self.cores = arrange(&SysLoad::groups(&self.load.cores, 16), sound.bass);
        self.clock += dt * (1.5 + sound.bass * 3.);
        let beat = self.pulse.hit(sound.bass, dt);
        if let Some(k) = beat {
            if self.rings.len() < 6 {
                self.rings.push(PKG + 0.6 - k * 0.2);
            }
            self.zap = self.zap.max(k.min(1.));
        }
        self.zap *= (-dt * 5.).exp();
        for r in &mut self.rings {
            *r += dt * 7.;
        }
        self.rings.retain(|r| *r < BOARD * 1.2);
        // Disk reads and network downloads flow to the die, writes and
        // uploads away from it; treble hurries them.
        let flows = [
            (self.disk.smooth.0, -1., (80, 220, 255)),
            (self.disk.smooth.1, 1., (255, 180, 60)),
            (self.net.smooth.0, -1., (90, 255, 150)),
            (self.net.smooth.1, 1., (230, 120, 255)),
        ];
        for (k, &(bps, dir, color)) in flows.iter().enumerate() {
            self.budget[k] += dt * (0.6 + level(bps).powf(1.3) * 45. + beat.map_or(0., |b| b * 20.));
            while self.budget[k] >= 1. {
                self.budget[k] -= 1.;
                if self.signals.len() >= 1500 {
                    continue;
                }
                let trace = (self.rng.next() * self.traces.len() as f32) as usize % self.traces.len();
                let v = (5. + self.rng.next() * 6.) / self.traces[trace].1 * dir;
                self.signals.push(Signal { trace, t: if dir < 0. { 1. } else { 0. }, v, color });
            }
        }
        let hurry = 1. + sound.treble * 1.2;
        for s in &mut self.signals {
            s.t += s.v * hurry * dt;
        }
        self.signals.retain(|s| (0. ..=1.).contains(&s.t));
        // Busy cores give off heat, and the one under the cursor more.
        let picked = self.pick.get().filter(|_| self.cursor);
        for (i, c) in self.cores.iter().enumerate() {
            let rate = c.v * c.v * 12. + beat.map_or(0., |k| k * 4.) + if picked == Some(i) { 14. } else { 0. };
            let count = (rate * dt + self.rng.next()) as usize;
            for _ in 0..count.min(6) {
                if self.sparks.len() >= 1200 {
                    break;
                }
                let p = [c.lo[0] + (c.hi[0] - c.lo[0]) * self.rng.next(), c.hi[1], c.lo[2] + (c.hi[2] - c.lo[2]) * self.rng.next()];
                self.sparks.push(Mote { p, v: [self.rng.signed() * 0.3, 1.2 + self.rng.next() * 1.8 + sound.bass * 2., self.rng.signed() * 0.3], life: 1. });
            }
        }
        for m in &mut self.sparks {
            m.p = add(m.p, mul(m.v, dt));
            m.life -= dt * 0.8;
        }
        self.sparks.retain(|m| m.life > 0.);
    }
    fn ram_chip(k: usize) -> (V, V) {
        let z = -7.2 + k as f32 * 1.85;
        ([9.2, 0., z], [12.4, 0.35, z + 1.35])
    }
    /// The core, or the memory, the cursor's ray meets first.
    fn pick_at(&self, o: V, d: V) -> Option<usize> {
        let mut best: Option<(f32, usize)> = None;
        let mut try_box = |lo: V, hi: V, id: usize| {
            if let Some(t) = hit_box(o, d, lo, hi) {
                if best.map_or(true, |b| t < b.0) {
                    best = Some((t, id));
                }
            }
        };
        for (i, c) in self.cores.iter().enumerate() {
            // The gap round a core counts too: small targets are hard to hit.
            let pad = (c.hi[0] - c.lo[0]) * 0.14;
            try_box([c.lo[0] - pad, 0.4, c.lo[2] - pad], [c.hi[0] + pad, c.hi[1], c.hi[2] + pad], i);
        }
        let (lo, _) = Self::ram_chip(0);
        let (_, hi) = Self::ram_chip(7);
        try_box(lo, hi, PICK_RAM);
        best.map(|b| b.1)
    }
    fn draw(&self, cv: &mut Canvas, sound: &Sound) {
        let t = self.time;
        let az = t * 0.035 + self.gaze.0 * 1.2;
        let el = (0.8 - self.gaze.1 * 0.4).clamp(0.38, 1.25);
        let target = [1., 0.4, 0.];
        cv.cam = Cam::look(orbit(target, az, el, 20. - self.bass * 1.2), target, 1.0, cv.cols, cv.rows, cv.cam.aspect);
        let spot = under(cv, self.gaze);
        let picked = if self.cursor { self.pick_at(cv.cam.pos, cv.cam.ray(spot.0, spot.1)) } else { None };
        self.pick.set(picked);
        let flash = self.pulse.slow.min(1.) * 0.3 + sound.bass * 0.3;
        // Nearest first, so the depth test spares the shading of what
        // they hide: the cores, the die, the package, then the board.
        let few = self.cores.len() <= 16;
        for (i, c) in self.cores.iter().enumerate() {
            let (lo, hi, v) = (c.lo, c.hi, c.v);
            let hot = picked == Some(i);
            let heat = grad(&[(0., (40, 90, 255)), (0.3, (40, 220, 220)), (0.55, (90, 255, 110)), (0.75, (255, 220, 60)), (1., (255, 60, 40))], v);
            let lift = if hot { 0.3 } else { 0. };
            let h = hi[1] - lo[1];
            let mut paint = |a: [f32; 3], _: f32| {
                if a[2] < 0.5 {
                    let busy = noise2(a[0] * 3. + t * (0.6 + v * 4.), a[1] * 3. - t * (0.4 + v * 3.));
                    let l = 0.55 + 0.35 * v + busy * 0.25 + lift;
                    Some((block(l), scale(heat, 0.55 + 0.45 * l)))
                } else {
                    let up = (a[1] - lo[1]) / h.max(0.05);
                    let l = 0.3 + 0.4 * up + lift;
                    Some((block(l), scale(heat, 0.25 + 0.4 * l)))
                }
            };
            solid_box(cv, lo, hi, &mut paint);
            let top = [(lo[0] + hi[0]) * 0.5, hi[1], (lo[2] + hi[2]) * 0.5];
            let width = hi[0] - lo[0];
            if hot {
                // Outline the core and name it, big.
                let y = hi[1] + 0.02;
                let corners = [[lo[0], y, lo[2]], [hi[0], y, lo[2]], [hi[0], y, hi[2]], [lo[0], y, hi[2]]];
                for k in 0..4 {
                    cv.line(corners[k], corners[(k + 1) % 4], None, (230, 250, 255));
                    cv.line(corners[k], [corners[k][0], lo[1], corners[k][2]], None, scale((230, 250, 255), 0.6));
                }
                tag(cv, add(top, [0., 1.6, 0.]), (7., 1.5), &format!("{} {:.0}%", c.name, v * 100.), (240, 250, 255));
            } else if few {
                tag(cv, add(top, [0., 0.5, 0.]), (width * 1.1, width * 0.45), &format!("{:.0}%", v * 100.), scale(mix(heat, (255, 255, 255), 0.5), 1.));
            }
        }
        let mut die = |a: [f32; 3], _: f32| {
            if a[2] < 0.5 {
                let check = (flo(a[0] * 3.) + flo(a[1] * 3.)) as i32 & 1 == 0;
                let shimmer = noise2(a[0] * 2. + t * 0.9, a[1] * 2. - t * 0.6) * flash;
                let l = if check { 0.3 } else { 0.42 } + shimmer;
                Some((if check { '░' } else { '▒' }, scale((90, 110, 160), l)))
            } else {
                Some(('█', (50, 56, 80)))
            }
        };
        solid_box(cv, [-DIE, 0.4, -DIE], [DIE, 0.55, DIE], &mut die);
        let mut substrate = |a: [f32; 3], _: f32| {
            if a[2] < 0.5 {
                let edge = PKG - a[0].abs().max(a[1].abs());
                if edge < 0.5 && fract((a[0] + a[1]) * 2.) < 0.5 {
                    return Some(('▪', mix((200, 160, 70), (255, 240, 180), self.zap)));
                }
                Some(('▓', (34, 40, 44)))
            } else {
                Some(('█', (22, 26, 30)))
            }
        };
        solid_box(cv, [-PKG, 0., -PKG], [PKG, 0.4, PKG], &mut substrate);
        // Memory: eight chips that fill with use.
        let mem = self.load.mem;
        let ram_hot = picked == Some(PICK_RAM);
        for k in 0..8 {
            let (lo, hi) = Self::ram_chip(k);
            let fill = (mem * 8. - k as f32).clamp(0., 1.);
            let mut chip = |a: [f32; 3], _: f32| {
                if a[2] < 0.5 {
                    let f = (a[0] - lo[0]) / (hi[0] - lo[0]);
                    if f < fill {
                        let busy = noise2(a[0] * 4. + t * 2., a[1] * 4. - t * 1.5);
                        Some(('▓', scale((90, 160, 255), 0.6 + busy * 0.4 + flash * 0.3 + ram_hot as u8 as f32 * 0.3)))
                    } else {
                        Some(('▒', (28, 32, 48)))
                    }
                } else {
                    Some(('█', (18, 20, 30)))
                }
            };
            solid_box(cv, lo, hi, &mut chip);
            if ram_hot {
                let y = hi[1] + 0.02;
                cv.line([lo[0], y, lo[2]], [hi[0], y, lo[2]], None, (200, 230, 255));
                cv.line([lo[0], y, hi[2]], [hi[0], y, hi[2]], None, (200, 230, 255));
            }
        }
        let ram = format!("RAM {:.1}/{:.0} GIB", mem * self.load.mem_total_gib, self.load.mem_total_gib);
        let (at, size) = if ram_hot { ([10.8, 2.2, 0.], (11., 2.)) } else { ([10.8, 0.9, 7.8], (6., 1.)) };
        tag(cv, at, size, &ram, (150, 190, 255));
        let copper = mix((120, 92, 40), (255, 220, 140), self.zap);
        for (pts, _) in &self.traces {
            for w in pts.windows(2) {
                long_line(cv, w[0], w[1], 1.2, |_| Some(copper));
            }
        }
        for s in &self.signals {
            let (pts, length) = &self.traces[s.trace];
            let at = |t: f32| -> V {
                let mut d = t.clamp(0., 1.) * length;
                for w in pts.windows(2) {
                    let l = len(sub(w[1], w[0]));
                    if d <= l {
                        return add(w[0], mul(sub(w[1], w[0]), d / l.max(1e-3)));
                    }
                    d -= l;
                }
                pts[pts.len() - 1]
            };
            let head = add(at(s.t), [0., 0.08, 0.]);
            let tail = add(at(s.t - s.v.signum() * 0.9 / length), [0., 0.08, 0.]);
            cv.line(tail, head, None, scale(s.color, 0.5));
            cv.point(head, '■', s.color);
        }
        // The board, with its copper grid.
        let mut board = |a: [f32; 3], d: f32| {
            let (x, z) = (a[0], a[1]);
            let fog = fade((d - 18.).max(0.) * (1. / 40.));
            let near_grid = (x - flo(x + 0.5)).abs() < 0.12 && (z - flo(z + 0.5)).abs() < 0.12;
            if near_grid {
                Some(('·', scale((40, 120, 70), fog)))
            } else {
                Some(('░', scale((8, 34, 20), fog)))
            }
        };
        poly(cv, &[([-BOARD, -0.05, -BOARD], [-BOARD, -BOARD, 0.]), ([BOARD, -0.05, -BOARD], [BOARD, -BOARD, 0.]), ([BOARD, -0.05, BOARD], [BOARD, BOARD, 0.]), ([-BOARD, -0.05, BOARD], [-BOARD, BOARD, 0.])], &mut board);
        // The clock ring, and the beats spreading over the board.
        let n = 160;
        let bright = 0.35 + sound.bass * 0.8;
        for i in 0..n {
            let a = i as f32 / n as f32 * TAU;
            if fract(a / TAU * 24. - self.clock) < 0.45 {
                continue;
            }
            cv.point([a.cos() * (PKG + 0.6), 0.05, a.sin() * (PKG + 0.6)], '·', scale((80, 230, 255), bright));
        }
        for &r in &self.rings {
            let fade = (1. - r / (BOARD * 1.2)).max(0.);
            let m = (TAU * r * 3.) as usize;
            for i in 0..m {
                let a = i as f32 / m as f32 * TAU;
                let p = [a.cos() * r, 0.04, a.sin() * r];
                if p[0].abs() < BOARD && p[2].abs() < BOARD {
                    cv.point(p, '·', scale((120, 230, 255), fade * 0.8));
                }
            }
        }
        for m in &self.sparks {
            let l = m.life;
            cv.point(m.p, speck(l), scale(mix((255, 60, 30), (255, 210, 120), l), 0.4 + l * 0.6));
        }
        if self.cursor {
            reticle(cv, spot, if picked.is_some() { 2. } else { 1.2 }, scale((200, 240, 255), if picked.is_some() { 1. } else { 0.5 }));
        }
        let io = format!(
            "DISK R {}  W {}    NET ▼ {}  ▲ {}",
            rate(self.disk.smooth.0),
            rate(self.disk.smooth.1),
            rate(self.net.smooth.0),
            rate(self.net.smooth.1)
        );
        hud(cv, &[(&self.load.hud(), (180, 200, 230)), (&io, (160, 180, 210))]);
    }
}

// ================================================================= hooks
impl NeonCity {
    pub(super) fn tick(&mut self, dt: f32, sound: &Sound, cursor: Option<(f32, f32)>, gaze: (f32, f32)) {
        self.step(dt, sound, cursor, gaze)
    }
    pub(super) fn paint(&self, cv: &mut Canvas, sound: &Sound) {
        self.draw(cv, sound)
    }
}
impl PacketGrid {
    pub(super) fn tick(&mut self, dt: f32, sound: &Sound, cursor: Option<(f32, f32)>, gaze: (f32, f32)) {
        self.step(dt, sound, cursor, gaze)
    }
    pub(super) fn paint(&self, cv: &mut Canvas, sound: &Sound) {
        self.draw(cv, sound)
    }
}
impl Silicon {
    pub(super) fn tick(&mut self, dt: f32, sound: &Sound, cursor: Option<(f32, f32)>, gaze: (f32, f32)) {
        self.step(dt, sound, cursor, gaze)
    }
    pub(super) fn paint(&self, cv: &mut Canvas, sound: &Sound) {
        self.draw(cv, sound)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn canvas(r: &mut Raster, cols: usize, rows: usize) -> Canvas<'_> {
        let cam = Cam::look([0., 1., 0.], [0., 1., 10.], 1., cols, rows, 2.);
        Canvas { r, cols, rows, cam }
    }
    fn beats(t: f32) -> Sound {
        let beat = (t * TAU * 2.).sin().max(0.).powi(6);
        Sound { bass: 0.2 + beat * 0.7, treble: 0.4 }
    }

    #[test]
    fn polygons_clip_at_the_near_plane_and_keep_perspective() {
        // A floor from behind the camera to far ahead: still drawn, and a
        // checker in world units stays the same size at the same depth.
        let mut r = Raster::new(80, 40);
        let mut cv = canvas(&mut r, 80, 40);
        let mut seen = vec![];
        let mut paint = |a: [f32; 3], d: f32| {
            seen.push((a, d));
            Some(('#', (255, 255, 255)))
        };
        poly(&mut cv, &[([-5., 0., -5.], [-5., -5., 0.]), ([5., 0., -5.], [5., -5., 0.]), ([5., 0., 50.], [5., 50., 0.]), ([-5., 0., 50.], [-5., 50., 0.])], &mut paint);
        assert!(seen.len() > 200, "the floor is drawn: {}", seen.len());
        // Perspective-correct: the world z read back matches the depth.
        for (a, d) in &seen {
            assert!((a[1] - d).abs() < d * 0.06 + 0.3, "z {} at depth {}", a[1], d);
        }
    }

    #[test]
    fn cell_maths_match_libm() {
        for i in -400..400 {
            let x = i as f32 * 0.137;
            assert_eq!(flo(x), x.floor(), "{x}");
            assert!((fract(x) - x.rem_euclid(1.)).abs() < 1e-5);
            if x >= 0. {
                assert!((fade(x) - (-x).exp()).abs() <= (-x).exp() * 0.005 + 1e-7, "{x}");
            }
        }
    }

    #[test]
    fn rates_read_and_print() {
        assert_eq!(rate(512.), "512 B/s");
        assert_eq!(rate(1_500_000.), "1.5 MB/s");
        assert!(level(0.) < 0.01 && level(1e8) > 0.99);
        let mut c = Counter::new(Feed::Net);
        for _ in 0..4 {
            c.step(COUNT_TICK);
        }
        assert!(c.smooth.0 >= 0. && c.smooth.1 >= 0.);
        assert!(!c.hist.is_empty());
    }

    #[test]
    fn text_grows_with_the_room_it_has() {
        assert_eq!(type_size("CPU 5%", 0), (6, 1));
        assert_eq!(type_size("CPU 5%", 2), (46, 5));
        assert_eq!(type_fit("CPU 5%", 200., 40., 4), 4);
        assert_eq!(type_fit("CPU 5%", 30., 4., 4), 1);
        assert_eq!(type_fit("CPU 5%", 10., 4., 4), 0);
        // Every character the scenes print has a glyph.
        for c in "0123456789ABCDEFGHIJKLMNOPQRSTUVWXYZ%./:-+▲▼".chars() {
            assert_ne!(glyph(c), [0; 5], "{c}");
        }
        // The status line is plain on a small grid and big on a dense one.
        for (rows, tall) in [(40, 1), (130, 5), (200, 8)] {
            let mut r = Raster::new(320, rows);
            let mut cv = canvas(&mut r, 320, rows);
            hud(&mut cv, &[("NET 1.2 MB/S", (255, 255, 255))]);
            let mut out = vec![];
            r.recycle(&mut out);
            let ys: std::collections::BTreeSet<_> = out.iter().map(|c| c.y).collect();
            assert_eq!(ys.len(), tall, "{rows} rows: text {} rows tall", ys.len());
            assert!(*ys.iter().max().unwrap() < rows - 1, "a row of margin below");
        }
    }

    #[test]
    fn the_city_wraps_without_a_seam() {
        let period = LOTS * BLOCKS;
        for i in [0i64, 5, 77, 4000] {
            for side in [-1., 1.] {
                let (a, b) = (lot(i, side), lot(i + period, side));
                assert_eq!(a.h, b.h);
                assert_eq!(a.face, b.face);
                assert!((b.z0 - a.z0 - BLOCK * BLOCKS as f32).abs() < 0.5);
            }
        }
    }

    #[test]
    fn the_cursor_steers_and_touches_every_cyber_scene() {
        let (cols, rows) = (240, 70);
        let quiet = Sound::default();
        // The avenue: the cursor moves the camera and scans a facade.
        let mut city = NeonCity::new();
        let centre = city.eye().0;
        city.step(0.01, &quiet, Some((0.8, 0.)), (0.8, 0.));
        assert!(city.eye().0[0] > centre[0] + 1.5, "the flight follows the cursor");
        let (eye, _) = city.eye();
        let first = ((city.z - LOT) / BLOCK).floor() as i64 * LOTS;
        let lots: Vec<Lot> = (first..first + 24).flat_map(|i| [lot(i, -1.), lot(i, 1.)]).collect();
        // Some rays cross a side street; most meet a facade on their side.
        let hits: Vec<Hack> = (1..10).filter_map(|k| NeonCity::pick(&lots, eye, norm([1., 0.2, k as f32 * 0.7]))).collect();
        assert!(hits.len() >= 3 && hits.iter().all(|h| h.side == 1.), "{} facades", hits.len());
        // The grid: the cursor marks the floor and draws packets in.
        let mut grid = PacketGrid::new();
        grid.step(0.01, &quiet, Some((0.3, 0.6)), (0.3, 0.6));
        let mut r = Raster::new(cols, rows);
        let mut cv = canvas(&mut r, cols, rows);
        grid.draw(&mut cv, &quiet);
        let (ax, az) = grid.aim.get().expect("the cursor meets the floor");
        grid.packets = vec![Packet { x: ax + 4., z: az, v: 0., size: 0.5, held: 0. }];
        for _ in 0..20 {
            grid.step(1. / 30., &quiet, Some((0.3, 0.6)), (0.3, 0.6));
        }
        let p = &grid.packets[0];
        assert!(p.x < ax + 3. && p.held > 0.3, "the packet is drawn in: x {} held {}", p.x - ax, p.held);
        // The chip: the cursor picks the core it rests on.
        let mut chip = Silicon::new();
        chip.load.cores = vec![0.9, 0.1, 0.5, 0.3];
        chip.step(0.01, &quiet, Some((0., 0.)), (0., 0.));
        let mut r = Raster::new(cols, rows);
        let mut cv = canvas(&mut r, cols, rows);
        chip.draw(&mut cv, &quiet);
        let o = cv.cam.pos;
        for (i, c) in chip.cores.iter().enumerate() {
            let top = [(c.lo[0] + c.hi[0]) * 0.5, c.hi[1], (c.lo[2] + c.hi[2]) * 0.5];
            assert_eq!(chip.pick_at(o, norm(sub(top, o))), Some(i));
        }
        let (lo, hi) = Silicon::ram_chip(3);
        assert_eq!(chip.pick_at(o, norm(sub(mul(add(lo, hi), 0.5), o))), Some(PICK_RAM));
    }

    #[test]
    fn beats_reach_every_cyber_scene() {
        let mut city = NeonCity::new();
        let mut grid = PacketGrid::new();
        let mut chip = Silicon::new();
        let (mut waves, mut ripples, mut zap) = (0, 0, 0f32);
        let mut t = 0.;
        for _ in 0..90 {
            t += 1. / 30.;
            let s = beats(t);
            city.step(1. / 30., &s, None, (0., 0.));
            grid.step(1. / 30., &s, None, (0., 0.));
            chip.step(1. / 30., &s, None, (0., 0.));
            waves = waves.max(city.waves.len());
            ripples = ripples.max(grid.ripples.len());
            zap = zap.max(chip.zap);
        }
        assert!(waves > 0 && ripples > 0 && zap > 0.3, "waves {waves} ripples {ripples} zap {zap}");
        assert!(!chip.rings.is_empty());
    }

    #[test]
    fn traffic_makes_packets_and_load_raises_cores() {
        let mut g = PacketGrid::new();
        g.net.now = (5e7, 1e6);
        g.net.smooth = (5e7, 1e6);
        for _ in 0..60 {
            g.net.smooth = (5e7, 1e6);
            g.step(1. / 30., &Sound::default(), None, (0., 0.));
        }
        let down = g.packets.iter().filter(|p| p.v < 0.).count();
        let up = g.packets.iter().filter(|p| p.v > 0.).count();
        assert!(down > up, "a download storm: {down} down, {up} up");
        let l = arrange(&SysLoad::groups(&[1.0, 0.0], 16), 0.);
        assert!(l[0].hi[1] - l[0].lo[1] > l[1].hi[1] - l[1].lo[1], "the busy core is taller");
        assert_eq!(l[0].name, "CPU0");
    }
}
