//! Solid 3D scenes on the cell raster: a perspective camera, a depth-tested
//! triangle and sphere rasterizer, and shading by glyph density.
//!
//! - `event-horizon`: a black hole whose accretion disk is bent over and under
//!   the shadow by a point-lens model, with jets, a lensed sky and stars torn
//!   apart into streams that join the disk.
//! - `supernova`: a star that pulses, collapses, explodes into a filamentary
//!   shell with a pulsar, and is reborn from the gas.
//! - `solar-system`: one planet per CPU core (speed, size and heat follow its
//!   load), an asteroid belt as dense as memory use and comets for swap.
//! - `abyss`: a lit sand floor with ripples and caustics, plankton, marine
//!   snow, kelp, boulders and a manta; the cursor is a lamp.
//! - `load-canyon`: CPU history as terrain flowing away from the camera, one
//!   ridge per core, flooded to the level of memory use.
//!
//! - `neon-avenue`, `packet-grid` and `silicon`: cyberpunk scenes, in `cyber`.
//!
//! All keep moving in silence; bass, treble, the cursor and, where named, the
//! machine's own load drive them.
use crate::{interactive_matrix::Sound, interactive_tunnel::Raster, neon_scenes::variant};
use std::cell::Cell;
use std::collections::VecDeque;
use std::f32::consts::{PI, TAU};

pub(crate) mod cyber;

type Color = (u8, u8, u8);
type V = [f32; 3];

// ---------------------------------------------------------------- vectors
fn add(a: V, b: V) -> V {
    [a[0] + b[0], a[1] + b[1], a[2] + b[2]]
}
fn sub(a: V, b: V) -> V {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}
fn mul(a: V, k: f32) -> V {
    [a[0] * k, a[1] * k, a[2] * k]
}
fn dot(a: V, b: V) -> f32 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}
fn cross(a: V, b: V) -> V {
    [a[1] * b[2] - a[2] * b[1], a[2] * b[0] - a[0] * b[2], a[0] * b[1] - a[1] * b[0]]
}
fn len(a: V) -> f32 {
    dot(a, a).sqrt()
}
fn norm(a: V) -> V {
    let l = len(a);
    if l > 1e-9 {
        mul(a, 1. / l)
    } else {
        [0., 1., 0.]
    }
}
/// Rotation about the vertical axis.
fn yaw(p: V, a: f32) -> V {
    let (s, c) = a.sin_cos();
    [p[0] * c + p[2] * s, p[1], -p[0] * s + p[2] * c]
}
/// Rotation about the x axis.
fn tilt(p: V, a: f32) -> V {
    let (s, c) = a.sin_cos();
    [p[0], p[1] * c - p[2] * s, p[1] * s + p[2] * c]
}
fn smoothstep(e0: f32, e1: f32, x: f32) -> f32 {
    let t = ((x - e0) / (e1 - e0)).clamp(0., 1.);
    t * t * (3. - 2. * t)
}

// ----------------------------------------------------------------- colour
fn scale(c: Color, k: f32) -> Color {
    (
        (c.0 as f32 * k).clamp(0., 255.) as u8,
        (c.1 as f32 * k).clamp(0., 255.) as u8,
        (c.2 as f32 * k).clamp(0., 255.) as u8,
    )
}
fn mix(a: Color, b: Color, t: f32) -> Color {
    let t = t.clamp(0., 1.);
    (
        (a.0 as f32 + (b.0 as f32 - a.0 as f32) * t) as u8,
        (a.1 as f32 + (b.1 as f32 - a.1 as f32) * t) as u8,
        (a.2 as f32 + (b.2 as f32 - a.2 as f32) * t) as u8,
    )
}
/// Piecewise-linear colour ramp over ascending stops.
fn grad(stops: &[(f32, Color)], t: f32) -> Color {
    if t <= stops[0].0 {
        return stops[0].1;
    }
    for w in stops.windows(2) {
        if t <= w[1].0 {
            return mix(w[0].1, w[1].1, (t - w[0].0) / (w[1].0 - w[0].0));
        }
    }
    stops[stops.len() - 1].1
}
/// Glyph density for a light level in [0, 1]. No ',' or ';': the preview
/// wire format reserves them.
fn ramp(l: f32) -> char {
    const R: [char; 10] = ['.', '·', ':', '-', '=', '+', '*', '#', '%', '@'];
    R[((l.clamp(0., 0.999)) * R.len() as f32) as usize]
}
/// Shade blocks for solid ground: smooth relief instead of letters.
fn block(l: f32) -> char {
    match l {
        l if l < 0.1 => '·',
        l if l < 0.2 => ':',
        l if l < 0.34 => '░',
        l if l < 0.55 => '▒',
        l if l < 0.8 => '▓',
        _ => '█',
    }
}
/// A point glyph that grows as it nears or brightens.
fn speck(l: f32) -> char {
    match l {
        l if l < 0.3 => '.',
        l if l < 0.55 => '·',
        l if l < 0.8 => '+',
        l if l < 1.05 => '*',
        _ => '✦',
    }
}

// ------------------------------------------------------------------ noise
fn hash3(x: i32, y: i32, z: i32) -> f32 {
    variant(
        (x as u32).wrapping_mul(73856093)
            ^ (y as u32).wrapping_mul(19349663)
            ^ (z as u32).wrapping_mul(83492791),
    )
}
fn s3(t: f32) -> f32 {
    t * t * (3. - 2. * t)
}
/// Trilinear value noise in [0, 1).
fn noise3(p: V) -> f32 {
    let (xi, yi, zi) = (p[0].floor(), p[1].floor(), p[2].floor());
    let (fx, fy, fz) = (s3(p[0] - xi), s3(p[1] - yi), s3(p[2] - zi));
    let (x, y, z) = (xi as i32, yi as i32, zi as i32);
    let l = |a: f32, b: f32, t: f32| a + (b - a) * t;
    let plane = |z: i32| {
        l(
            l(hash3(x, y, z), hash3(x + 1, y, z), fx),
            l(hash3(x, y + 1, z), hash3(x + 1, y + 1, z), fx),
            fy,
        )
    };
    l(plane(z), plane(z + 1), fz)
}
/// Value noise in [0, 1) whose lattice repeats every `period` cells in z.
fn noise2p(x: f32, z: f32, period: i32) -> f32 {
    let (xi, zi) = (x.floor(), z.floor());
    let (fx, fz) = (s3(x - xi), s3(z - zi));
    let (x, z0) = (xi as i32, (zi as i32).rem_euclid(period));
    let z1 = (z0 + 1) % period;
    let h = |i: i32, j: i32| hash3(i, j, 7);
    let a = h(x, z0) + (h(x + 1, z0) - h(x, z0)) * fx;
    let b = h(x, z1) + (h(x + 1, z1) - h(x, z1)) * fx;
    a + (b - a) * fz
}

struct Rng(u32);
impl Rng {
    fn new() -> Self {
        Self(rand::random())
    }
    fn next(&mut self) -> f32 {
        self.0 = self.0.wrapping_mul(1664525).wrapping_add(1013904223);
        (self.0 >> 8) as f32 / 16777216.
    }
    fn signed(&mut self) -> f32 {
        self.next() * 2. - 1.
    }
    fn unit(&mut self) -> V {
        let z = self.signed();
        let a = self.next() * TAU;
        let r = (1. - z * z).max(0.).sqrt();
        [r * a.cos(), z, r * a.sin()]
    }
}

/// Rising edges of a smoothed level: a beat, not a loud passage.
#[derive(Default)]
struct Pulse {
    slow: f32,
    cool: f32,
}
impl Pulse {
    fn hit(&mut self, v: f32, dt: f32) -> Option<f32> {
        self.cool -= dt;
        let rise = v - self.slow;
        self.slow += (v - self.slow) * (1. - (-dt * 2.5).exp());
        (rise > 0.1 && v > 0.18 && self.cool <= 0.).then(|| {
            self.cool = 0.2;
            (rise * 2. + v * 0.4).min(1.5)
        })
    }
}

// ----------------------------------------------------------------- camera
const NEAR: f32 = 0.1;
#[derive(Clone, Copy)]
struct Cam {
    pos: V,
    right: V,
    up: V,
    fwd: V,
    /// Focal length in rows.
    f: f32,
    cx: f32,
    cy: f32,
    /// Cell height over cell width.
    aspect: f32,
}
impl Cam {
    fn look(pos: V, target: V, fov: f32, cols: usize, rows: usize, aspect: f32) -> Self {
        let fwd = norm(sub(target, pos));
        let right = norm(cross([0., 1., 0.], fwd));
        let up = cross(fwd, right);
        // Wide grids keep the vertical field; narrow ones keep the width.
        let wide = cols as f32 / (rows as f32 * aspect).max(1.);
        let f = rows as f32 * 0.5 / (fov * 0.5).tan() * (wide / 1.6).min(1.);
        Self {
            pos,
            right,
            up,
            fwd,
            f: f.max(1.),
            cx: cols as f32 * 0.5,
            cy: rows as f32 * 0.5,
            aspect,
        }
    }
    fn view(&self, p: V) -> V {
        let d = sub(p, self.pos);
        [dot(d, self.right), dot(d, self.up), dot(d, self.fwd)]
    }
    fn screen(&self, v: V) -> Option<(f32, f32, f32)> {
        (v[2] > NEAR).then(|| {
            (
                self.cx + v[0] / v[2] * self.f * self.aspect,
                self.cy - v[1] / v[2] * self.f,
                v[2],
            )
        })
    }
    fn project(&self, p: V) -> Option<(f32, f32, f32)> {
        self.screen(self.view(p))
    }
    /// Screen position of a direction at infinity.
    fn sky(&self, d: V) -> Option<(f32, f32)> {
        let v = [dot(d, self.right), dot(d, self.up), dot(d, self.fwd)];
        (v[2] > 0.02).then(|| {
            (
                self.cx + v[0] / v[2] * self.f * self.aspect,
                self.cy - v[1] / v[2] * self.f,
            )
        })
    }
    fn ray(&self, sx: f32, sy: f32) -> V {
        norm(add(
            self.fwd,
            add(
                mul(self.right, (sx - self.cx) / (self.f * self.aspect)),
                mul(self.up, (self.cy - sy) / self.f),
            ),
        ))
    }
}
/// A camera orbiting `target` at `dist`, `az` round and `el` above.
fn orbit(target: V, az: f32, el: f32, dist: f32) -> V {
    add(
        target,
        [dist * el.cos() * az.sin(), dist * el.sin(), -dist * el.cos() * az.cos()],
    )
}

/// A projected vertex: screen position, 1/depth and three attributes that are
/// interpolated across the triangle.
#[derive(Clone, Copy)]
struct Vx {
    x: f32,
    y: f32,
    w: f32,
    a: [f32; 3],
}

struct Canvas<'a> {
    r: &'a mut Raster,
    cols: usize,
    rows: usize,
    cam: Cam,
}
impl Canvas<'_> {
    fn dot(&mut self, x: f32, y: f32, z: f32, ch: char, c: Color) {
        if x >= 0. && y >= 0. && x < self.cols as f32 && y < self.rows as f32 {
            self.r.dot(x as i32, y as i32, z, ch, c);
        }
    }
    fn point(&mut self, p: V, ch: char, c: Color) -> Option<(f32, f32, f32)> {
        let s = self.cam.project(p)?;
        self.dot(s.0, s.1, s.2, ch, c);
        Some(s)
    }
    /// A segment with one depth: keep segments short where depth matters.
    fn line(&mut self, a: V, b: V, ch: Option<char>, c: Color) {
        let (mut va, mut vb) = (self.cam.view(a), self.cam.view(b));
        if va[2] < NEAR && vb[2] < NEAR {
            return;
        }
        // Clip to the near plane.
        if va[2] < NEAR || vb[2] < NEAR {
            let t = (NEAR - va[2]) / (vb[2] - va[2]);
            let cut = add(va, mul(sub(vb, va), t));
            if va[2] < NEAR {
                va = cut;
            } else {
                vb = cut;
            }
            let push = |v: &mut V| v[2] = v[2].max(NEAR * 1.01);
            push(&mut va);
            push(&mut vb);
        }
        let (Some(sa), Some(sb)) = (self.cam.screen(va), self.cam.screen(vb)) else {
            return;
        };
        self.r.line_with_glyph(
            (sa.0 - 0.5, sa.1 - 0.5),
            (sb.0 - 0.5, sb.1 - 0.5),
            (sa.2 + sb.2) * 0.5,
            c,
            ch.is_none(),
            ch,
        );
    }
    fn vx(&self, p: V, a: [f32; 3]) -> Option<Vx> {
        let (x, y, z) = self.cam.project(p)?;
        Some(Vx { x, y, w: 1. / z, a })
    }
    /// Fills a triangle at cell centres. `paint` gets the interpolated
    /// attributes and the depth and returns the cell, or nothing to skip it.
    fn tri(&mut self, p: Vx, q: Vx, s: Vx, paint: &mut impl FnMut([f32; 3], f32) -> Option<(char, Color)>) {
        let area = (q.x - p.x) * (s.y - p.y) - (q.y - p.y) * (s.x - p.x);
        if !area.is_finite() || area.abs() < 1e-5 {
            return;
        }
        let inv = 1. / area;
        let lo_x = p.x.min(q.x).min(s.x).max(0.);
        let hi_x = p.x.max(q.x).max(s.x).min(self.cols as f32);
        let lo_y = p.y.min(q.y).min(s.y).max(0.);
        let hi_y = p.y.max(q.y).max(s.y).min(self.rows as f32);
        if lo_x >= hi_x || lo_y >= hi_y {
            return;
        }
        let (x0, x1) = ((lo_x - 0.5).max(0.).ceil() as usize, ((hi_x - 0.5).floor() as usize).min(self.cols - 1));
        let (y0, y1) = ((lo_y - 0.5).max(0.).ceil() as usize, ((hi_y - 0.5).floor() as usize).min(self.rows - 1));
        // Each weight is linear along a row, and so is everything blended
        // from them: per row, solve for the span where all three weights
        // are non-negative and set up depth and attributes as a + b·x, so a
        // cell costs a few multiply-adds and one division. A cell of slack
        // each side of the span leaves the exact test to decide.
        let (b0, b1) = ((q.y - s.y) * inv, (s.y - p.y) * inv);
        let b2 = -b0 - b1;
        // 1/depth and the three attributes, blended by three weights.
        let attrs = |v: &Vx| [v.w, v.a[0], v.a[1], v.a[2]];
        let (ap, aq, as_) = (attrs(&p), attrs(&q), attrs(&s));
        let blend = |k0: f32, k1: f32, k2: f32| -> [f32; 4] { std::array::from_fn(|i| k0 * ap[i] + k1 * aq[i] + k2 * as_[i]) };
        let slope = blend(b0, b1, b2);
        for y in y0..=y1 {
            let py = y as f32 + 0.5;
            let a0 = (q.x * (s.y - py) - (q.y - py) * s.x) * inv;
            let a1 = (s.x * (p.y - py) - (s.y - py) * p.x) * inv;
            let a2 = 1. - a0 - a1;
            let (mut lo, mut hi) = (x0 as f32 + 0.5, x1 as f32 + 0.5);
            for (a, b) in [(a0, b0), (a1, b1), (a2, b2)] {
                if b.abs() < 1e-12 {
                    if a < -1e-4 {
                        hi = f32::MIN;
                    }
                } else if b > 0. {
                    lo = lo.max((-1e-4 - a) / b);
                } else {
                    hi = hi.min((-1e-4 - a) / b);
                }
            }
            if !(lo <= hi) {
                continue;
            }
            let xa = ((lo - 0.5).floor() as i64 - 1).max(x0 as i64) as usize;
            let xb = ((hi - 0.5).ceil() as i64 + 1).min(x1 as i64);
            if xb < xa as i64 {
                continue;
            }
            let base = blend(a0, a1, a2);
            for x in xa..=xb as usize {
                let px = x as f32 + 0.5;
                let (w0, w1) = (a0 + b0 * px, a1 + b1 * px);
                if w0 < -1e-4 || w1 < -1e-4 || 1. - w0 - w1 < -1e-4 {
                    continue;
                }
                let w = base[0] + slope[0] * px;
                if w <= 0. {
                    continue;
                }
                let z = 1. / w;
                if self.r.hidden(x, y, z) {
                    continue;
                }
                let at = [base[1] + slope[1] * px, base[2] + slope[2] * px, base[3] + slope[3] * px];
                if let Some((ch, c)) = paint(at, z) {
                    self.r.dot(x as i32, y as i32, z, ch, c);
                }
            }
        }
    }
    /// Ray-traces a sphere over its screen bounds. `paint` gets the world hit
    /// point, the outward normal and the depth.
    fn sphere(&mut self, c: V, rad: f32, paint: &mut impl FnMut(V, V, f32) -> Option<(char, Color)>) {
        let v = self.cam.view(c);
        if v[2] <= rad + NEAR {
            return;
        }
        let Some((sx, sy, _)) = self.cam.screen(v) else {
            return;
        };
        // Off-axis spheres project to ellipses stretched outward: pad for it.
        let off = ((v[0] * v[0] + v[1] * v[1]).sqrt() / v[2]).min(2.);
        let sr = rad / (v[2] - rad).max(NEAR) * self.cam.f * (1.1 + off * 0.5) + 1.;
        let x0 = (sx - sr * self.cam.aspect).max(0.) as usize;
        let x1 = ((sx + sr * self.cam.aspect).max(0.) as usize).min(self.cols.saturating_sub(1));
        let y0 = (sy - sr).max(0.) as usize;
        let y1 = ((sy + sr).max(0.) as usize).min(self.rows.saturating_sub(1));
        if sx + sr * self.cam.aspect < 0. || sy + sr < 0. || x0 > x1 || y0 > y1 {
            return;
        }
        let oc = sub(self.cam.pos, c);
        let cc = dot(oc, oc) - rad * rad;
        for y in y0..=y1 {
            for x in x0..=x1 {
                let d = self.cam.ray(x as f32 + 0.5, y as f32 + 0.5);
                let b = dot(oc, d);
                let h = b * b - cc;
                if h < 0. {
                    continue;
                }
                let t = -b - h.sqrt();
                if t < NEAR {
                    continue;
                }
                let hit = add(self.cam.pos, mul(d, t));
                let n = mul(sub(hit, c), 1. / rad);
                let depth = t * dot(d, self.cam.fwd);
                if let Some((ch, col)) = paint(hit, n, depth) {
                    self.r.dot(x as i32, y as i32, depth, ch, col);
                }
            }
        }
    }
    fn text(&mut self, x: i32, y: i32, z: f32, s: &str, c: Color) {
        for (i, ch) in s.chars().enumerate() {
            let ch = match ch {
                ',' => '·',
                ';' => ':',
                c => c,
            };
            if ch != ' ' {
                self.r.dot(x + i as i32, y, z, ch, c);
            }
        }
    }
    /// Text centred on a world point, in front of everything near it.
    fn label(&mut self, p: V, rise: f32, s: &str, c: Color) {
        if self.cols < 90 {
            return;
        }
        if let Some((x, y, z)) = self.cam.project(p) {
            let n = s.chars().count() as f32;
            self.text((x - n * 0.5) as i32, (y - rise) as i32, z - 0.5, s, c);
        }
    }
}

// ------------------------------------------------------------------- sky
struct Star {
    dir: V,
    mag: f32,
    phase: f32,
}
fn starfield(n: usize, rng: &mut Rng) -> Vec<Star> {
    (0..n)
        .map(|_| Star {
            dir: rng.unit(),
            mag: rng.next().powi(3),
            phase: rng.next() * TAU,
        })
        .collect()
}
fn draw_sky(cv: &mut Canvas, stars: &[Star], time: f32, treble: f32) {
    for s in stars {
        if let Some((x, y)) = cv.cam.sky(s.dir) {
            let tw = 0.65 + 0.35 * (time * (1.5 + s.mag * 3.) + s.phase).sin();
            let l = (0.25 + s.mag * 0.9) * tw + treble * s.mag * 0.6;
            cv.dot(x, y, 1e5, speck(l), scale((190, 205, 255), 0.35 + l * 0.7));
        }
    }
}
/// Radial streamers round a disc on screen, as round a star.
fn corona(cv: &mut Canvas, (cx, cy): (f32, f32), radius: f32, depth: f32, time: f32, color: Color, reach: f32, light: f32) {
    let aspect = cv.cam.aspect;
    let n = ((TAU * radius * aspect * 1.2) as usize + 24).min(3000);
    let steps = (radius * reach).ceil().max(2.) as usize * 2;
    for i in 0..n {
        let a = i as f32 / n as f32 * TAU;
        let streak = noise3([a.cos() * 2.5 + 9., a.sin() * 2.5, time * 0.3]) * 0.7 + noise3([a.cos() * 7., a.sin() * 7., time * 0.6]) * 0.3;
        let len = reach * (0.25 + streak * streak * 1.6);
        for k in 0..steps {
            let f = k as f32 / steps as f32;
            if f > len / reach {
                break;
            }
            let r = radius * (1.03 + f * reach);
            let l = light * (1. - f * reach / len).max(0.) * (0.5 + streak);
            if l < 0.06 {
                continue;
            }
            cv.dot(cx + a.cos() * r * aspect, cy + a.sin() * r, depth, if f < 0.08 { ramp(l) } else { speck(l * 0.8) }, scale(color, 0.25 + l * 0.8));
        }
    }
}
/// A ragged glow round a disc on screen: corona, photon ring.
fn halo(cv: &mut Canvas, (cx, cy): (f32, f32), radius: f32, depth: f32, time: f32, color: Color, layers: &[(f32, f32)], gate: f32) {
    let aspect = cv.cam.aspect;
    let n = ((TAU * radius * aspect * 1.4) as usize + 24).min(4000);
    for (li, &(reach, light)) in layers.iter().enumerate() {
        for i in 0..n {
            let a = i as f32 / n as f32 * TAU;
            let streak = noise3([a.cos() * 3. + 9., a.sin() * 3., time * 0.35 + li as f32 * 1.7]);
            if streak < gate {
                continue;
            }
            let r = radius * (reach + (streak - gate) * 0.25 * li as f32);
            let l = light * (0.6 + streak * 0.6);
            cv.dot(cx + a.cos() * r * aspect, cy + a.sin() * r, depth, ramp(l), scale(color, l));
        }
    }
}

// --------------------------------------------------------------- machine
/// CPU per core, memory and swap from /proc, a few times a second. Where
/// /proc cannot be read a believable idle wobble stands in.
pub(crate) struct SysLoad {
    /// Smoothed load per core, 0 to 1, for drawing.
    cores: Vec<f32>,
    /// Latest measured load per core.
    target: Vec<f32>,
    total: f32,
    mem: f32,
    mem_total_gib: f32,
    swap: f32,
    loadavg: [f32; 3],
    prev: Vec<(u64, u64)>,
    clock: f32,
    ticks: u32,
    time: f32,
}
impl SysLoad {
    fn new() -> Self {
        let mut s = Self {
            cores: vec![],
            target: vec![],
            total: 0.,
            mem: 0.4,
            mem_total_gib: 16.,
            swap: 0.,
            loadavg: [0.; 3],
            prev: vec![],
            clock: 0.,
            ticks: 0,
            time: 0.,
        };
        s.sample();
        s.cores = s.target.clone();
        s
    }
    fn step(&mut self, dt: f32) -> bool {
        self.time += dt;
        self.clock += dt;
        let fresh = self.clock >= 0.25;
        if fresh {
            self.clock = 0.;
            self.sample();
        }
        if self.cores.len() != self.target.len() {
            self.cores = self.target.clone();
        }
        let k = 1. - (-dt * 6.).exp();
        for (c, t) in self.cores.iter_mut().zip(&self.target) {
            *c += (t - *c) * k;
        }
        self.total = if self.cores.is_empty() {
            0.
        } else {
            self.cores.iter().sum::<f32>() / self.cores.len() as f32
        };
        fresh
    }
    fn sample(&mut self) {
        self.ticks = self.ticks.wrapping_add(1);
        let stat = std::fs::read_to_string("/proc/stat").ok();
        let mut now = vec![];
        for line in stat.iter().flat_map(|s| s.lines()) {
            let Some(rest) = line.strip_prefix("cpu") else { break };
            if !rest.starts_with(|c: char| c.is_ascii_digit()) {
                continue;
            }
            let f: Vec<u64> = rest.split_whitespace().skip(1).take(8).filter_map(|v| v.parse().ok()).collect();
            if f.len() >= 5 {
                let idle = f[3] + f[4];
                now.push((idle, f.iter().sum::<u64>()));
            }
        }
        if now.is_empty() {
            // No /proc: eight cores that wander.
            let t = self.time;
            self.target = (0..8).map(|i| (0.1 + 0.6 * noise3([t * 0.3, i as f32 * 3.7, 1.])).clamp(0., 1.)).collect();
        } else {
            if self.prev.len() == now.len() {
                self.target = now
                    .iter()
                    .zip(&self.prev)
                    .map(|(&(i1, t1), &(i0, t0))| {
                        let dt = t1.saturating_sub(t0);
                        if dt == 0 {
                            0.
                        } else {
                            1. - i1.saturating_sub(i0) as f32 / dt as f32
                        }
                    })
                    .map(|v| v.clamp(0., 1.))
                    .collect();
            } else if self.target.len() != now.len() {
                self.target = vec![0.05; now.len()];
            }
            self.prev = now;
        }
        // Memory changes slowly: once a second is plenty.
        if self.ticks % 4 == 1 {
            if let Ok(m) = std::fs::read_to_string("/proc/meminfo") {
                let get = |k: &str| {
                    m.lines()
                        .find(|l| l.starts_with(k))
                        .and_then(|l| l.split_whitespace().nth(1))
                        .and_then(|v| v.parse::<f32>().ok())
                };
                if let (Some(t), Some(a)) = (get("MemTotal:"), get("MemAvailable:")) {
                    self.mem = (1. - a / t.max(1.)).clamp(0., 1.);
                    self.mem_total_gib = t / 1048576.;
                }
                if let (Some(t), Some(f)) = (get("SwapTotal:"), get("SwapFree:")) {
                    self.swap = if t > 0. { (1. - f / t).clamp(0., 1.) } else { 0. };
                }
            }
            if let Ok(l) = std::fs::read_to_string("/proc/loadavg") {
                for (slot, v) in self.loadavg.iter_mut().zip(l.split_whitespace()) {
                    *slot = v.parse().unwrap_or(0.);
                }
            }
        }
    }
    /// At most `n` values: cores averaged in equal runs when there are more.
    fn groups(values: &[f32], n: usize) -> Vec<(f32, String)> {
        if values.len() <= n {
            return values.iter().enumerate().map(|(i, &v)| (v, format!("cpu{i}"))).collect();
        }
        let run = values.len().div_ceil(n);
        values
            .chunks(run)
            .enumerate()
            .map(|(i, c)| {
                let first = i * run;
                (c.iter().sum::<f32>() / c.len() as f32, format!("cpu{}-{}", first, first + c.len() - 1))
            })
            .collect()
    }
    fn hud(&self) -> String {
        format!(
            "CPU {:>3.0}%   RAM {:.1}/{:.1} GiB   SWAP {:.0}%   LOAD {:.2} {:.2} {:.2}",
            self.total * 100.,
            self.mem * self.mem_total_gib,
            self.mem_total_gib,
            self.swap * 100.,
            self.loadavg[0],
            self.loadavg[1],
            self.loadavg[2]
        )
    }
}

/// A free particle.
struct Mote {
    p: V,
    v: V,
    life: f32,
}

// ======================================================== event horizon
/// A hot spot riding the disk: a beat, or a star that fell in.
struct Spot {
    r: f32,
    a: f32,
    heat: f32,
}
/// Rock or star dust thrown in where the cursor points; `size` 0 is dust.
struct Rock {
    p: V,
    v: V,
    size: f32,
}
pub(crate) struct Horizon {
    spots: Vec<Spot>,
    debris: Vec<Rock>,
    /// The world point under the cursor, found by the last draw.
    aim: std::cell::Cell<Option<V>>,
    spawn: f32,
    /// A flash of the photon ring each time the hole swallows something.
    gulp: f32,
    jets: Vec<Mote>,
    stream: Vec<Mote>,
    stars: Vec<Star>,
    time: f32,
    drift: f32,
    meal: f32,
    jet_budget: f32,
    glow: f32,
    pulse: Pulse,
    rng: Rng,
}
const GM: f32 = 9.;
/// Schwarzschild radius; the shadow is 2.6 of them across the axis.
const RS: f32 = 0.62;
const B_CRIT: f32 = 2.6 * RS;
const INNER: f32 = 1.9;
const OUTER: f32 = 7.4;
fn disk_color(t: f32) -> Color {
    grad(
        &[
            (0.0, (90, 20, 30)),
            (0.25, (200, 60, 30)),
            (0.45, (255, 140, 50)),
            (0.65, (255, 210, 130)),
            (0.85, (255, 245, 225)),
            (1.0, (200, 225, 255)),
        ],
        t,
    )
}
fn kepler(r: f32) -> f32 {
    (GM / (r * r * r)).sqrt() * 0.55
}
impl Horizon {
    fn new() -> Self {
        let mut rng = Rng::new();
        let stars = starfield(1100, &mut rng);
        Self {
            spots: vec![],
            debris: vec![],
            aim: std::cell::Cell::new(None),
            spawn: 0.,
            gulp: 0.,
            jets: vec![],
            stream: vec![],
            stars,
            time: rng.next() * 100.,
            drift: rng.next() * TAU,
            meal: 6.,
            jet_budget: 0.,
            glow: 0.,
            pulse: Pulse::default(),
            rng,
        }
    }
    fn step(&mut self, dt: f32, sound: &Sound, cursor: Option<(f32, f32)>) {
        // The disk texture shears with time; wrapping it now and then keeps
        // f32 precision, at the cost of one jump every few hours.
        self.time = (self.time + dt) % 20000.;
        self.drift += dt * 0.05;
        self.glow += (sound.bass - self.glow) * (1. - (-dt * 6.).exp());
        let feed = 0.05 + sound.bass * 0.35;
        for s in &mut self.spots {
            s.a = (s.a + kepler(s.r) * dt).rem_euclid(TAU);
            s.r -= dt * feed * (INNER * 1.5 / s.r);
            s.heat *= (-dt * 0.45).exp();
        }
        self.spots.retain(|s| s.r > INNER && s.heat > 0.05);
        // A beat ignites a hot spot that winds inward with the flow.
        let beat = self.pulse.hit(sound.bass, dt);
        if let Some(k) = beat {
            if self.spots.len() < 10 {
                self.spots.push(Spot { r: INNER + 0.6 + self.rng.next() * 3.5, a: self.rng.next() * TAU, heat: k * 1.4 });
            }
        }
        self.feed_debris(dt, sound, cursor.and(self.aim.get()), beat);
        // Treble drives the polar jets.
        self.jet_budget += dt * (30. + sound.treble * 650.);
        while self.jet_budget >= 1. {
            self.jet_budget -= 1.;
            if self.jets.len() >= 2600 {
                continue;
            }
            let side = if self.rng.next() < 0.5 { -1. } else { 1. };
            let (sx, sz) = (self.rng.signed() * 0.18, self.rng.signed() * 0.18);
            self.jets.push(Mote {
                // Born outside the shadow, so no beam runs across the hole.
                p: [sx * 0.6, side * B_CRIT * 1.15, sz * 0.6],
                v: [sx, side * (6. + self.rng.next() * 6. + sound.treble * 4.), sz],
                life: 1.,
            });
        }
        let swirl = dt * 1.8;
        for m in &mut self.jets {
            m.p = yaw(add(m.p, mul(m.v, dt)), swirl);
            m.life -= dt * 0.55;
        }
        self.jets.retain(|m| m.life > 0.);
        // Now and then a star wanders too close and is torn into a stream.
        self.meal -= dt * (1. + sound.bass * 2.);
        if self.meal <= 0. && self.stream.is_empty() {
            self.meal = 16. + self.rng.next() * 14.;
            let phi = self.rng.next() * TAU;
            let start = [phi.cos() * 11., self.rng.signed() * 1.2, phi.sin() * 11.];
            let along = [-phi.sin(), 0., phi.cos()];
            let speed = (GM / 11.).sqrt() * 0.42;
            for _ in 0..700 {
                let off = mul(self.rng.unit(), 0.55 * self.rng.next().cbrt());
                self.stream.push(Mote {
                    p: add(start, off),
                    v: mul(along, speed * (1. + self.rng.signed() * 0.14)),
                    life: 1.,
                });
            }
        }
        let h = dt / 3.;
        let drag = (-h * 0.12).exp();
        let mut fell = None;
        for m in &mut self.stream {
            for _ in 0..3 {
                let d = len(m.p).max(0.3);
                m.v = mul(add(m.v, mul(m.p, -GM / (d * d * d) * h)), drag);
                m.p = add(m.p, mul(m.v, h));
            }
            m.life -= dt * 0.025;
            let d = len(m.p);
            if d < INNER + 0.5 {
                fell = Some((d.max(INNER + 0.1), m.p[2].atan2(m.p[0]).rem_euclid(TAU)));
                m.life = 0.;
            } else if d > 18. {
                m.life = 0.;
            }
        }
        self.stream.retain(|m| m.life > 0.);
        // What falls in lights the disk where it lands.
        if let Some((r, a)) = fell {
            self.land(r, a, 0.02);
        }
    }
    /// Rocks and dust appear round the cursor, steadily while it is there and
    /// in a burst on each beat, then spiral in and are swallowed. Without a
    /// cursor a beat still throws a little dust in from the dark.
    fn feed_debris(&mut self, dt: f32, sound: &Sound, aim: Option<V>, beat: Option<f32>) {
        self.gulp *= (-dt * 3.).exp();
        let from = match aim {
            Some(a) => {
                self.spawn += dt * (16. + sound.bass * 90. + sound.treble * 40.) + beat.map_or(0., |k| 30. + k * 60.);
                Some(a)
            }
            None => beat.map(|k| {
                self.spawn += 12. + k * 30.;
                let phi = self.rng.next() * TAU;
                [phi.cos() * 10.5, self.rng.signed() * 2.5, phi.sin() * 10.5]
            }),
        };
        if let Some(at) = from {
            while self.spawn >= 1. {
                self.spawn -= 1.;
                if self.debris.len() >= 1600 {
                    continue;
                }
                let size = if self.rng.next() < 0.14 { 0.4 + self.rng.next() * 0.6 } else { 0. };
                let p = add(at, mul(self.rng.unit(), 0.25 + self.rng.next() * (0.5 + sound.bass * 0.6)));
                let r = len(p).max(1.);
                // Mostly round the hole, a little inward: a slow spiral in.
                let along = norm(cross([0., 1., 0.], p));
                let v = add(mul(along, (GM / r).sqrt() * (0.5 + self.rng.next() * 0.25)), add(mul(p, -0.06), mul(self.rng.unit(), 0.25)));
                self.debris.push(Rock { p, v, size });
            }
        }
        self.spawn = self.spawn.min(4.);
        let h = dt / 3.;
        let drag = (-h * 0.3).exp();
        let mut eaten = vec![];
        self.debris.retain_mut(|m| {
            for _ in 0..3 {
                let d = len(m.p).max(0.3);
                m.v = mul(add(m.v, mul(m.p, -GM / (d * d * d) * h)), drag);
                m.p = add(m.p, mul(m.v, h));
            }
            let d = len(m.p);
            let flat = (m.p[0] * m.p[0] + m.p[2] * m.p[2]).sqrt();
            if d < B_CRIT * 1.05 || (flat < INNER + 0.3 && m.p[1].abs() < 0.4) {
                eaten.push((flat.max(INNER + 0.1), m.p[2].atan2(m.p[0]).rem_euclid(TAU), m.size));
                return false;
            }
            d < 22.
        });
        for (r, a, size) in eaten {
            self.gulp = (self.gulp + 0.03 + size * 0.35).min(1.5);
            self.land(r, a, 0.01 + size * 0.3);
        }
    }
    fn land(&mut self, r: f32, a: f32, heat: f32) {
        {
            let full = self.spots.len() >= 14;
            match self.spots.iter_mut().find(|s| ((s.a - a + PI).rem_euclid(TAU) - PI).abs() < 0.4 && (s.r - r).abs() < 0.6) {
                Some(s) => s.heat = (s.heat + heat).min(2.),
                None if !full => self.spots.push(Spot { r, a, heat: 0.5 + heat }),
                None => {}
            }
        }
    }
    fn draw(&self, cv: &mut Canvas, sound: &Sound, gaze: (f32, f32)) {
        // Nearly edge on, so the far side of the disk rises over the shadow;
        // the cursor only adds a touch of parallax, so the camera neither
        // swings round with every move nor falls flat into the plane.
        let az = self.drift + gaze.0 * 0.3;
        let el = (0.14 - gaze.1 * 0.08).clamp(0.07, 0.24);
        let dist = 13.5 - self.glow * 0.8;
        cv.cam = Cam::look(orbit([0., 0., 0.], az, el, dist), [0., 0., 0.], 1.0, cv.cols, cv.rows, cv.cam.aspect);
        let cam = cv.cam;
        // Under the cursor, on the plane through the hole that faces us.
        let (ux, uy) = ((gaze.0 + 1.) * 0.5 * cv.cols as f32, (gaze.1 + 1.) * 0.5 * cv.rows as f32);
        let ray = cam.ray(ux, uy);
        let t = -dot(cam.pos, cam.fwd) / dot(ray, cam.fwd).max(0.05);
        let p = add(cam.pos, mul(ray, t));
        self.aim.set(Some(mul(p, (15. / len(p).max(1e-3)).min(1.))));
        let Some((hx, hy, hd)) = cam.project([0., 0., 0.]) else { return };
        let aspect = cam.aspect;
        let shadow = B_CRIT * cam.f / hd;
        let einstein = shadow * 1.2;
        // Point lens for what is not ray traced: stars, jets, the stream.
        let lens = |sx: f32, sy: f32, behind: f32| -> ((f32, f32), Option<((f32, f32), f32)>) {
            if behind <= 0. {
                return ((sx, sy), None);
            }
            let e = einstein * (behind / (behind + 2.)).sqrt();
            let (dx, dy) = ((sx - hx) / aspect, sy - hy);
            let d = (dx * dx + dy * dy).sqrt();
            let (ux, uy) = if d > 1e-3 { (dx / d, dy / d) } else { (0., -1.) };
            let root = (d * d + 4. * e * e).sqrt();
            let d1 = 0.5 * (d + root);
            let d2 = 0.5 * (d - root);
            let weak = (e * e / (d1 * d1).max(1e-3)).min(1.) * 0.7;
            (
                (hx + ux * d1 * aspect, hy + uy * d1),
                (d2.abs() > shadow).then(|| ((hx + ux * d2 * aspect, hy + uy * d2), weak)),
            )
        };
        for s in &self.stars {
            let Some((x, y)) = cam.sky(s.dir) else { continue };
            let ((x, y), _) = lens(x, y, 1e3);
            let tw = 0.65 + 0.35 * (self.time * (1.5 + s.mag * 3.) + s.phase).sin();
            let l = (0.25 + s.mag * 0.9) * tw + sound.treble * s.mag * 0.6;
            cv.dot(x, y, 1e5, speck(l), scale((190, 205, 255), 0.35 + l * 0.7));
        }
        // The disk, ray traced per cell: each ray runs straight to its
        // closest approach, then leaves bent toward the hole, so the far side
        // of the disk rises over the shadow and its underside shows beneath.
        let mut x0 = hx - shadow * aspect * 2.5;
        let mut x1 = hx + shadow * aspect * 2.5;
        let mut y0 = hy - shadow * 2.5;
        let mut y1 = hy + shadow * 2.5;
        for i in 0..24 {
            let a = i as f32 / 24. * TAU;
            if let Some((x, y, _)) = cam.project([OUTER * a.cos(), 0., OUTER * a.sin()]) {
                x0 = x0.min(x - 1.);
                x1 = x1.max(x + 1.);
                y0 = y0.min(y - 1.);
                y1 = y1.max(y + 1.);
            } else {
                (x0, x1, y0, y1) = (0., cv.cols as f32, 0., cv.rows as f32);
            }
        }
        let (x0, x1) = (x0.max(0.) as usize, (x1.max(0.) as usize).min(cv.cols - 1));
        let (y0, y1) = (y0.max(0.) as usize, (y1.max(0.) as usize).min(cv.rows - 1));
        let c = cam.pos;
        let time = self.time;
        let glow = 0.8 + self.glow * 0.6 + self.gulp * 0.25;
        let bend_tail = PI - 2. * RS / B_CRIT;
        for y in y0..=y1 {
            for x in x0..=x1 {
                let d = cam.ray(x as f32 + 0.5, y as f32 + 0.5);
                let tca = -dot(c, d);
                let mut hit = None;
                if d[1].abs() > 1e-5 {
                    let t = -c[1] / d[1];
                    if t > 0. && (tca <= 0. || t <= tca) {
                        let h = add(c, mul(d, t));
                        let r = (h[0] * h[0] + h[2] * h[2]).sqrt();
                        if (INNER..OUTER).contains(&r) {
                            hit = Some((h, d, t, r));
                        }
                    }
                }
                let mut ring = 0.;
                if hit.is_none() && tca > 0. {
                    let p = add(c, mul(d, tca));
                    let b = len(p);
                    if b < B_CRIT {
                        cv.r.dot(x as i32, y as i32, tca * dot(d, cam.fwd), ' ', (0, 0, 0));
                        continue;
                    }
                    let alpha = 2. * RS / b + bend_tail * (B_CRIT / b).powi(10);
                    let d2 = norm(add(mul(d, alpha.cos()), mul(p, -alpha.sin() / b)));
                    if d2[1].abs() > 1e-5 {
                        let s = -p[1] / d2[1];
                        if s > 0. {
                            let h = add(p, mul(d2, s));
                            let r = (h[0] * h[0] + h[2] * h[2]).sqrt();
                            if (INNER..OUTER).contains(&r) {
                                hit = Some((h, d2, tca + s, r));
                            }
                        }
                    }
                    ring = (1. - (b - B_CRIT) / (B_CRIT * 0.09)).max(0.);
                }
                if let Some((h, dir, path, r)) = hit {
                    let a = h[2].atan2(h[0]).rem_euclid(TAU);
                    let temp = (INNER / r).powf(0.9);
                    // Beaming: the side coming toward us burns brighter.
                    let approach = -(dir[2] * a.cos() - dir[0] * a.sin());
                    let doppler = (1. + 0.45 * approach * temp.sqrt()).powi(3);
                    let u = (a - kepler(r) * time) / TAU * 18.;
                    let streak = noise2p(r * 2.4, u, 18);
                    let fine = noise2p(r * 7.5, u * 3., 54);
                    let tex = 0.35 + 0.8 * streak * streak + 0.35 * fine;
                    let mut spot = 0.;
                    for s in &self.spots {
                        let da = (a - s.a + PI).rem_euclid(TAU) - PI;
                        spot += s.heat * (-((r - s.r).powi(2) / 0.2 + (da * r).powi(2) / 0.6)).exp();
                    }
                    let edge = smoothstep(INNER, INNER + 0.2, r) * (1. - smoothstep(OUTER - 2.2, OUTER, r));
                    let l = ((0.18 + temp * 0.95) * tex * doppler * glow * edge + spot).min(1.8);
                    if l < 0.04 {
                        continue;
                    }
                    let color = disk_color((temp * 0.9 + spot * 0.3 + approach * 0.08).min(1.));
                    cv.r.dot(x as i32, y as i32, path * dot(d, cam.fwd), ramp(l * 0.72), scale(color, 0.3 + l * 0.75));
                } else if ring > 0. {
                    let flicker = 0.75 + 0.25 * noise3([x as f32 * 0.3, y as f32 * 0.3, time * 2.]) + sound.treble * 0.3 + self.gulp * 0.6;
                    let l = ring * flicker;
                    cv.r.dot(x as i32, y as i32, hd, ramp(l * 0.8), scale((255, 215, 170), 0.3 + l * 0.8));
                }
            }
        }
        let put = |cv: &mut Canvas, p: V, l: f32, color: Color, ch: char| {
            let v = cam.view(p);
            let Some((sx, sy, z)) = cam.screen(v) else { return };
            let (primary, secondary) = lens(sx, sy, z - hd);
            cv.dot(primary.0, primary.1, z, ch, color);
            if let Some((p2, k)) = secondary {
                if l * k > 0.08 {
                    cv.dot(p2.0, p2.1, z, ramp(l * k), scale(color, k * 1.4));
                }
            }
        };
        for m in &self.jets {
            let l = m.life * (0.55 + sound.treble * 0.7);
            let color = scale(mix((150, 120, 255), (140, 220, 255), m.life), 0.4 + l * 0.8);
            put(cv, m.p, l, color, if m.life > 0.8 { '|' } else { speck(l) });
        }
        for m in &self.stream {
            let l = (0.6 + (len(m.v) * 0.25).min(0.6)) * m.life.min(0.3) / 0.3;
            put(cv, m.p, l, scale((255, 225, 170), l), speck(l));
        }
        // Debris heats as it nears the hole; rocks stretch out along their
        // fall, torn by the tide, before they vanish into it.
        for m in &self.debris {
            let d = len(m.p);
            let heat = (1. - (d - B_CRIT) / 7.).clamp(0., 1.);
            if m.size == 0. {
                let l = 0.45 + heat * 0.6 + sound.treble * 0.25;
                let color = mix((160, 190, 255), (255, 200, 120), heat);
                put(cv, m.p, l, scale(color, 0.4 + l * 0.6), speck(l));
            } else {
                let color = mix((150, 135, 125), disk_color(0.55 + heat * 0.4), heat.powf(0.7));
                let l = 0.6 + heat * 0.5;
                let back = mul(norm(m.v), -1.);
                let tail = ((4.5 - d) / 2.5).clamp(0., 1.) * 4.;
                for k in (1..=tail as usize).rev() {
                    let q = add(m.p, mul(back, k as f32 * 0.16 * (1. + m.size)));
                    put(cv, q, l * 0.7, scale(color, 0.8 - k as f32 * 0.12), if k < 2 { '•' } else { '·' });
                }
                put(cv, m.p, l, scale(color, 0.5 + l * 0.5), if m.size > 0.75 { '●' } else { '•' });
            }
        }
    }
}

// ============================================================ supernova
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Phase {
    Star,
    Collapse,
    Blast,
    Rebirth,
}
#[derive(Clone, Copy, PartialEq, Eq)]
enum Ejecta {
    Shell,
    Inner,
    Ring,
    Lobe,
}
struct Shard {
    dir: V,
    speed: f32,
    /// How much of a filament it sits on, 0 to 1.
    ridge: f32,
    oxygen: bool,
    kind: Ejecta,
}
pub(crate) struct Supernova {
    phase: Phase,
    t: f32,
    time: f32,
    spin: f32,
    radius: f32,
    zoom: f32,
    shards: Vec<Shard>,
    flares: Vec<Mote>,
    gas: Vec<Mote>,
    waves: Vec<f32>,
    pulsar: f32,
    stars: Vec<Star>,
    pulse: Pulse,
    rng: Rng,
}
const LIFE: f32 = 24.;
const COLLAPSE: f32 = 1.6;
const BLAST: f32 = 30.;
const REBIRTH: f32 = 8.;
const SHELL_TAU: f32 = 4.5;
impl Supernova {
    fn new() -> Self {
        let mut rng = Rng::new();
        let stars = starfield(800, &mut rng);
        let mut s = Self {
            phase: Phase::Star,
            t: 4.,
            time: 0.,
            spin: rng.next() * TAU,
            radius: 2.,
            zoom: 12.,
            shards: vec![],
            flares: vec![],
            gas: vec![],
            waves: vec![],
            pulsar: 0.,
            stars,
            pulse: Pulse::default(),
            rng,
        };
        s.radius = s.star_radius(&Sound::default());
        s
    }
    fn star_radius(&self, sound: &Sound) -> f32 {
        let grow = smoothstep(0., 4., self.t);
        let late = ((self.t - (LIFE - 6.)) / 6.).clamp(0., 1.);
        (0.35 + 1.85 * grow)
            * (1. + 0.05 * (self.time * 1.3).sin() + sound.bass * 0.14 + late * 0.12 * (self.time * (6. + late * 8.)).sin())
    }
    fn explode(&mut self) {
        self.shards.clear();
        let seed = self.rng.next() * 100.;
        let axis = norm([self.rng.signed() * 0.4, 1., self.rng.signed() * 0.4]);
        for i in 0..10000 {
            let (dir, speed, kind) = if i < 1100 {
                // An equatorial ring, as round some real remnants.
                let a = self.rng.next() * TAU;
                let flat = norm(cross(axis, [a.cos(), 0., a.sin()]));
                (flat, 0.78 + self.rng.next() * 0.04, Ejecta::Ring)
            } else if i < 1900 {
                // Two faint lobes along the axis.
                let side = if i % 2 == 0 { 1. } else { -1. };
                (norm(add(mul(axis, side), mul(self.rng.unit(), 0.38))), 1.1 + self.rng.next() * 0.3, Ejecta::Lobe)
            } else if i < 3000 {
                (self.rng.unit(), 0.25 + self.rng.next() * 0.6, Ejecta::Inner)
            } else {
                (self.rng.unit(), 1., Ejecta::Shell)
            };
            // Filaments: the crests of a noise field over direction. They
            // also stand a little proud of the shell.
            let n = noise3(add(mul(dir, 3.1), [seed, 0., 0.])) * 0.7 + noise3(add(mul(dir, 8.3), [0., seed, 0.])) * 0.3;
            let ridge = (1. - (n * 2. - 1.).abs()).powi(6);
            let speed = if kind == Ejecta::Shell { speed * (0.93 + ridge * 0.12) } else { speed };
            let oxygen = noise3(add(mul(dir, 1.4), [0., 0., seed])) > 0.52;
            self.shards.push(Shard { dir, speed: speed * 1.6, ridge, oxygen, kind });
        }
        self.waves.clear();
    }
    /// The radius of the outer shell `t` seconds after the blast.
    fn front(t: f32) -> f32 {
        1.6 * (SHELL_TAU * (1. - (-t / SHELL_TAU).exp()) + 0.05 * t)
    }
    fn step(&mut self, dt: f32, sound: &Sound) {
        self.time += dt;
        self.spin += dt * 0.12;
        let beat = self.pulse.hit(sound.bass, dt);
        match self.phase {
            Phase::Star => {
                // Strong music hastens the end.
                self.t += dt * (1. + sound.bass * 1.2);
                self.radius = self.star_radius(sound);
                let rate = 8. + sound.treble * 140. + beat.map_or(0., |k| k * 900.);
                let n = (rate * dt + self.rng.next()) as usize;
                for _ in 0..n.min(200) {
                    if self.flares.len() >= 1600 {
                        break;
                    }
                    let d = self.rng.unit();
                    let side = norm(cross(d, [0., 1., 0.]));
                    let kick = 0.9 + self.rng.next() * 1.6 + sound.bass;
                    self.flares.push(Mote {
                        p: mul(d, self.radius * 1.01),
                        v: add(mul(d, kick * self.radius * 0.6), mul(side, self.rng.signed() * 0.8)),
                        life: 1.,
                    });
                }
                if self.t >= LIFE {
                    self.phase = Phase::Collapse;
                    self.t = 0.;
                }
            }
            Phase::Collapse => {
                self.t += dt;
                let k = (self.t / COLLAPSE).min(1.);
                self.radius = (2.2 * (1. - k * k * k)).max(0.12);
                for m in &mut self.flares {
                    m.v = mul(m.p, -3.);
                }
                if self.t >= COLLAPSE {
                    self.phase = Phase::Blast;
                    self.t = 0.;
                    self.flares.clear();
                    self.explode();
                }
            }
            Phase::Blast => {
                self.t += dt * (1. + sound.bass * 0.35);
                self.pulsar += dt * (2.4 + sound.bass * 5.);
                if beat.is_some() && self.waves.len() < 6 {
                    self.waves.push(0.);
                }
                for w in &mut self.waves {
                    *w += dt * 7.;
                }
                let reach = Self::front(self.t);
                self.waves.retain(|&w| w < reach + 2.);
                if self.t >= BLAST {
                    self.phase = Phase::Rebirth;
                    self.t = 0.;
                    self.gas.clear();
                    for _ in 0..1800 {
                        let d = self.rng.unit();
                        let p = mul(d, 5. + self.rng.next() * 7.);
                        let swirl = mul(norm(cross([0., 1., 0.], p)), 1.2 + self.rng.next());
                        self.gas.push(Mote { p, v: swirl, life: 1. });
                    }
                }
            }
            Phase::Rebirth => {
                self.t += dt;
                self.pulsar += dt * 2.;
                for m in &mut self.gas {
                    let d = len(m.p).max(0.4);
                    let pull = mul(m.p, -14. / (d * d * d).max(4.));
                    let flatten = [0., -m.p[1] * 0.4, 0.];
                    m.v = mul(add(m.v, mul(add(pull, flatten), dt)), (-dt * 0.25).exp());
                    m.p = add(m.p, mul(m.v, dt));
                    if d < 0.5 {
                        m.life = 0.;
                    }
                }
                self.gas.retain(|m| m.life > 0.);
                if self.t >= REBIRTH {
                    self.phase = Phase::Star;
                    self.t = 0.;
                    self.shards.clear();
                    self.gas.clear();
                }
            }
        }
        for m in &mut self.flares {
            let d = len(m.p).max(0.2);
            m.v = add(m.v, mul(m.p, -6. * dt / (d * d * d)));
            m.p = add(m.p, mul(m.v, dt));
            m.life -= dt * 0.35;
            if len(m.p) < self.radius * 0.98 {
                m.life = 0.;
            }
        }
        self.flares.retain(|m| m.life > 0.);
        let target = match self.phase {
            Phase::Star | Phase::Collapse => self.radius * 3.4 + 4.,
            Phase::Blast => Self::front(self.t) * 2.6 + 4.,
            Phase::Rebirth => Self::front(BLAST) * 2.6 * (1. - self.t / REBIRTH * 0.6) + 4.,
        }
        .clamp(8., 34.);
        // The camera keeps ahead of a blast; it settles back slowly.
        let rate = if target > self.zoom { 2.5 } else { 0.6 };
        self.zoom += (target - self.zoom) * (1. - (-dt * rate).exp());
    }
    fn draw(&self, cv: &mut Canvas, sound: &Sound, gaze: (f32, f32)) {
        let az = self.spin + gaze.0 * 0.35;
        let el = (0.32 - gaze.1 * 0.15).clamp(0.1, 0.6);
        let eye = orbit([0., 0., 0.], az, el, self.zoom);
        cv.cam = Cam::look(eye, [0., 0., 0.], 1.0, cv.cols, cv.rows, cv.cam.aspect);
        let cam = cv.cam;
        draw_sky(cv, &self.stars, self.time, sound.treble);
        let time = self.time;
        match self.phase {
            Phase::Star | Phase::Collapse => {
                let age = (self.t / LIFE).clamp(0., 1.);
                let collapse = if self.phase == Phase::Collapse { self.t / COLLAPSE } else { 0. };
                let hot = grad(&[(0., (255, 215, 140)), (0.4, (255, 160, 70)), (1., (255, 85, 40))], age);
                let hot = mix(hot, (215, 230, 255), collapse);
                let boil = 0.2 + age * 0.35;
                let punch = 1. + sound.bass * 0.35 + collapse * 0.8;
                cv.sphere([0., 0., 0.], self.radius, &mut |hit, n, _| {
                    let q = yaw(n, -time * 0.08);
                    let g = noise3(add(mul(q, 3.2), [0., time * boil, 0.])) * 0.6 + noise3(add(mul(q, 8.5), [time * 0.4, 0., 0.])) * 0.4;
                    let limb = dot(n, norm(sub(cam.pos, hit))).max(0.).powf(0.7);
                    let g = smoothstep(0.25, 0.8, g);
                    let l = (0.15 + limb * (0.45 + g * 0.75)) * punch;
                    let color = mix(scale(hot, 0.6), mix(hot, (255, 245, 220), 0.35), g);
                    Some((ramp(l * 0.95), scale(color, 0.55 + l * 0.7)))
                });
                if let Some((sx, sy, sz)) = cam.project([0., 0., 0.]) {
                    let sr = self.radius * cam.f / sz;
                    corona(cv, (sx, sy), sr, sz + self.radius * 0.9, time, hot, 0.9 + sound.bass * 0.5, 0.75 + sound.treble * 0.4);
                }
                for m in &self.flares {
                    let l = m.life * (0.7 + sound.treble * 0.5);
                    cv.point(m.p, speck(l), scale(mix((255, 120, 40), (255, 230, 150), m.life), 0.4 + l * 0.7));
                }
            }
            Phase::Blast | Phase::Rebirth => {
                let fade = if self.phase == Phase::Rebirth { 1. - self.t / REBIRTH } else { 1. };
                let t = if self.phase == Phase::Rebirth { BLAST } else { self.t };
                // The flash: a white disc that swells and dies in a second.
                if self.phase == Phase::Blast && self.t < 1.4 {
                    if let Some((sx, sy, sz)) = cam.project([0., 0., 0.]) {
                        let k = 1. - self.t / 1.4;
                        let rad = cv.rows as f32 * (0.15 + self.t * 0.5);
                        let aspect = cam.aspect;
                        let (x0, x1) = ((sx - rad * aspect).max(0.) as usize, ((sx + rad * aspect).max(0.) as usize).min(cv.cols - 1));
                        let (y0, y1) = ((sy - rad).max(0.) as usize, ((sy + rad).max(0.) as usize).min(cv.rows - 1));
                        for y in y0..=y1 {
                            for x in x0..=x1 {
                                let (dx, dy) = ((x as f32 + 0.5 - sx) / aspect, y as f32 + 0.5 - sy);
                                let d = (dx * dx + dy * dy).sqrt() / rad;
                                if d < 1. {
                                    let l = (1. - d).powi(2) * k * 1.3;
                                    if l > 0.05 {
                                        cv.r.dot(x as i32, y as i32, sz + 40., ramp(l), scale((235, 240, 255), l));
                                    }
                                }
                            }
                        }
                    }
                }
                // The pulsar's lighthouse: a lit beam flashes the nebula.
                let ob: f32 = 0.65;
                let m = norm(tilt([ob.sin() * self.pulsar.cos(), ob.cos(), ob.sin() * self.pulsar.sin()], 0.35));
                let facing = dot(m, norm(cam.pos)).abs();
                let flash = facing.powi(12) * 0.6;
                let front = Self::front(t);
                let shell_cells = |cv: &mut Canvas, s: &Shard| {
                    let r = s.speed / 1.6 * front;
                    let p = mul(s.dir, r);
                    let heat = (-t / (2.5 + s.speed)).exp();
                    let cool = match s.kind {
                        Ejecta::Ring => (170, 235, 255),
                        Ejecta::Lobe => (200, 120, 255),
                        _ if s.oxygen => (60, 215, 200),
                        _ => (255, 70, 100),
                    };
                    let color = grad(&[(0., cool), (0.35, (255, 130, 60)), (0.7, (255, 225, 140)), (1., (235, 240, 255))], heat);
                    let body = match s.kind {
                        Ejecta::Shell => 0.06 + s.ridge * 1.1,
                        Ejecta::Inner => 0.2 + s.ridge * 0.3,
                        Ejecta::Ring => 0.85,
                        Ejecta::Lobe => 0.3 + s.ridge * 0.4,
                    };
                    let mut l = (body + heat * 0.7 + flash * s.ridge) * fade * (0.85 + sound.bass * 0.35);
                    for w in &self.waves {
                        let near = 1. - (r - w).abs() / 0.8;
                        if near > 0. {
                            l += near * 0.9;
                        }
                    }
                    if l > 0.22 {
                        cv.point(p, ramp(l * 0.8), scale(color, 0.3 + l * 0.8));
                    }
                };
                for s in &self.shards {
                    shell_cells(cv, s);
                }
                cv.sphere([0., 0., 0.], 0.22, &mut |_, n, _| {
                    let l = 0.7 + n[1].abs() * 0.3 + flash;
                    Some(('@', scale((210, 230, 255), l)))
                });
                for side in [1., -1.] {
                    let dir = mul(m, side);
                    for k in 1..36 {
                        let reach = k as f32 * 0.22;
                        let l = (1. - k as f32 / 36.) * (0.5 + flash * 1.5 + sound.treble * 0.4) * fade.max(0.3);
                        let core = mul(dir, 0.25 + reach);
                        cv.point(core, ramp(l), scale((170, 200, 255), 0.3 + l));
                        if k % 3 == 0 {
                            let spread = norm(cross(dir, [0.3, 0.1, 0.9]));
                            let spread2 = cross(dir, spread);
                            let cone = reach * 0.12;
                            for (a, b) in [(1., 0.), (-1., 0.), (0., 1.), (0., -1.)] {
                                cv.point(add(core, add(mul(spread, a * cone), mul(spread2, b * cone))), '·', scale((140, 170, 255), l * 0.8));
                            }
                        }
                    }
                }
                for g in &self.gas {
                    let l = 0.35 + (1. - len(g.p) / 12.).max(0.) * 0.6;
                    cv.point(g.p, speck(l), scale(mix((90, 120, 220), (255, 170, 110), 1. - len(g.p) / 12.), 0.3 + l * 0.6));
                }
                if self.phase == Phase::Rebirth {
                    let k = self.t / REBIRTH;
                    cv.sphere([0., 0., 0.], 0.15 + k * 0.3, &mut |_, n, _| {
                        let l = 0.6 + n[2].abs() * 0.3;
                        Some((ramp(l), scale((255, 190, 120), l)))
                    });
                }
            }
        }
    }
}

// ========================================================= solar system
struct Comet {
    a: f32,
    e: f32,
    node: f32,
    inc: f32,
    m: f32,
}
pub(crate) struct Solar {
    load: SysLoad,
    phases: Vec<f32>,
    time: f32,
    belt: Vec<(f32, f32, f32, f32)>,
    comets: Vec<Comet>,
    flares: Vec<Mote>,
    stars: Vec<Star>,
    sun: f32,
    pulse: Pulse,
    rng: Rng,
}
const PLANETS: usize = 12;
const PLANET_TINTS: [Color; 6] = [(120, 170, 255), (210, 180, 130), (110, 220, 200), (190, 140, 255), (230, 150, 110), (150, 210, 120)];
impl Solar {
    fn new() -> Self {
        let mut rng = Rng::new();
        let belt = (0..4200).map(|_| (rng.signed(), rng.next() * TAU, rng.signed() * 0.12, rng.next())).collect();
        let comets = (0..4)
            .map(|i| Comet { a: 9. + rng.next() * 5., e: 0.75 + rng.next() * 0.15, node: rng.next() * TAU, inc: rng.signed() * 0.6, m: i as f32 * 1.6 })
            .collect();
        let stars = starfield(700, &mut rng);
        Self { load: SysLoad::new(), phases: vec![], time: 0., belt, comets, flares: vec![], stars, sun: 1.4, pulse: Pulse::default(), rng }
    }
    fn layout(&self) -> (Vec<(f32, String)>, f32) {
        let groups = SysLoad::groups(&self.load.cores, PLANETS);
        let n = groups.len().max(1);
        let gap = (11. / (n.max(2) - 1) as f32).clamp(1.05, 2.4);
        (groups, gap)
    }
    fn orbit_radius(i: usize, gap: f32) -> f32 {
        3.9 + i as f32 * gap
    }
    fn planet_at(&self, i: usize, gap: f32) -> V {
        let a = Self::orbit_radius(i, gap);
        let e = 0.03 + 0.05 * variant(i as u32 * 7 + 1);
        let theta = self.phases.get(i).copied().unwrap_or(0.);
        Self::orbit_point(i, a, e, theta)
    }
    fn orbit_point(i: usize, a: f32, e: f32, theta: f32) -> V {
        let r = a * (1. - e * e) / (1. + e * theta.cos());
        let inc = (variant(i as u32 * 13 + 5) - 0.5) * 0.16;
        let node = variant(i as u32 * 31 + 2) * TAU;
        yaw(tilt([r * theta.cos(), 0., r * theta.sin()], inc), node)
    }
    fn step(&mut self, dt: f32, sound: &Sound) {
        self.time += dt;
        self.load.step(dt);
        let (groups, gap) = self.layout();
        self.phases.resize(groups.len(), 0.);
        for (i, ((load, _), ph)) in groups.iter().zip(&mut self.phases).enumerate() {
            let a = Self::orbit_radius(i, gap);
            // Busy cores race round; idle ones drift.
            *ph = (*ph + dt * (0.07 + load * 1.1 + sound.bass * 0.15) * (4. / a).sqrt()).rem_euclid(TAU);
        }
        let target = 1.7 + self.load.total * 1. + sound.bass * 0.22;
        self.sun += (target - self.sun) * (1. - (-dt * 5.).exp());
        for c in &mut self.comets {
            c.m = (c.m + dt * 0.28 * (6. / c.a).powf(1.5) * (1. + sound.treble * 1.5)).rem_euclid(TAU);
        }
        let beat = self.pulse.hit(sound.bass, dt);
        let n = ((4. + sound.treble * 40.) * dt + self.rng.next()) as usize + beat.map_or(0, |k| (k * 120.) as usize);
        for _ in 0..n {
            if self.flares.len() >= 1200 {
                break;
            }
            let d = self.rng.unit();
            let kick = 0.6 + self.rng.next() * 1.2 + sound.bass * 0.8;
            self.flares.push(Mote { p: mul(d, self.sun), v: add(mul(d, kick), mul(norm(cross(d, [0., 1., 0.])), self.rng.signed() * 0.5)), life: 1. });
        }
        for m in &mut self.flares {
            let d = len(m.p).max(0.3);
            m.v = add(m.v, mul(m.p, -2.2 * dt / (d * d * d)));
            m.p = add(m.p, mul(m.v, dt));
            m.life -= dt * 0.45;
            if len(m.p) < self.sun * 0.97 {
                m.life = 0.;
            }
        }
        self.flares.retain(|m| m.life > 0.);
    }
    fn draw(&self, cv: &mut Canvas, sound: &Sound, gaze: (f32, f32)) {
        let (groups, gap) = self.layout();
        let outer = Self::orbit_radius(groups.len().saturating_sub(1), gap);
        let az = self.time * 0.03 + gaze.0 * 0.35;
        let el = (0.55 - gaze.1 * 0.15).clamp(0.3, 0.8);
        // Fit the outer orbit across the width and its tilt down the height.
        let fov: f32 = 0.95;
        let half = (fov * 0.5).tan();
        let wide = (cv.cols as f32 / (cv.rows as f32 * cv.cam.aspect).max(1.)).min(1.6) * half;
        let dist = ((outer + 1.) / wide).max((outer * el.sin() + 2.) / half) * 1.02 + 1.;
        cv.cam = Cam::look(orbit([0., 0., 0.], az, el, dist), [0., 0., 0.], fov, cv.cols, cv.rows, cv.cam.aspect);
        let cam = cv.cam;
        let time = self.time;
        draw_sky(cv, &self.stars, time, sound.treble);
        // The sun, hotter and larger with total CPU.
        let total = self.load.total;
        let core = mix((255, 190, 80), (255, 245, 210), total);
        cv.sphere([0., 0., 0.], self.sun, &mut |hit, n, _| {
            let q = yaw(n, -time * 0.05);
            let g = noise3(add(mul(q, 4.), [0., time * 0.3, 0.])) * 0.6 + noise3(add(mul(q, 11.), [time * 0.5, 0., 0.])) * 0.4;
            let limb = dot(n, norm(sub(cam.pos, hit))).max(0.).powf(0.4);
            let l = limb * (0.55 + g * 0.6) * (1. + sound.bass * 0.3);
            Some((ramp(l * 0.95), scale(mix((230, 100, 30), core, g * 1.3), 0.4 + l * 0.7)))
        });
        if let Some((sx, sy, sz)) = cam.project([0., 0., 0.]) {
            let sr = self.sun * cam.f / sz;
            corona(cv, (sx, sy), sr, sz + self.sun * 0.9, time, core, 0.7 + total * 0.6 + sound.bass * 0.4, 0.7 + sound.treble * 0.3);
        }
        for m in &self.flares {
            let l = m.life * (0.7 + sound.treble * 0.4);
            cv.point(m.p, speck(l), scale((255, 170, 70), 0.35 + l * 0.7));
        }
        // Memory: an asteroid belt between the inner third and the rest.
        let mem = self.load.mem;
        let k = groups.len() / 3;
        let belt_r = Self::orbit_radius(k, gap) + gap * 0.5;
        let belt_color = grad(&[(0., (110, 200, 130)), (0.6, (230, 190, 90)), (0.9, (255, 90, 70))], mem);
        let count = (300. + mem * 3900.) as usize;
        for &(u, a0, y, seed) in self.belt.iter().take(count) {
            let r = belt_r + u * gap * 0.32;
            let a = a0 + time * 0.25 * (5. / r).powf(1.5);
            let tw = 0.5 + 0.5 * (time * 2. + seed * 40.).sin() * sound.treble;
            cv.point([r * a.cos(), y, r * a.sin()], if seed < 0.08 { '•' } else { '.' }, scale(belt_color, 0.35 + seed * 0.4 + tw * 0.3));
        }
        for (i, (load, name)) in groups.iter().enumerate() {
            let a = Self::orbit_radius(i, gap);
            let e = 0.03 + 0.05 * variant(i as u32 * 7 + 1);
            let tint = PLANET_TINTS[i % PLANET_TINTS.len()];
            let color = mix(tint, (255, 80, 50), load.powf(1.4));
            // The orbit, dotted, and a bright wake that lengthens with speed.
            let theta = self.phases.get(i).copied().unwrap_or(0.);
            let steps = (a * 10.) as usize;
            for s in 0..steps {
                let th = s as f32 / steps as f32 * TAU;
                cv.point(Self::orbit_point(i, a, e, th), '·', (34, 40, 56));
            }
            let wake = 0.3 + load * 2.;
            let ws = (wake * a * 5.) as usize + 2;
            for s in 0..ws {
                let f = s as f32 / ws as f32;
                let p = Self::orbit_point(i, a, e, theta - f * wake);
                cv.point(p, if f < 0.3 { '•' } else { '·' }, scale(color, 0.9 - f * 0.75));
            }
            let c = self.planet_at(i, gap);
            let pr = (0.55 + load * 0.5 + 0.15 * variant(i as u32 * 3 + 11)) * (1. + sound.bass * 0.12);
            let banded = i % 3 == 0;
            cv.sphere(c, pr, &mut |hit, n, _| {
                let l_dir = norm(mul(hit, -1.));
                let diff = dot(n, l_dir).max(0.);
                let tex = if banded {
                    0.6 + 0.4 * (n[1] * 9. + noise3(add(mul(n, 2.), [i as f32, time * 0.2, 0.])) * 3.).sin()
                } else {
                    0.55 + noise3(add(mul(n, 3.5), [i as f32 * 5., 0., 0.])) * 0.6
                };
                let l = 0.07 + diff * tex * 1.05;
                Some((ramp(l), scale(color, 0.25 + l * 0.9)))
            });
            if let Some((_, _, z)) = cam.project(c) {
                let rise = pr * cam.f / z + 1.5;
                let tag = if cv.cols >= 170 { format!("{name} {:.0}%", load * 100.) } else { format!("{:.0}%", load * 100.) };
                cv.label(c, rise, &tag, scale(color, 0.85));
            }
        }
        // Swap: comets, more of them as swap fills; treble stretches tails.
        let shown = 1 + (self.load.swap * 4.).round() as usize;
        for c in self.comets.iter().take(shown.min(self.comets.len())) {
            let mut ecc = c.m;
            for _ in 0..5 {
                ecc = c.m + c.e * ecc.sin();
            }
            let x = c.a * (ecc.cos() - c.e);
            let z = c.a * (1. - c.e * c.e).sqrt() * ecc.sin();
            let p = yaw(tilt([x, 0., z], c.inc), c.node);
            let away = norm(p);
            let tail = 1. + sound.treble * 2.5 + self.load.swap * 3. + 4. / len(p).max(1.);
            for s in 0..18 {
                let f = s as f32 / 18.;
                cv.point(add(p, mul(away, f * tail)), if s == 0 { '✦' } else { speck(1. - f) }, scale((170, 230, 255), 1. - f * 0.8));
            }
        }
        let hud = self.load.hud();
        cv.text(2, cv.rows as i32 - 2, 0., &hud, (150, 170, 200));
    }
}

// ================================================================ abyss
const PERIOD: f32 = 1024.;
/// A wavenumber that repeats exactly over the world's period.
fn kz(n: f32) -> f32 {
    TAU * n / PERIOD
}
fn floor_h(x: f32, z: f32) -> f32 {
    let big = noise2p(x / 16., z / 16., (PERIOD / 16.) as i32);
    let mid = noise2p(x / 4., z / 4., (PERIOD / 4.) as i32);
    let ripple = (z * kz(640.) + x * 0.35 + mid * 4.).sin() * 0.035 * (0.4 + big);
    big * 2.2 + mid * 0.45 + ripple - 2.
}
fn caustic(x: f32, z: f32, t: f32) -> f32 {
    let c = (x * 1.1 + t * 0.9 + (z * kz(114.) + t * 0.5).sin()).sin() * (z * kz(212.) - t * 0.7 + (x * 0.8 - t * 0.4).sin()).sin();
    (1. - c.abs()).powi(7)
}
struct Kelp {
    root: V,
    tall: f32,
    phase: f32,
    glow: bool,
}
/// A fish of one of the abyss's shoals; `kind` picks the shoal.
struct Fish {
    p: V,
    v: V,
    phase: f32,
    kind: usize,
}
/// The shoals: silver fish, a few large golden ones and glowing lanternfish.
/// Each is (how many, how wide, how high, how deep the shoal spreads, top speed).
const SHOALS: [(usize, V, f32); 3] = [(150, [2.8, 1., 2.], 4.5), (22, [3.8, 1.3, 3.2], 3.), (60, [2.2, 1.5, 2.2], 3.6)];
pub(crate) struct Abyss {
    time: f32,
    z: f32,
    cam_y: f32,
    plankton: Vec<V>,
    snow: Vec<V>,
    waves: Vec<(V, f32)>,
    plume: Vec<Mote>,
    manta: f32,
    school: Vec<Fish>,
    lamp: Cell<Option<V>>,
    cursor: Option<(f32, f32)>,
    last_gaze: (f32, f32),
    pulse: Pulse,
    rng: Rng,
}
impl Abyss {
    fn new() -> Self {
        let mut rng = Rng::new();
        let z = rng.next() * PERIOD;
        let cam_y = floor_h(0., z) + 2.4;
        let box_point = |rng: &mut Rng| [rng.signed() * 16., cam_y - 3. + rng.next() * 7., z - 1. + rng.next() * 31.];
        let plankton = (0..5200).map(|_| box_point(&mut rng)).collect();
        let snow = (0..900).map(|_| box_point(&mut rng)).collect();
        let school = SHOALS
            .iter()
            .enumerate()
            .flat_map(|(kind, &(n, _, _))| (0..n).map(move |_| kind))
            .map(|kind| Fish { p: box_point(&mut rng), v: [0., 0., 0.], phase: rng.next() * TAU, kind })
            .collect();
        Self { time: 0., z, cam_y, plankton, snow, waves: vec![], plume: vec![], manta: rng.next() * TAU, school, lamp: Cell::new(None), cursor: None, last_gaze: (0., 0.), pulse: Pulse::default(), rng }
    }
    fn eye(&self) -> V {
        [(self.time * 0.05).sin() * 4., self.cam_y, self.z]
    }
    fn step(&mut self, dt: f32, sound: &Sound, cursor: Option<(f32, f32)>, gaze: (f32, f32)) {
        self.time += dt;
        self.cursor = cursor;
        let speed = 0.7 + sound.bass * 0.5;
        self.z += dt * speed;
        if self.z >= PERIOD {
            // The world repeats exactly; carry everything back with it.
            self.z -= PERIOD;
            for p in self.plankton.iter_mut().chain(self.snow.iter_mut()) {
                p[2] -= PERIOD;
            }
            for (c, _) in &mut self.waves {
                c[2] -= PERIOD;
            }
            for m in &mut self.plume {
                m.p[2] -= PERIOD;
            }
            for f in &mut self.school {
                f.p[2] -= PERIOD;
            }
        }
        let eye = self.eye();
        let ground = (0..4).map(|i| floor_h(eye[0], self.z + i as f32 * 1.5)).fold(f32::MIN, f32::max);
        self.cam_y += (ground + 2.3 - self.cam_y) * (1. - (-dt * 1.2).exp());
        let lamp = cursor.and(self.lamp.get());
        let t = self.time;
        let (lo_y, hi_y) = (self.cam_y - 3.5, self.cam_y + 4.5);
        for (i, p) in self.plankton.iter_mut().enumerate() {
            let k = i as f32;
            let mut v = [
                (p[2] * 0.3 + t * 0.2 + k).sin() * 0.18,
                (p[0] * 0.4 + t * 0.3 + k * 0.7).sin() * 0.07 + 0.02,
                (p[1] * 0.5 + t * 0.25 + k * 1.3).cos() * 0.12 - 0.05,
            ];
            if let Some(l) = lamp {
                // Drawn to the light.
                let d = sub(l, *p);
                let dist = len(d);
                if dist < 4. && dist > 0.2 {
                    v = add(v, mul(d, (1. - dist / 4.) * 0.6 / dist));
                }
            }
            *p = add(*p, mul(v, dt));
            if p[2] < self.z - 1.5 {
                p[2] += 31.;
            } else if p[2] > self.z + 30. {
                p[2] -= 31.;
            }
            if p[0] < eye[0] - 16. {
                p[0] += 32.;
            } else if p[0] > eye[0] + 16. {
                p[0] -= 32.;
            }
            if p[1] < lo_y {
                p[1] += 8.;
            } else if p[1] > hi_y {
                p[1] -= 8.;
            }
        }
        for p in &mut self.snow {
            p[1] -= dt * 0.18;
            p[0] += (t * 0.4 + p[2]).sin() * dt * 0.05;
            if p[1] < floor_h(p[0], p[2]) || p[1] < lo_y {
                p[1] = hi_y;
            }
            if p[2] < self.z - 1.5 {
                p[2] += 31.;
            }
            if p[0] < eye[0] - 16. {
                p[0] += 32.;
            } else if p[0] > eye[0] + 16. {
                p[0] -= 32.;
            }
        }
        // A beat sends a bioluminescent ring through the water.
        if let Some(k) = self.pulse.hit(sound.bass, dt) {
            let at = lamp.unwrap_or_else(|| [eye[0] + self.rng.signed() * 8., self.cam_y - 1.5, self.z + 6. + self.rng.next() * 14.]);
            if self.waves.len() < 5 {
                self.waves.push((at, 0.));
            }
            if let Some(l) = lamp {
                for _ in 0..(k * 60.) as usize {
                    let v = [self.rng.signed() * 0.8, 0.8 + self.rng.next() * 1.4, self.rng.signed() * 0.8];
                    self.plume.push(Mote { p: l, v, life: 1. });
                }
            }
        }
        for (_, age) in &mut self.waves {
            *age += dt;
        }
        self.waves.retain(|(_, a)| *a < 4.5);
        // The lamp stirs the sand as it moves.
        let moved = ((gaze.0 - self.last_gaze.0).powi(2) + (gaze.1 - self.last_gaze.1).powi(2)).sqrt() / dt.max(1e-3);
        self.last_gaze = gaze;
        if let Some(l) = lamp {
            let n = (moved * 40. * dt + self.rng.next() * 0.6) as usize;
            for _ in 0..n.min(30) {
                let v = [self.rng.signed() * 0.6, 0.5 + self.rng.next(), self.rng.signed() * 0.6];
                self.plume.push(Mote { p: add(l, [self.rng.signed() * 0.4, 0.05, self.rng.signed() * 0.4]), v, life: 1. });
            }
        }
        for m in &mut self.plume {
            m.v = mul(add(m.v, [0., -0.6 * dt, 0.]), (-dt * 0.9).exp());
            m.p = add(m.p, mul(m.v, dt));
            m.life -= dt * 0.3;
            let floor = floor_h(m.p[0], m.p[2]);
            if m.p[1] < floor {
                m.p[1] = floor;
                m.v = [0., 0., 0.];
            }
        }
        self.plume.retain(|m| m.life > 0.);
        if self.plume.len() > 1500 {
            let extra = self.plume.len() - 1500;
            self.plume.drain(..extra);
        }
        self.manta += dt * 0.11 * (1. + sound.bass * 0.4);
        // A school wheels round a wandering leader, darts on the beat and
        // scatters from the lamp.
        // Each shoal follows its own leader at its own height and distance.
        let leads = [
            [eye[0] + (t * 0.23).sin() * 6., self.cam_y - 0.3 + (t * 0.37).sin() * 1.2, self.z + 10. + (t * 0.19).sin() * 4.],
            [eye[0] + (t * 0.15 + 2.).sin() * 7., self.cam_y + 0.7 + (t * 0.21).sin() * 0.8, self.z + 15. + (t * 0.11).sin() * 5.],
            [eye[0] + (t * 0.31 + 4.).sin() * 5., self.cam_y - 1.4 + (t * 0.43).sin() * 0.7, self.z + 7. + (t * 0.27).sin() * 3.],
        ];
        let dash = 1. + sound.bass * 1.5;
        for (i, f) in self.school.iter_mut().enumerate() {
            let k = i as f32;
            let (_, spread, top) = SHOALS[f.kind];
            let lead = leads[f.kind];
            let off = yaw([(k * 0.71).sin() * spread[0], (k * 1.37).cos() * spread[1], (k * 0.53).cos() * spread[2]], t * 0.45 + f.phase);
            let goal = add(lead, off);
            let mut acc = mul(sub(goal, f.p), 1.6);
            if let Some(l) = lamp {
                let away = sub(f.p, l);
                let d = len(away);
                if d < 3.5 {
                    acc = add(acc, mul(away, (3.5 - d) * 6. / d.max(0.2)));
                }
            }
            f.v = mul(add(f.v, mul(acc, dt)), (-dt * 1.4).exp());
            let speed = len(f.v);
            let top = top * dash;
            if speed > top {
                f.v = mul(f.v, top / speed);
            }
            f.p = add(f.p, mul(f.v, dt));
        }
    }
    fn bio(&self, p: V) -> f32 {
        self.waves
            .iter()
            .map(|&(c, age)| {
                let near = 1. - (len(sub(p, c)) - age * 5.).abs() / 1.;
                near.max(0.) * (1. - age / 4.5)
            })
            .sum()
    }
    fn draw(&self, cv: &mut Canvas, sound: &Sound, gaze: (f32, f32)) {
        let cursor = self.cursor.map(|_| gaze);
        let eye = self.eye();
        let t = self.time;
        let target = [eye[0] + (t * 0.07).sin() * 2. + gaze.0 * 1.2, self.cam_y - 1.3 - gaze.1 * 0.6, self.z + 7.];
        cv.cam = Cam::look(eye, target, 1.1, cv.cols, cv.rows, cv.cam.aspect);
        let cam = cv.cam;
        // Where the lamp meets the floor: march the cursor's ray.
        let lamp = cursor.and_then(|(gx, gy)| {
            let d = cam.ray((gx + 1.) * 0.5 * cv.cols as f32, (gy + 1.) * 0.5 * cv.rows as f32);
            let mut s = 0.3;
            while s < 40. {
                let p = add(eye, mul(d, s));
                if p[1] <= floor_h(p[0], p[2]) {
                    return Some(p);
                }
                s += 0.25 + s * 0.04;
            }
            None
        });
        self.lamp.set(lamp);
        let fog_col: Color = (3, 20, 38);
        let fog = |depth: f32| (-depth * 0.065).exp();
        let sun = norm([0.5, 0.8, 0.25]);
        let caustic_gain = 0.75 + sound.treble * 0.6;
        let light_at = |p: V, n: V| -> (f32, f32, f32) {
            let diff = dot(n, sun).max(0.);
            let ca = caustic(p[0], p[2], t) * caustic_gain;
            let lamp_k = lamp.map_or(0., |l| {
                let d2 = (p[0] - l[0]).powi(2) + (p[2] - l[2]).powi(2);
                (-d2 / 2.4).exp() * 1.3
            });
            let bio = self.bio(p);
            (0.08 + diff * 0.62 + ca + lamp_k + bio * 0.8, lamp_k, bio)
        };
        // The sand: a world-fixed grid, so it never swims.
        let step = 0.6;
        let x_lo = ((eye[0] - 22.) / step).floor() as i32;
        let z_lo = ((self.z - 0.5) / step).floor() as i32;
        let (nx, nz) = (74usize, 58usize);
        let mut row_prev: Vec<Option<Vx>> = Vec::with_capacity(nx);
        let mut row: Vec<Option<Vx>> = Vec::with_capacity(nx);
        let sand: Color = (196, 170, 120);
        let mut paint = |a: [f32; 3], depth: f32| {
            let f = fog(depth);
            let base = mix(mix(sand, (255, 215, 160), a[1].min(1.)), (90, 230, 255), a[2].min(1.));
            let l = a[0];
            Some((block(l * f), mix(fog_col, scale(base, 0.3 + l * 0.75), f)))
        };
        // Heights once per vertex, with a one-vertex border for the normals.
        let w = nx + 2;
        let mut heights = vec![0f32; w * (nz + 2)];
        for j in 0..nz + 2 {
            let z = (z_lo + j as i32 - 1) as f32 * step;
            for i in 0..w {
                heights[j * w + i] = floor_h((x_lo + i as i32 - 1) as f32 * step, z);
            }
        }
        for j in 0..nz {
            let z = (z_lo + j as i32) as f32 * step;
            row.clear();
            for i in 0..nx {
                let x = (x_lo + i as i32) as f32 * step;
                let at = |di: usize, dj: usize| heights[(j + dj) * w + i + di];
                let h = at(1, 1);
                // Relief exaggerated for the light, so dunes read at a glance.
                let n = norm([(at(0, 1) - at(2, 1)) * 1.1, 0.6, (at(1, 0) - at(1, 2)) * 1.1]);
                let p = [x, h, z];
                let (l, lk, bio) = light_at(p, n);
                row.push(cv.vx(p, [l, lk, bio]));
            }
            if j > 0 {
                for i in 0..nx - 1 {
                    if let (Some(a), Some(b), Some(c), Some(d)) = (row_prev[i], row_prev[i + 1], row[i], row[i + 1]) {
                        cv.tri(a, b, c, &mut paint);
                        cv.tri(b, d, c, &mut paint);
                    }
                }
            }
            std::mem::swap(&mut row, &mut row_prev);
        }
        // Boulders, half sunk, on an 8-unit lattice.
        let tile = 8.;
        let per = (PERIOD / tile) as i32;
        for tz in ((self.z - 2.) / tile).floor() as i32..=((self.z + 34.) / tile).floor() as i32 {
            for tx in ((eye[0] - 22.) / tile).floor() as i32..=((eye[0] + 22.) / tile).floor() as i32 {
                let id = hash3(tx, tz.rem_euclid(per), 11);
                if id > 0.16 {
                    continue;
                }
                let x = (tx as f32 + 0.2 + hash3(tx, tz.rem_euclid(per), 12) * 0.6) * tile;
                let z = (tz as f32 + 0.2 + hash3(tx, tz.rem_euclid(per), 13) * 0.6) * tile;
                let rad = 0.6 + id * 6.;
                let c = [x, floor_h(x, z) + rad * 0.35, z];
                cv.sphere(c, rad, &mut |hit, n, depth| {
                    let (l, lk, bio) = light_at(hit, n);
                    let moss = noise3(mul(hit, 1.7));
                    let base = mix(mix((88, 92, 98), (70, 110, 80), moss), (255, 215, 160), lk.min(1.));
                    let base = mix(base, (90, 230, 255), bio.min(1.));
                    let f = fog(depth);
                    Some((block(l * f), mix(fog_col, scale(base, 0.25 + l * 0.7), f)))
                });
            }
        }
        // Kelp on a 4-unit lattice, swaying more with the bass.
        let tile = 4.;
        let per = (PERIOD / tile) as i32;
        for tz in ((self.z - 1.) / tile).floor() as i32..=((self.z + 30.) / tile).floor() as i32 {
            for tx in ((eye[0] - 18.) / tile).floor() as i32..=((eye[0] + 18.) / tile).floor() as i32 {
                let tzw = tz.rem_euclid(per);
                let id = hash3(tx, tzw, 21);
                if id > 0.2 {
                    continue;
                }
                let x = (tx as f32 + hash3(tx, tzw, 22)) * tile;
                let z = (tz as f32 + hash3(tx, tzw, 23)) * tile;
                let k = Kelp { root: [x, floor_h(x, z), z], tall: 1.4 + id * 12., phase: id * 40., glow: id < 0.05 };
                let segs = 9;
                let mut prev = k.root;
                for s in 1..=segs {
                    let f = s as f32 / segs as f32;
                    let sway = (t * 0.8 + k.phase + f * 2.2).sin() * 0.35 * f * (1. + sound.bass * 1.2);
                    let p = [k.root[0] + sway, k.root[1] + f * k.tall, k.root[2] + (t * 0.5 + k.phase).cos() * 0.15 * f];
                    let depth = cam.view(p)[2];
                    let fade = fog(depth);
                    let lk = lamp.map_or(0., |l| (-(len(sub(p, l)).powi(2)) / 3.).exp());
                    let color = mix(fog_col, scale(mix((95, 175, 80), (255, 220, 150), lk), 0.6 + lk * 0.5), fade.max(0.15));
                    let ch = if sway > 0.12 { ')' } else if sway < -0.12 { '(' } else { '|' };
                    cv.line(prev, p, Some(ch), color);
                    cv.line(add(prev, [0.14, 0., 0.]), add(p, [0.14, 0., 0.]), Some(ch), scale(color, 0.7));
                    prev = p;
                }
                if k.glow {
                    let l = 0.6 + sound.treble * 0.8 + (t * 2. + k.phase).sin() * 0.2;
                    cv.point(prev, speck(l), scale((120, 255, 220), l));
                }
            }
        }
        // The manta, gliding ahead and banking through its turns.
        let m = self.manta;
        let at = |m: f32| [eye[0] + m.sin() * 6., self.cam_y + 0.3 + (m * 2.3).sin() * 0.6, self.z + 13. + (m * 0.7).cos() * 3.];
        let (here, next) = (at(m), at(m + 0.01));
        let heading = norm(sub(next, here));
        let bank = (heading[0] * 1.2).clamp(-0.6, 0.6);
        let flap = (t * 1.7 * (1. + sound.bass * 0.8)).sin() * 0.55;
        let yaw_a = heading[0].atan2(heading[2]);
        let place = |p: V| add(here, yaw(rot_z(p, -bank), yaw_a));
        let pts = [
            [0., 0.05, 1.1],
            [0., 0., -0.9],
            [-0.4, 0.12, 0.15],
            [0.4, 0.12, 0.15],
            [-0.95, flap * 0.45, -0.05],
            [0.95, flap * 0.45, -0.05],
            [-1.8, flap, -0.35],
            [1.8, flap, -0.35],
        ]
        .map(|p| place(mul(p, 0.95)));
        let tris = [(0, 2, 3), (2, 1, 3), (0, 4, 2), (2, 4, 1), (0, 6, 4), (4, 6, 1), (0, 3, 5), (3, 1, 5), (0, 5, 7), (5, 1, 7)];
        for (a, b, c) in tris {
            let n = norm(cross(sub(pts[b], pts[a]), sub(pts[c], pts[a])));
            let l = 0.3 + dot(n, sun).abs() * 0.7 + self.bio(pts[a]) * 0.5;
            let (Some(va), Some(vb), Some(vc)) = (cv.vx(pts[a], [l, 0., 0.]), cv.vx(pts[b], [l, 0., 0.]), cv.vx(pts[c], [l, 0., 0.])) else { continue };
            cv.tri(va, vb, vc, &mut |a, depth| {
                let f = (-depth * 0.03).exp();
                Some((ramp(a[0] * f * 0.9), mix(fog_col, scale((115, 145, 180), 0.35 + a[0] * 0.8), f)))
            });
        }
        cv.line(pts[1], place([0., 0., -2.2]), Some('·'), (60, 80, 100));
        // Water: plankton, glowing where a ring passes or near the lamp.
        for (i, p) in self.plankton.iter().enumerate() {
            let Some((x, y, z)) = cam.project(*p) else { continue };
            let f = fog(z);
            let tw = 0.5 + 0.5 * (t * 2.2 + i as f32).sin();
            let bio = self.bio(*p);
            let lk = lamp.map_or(0., |l| (1. - len(sub(*p, l)) / 4.).max(0.));
            let l = (0.25 + tw * 0.25 + sound.treble * 0.35 * tw + bio * 1.2 + lk) * (0.35 + 0.65 * f);
            let near = 1. / z.max(0.5);
            let ch = if near > 0.5 { 'o' } else if near > 0.2 { '°' } else { speck(l) };
            let color = mix((40, 120, 140), (120, 255, 240), (bio + lk).min(1.));
            cv.dot(x, y, z, ch, mix(fog_col, scale(color, 0.4 + l), f.max(0.3)));
        }
        for f in &self.school {
            let Some((x, y, z)) = cam.project(f.p) else { continue };
            let ahead = cam.project(add(f.p, mul(f.v, 0.1))).map_or(0., |a| a.0 - x);
            let fade = fog(z);
            let glint = 0.55 + 0.45 * (t * 3. + f.phase * 5.).sin().max(0.) * (0.4 + sound.bass);
            let right = ahead >= 0.;
            let dir = if right { 1. } else { -1. };
            match f.kind {
                0 => {
                    cv.dot(x, y, z, if right { '>' } else { '<' }, mix(fog_col, scale((175, 200, 220), glint), fade.max(0.25)));
                    if z < 6. {
                        // Near fish get a tail.
                        cv.dot(x - dir, y, z, '=', mix(fog_col, scale((140, 165, 190), glint), fade));
                    }
                }
                1 => {
                    // Large golden fish: a whole fish when near, a fleck far off.
                    let gold = mix(fog_col, scale((255, 175, 70), 0.5 + glint * 0.6), fade.max(0.3));
                    if z < 14. {
                        let body = if right { ['>', '<', '>'] } else { ['<', '>', '<'] };
                        for (j, ch) in body.into_iter().enumerate() {
                            cv.dot(x + j as f32 - 1., y, z, ch, gold);
                        }
                        if z < 7. {
                            cv.dot(x - dir * 2., y, z, if (t * 6. + f.phase).sin() > 0. { '/' } else { '\\' }, scale(gold, 0.8));
                        }
                    } else {
                        cv.dot(x, y, z, if right { '>' } else { '<' }, gold);
                    }
                }
                _ => {
                    // Lanternfish: a dim body behind a light that pulses
                    // with the treble and flares when a ring passes.
                    let lure = (0.5 + sound.treble * 0.8 + self.bio(f.p) * 1.5 + (t * 2.5 + f.phase).sin() * 0.2).min(1.6);
                    cv.dot(x, y, z, '·', mix(fog_col, (70, 95, 130), fade.max(0.25)));
                    cv.dot(x + dir, y, z - 0.01, speck(lure), mix(fog_col, scale((110, 255, 225), 0.4 + lure * 0.5), fade.max(0.5)));
                }
            }
        }
        for p in &self.snow {
            if let Some((x, y, z)) = cam.project(*p) {
                let f = fog(z);
                cv.dot(x, y, z, '.', mix(fog_col, (190, 200, 210), f * 0.8));
            }
        }
        for m in &self.plume {
            if let Some((x, y, z)) = cam.project(m.p) {
                let f = fog(z);
                cv.dot(x, y, z, if m.life > 0.7 { ':' } else { '·' }, mix(fog_col, scale(sand, 0.4 + m.life * 0.6), f));
            }
        }
        // Shafts of light from the surface, behind everything else.
        let top = (cv.rows as f32 * 0.7) as usize;
        let aspect = cam.aspect;
        for y in 0..top {
            let fall = 1. - y as f32 / top as f32;
            for x in 0..cv.cols {
                let s = (x as f32 / aspect + y as f32 * 0.55) * 0.22 + gaze.0 * 0.4;
                let n = noise3([s, 0.5, t * 0.08]);
                if n > 0.64 {
                    let l = (n - 0.64) / 0.36 * fall * (0.7 + sound.treble * 0.4);
                    // Dithered, so a shaft is a haze rather than a slab.
                    if l > 0.08 && hash3(x as i32, y as i32, (t * 4.) as i32) < l * 1.6 {
                        cv.r.dot(x as i32, y as i32, 9e3, if l > 0.4 { '\\' } else { '·' }, scale((70, 140, 170), 0.25 + l * 0.6));
                    }
                }
            }
        }
    }
}
/// Rotation about the z axis.
fn rot_z(p: V, a: f32) -> V {
    let (s, c) = a.sin_cos();
    [p[0] * c - p[1] * s, p[0] * s + p[1] * c, p[2]]
}

// ========================================================== load canyon
pub(crate) struct Canyon {
    load: SysLoad,
    hist: VecDeque<Vec<f32>>,
    pushed: u64,
    clock: f32,
    time: f32,
    ceiling: f32,
    sparks: Vec<Mote>,
    /// Dust and embers drifting over the whole canyon, carried back with it.
    drift: Vec<V>,
    pulse: Pulse,
    rng: Rng,
}
const HISTORY: usize = 96;
const DRIFT: usize = 1400;
const TICK: f32 = 0.25;
const LANES: usize = 24;
const LANE_W: f32 = 1.3;
const ROW_D: f32 = 0.42;
const PEAK: f32 = 4.2;
impl Canyon {
    fn new() -> Self {
        let load = SysLoad::new();
        let first: Vec<f32> = SysLoad::groups(&load.target, LANES).into_iter().map(|(v, _)| v).collect();
        let hist = (0..HISTORY).map(|_| first.clone()).collect();
        let mut rng = Rng::new();
        let width = first.len().max(1) as f32 * LANE_W;
        let drift = (0..DRIFT).map(|_| [rng.next() * width, rng.next() * (PEAK + 3.), rng.next() * HISTORY as f32 * ROW_D]).collect();
        Self { load, hist, pushed: 0, clock: 0., time: 0., ceiling: 0.25, sparks: vec![], drift, pulse: Pulse::default(), rng }
    }
    fn step(&mut self, dt: f32, sound: &Sound) {
        self.time += dt;
        self.load.step(dt);
        self.clock += dt;
        while self.clock >= TICK {
            self.clock -= TICK;
            let row: Vec<f32> = SysLoad::groups(&self.load.cores, LANES).into_iter().map(|(v, _)| v).collect();
            if self.hist.front().is_some_and(|f| f.len() != row.len()) {
                self.hist.clear();
            }
            self.hist.push_front(row);
            self.hist.truncate(HISTORY);
            self.pushed += 1;
        }
        // Small loads still make relief: the scale follows the recent peak.
        let peak = self.hist.iter().flatten().fold(0f32, |m, &v| m.max(v));
        let target = peak.max(0.1);
        self.ceiling += (target - self.ceiling) * (1. - (-dt * 0.8).exp());
        let beat = self.pulse.hit(sound.bass, dt);
        if let Some(front) = self.hist.front() {
            for (i, &v) in front.iter().enumerate() {
                let rate = (0.6 + (v / self.ceiling).powi(2) * 30.) * (1. + sound.bass * 2.5) + beat.map_or(0., |k| k * 70.);
                let count = (rate * dt + self.rng.next()) as usize;
                for _ in 0..count.min(40) {
                    if self.sparks.len() >= 5000 {
                        break;
                    }
                    let x = (i as f32 + 0.5) * LANE_W;
                    let h = PEAK * v / self.ceiling;
                    self.sparks.push(Mote {
                        p: [x + self.rng.signed() * 0.2, h, 0.1],
                        v: [self.rng.signed() * 0.6, 2. + self.rng.next() * 3. + sound.bass * 3., 0.4 + self.rng.next() * 0.8],
                        life: 1.,
                    });
                }
            }
        }
        for m in &mut self.sparks {
            m.v[1] -= 4. * dt;
            m.p = add(m.p, mul(m.v, dt));
            m.life -= dt * 0.7;
        }
        self.sparks.retain(|m| m.life > 0. && m.p[1] > -0.5);
        // The dust rides back with the history and lifts on the bass.
        let width = self.hist.front().map_or(1, |f| f.len().max(1)) as f32 * LANE_W;
        let length = HISTORY as f32 * ROW_D;
        let (t, flow) = (self.time, ROW_D / TICK);
        for (i, p) in self.drift.iter_mut().enumerate() {
            let k = i as f32;
            p[0] += (p[2] * 0.4 + t * 0.3 + k).sin() * dt * (0.3 + sound.bass * 1.2);
            p[1] += ((p[0] * 0.5 + t * 0.4 + k * 0.7).sin() * 0.25 + sound.bass * 0.6 - 0.05) * dt;
            p[2] += flow * dt;
            if p[2] > length {
                p[2] -= length;
            }
            p[0] = p[0].rem_euclid(width);
            if p[1] > PEAK + 3. || p[1] < 0. {
                p[1] = p[1].rem_euclid(PEAK + 3.);
            }
        }
    }
    fn height(&self, k: usize, q: f32, sound: &Sound) -> f32 {
        let row = &self.hist[k.min(self.hist.len() - 1)];
        let n = row.len().max(1);
        let f = (q - 0.5).clamp(0., n as f32 - 1.);
        let i0 = f.floor() as usize;
        let i1 = (i0 + 1).min(n - 1);
        let s = s3(f - i0 as f32);
        let v = row.get(i0).copied().unwrap_or(0.) * (1. - s) + row.get(i1).copied().unwrap_or(0.) * s;
        let ridge = 0.55 + 0.45 * (TAU * (q - 0.5)).cos();
        let live = sound.bass * 0.4 * (q * 2.7 + self.time * 6.).sin() * (-(k as f32) * 0.25).exp();
        // Square root: an idle core still shows relief beside a busy one.
        let grain = noise2p(q * 1.7, (self.pushed as f32 - k as f32) * 0.35, 1 << 20) * 0.18;
        PEAK * (v / self.ceiling).clamp(0., 1.).sqrt() * ridge + 0.05 + grain + live
    }
    fn draw(&self, cv: &mut Canvas, sound: &Sound, gaze: (f32, f32)) {
        let Some(front) = self.hist.front() else { return };
        let n = front.len().max(1);
        let width = n as f32 * LANE_W;
        // The whole canyon is the stage: the cursor swings the camera nearly
        // all the way round and from low over the ridges to high above, so
        // the newest relief can be watched from afar and from behind.
        let length = self.hist.len() as f32 * ROW_D;
        let target = [width * 0.5, 1.2, length * 0.42];
        let az = gaze.0 * 2.9 + (self.time * 0.04).sin() * 0.35;
        let el = (0.35 - gaze.1 * 0.4).clamp(0.14, 1.2);
        let dist = width.max(length) * 0.62 + 4.;
        cv.cam = Cam::look(orbit(target, az, el, dist), target, 1.1, cv.cols, cv.rows, cv.cam.aspect);
        let cam = cv.cam;
        let sub_n = 4usize;
        let nx = n * sub_n + 1;
        let scroll = self.clock / TICK;
        let water = self.load.mem * PEAK;
        let light = norm([-0.45, 0.85, -0.4]);
        let rows = self.hist.len();
        let mut heights = vec![0f32; nx * rows];
        for k in 0..rows {
            for i in 0..nx {
                heights[k * nx + i] = self.height(k, i as f32 / sub_n as f32, sound);
            }
        }
        let hx = LANE_W / sub_n as f32;
        let ceiling = self.ceiling;
        let mut paint = |a: [f32; 3], depth: f32| {
            let h = a[1];
            let v = (h / PEAK).clamp(0., 1.);
            let base = grad(&[(0., (45, 30, 100)), (0.25, (40, 160, 190)), (0.55, (225, 60, 170)), (0.8, (255, 150, 50)), (1., (255, 245, 210))], v);
            let mut color = scale(base, 0.3 + a[0] * 0.85 + a[2] * 0.5);
            // The shore: where memory's level meets the terrain.
            let shore = 1. - (h - water).abs() / 0.12;
            if shore > 0. {
                return Some(('~', mix(color, (140, 220, 255), 0.5 + shore * 0.5)));
            }
            if h < water {
                color = mix(color, (30, 80, 170), ((water - h) / 2.).min(0.4));
            }
            let f = (-depth * 0.02).exp();
            Some((block((a[0] * 0.8 + a[2] * 0.3) * (0.4 + 0.6 * f)), scale(color, 0.35 + 0.65 * f)))
        };
        let mut prev: Vec<Option<Vx>> = vec![None; nx];
        let mut cur: Vec<Option<Vx>> = vec![None; nx];
        for k in 0..rows {
            let z = (k as f32 + scroll) * ROW_D;
            let stamp = (self.pushed as i64 - k as i64).rem_euclid(16) == 0;
            for i in 0..nx {
                let h = heights[k * nx + i];
                let left = heights[k * nx + i.saturating_sub(1)];
                let right = heights[k * nx + (i + 1).min(nx - 1)];
                let near = heights[k.saturating_sub(1) * nx + i];
                let far = heights[(k + 1).min(rows - 1) * nx + i];
                let nrm = norm([left - right, 2. * hx, near - far]);
                let l = 0.22 + dot(nrm, light).max(0.) * 0.85;
                cur[i] = cv.vx([i as f32 * hx, h, z], [l, h, if stamp { 0.6 } else { 0. }]);
            }
            if k > 0 {
                for i in 0..nx - 1 {
                    if let (Some(a), Some(b), Some(c), Some(d)) = (prev[i], prev[i + 1], cur[i], cur[i + 1]) {
                        cv.tri(a, b, c, &mut paint);
                        cv.tri(b, d, c, &mut paint);
                    }
                }
            }
            std::mem::swap(&mut prev, &mut cur);
            if stamp && k > 0 {
                let secs = k as f32 * TICK;
                cv.label([-0.6, 0., z], 0., &format!("-{secs:.0}s"), (110, 120, 150));
            }
        }
        // Memory floods the canyon; bass ripples the surface.
        let mut wz = 0.;
        while wz < rows as f32 * ROW_D {
            let mut wx = 0.;
            while wx <= width {
                let wave = (wx * 1.3 + self.time * 2.) .sin() * (wz * 0.9 - self.time * 1.4).cos();
                let y = water + wave * (0.05 + sound.bass * 0.25);
                let tw = 0.5 + 0.5 * (wx * 7.1 + wz * 3.3 + self.time * 3.).sin();
                let l = 0.35 + tw * sound.treble * 0.7;
                if wave > 0.2 {
                    cv.point([wx, y, wz], '~', scale((80, 150, 240), l));
                }
                wx += 0.9;
            }
            wz += 1.7;
        }
        for m in &self.sparks {
            let l = m.life * (0.8 + sound.treble * 0.5);
            cv.point(m.p, speck(l), scale((255, 220, 150), 0.4 + l * 0.7));
        }
        for (i, p) in self.drift.iter().enumerate() {
            let tw = 0.5 + 0.5 * (self.time * 2.5 + i as f32 * 1.7).sin();
            let l = 0.3 + tw * (0.3 + sound.treble * 0.5);
            let color = grad(&[(0., (90, 140, 220)), (0.5, (220, 110, 200)), (1., (255, 200, 120))], p[1] / (PEAK + 3.));
            cv.point(*p, if tw > 0.9 { '+' } else { speck(l) }, scale(color, 0.3 + l * 0.6));
        }
        let groups = SysLoad::groups(&self.load.cores, LANES);
        let spacing = cam.project([LANE_W, 0., 0.]).zip(cam.project([0., 0., 0.])).map_or(0., |(a, b)| (a.0 - b.0).abs());
        for (i, (v, name)) in groups.iter().enumerate() {
            let x = (i as f32 + 0.5) * LANE_W;
            let h = self.height(0, i as f32 + 0.5, sound);
            if spacing >= 5. {
                cv.label([x, h, 0.], 1.5, &format!("{:.0}%", v * 100.), (235, 235, 245));
            }
            if spacing >= 7. {
                cv.label([x, 0., -0.8], 0., name, (130, 140, 170));
            }
        }
        let hud = self.load.hud();
        cv.text(2, cv.rows as i32 - 2, 0., &hud, (150, 170, 200));
        cv.text(2, 1, 0., &format!("scale {:.0}%   water = RAM {:.0}%", ceiling * 100., self.load.mem * 100.), (110, 120, 150));
    }
}

// ============================================================== dispatch
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Which {
    Horizon,
    Supernova,
    Solar,
    Abyss,
    Canyon,
    City,
    Grid,
    Silicon,
}
pub(crate) enum Deep {
    Horizon(Horizon),
    Supernova(Supernova),
    Solar(Solar),
    Abyss(Abyss),
    Canyon(Canyon),
    City(cyber::NeonCity),
    Grid(cyber::PacketGrid),
    Silicon(cyber::Silicon),
}
impl Deep {
    pub(crate) fn new(which: Which) -> Self {
        match which {
            Which::Horizon => Self::Horizon(Horizon::new()),
            Which::Supernova => Self::Supernova(Supernova::new()),
            Which::Solar => Self::Solar(Solar::new()),
            Which::Abyss => Self::Abyss(Abyss::new()),
            Which::Canyon => Self::Canyon(Canyon::new()),
            Which::City => Self::City(cyber::NeonCity::new()),
            Which::Grid => Self::Grid(cyber::PacketGrid::new()),
            Which::Silicon => Self::Silicon(cyber::Silicon::new()),
        }
    }
    /// `cursor` is the smoothed pointer in -1..1 when there is one.
    pub(crate) fn step(&mut self, dt: f32, sound: &Sound, cursor: Option<(f32, f32)>, gaze: (f32, f32)) {
        match self {
            Self::Horizon(s) => s.step(dt, sound, cursor),
            Self::Supernova(s) => s.step(dt, sound),
            Self::Solar(s) => s.step(dt, sound),
            Self::Abyss(s) => s.step(dt, sound, cursor, gaze),
            Self::Canyon(s) => s.step(dt, sound),
            Self::City(s) => s.tick(dt, sound, cursor, gaze),
            Self::Grid(s) => s.tick(dt, sound, cursor, gaze),
            Self::Silicon(s) => s.tick(dt, sound, cursor, gaze),
        }
    }
    pub(crate) fn draw(&self, r: &mut Raster, cols: usize, rows: usize, aspect: f32, sound: &Sound, gaze: (f32, f32)) {
        if cols < 2 || rows < 2 {
            return;
        }
        let cam = Cam::look([0., 0., -10.], [0., 0., 0.], 1., cols, rows, aspect);
        let mut cv = Canvas { r, cols, rows, cam };
        match self {
            Self::Horizon(s) => s.draw(&mut cv, sound, gaze),
            Self::Supernova(s) => s.draw(&mut cv, sound, gaze),
            Self::Solar(s) => s.draw(&mut cv, sound, gaze),
            Self::Abyss(s) => s.draw(&mut cv, sound, gaze),
            Self::Canyon(s) => s.draw(&mut cv, sound, gaze),
            Self::City(s) => s.paint(&mut cv, sound),
            Self::Grid(s) => s.paint(&mut cv, sound),
            Self::Silicon(s) => s.paint(&mut cv, sound),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn frame(scene: &Deep, cols: usize, rows: usize) -> Vec<crate::asm_direct_bridge::DirectCell> {
        frame_at(scene, cols, rows, (0.2, -0.1))
    }
    fn frame_at(scene: &Deep, cols: usize, rows: usize, gaze: (f32, f32)) -> Vec<crate::asm_direct_bridge::DirectCell> {
        let mut r = Raster::new(cols, rows);
        scene.draw(&mut r, cols, rows, 2.0, &Sound::default(), gaze);
        let mut out = vec![];
        r.recycle_visible(&mut out);
        out
    }

    #[test]
    fn triangles_tile_without_holes_or_overlap_gaps() {
        // Two triangles of a quad cover every cell centre inside it once.
        let mut r = Raster::new(40, 20);
        let cam = Cam::look([0., 0., -5.], [0., 0., 0.], 1., 40, 20, 2.);
        let mut cv = Canvas { r: &mut r, cols: 40, rows: 20, cam };
        let q = [[-1., -1., 0.], [1., -1., 0.], [-1., 1., 0.], [1., 1., 0.]];
        let v: Vec<Vx> = q.iter().map(|&p| cv.vx(p, [1., 0., 0.]).unwrap()).collect();
        let mut n = 0;
        let mut count = |_: [f32; 3], _: f32| {
            n += 1;
            Some(('#', (255, 255, 255)))
        };
        cv.tri(v[0], v[1], v[2], &mut count);
        cv.tri(v[1], v[3], v[2], &mut count);
        let mut out = vec![];
        r.recycle(&mut out);
        let (xs, ys): (Vec<_>, Vec<_>) = out.iter().map(|c| (c.x, c.y)).unzip();
        let (w, h) = (xs.iter().max().unwrap() - xs.iter().min().unwrap() + 1, ys.iter().max().unwrap() - ys.iter().min().unwrap() + 1);
        assert_eq!(out.len(), w * h, "a filled rectangle");
        assert!(n >= out.len());
    }

    #[test]
    fn spheres_are_round_and_hide_what_is_behind() {
        let mut r = Raster::new(80, 40);
        let cam = Cam::look([0., 0., -6.], [0., 0., 0.], 1., 80, 40, 2.);
        let mut cv = Canvas { r: &mut r, cols: 80, rows: 40, cam };
        cv.point([0., 0., 3.], 'x', (255, 0, 0));
        cv.sphere([0., 0., 0.], 1., &mut |_, _, _| Some(('o', (0, 255, 0))));
        let mut out = vec![];
        r.recycle(&mut out);
        assert!(out.iter().all(|c| c.ch != 'x'), "the point behind is hidden");
        let width = out.iter().filter(|c| c.y == 20).count() as f32;
        let height = out.iter().filter(|c| c.x == 40).count() as f32;
        assert!((width / height - 2.).abs() < 0.35, "round on screen: {width} x {height}");
    }

    #[test]
    fn the_sand_repeats_exactly_over_the_period() {
        for i in 0..200 {
            let (x, z) = (i as f32 * 0.37 - 30., i as f32 * 1.91);
            assert!((floor_h(x, z) - floor_h(x, z + PERIOD)).abs() < 2e-3, "{x} {z}");
        }
    }

    #[test]
    fn machine_load_is_read_and_grouped() {
        let mut l = SysLoad::new();
        for _ in 0..8 {
            l.step(0.25);
        }
        assert!(!l.cores.is_empty());
        assert!(l.cores.iter().all(|v| (0. ..=1.).contains(v)));
        assert!((0. ..=1.).contains(&l.mem));
        let g = SysLoad::groups(&[0.; 40], 12);
        assert!(g.len() <= 12);
        assert_eq!(g[0].1, "cpu0-3");
    }

    #[test]
    fn supernova_runs_its_whole_cycle() {
        let mut s = Supernova::new();
        let mut seen = vec![s.phase];
        let quiet = Sound::default();
        for _ in 0..(80. / 0.05) as usize {
            s.step(0.05, &quiet);
            if seen.last() != Some(&s.phase) {
                seen.push(s.phase);
            }
        }
        assert_eq!(&seen[..4], &[Phase::Star, Phase::Collapse, Phase::Blast, Phase::Rebirth]);
        let deep = Deep::Supernova(s);
        assert!(frame(&deep, 120, 40).len() > 50);
    }

    #[test]
    fn a_star_torn_by_the_hole_feeds_the_disk() {
        let mut h = Horizon::new();
        h.meal = 0.;
        let quiet = Sound::default();
        h.step(0.05, &quiet, None);
        assert!(!h.stream.is_empty());
        h.meal = f32::MAX;
        let mut lit = false;
        for _ in 0..1400 {
            h.step(0.05, &quiet, None);
            lit |= !h.spots.is_empty();
        }
        assert!(h.stream.is_empty(), "the stream fell in, left or faded");
        assert!(lit, "what fell in lit the disk");
    }

    #[test]
    fn the_cursor_feeds_the_hole_rocks_and_dust() {
        let mut deep = Deep::Horizon(Horizon::new());
        let quiet = Sound::default();
        let gaze = (0.6, -0.3);
        frame_at(&deep, 160, 50, gaze);
        let Deep::Horizon(h) = &deep else { unreachable!() };
        let aim = h.aim.get().expect("the draw found the point under the cursor");
        let mut seen = 0;
        let mut eaten: f32 = 0.;
        for i in 0..800 {
            deep.step(0.05, &quiet, Some(gaze), gaze);
            let Deep::Horizon(h) = &deep else { unreachable!() };
            if i == 20 {
                seen = h.debris.len();
                // Born where the cursor is, not somewhere else.
                let near = h.debris.iter().filter(|m| len(sub(m.p, aim)) < 2.5).count();
                assert!(near * 2 > seen, "{near} of {seen} near the cursor");
                assert!(h.debris.iter().any(|m| m.size > 0.), "some are rocks");
            }
            eaten = eaten.max(h.gulp);
        }
        assert!(seen > 10, "the cursor threw debris in: {seen}");
        assert!(eaten > 0.02, "the hole swallowed some of it");
        let mut none = Horizon::new();
        for _ in 0..40 {
            none.step(0.05, &quiet, None);
        }
        assert!(none.debris.is_empty(), "no cursor, no music: nothing thrown in");
    }

    #[test]
    fn the_canyon_turns_round_and_fills_with_particles() {
        let mut c = Canyon::new();
        let quiet = Sound::default();
        for _ in 0..80 {
            c.step(0.05, &quiet);
        }
        assert!(!c.sparks.is_empty(), "the newest ridge throws sparks");
        let length = HISTORY as f32 * ROW_D;
        assert!(c.drift.iter().all(|p| (0. ..=length).contains(&p[2]) && (0. ..=PEAK + 3.).contains(&p[1])));
        let deep = Deep::Canyon(c);
        // From either side and from behind, the canyon is still in view.
        for gaze in [(0., 0.), (0.95, 0.), (-0.95, 0.), (0., -0.9), (0., 0.9)] {
            let n = frame_at(&deep, 160, 50, gaze).len();
            assert!(n > 1000, "{gaze:?}: {n} cells");
        }
    }

    #[test]
    fn the_abyss_has_its_shoals() {
        let mut a = Abyss::new();
        let quiet = Sound::default();
        for _ in 0..100 {
            a.step(0.05, &quiet, None, (0., 0.));
        }
        for kind in 0..SHOALS.len() {
            assert_eq!(a.school.iter().filter(|f| f.kind == kind).count(), SHOALS[kind].0);
        }
        assert!(a.plankton.len() >= 5000);
        let deep = Deep::Abyss(a);
        let cells = frame(&deep, 200, 60);
        let warm = cells.iter().filter(|c| matches!(c.ch, '>' | '<') && c.color.0 as i32 > c.color.2 as i32 + 30).count();
        assert!(warm > 0, "a golden fish shows");
    }

    #[test]
    fn the_abyss_lamp_finds_the_floor() {
        let mut a = Abyss::new();
        let quiet = Sound::default();
        for _ in 0..10 {
            a.step(0.05, &quiet, Some((0., 0.6)), (0., 0.6));
        }
        let deep = Deep::Abyss(a);
        frame(&deep, 160, 50);
        let Deep::Abyss(a) = &deep else { unreachable!() };
        assert!(a.lamp.get().is_some());
    }

    /// Renders scenes to PNG stills for review and for the selector posters:
    /// SCENES="event-horizon abyss" SNAPS="6,20" POSTER_DIR=/tmp/out \
    ///   cargo test --release --no-default-features --bin ttfx-bg-rs -- render_stills --ignored --nocapture
    #[test]
    #[ignore = "manual: writes PNG files"]
    fn render_stills() {
        use crate::neon_scenes::{Scene, Style};
        use gtk4::cairo::{Context, Format, ImageSurface};
        let names = std::env::var("SCENES").unwrap_or_else(|_| "event-horizon supernova solar-system abyss load-canyon neon-avenue packet-grid silicon".into());
        let snaps: Vec<f32> = std::env::var("SNAPS").unwrap_or_else(|_| "8".into()).split(',').filter_map(|v| v.trim().parse().ok()).collect();
        let dir = std::path::PathBuf::from(std::env::var("POSTER_DIR").unwrap_or_else(|_| "stills".into()));
        std::fs::create_dir_all(&dir).unwrap();
        let (cols, rows): (usize, usize) = std::env::var("GRID").ok().and_then(|g| {
            let (c, r) = g.split_once('x')?;
            Some((c.parse().ok()?, r.parse().ok()?))
        }).unwrap_or((240, 68));
        let (cw, ch) = (8., 17.);
        for name in names.split_whitespace() {
            let mut scene = Scene::new(Style::parse(name).unwrap());
            let mut cells = vec![];
            scene.draw(cols, rows, ch as f32 / cw as f32, &mut cells);
            let dt = 1. / 30.;
            let mut t = 0f32;
            for &snap in &snaps {
                while t < snap {
                    let beat = (t * TAU * 2.).sin().max(0.).powi(6);
                    // QUIET=1 renders silence.
                    let quiet = std::env::var("QUIET").is_ok_and(|v| v == "1");
                    let sound = if quiet { Sound::default() } else { Sound { bass: 0.25 + beat * 0.6, treble: 0.3 + 0.3 * (t * 7.).sin().abs() } };
                    // POINTER="x,y" (0..1) holds the cursor still.
                    let held = std::env::var("POINTER").ok().and_then(|p| {
                        let (x, y) = p.split_once(',')?;
                        Some((x.parse().ok()?, y.parse().ok()?))
                    });
                    let pointer = held.unwrap_or((0.5 + (t * 0.21).sin() * 0.25, 0.55 + (t * 0.17).cos() * 0.15));
                    scene.step(dt, Some(pointer), sound);
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
                // Binary PPM: cairo here is built without PNG support.
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
                let path = dir.join(format!("{name}-{snap:04.1}s.ppm"));
                std::fs::write(&path, ppm).unwrap();
                println!("STILL {} cells {}", path.display(), cells.len());
            }
        }
    }
}
