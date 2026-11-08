//! Four continuous journeys sharing the existing cell renderer, not image loops.
use crate::{
    asm_direct_bridge::DirectCell,
    frame_wakeup::FrameWakeup,
    interactive_matrix::{PointerState, Sound},
    interactive_tunnel::Raster,
};
use std::{
    sync::{
        atomic::{AtomicBool, AtomicU64, Ordering},
        Arc, Mutex,
    },
    time::{Duration, Instant},
};
type Color = (u8, u8, u8);
type P = (f32, f32, f32);
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Style {
    City,
    Vector,
    Circuit,
    Sunset,
    Showcase(crate::showcase_scenes::Kind),
}
impl Style {
    pub(crate) fn parse(name: &str) -> Option<Self> {
        Some(match name {
            "neon-city" => Self::City,
            "vector-highway" => Self::Vector,
            "circuit-run" => Self::Circuit,
            "sunset-drive" => Self::Sunset,
            other => Self::Showcase(crate::showcase_scenes::Kind::parse(other)?),
        })
    }
}
struct Accent {
    z: f32,
    side: f32,
    treble: f32,
    offset: f32,
    height: f32,
}
pub(crate) struct Scene {
    style: Style,
    travel: f32,
    time: f32,
    gaze: (f32, f32),
    sound: Sound,
    accents: Vec<Accent>,
    spawn: f32,
    serial: u32,
    showcase: Option<crate::showcase_scenes::Showcase>,
    raster: std::cell::RefCell<Option<Raster>>,
    /// World heights do not move until the next three-unit terrain strip.
    terrain: std::cell::RefCell<Option<(i32, [[f32; 21]; 30])>>,
    /// The last depth lit and its light; a new frame starts it over.
    glow_at: std::cell::Cell<(f32, f32, f32)>,
    /// Whether a pointer is over the scene; it carries a lamp.
    cursor: bool,
    /// Ages of the rings a beat sends out from the cursor.
    ripples: Vec<f32>,
    bass_avg: f32,
    beat_rest: f32,
    #[cfg(test)]
    exhaustive: bool,
}
impl Scene {
    pub(crate) fn new(style: Style) -> Self {
        Self {
            style,
            travel: 0.,
            time: 0.,
            gaze: (0., 0.),
            sound: Sound::default(),
            accents: vec![],
            spawn: 0.,
            serial: rand::random(),
            raster: Default::default(),
            terrain: Default::default(),
            glow_at: std::cell::Cell::new((f32::NAN, 0., 0.)),
            cursor: false,
            ripples: vec![],
            bass_avg: 0.,
            beat_rest: 0.,
            #[cfg(test)]
            exhaustive: false,
            showcase: match style {
                Style::Showcase(kind) => Some(crate::showcase_scenes::Showcase::new(kind)),
                _ => None,
            },
        }
    }
    pub(crate) fn step(&mut self, dt: f32, pointer: Option<(f32, f32)>, sound: Sound) {
        self.glow_at.set((f32::NAN, 0., 0.));
        if let Some(show) = &mut self.showcase {
            show.step(dt, pointer, sound);
            return;
        }
        let dt = dt.clamp(0., 0.1);
        // Soft gain retains quiet music and dynamics instead of clipping loud tracks.
        let drive = |v: f32| 1. - (-3. * v.clamp(0., 1.)).exp();
        self.sound.follow(
            Sound {
                bass: drive(sound.bass),
                treble: drive(sound.treble),
            },
            dt,
        );
        // A beat is bass well over its recent level; each sends out a ring.
        let bass = drive(sound.bass);
        self.beat_rest -= dt;
        if bass > self.bass_avg * 1.3 + 0.1 && self.beat_rest <= 0. {
            self.ripples.push(0.);
            self.beat_rest = 0.28;
        }
        self.bass_avg += (bass - self.bass_avg) * (1. - (-dt * 1.5).exp());
        for r in &mut self.ripples {
            *r += dt;
        }
        self.ripples.retain(|r| *r < 1.);
        if self.ripples.len() > 6 {
            self.ripples.remove(0);
        }
        self.cursor = pointer.is_some();
        self.time = (self.time + dt) % 760.;
        self.travel = (self.travel + dt * 6.) % 6144.;
        for a in &mut self.accents {
            a.z -= dt * 6.;
        }
        self.accents.retain(|a| a.z > 0.9);
        // Music populates the world. Born in the distance, each object lives
        // through a complete approach even when the note has already ended.
        self.spawn += dt * (self.sound.bass * 3. + self.sound.treble * 2.);
        while self.spawn >= 1. {
            self.spawn -= 1.;
            if self.accents.len() < 32 {
                let h = variant(self.serial);
                self.accents.push(Accent {
                    // Pre-generated beyond the fog boundary; never born visible.
                    z: 80. + h * 10.,
                    side: if variant(self.serial.wrapping_add(17)) < 0.5 {
                        -1.
                    } else {
                        1.
                    },
                    treble: self.sound.treble,
                    offset: h,
                    height: variant(self.serial.wrapping_add(91)),
                });
            }
            self.serial = self.serial.wrapping_add(1);
        }
        let target = pointer
            .map(|(x, y)| ((x - 0.5) * 2., (y - 0.5) * 2.))
            .unwrap_or((0., 0.));
        let k = 1. - (-dt * 4.).exp();
        self.gaze.0 += (target.0.clamp(-1., 1.) - self.gaze.0) * k;
        self.gaze.1 += (target.1.clamp(-1., 1.) - self.gaze.1) * k;
    }
    pub(crate) fn draw(&self, cols: usize, rows: usize, aspect: f32, out: &mut Vec<DirectCell>) {
        self.draw_into::<false>(cols, rows, aspect, out);
    }
    fn draw_with_print(&self, cols: usize, rows: usize, aspect: f32, out: &mut Vec<DirectCell>) -> u64 {
        self.draw_into::<true>(cols, rows, aspect, out)
    }
    fn draw_into<const PRINT: bool>(&self, cols: usize, rows: usize, aspect: f32, out: &mut Vec<DirectCell>) -> u64 {
        out.clear();
        if let Some(show) = &self.showcase {
            show.draw(cols, rows, aspect, out);
            return if PRINT { frame_print(out) } else { 0 };
        }
        if cols == 0 || rows == 0 {
            return if PRINT { 1 } else { 0 };
        }
        let mut slot = self.raster.borrow_mut();
        let r = match slot.take() {
            Some(r) if r.fits(cols, rows) => r,
            _ => Raster::new(cols, rows),
        };
        let mut c = Canvas {
            r,
            cols,
            rows,
            aspect: aspect.clamp(1., 4.),
            gaze: self.gaze,
            style: self.style,
            solid_body: false,
            aim: None,
            focus: None,
        };
        c.aim = self.aim(&c);
        match self.style {
            Style::City => self.city(&mut c),
            Style::Vector => self.vector(&mut c),
            Style::Circuit => self.circuit(&mut c),
            Style::Sunset => self.sunset(&mut c),
            Style::Showcase(_) => unreachable!("delegated above"),
        }
        self.draw_accents(&mut c);
        self.draw_focus(&mut c);
        self.cursor_light(&mut c);
        let print = if PRINT { c.r.recycle_with_print(out) } else { c.r.recycle(out); 0 };
        *slot = Some(c.r);
        print
    }
    fn draw_accents(&self, c: &mut Canvas) {
        for a in &self.accents {
            let z = a.z;
            let visibility = arrival_visibility(z, 64., 44.);
            if visibility == 0. {
                continue;
            }
            let color = match self.style {
                Style::City => (255, 160, 65),
                Style::Vector => (100, 255, 180),
                Style::Circuit => (80, 210, 255),
                Style::Sunset => (255, 110, 175),
                Style::Showcase(_) => return,
            };
            let color = scale(color, (0.65 + a.treble * 0.35) * visibility);
            match self.style {
                Style::City => {
                    // Music lights whole towers from inside (see `city`), and
                    // sends a searchlight beam up from the lit roof.
                    let x = a.side * 6.2;
                    let sweep = (self.time * 1.7 + a.offset * 6.).sin() * 1.6;
                    c.line(
                        (x, -1.5, z + 2.),
                        (x + sweep, -9., z + 2.),
                        scale(color, 0.45),
                        Some('·'),
                    );
                }
                Style::Vector => {
                    // Only the posts: the box between them holds the road and
                    // everything seen through the gate, and would always win.
                    let w = 3.4 + a.offset;
                    for side in [-1., 1.] {
                        let x = side * w;
                        let top = -3.4 - a.height;
                        c.offer(&[(x - 0.45, top, z), (x + 0.45, 2.2, z), (x - 0.45, top, z + 0.7), (x + 0.45, 2.2, z + 0.7)], || "GATE".into());
                    }
                    // Thick portal frames, no vehicles on this road.
                    for side in [-1., 1.] {
                        c.box3(
                            (side * (3.4 + a.offset) - 0.13, -3.2 - a.height, z),
                            (side * (3.4 + a.offset) + 0.13, 2.2, z + 0.7),
                            color,
                        );
                    }
                    c.box3(
                        (-3.4 - a.offset, -3.4 - a.height, z),
                        (3.4 + a.offset, -3.15 - a.height, z + 0.7),
                        color,
                    );
                }
                Style::Circuit => {
                    // Roadside crystal markers: front/back diamonds and ribs.
                    let x = a.side * (0.8 + a.offset * 7.8);
                    let ground = lunar_ground(x, self.travel + z) - 2.2;
                    c.offer(&[(x - 0.65, -0.6 + ground, z), (x + 0.65, 2.2 + ground, z + 1.2)], || "CRYSTAL".into());
                    for dz in [0., 1.2] {
                        let pts = [
                            (x, -0.6 + ground, z + dz),
                            (x + 0.65, 0.8 + ground, z + dz),
                            (x, 2.2 + ground, z + dz),
                            (x - 0.65, 0.8 + ground, z + dz),
                        ];
                        c.face(pts, scale(color, if dz == 0. { 0.32 } else { 0.12 }));
                        for i in 0..4 {
                            c.line(pts[i], pts[(i + 1) % 4], color, None);
                        }
                    }
                    for (dx, y) in [(0., -0.6), (0.65, 0.8), (0., 2.2), (-0.65, 0.8)] {
                        c.line(
                            (x + dx, y + ground, z),
                            (x + dx, y + ground, z + 1.2),
                            scale(color, 0.6),
                            None,
                        );
                    }
                }
                Style::Sunset if a.height > 0.62 => {
                    // Neon saguaro: a tall cactus wrapped in pink light.
                    let x = a.side * (5. + a.offset * 3.);
                    c.offer(&[(x - 0.6, 2.2, z), (x + 0.6, -2.4, z)], || "SAGUARO".into());
                    for dx in [-0.6, 0., 0.6] {
                        let h = if dx == 0. { 4.4 } else { 2.6 };
                        c.line(
                            (x + dx, 2.2 - h * 0.35, z),
                            (x + dx, 2.2 - h, z),
                            color,
                            Some('▌'),
                        );
                        c.line(
                            (x, 2.2 - h * 0.35 - 0.3, z),
                            (x + dx, 2.2 - h * 0.35, z),
                            color,
                            None,
                        );
                    }
                    c.line((x, 2.2, z), (x, -2.2, z), color, Some('▌'));
                    c.line(
                        (x - 0.2, -2.4, z - 0.03),
                        (x + 0.2, -2.4, z - 0.03),
                        scale((255, 230, 120), visibility),
                        Some('*'),
                    );
                }
                Style::Sunset if a.height > 0.34 => {
                    // Lit shrubs scattered on the sand.
                    let x = a.side * (5.5 + a.offset * 4.);
                    c.offer(&[(x - 0.7, 2.2, z), (x + a.side * 2.5, 1.4, z + 1.2)], || "SHRUBS".into());
                    for k in 0..3 {
                        let x = a.side * (5.5 + a.offset * 4. + k as f32 * 0.9);
                        let r = 0.45 + (k % 2) as f32 * 0.2;
                        for (sx, sy) in [(-r, 0.), (0., -r), (r, 0.), (0., 0.)] {
                            c.line(
                                (x + sx, 2.2 + sy * 0.3, z + k as f32 * 0.6),
                                (x, 2.2 - r, z + k as f32 * 0.6),
                                scale(color, 0.8),
                                None,
                            );
                        }
                        c.line(
                            (x - 0.1, 2.2 - r, z + k as f32 * 0.6 - 0.02),
                            (x + 0.1, 2.2 - r, z + k as f32 * 0.6 - 0.02),
                            scale((255, 220, 140), visibility),
                            Some('*'),
                        );
                    }
                }
                Style::Sunset => {
                    // Music adds neon roadside signs, with a thick housing.
                    let x = a.side * (4.1 + a.offset * 3.);
                    c.offer(&[(x - 0.7, 2.2, z), (x + 0.7, -1.8, z)], || "NEON SIGN".into());
                    c.line((x, 2.2, z), (x, -1.5, z), scale(color, 0.55), Some('▌'));
                    c.box3((x - 0.7, -1.8, z), (x + 0.7, -0.5, z + 0.5), color);
                    for y in [-1.5, -1.1, -0.7] {
                        c.line(
                            (x - 0.45, y, z - 0.04),
                            (x + 0.45, y, z - 0.04),
                            scale((255, 220, 125), visibility),
                            Some('▬'),
                        );
                    }
                }
                Style::Showcase(_) => {}
            }
        }
    }
    /// The screen cell under the cursor, when there is one.
    fn aim(&self, c: &Canvas) -> Option<(f32, f32)> {
        self.cursor.then(|| ((self.gaze.0 + 1.) * 0.5 * c.cols as f32, (self.gaze.1 + 1.) * 0.5 * c.rows as f32))
    }
    /// The element under the cursor comes into focus: it brightens, corner
    /// marks close in round it, breathing with the music, and its name
    /// shows over it in the pixel font, as large as there is room for.
    fn draw_focus(&self, c: &mut Canvas) {
        let Some(f) = c.focus.take() else { return };
        let (x0, y0) = (f.poly.iter().map(|p| p.0).fold(f32::MAX, f32::min), f.poly.iter().map(|p| p.1).fold(f32::MAX, f32::min));
        let (x1, y1) = (f.poly.iter().map(|p| p.0).fold(f32::MIN, f32::max), f.poly.iter().map(|p| p.1).fold(f32::MIN, f32::max));
        let (x0, y0) = (x0.max(0.), y0.max(0.));
        let (x1, y1) = (x1.min(c.cols as f32 - 1.), y1.min(c.rows as f32 - 1.));
        let rect = f.poly[0].0 == f.poly[3].0 && f.poly[0].1 == f.poly[1].1;
        let glow = 0.55 + self.sound.bass * 0.35;
        let poly = f.poly;
        c.r.lift((x0 as usize, x1 as usize + 1), (y0 as usize, y1 as usize + 1), |x, y| {
            if rect || inside(&poly, (x as f32 + 0.5, y as f32 + 0.5)) { glow } else { 0. }
        });
        let accent = match self.style {
            Style::City => (120, 235, 255),
            Style::Vector => (170, 255, 190),
            Style::Circuit => (150, 230, 255),
            _ => (255, 170, 225),
        };
        if !rect {
            // A patch of ground: its edges in light.
            for i in 0..4 {
                let (a, b) = (poly[i], poly[(i + 1) % 4]);
                c.r.line_with_glyph(a, b, 0.05, accent, false, Some('·'));
            }
        }
        let breathe = 1. + (self.time * 4.).sin().abs() * 0.6 + self.sound.bass;
        let (bx, by) = (breathe * c.aspect, breathe * 0.5);
        for (x, y, ch) in [(x0 - bx, y0 - by, '┌'), (x1 + bx, y0 - by, '┐'), (x0 - bx, y1 + by, '└'), (x1 + bx, y1 + by, '┘')] {
            c.r.dot(x.round() as i32, y.round() as i32, 0.03, ch, accent);
        }
        let room = ((x1 - x0) * 1.5).max(c.cols as f32 * 0.12);
        let most = (c.rows / 40).max(1);
        let k = (1..=most).rev().find(|&k| crate::scenes3d::cyber::type_size(&f.name, k).0 as f32 <= room).unwrap_or(1);
        let (w, h) = crate::scenes3d::cyber::type_size(&f.name, k);
        let at = ((x0 + x1) * 0.5 - w as f32 * 0.5).clamp(1., (c.cols as f32 - w as f32 - 1.).max(1.)) as i32;
        // Over it, or under it when it touches the top.
        let over = y0 - by - h as f32 - 1.;
        let y = if over >= 0. { over } else { (y1 + by + 1.).min(c.rows as f32 - h as f32) };
        crate::scenes3d::cyber::pixel_text(&mut c.r, (c.cols, c.rows), (at, y.round() as i32, 0.04), &f.name, k, scale(accent, 0.9 + self.sound.treble * 0.2));
    }
    /// Each beat sends a ring of light out from the cursor (from the
    /// vanishing point when there is none), brightening what it crosses.
    fn cursor_light(&self, c: &mut Canvas) {
        let (cols, rows, aspect) = (c.cols as f32, c.rows as f32, c.aspect);
        let aim = self.aim(c);
        let at = aim.unwrap_or((cols * 0.5, c.horizon_y()));
        let rings: Vec<(f32, f32)> = self.ripples.iter().map(|&t| (t * rows * 0.95, (1. - t) * 0.7)).collect();
        let w = (rows * 0.035).max(1.2);
        let reach = rings.iter().map(|r| r.0 + w).fold(0., f32::max);
        let span = |centre: f32, half: f32| ((centre - half).max(0.) as usize, (centre + half + 1.).max(0.) as usize);
        if !rings.is_empty() {
            c.r.lift(span(at.0, reach * aspect), span(at.1, reach), |x, y| {
            let (dx, dy) = ((x as f32 - at.0) / aspect, y as f32 - at.1);
            let d = (dx * dx + dy * dy).sqrt();
            let mut k = 0.;
            for &(r, s) in &rings {
                let e = 1. - (d - r).abs() / w;
                if e > 0. {
                    k += e * s;
                }
            }
            k
            });
        }
        if let Some((x, y)) = aim {
            let color = match self.style {
                Style::City => (120, 235, 255),
                Style::Vector => (150, 255, 170),
                Style::Circuit => (120, 220, 255),
                _ => (255, 140, 210),
            };
            let r = (rows * 0.04).max(1.);
            let (dx, dy) = (r * aspect, r);
            for (sx, sy, ch) in [(-1., -1., '┌'), (1., -1., '┐'), (-1., 1., '└'), (1., 1., '┘')] {
                c.r.dot((x + sx * dx).round() as i32, (y + sy * dy).round() as i32, 0.02, ch, color);
            }
        }
    }
    fn glow(&self, color: Color, z: f32) -> Color {
        // Whole floors of windows share a depth, and this runs for each one.
        let (at, mut distance, mut energy) = self.glow_at.get();
        if at != z {
            distance = 0.36 + 0.64 / (1. + z * 0.035);
            // Moving pools of light reveal the beat along depth, with white headroom.
            let wave = (0.5 + 0.5 * (z * 0.42 - self.travel * 0.35).cos()).powi(3);
            energy = (self.sound.treble * 0.65 + self.sound.bass * wave * 0.7).clamp(0., 1.);
            self.glow_at.set((z, distance, energy));
        }
        let base = scale(color, distance * (0.55 + energy * 0.65));
        let lift = |v: u8| (v as f32 + (255. - v as f32) * energy * 0.55) as u8;
        (lift(base.0), lift(base.1), lift(base.2))
    }
    fn road(&self, c: &mut Canvas, color: Color, grid: bool) {
        for lane in [-3., -1., 1., 3.] {
            c.line(
                (lane, 2.2, 0.8),
                (lane, 2.2, 80.),
                self.glow(color, 4.),
                Some('─'),
            );
        }
        for n in 0..35 {
            let z = 0.8 + (n as f32 * 2.4 - self.travel).rem_euclid(84.);
            if grid {
                c.line(
                    (-14., 2.2, z),
                    (14., 2.2, z),
                    self.glow(color, z),
                    Some('─'),
                );
            } else {
                for x in [-1., 1.] {
                    c.line(
                        (x, 2.18, z),
                        (x, 2.18, z + 0.65),
                        self.glow(color, z),
                        Some('▬'),
                    );
                }
            }
        }
        if grid {
            for x in -10..=10 {
                c.line(
                    (x as f32 * 1.4, 2.2, 0.8),
                    (x as f32 * 1.4, 2.2, 84.),
                    scale(color, 0.5),
                    None,
                );
            }
        }
    }
    fn city(&self, c: &mut Canvas) {
        self.road(c, (45, 130, 255), false);
        // Night sky: a dim dotted skyline haze and stars that react to treble.
        for i in 0..70 {
            let x = ((i * 97) % 211) as f32 / 211.;
            let y = ((i * 41) % 53) as f32 / 53. * 0.26;
            let twinkle = 0.5 + 0.5 * (self.time * 2.3 + i as f32).sin();
            c.r.dot(
                (x * c.cols as f32) as i32,
                (y * c.rows as f32) as i32,
                300.,
                if twinkle > 0.8 { '+' } else { '.' },
                scale(
                    (120, 150, 200),
                    0.45 + twinkle * 0.35 + self.sound.treble * 0.4,
                ),
            );
        }
        // Stacked towers with front planes, receding roof edges and windows.
        // Windows switch on with the music: treble lights more of them, bass
        // pushes a warm pulse up the floors, and each musical object lights an
        // entire tower in one colour as it approaches.
        let phase = self.travel % 6.;
        let first = (self.travel / 6.).floor() as usize;
        let lit_share = 0.40 + self.sound.treble * 0.5;
        // The nearest tower under the cursor, by its front or its street wall.
        let tower_at = |n: usize, side: f32| {
            let z = 1. + n as f32 * 6. - phase;
            let id = (first + n) % 1024;
            let height = 4. + ((id * 37 + if side < 0. { 3 } else { 0 }) % 7) as f32;
            (z, id, height, side * 4.2, side * (7.5 + (id % 3) as f32), 2.2 - height, z + 4.5)
        };
        let mut hover: Option<(usize, f32, f32)> = None;
        if let Some((ux, uy)) = self.aim(c) {
            for n in 0..17 {
                for side in [-1., 1.] {
                    let (z, _, _, x, outer, top, far) = tower_at(n, side);
                    if z < 0.8 || hover.is_some_and(|h| h.2 < z) {
                        continue;
                    }
                    let inside = |a: P, b: P| {
                        let (p, q) = (c.project(a), c.project(b));
                        (p.0.min(q.0)..=p.0.max(q.0)).contains(&ux) && (p.1.min(q.1)..=p.1.max(q.1)).contains(&uy)
                    };
                    // The street wall runs from the camera into the distance:
                    // its bounding box would cover the road, so test the wall.
                    let wall = [(x, top, z), (x, top, far), (x, 2.2, far), (x, 2.2, z)].map(|p| c.project(p));
                    if inside((x, 2.2, z), (outer, top, z)) || crate::neon_scenes::inside(&wall, (ux, uy)) {
                        hover = Some((n, side, z));
                    }
                }
            }
        }
        // A tower wakes its own way; anything else the cursor rests on, such
        // as the cars, comes into focus like the map elements of the other roads.
        if hover.is_some() {
            c.aim = None;
        }
        for n in (0..17).rev() {
            let z = 1. + n as f32 * 6. - phase;
            if z < 0.8 {
                continue;
            }
            for side in [-1., 1.] {
                let id = (first + n) % 1024;
                let tower = self
                    .accents
                    .iter()
                    .filter(|a| a.side == side && (a.z - z).abs() < 3.)
                    .map(|a| (arrival_visibility(a.z, 64., 44.), a.height))
                    .fold((0., 0.), |m: (f32, f32), v| if v.0 > m.0 { v } else { m });
                let height = 4. + ((id * 37 + if side < 0. { 3 } else { 0 }) % 7) as f32;
                let x = side * 4.2;
                let outer = side * (7.5 + (id % 3) as f32);
                let top = 2.2 - height;
                let far = z + 4.5;
                let color = self.glow((10, 195, 255), z);
                let hot = hover.is_some_and(|h| h.0 == n && h.1 == side);
                let color = if hot {
                    (235, 250, 255)
                } else if tower.0 > 0. {
                    scale((120, 235, 255), 0.8 + tower.0 * 0.5)
                } else {
                    color
                };
                if hot {
                    // Its name over the roof, as large as the roof allows.
                    let (a, b) = (c.project((x, top, z)), c.project((outer, top, z)));
                    let label = format!("TOWER {id:03}");
                    // Over the part of the roof that is on screen.
                    let (l, r) = (a.0.min(b.0).max(0.), a.0.max(b.0).min(c.cols as f32));
                    let room = ((r - l) * 1.3).max(c.cols as f32 * 0.12);
                    let k = (1..=(c.rows / 40).max(1))
                        .rev()
                        .find(|&k| crate::scenes3d::cyber::type_size(&label, k).0 as f32 <= room)
                        .unwrap_or(1);
                    let (w, h) = crate::scenes3d::cyber::type_size(&label, k);
                    let at = ((l + r) * 0.5 - w as f32 * 0.5).clamp(1., (c.cols as f32 - w as f32 - 1.).max(1.)) as i32;
                    let lift = (a.1.min(b.1) - h as f32 - 1.).round() as i32;
                    let tint = scale((150, 240, 255), 0.85 + self.sound.bass * 0.3);
                    crate::scenes3d::cyber::pixel_text(&mut c.r, (c.cols, c.rows), (at, lift.max(0), 0.4), &label, k, tint);
                }
                c.face(
                    [(x, 2.2, z), (outer, 2.2, z), (outer, top, z), (x, top, z)],
                    scale(color, 0.12),
                );
                c.face(
                    [(x, 2.2, z), (x, top, z), (x, top, far), (x, 2.2, far)],
                    scale(color, 0.22),
                );
                c.face(
                    [
                        (x, top, z),
                        (outer, top, z),
                        (outer, top, far),
                        (x, top, far),
                    ],
                    scale(color, 0.32),
                );
                for (a, b) in [
                    ((x, 2.2, z), (x, top, z)),
                    ((outer, 2.2, z), (outer, top, z)),
                    ((x, top, z), (outer, top, z)),
                    ((x, top, z), (x, top, far)),
                    ((outer, top, z), (outer, top, far)),
                    ((x, top, far), (outer, top, far)),
                    ((x, 2.2, z), (x, 2.2, far)),
                ] {
                    c.line(
                        a,
                        b,
                        color,
                        Some(if (a.0 - b.0).abs() < 0.01 && (a.1 - b.1).abs() > 0.5 {
                            '▌'
                        } else {
                            '▬'
                        }),
                    );
                }
                let floors = height as usize * 2;
                for floor in 0..floors {
                    let y = 2. - floor as f32 * 0.48;
                    // A warm pulse climbs the facade on bass, floor by floor.
                    let climb =
                        (floor as f32 / floors as f32 - (self.time * 1.4).fract()).rem_euclid(1.);
                    let pulse = if climb < 0.18 {
                        (1. - climb / 0.18) * self.sound.bass
                    } else {
                        0.
                    };
                    for win in 0..5 {
                        let wx = x + (outer - x) * (win as f32 + 0.5) / 5.;
                        let seed = variant((id * 131 + floor * 17 + win * 7) as u32);
                        let lit = seed < lit_share;
                        let light = if hot {
                            // A scan line climbs the tower under the cursor.
                            let scan = (floor as f32 / floors as f32 - (self.time * 0.9).fract()).abs() < 0.1;
                            if scan { (255, 255, 255) } else { scale((120, 230, 255), 0.85 + self.sound.treble * 0.3) }
                        } else if tower.0 > 0. {
                            // Whole tower on: amber or cold white, per object.
                            let on = if tower.1 < 0.5 {
                                (255, 190, 70)
                            } else {
                                (200, 240, 255)
                            };
                            scale(on, 0.45 + tower.0 * 0.75)
                        } else if lit {
                            let warm = seed * 6. % 1. < 0.22 + self.sound.bass * 0.4;
                            let base = if warm { (255, 185, 35) } else { (25, 130, 210) };
                            scale(base, 0.7 + self.sound.treble * 0.5 + pulse * 0.6)
                        } else if pulse > 0.05 {
                            scale((255, 150, 40), pulse * 0.9)
                        } else {
                            (12, 28, 55)
                        };
                        c.line(
                            (wx, y, z - 0.025),
                            (wx + (outer - x) * 0.11, y, z - 0.025),
                            self.glow(light, z),
                            Some('▬'),
                        );
                    }
                    c.line((x, y, z), (x, y, far), scale(color, 0.55), None);
                    // The same floor seen on the street-facing wall: windows
                    // along the depth axis, so the facade reads from the road.
                    for win in 0..5 {
                        let wz = z + (far - z) * (win as f32 + 0.3) / 5.;
                        let seed = variant((id * 131 + floor * 17 + (win + 5) * 7) as u32);
                        let lit = seed < lit_share;
                        let light = if hot {
                            // A scan line climbs the tower under the cursor.
                            let scan = (floor as f32 / floors as f32 - (self.time * 0.9).fract()).abs() < 0.1;
                            if scan { (255, 255, 255) } else { scale((120, 230, 255), 0.85 + self.sound.treble * 0.3) }
                        } else if tower.0 > 0. {
                            let on = if tower.1 < 0.5 {
                                (255, 190, 70)
                            } else {
                                (200, 240, 255)
                            };
                            scale(on, 0.45 + tower.0 * 0.75)
                        } else if lit {
                            let warm = seed * 6. % 1. < 0.22 + self.sound.bass * 0.4;
                            let base = if warm { (255, 185, 35) } else { (25, 130, 210) };
                            scale(base, 0.7 + self.sound.treble * 0.5 + pulse * 0.6)
                        } else if pulse > 0.05 {
                            scale((255, 150, 40), pulse * 0.9)
                        } else {
                            (12, 28, 55)
                        };
                        c.line(
                            (x - side * 0.03, y, wz),
                            (x - side * 0.03, y, wz + (far - z) * 0.09),
                            self.glow(light, wz),
                            Some('▬'),
                        );
                    }
                }
            }
        }
        // Traffic ahead: small cars we slowly overtake, each a dark body
        // with a cabin, red tail lamps and a headlight glow on the asphalt.
        for n in 0..8 {
            let z = 0.65 + (n as f32 * 10. - self.travel * 0.65).rem_euclid(76.);
            let x = if n % 2 == 0 { -1.7 } else { 1.7 };
            let body = match n % 3 {
                0 => (120, 150, 220),
                1 => (210, 110, 180),
                _ => (90, 190, 200),
            };
            // Corners of both ends: the near face is the larger on screen.
            c.offer(
                &[(x - 0.42, 1.52, z), (x + 0.42, 2.16, z), (x - 0.42, 1.52, z + 1.7), (x + 0.42, 2.16, z + 1.7)],
                || ["CRUISER", "TAXI", "COUPE"][n % 3].into(),
            );
            c.box3((x - 0.42, 1.8, z), (x + 0.42, 2.16, z + 1.7), body);
            c.box3(
                (x - 0.3, 1.52, z + 0.45),
                (x + 0.3, 1.8, z + 1.2),
                scale(body, 0.8),
            );
            let lamp = scale((255, 60, 50), 0.8 + self.sound.bass * 0.4);
            for side in [-1., 1.] {
                if z > 12. {
                    // Far away a lamp is a single dim point, not a block.
                    let q = c.project((x + side * 0.3, 1.97, z - 0.03));
                    c.r.dot(
                        q.0.round() as i32,
                        q.1.round() as i32,
                        z - 0.03,
                        if z > 30. { '·' } else { '▪' },
                        scale(lamp, if z > 30. { 0.5 } else { 0.75 }),
                    );
                } else {
                    c.line(
                        (x + side * 0.36, 1.97, z - 0.03),
                        (x + side * 0.24, 1.97, z - 0.03),
                        lamp,
                        Some('█'),
                    );
                }
            }
            c.line(
                (x - 0.5, 2.19, z + 1.8),
                (x + 0.5, 2.19, z + 3.2),
                (120, 110, 60),
                Some('░'),
            );
        }
    }
    fn vector(&self, c: &mut Canvas) {
        self.road(c, (40, 240, 65), false);
        // A wireframe planet rises behind the vanishing point and turns slowly;
        // its meridians brighten with treble, its equator band with bass.
        let cx = c.cols as f32 * 0.5 - c.gaze.0 * c.cols as f32 * 0.04;
        let cy = c.horizon_y() - c.rows as f32 * 0.3;
        let radius = c.rows as f32 * 0.3;
        c.offer_screen((cx, cy), (radius * c.aspect, radius), 250., "PLANET");
        let planet = scale((60, 230, 90), 0.42 + self.sound.treble * 0.5);
        for k in 0..3 {
            let spin = self.time * 0.25 + k as f32 * std::f32::consts::PI / 3.;
            let rx = radius * c.aspect * spin.cos();
            let mut prev = None;
            for i in 0..=40 {
                let t = i as f32 / 40. * std::f32::consts::TAU;
                let q = (cx + rx * t.sin(), cy + radius * t.cos());
                if let Some(o) = prev {
                    c.r.line_with_glyph(o, q, 250., planet, false, Some('·'));
                }
                prev = Some(q);
            }
        }
        for lat in [-0.55, 0., 0.55] {
            let ry = radius * (1. - lat * lat as f32).sqrt();
            let color = if lat == 0. {
                scale((160, 255, 180), 0.5 + self.sound.bass * 0.7)
            } else {
                planet
            };
            let mut prev = None;
            for i in 0..=48 {
                let t = i as f32 / 48. * std::f32::consts::TAU;
                let q = (
                    cx + ry * c.aspect * t.cos(),
                    cy + radius * lat + ry * 0.22 * t.sin(),
                );
                if let Some(o) = prev {
                    c.r.line_with_glyph(o, q, 250., color, false, Some('─'));
                }
                prev = Some(q);
            }
        }
        // Rolling vector hills beyond both walls, lit from the road side.
        let phase = self.travel % 3.;
        let index = (self.travel / 3.).floor() as i32;
        for n in (0..26).rev() {
            let z = 0.8 + n as f32 * 3. - phase;
            let far = z + 3.;
            for side in [-1., 1.] {
                for band in 0..6 {
                    let hill = |b: i32, k: i32, zz: f32| {
                        let x = side * (4.2 + b as f32 * 1.8);
                        let w = k as f32 * 3.;
                        let h = ((w * 0.13 + b as f32 * 0.9 + side).sin() * 0.5 + 0.5)
                            * (0.6 + b as f32 * 0.55);
                        (x, 2.2 - h, zz)
                    };
                    let a = hill(band, index + n, z);
                    let b = hill(band + 1, index + n, z);
                    let d = hill(band, index + n + 1, far);
                    if band < 5 {
                        let e = hill(band + 1, index + n + 1, far);
                        c.offer_quad([a, b, e, d], || format!("RIDGE {:.0}M", ((2.2 - (a.1 + b.1) * 0.5) * 100.).max(0.)));
                    }
                    let color = scale(self.glow((35, 210, 70), z), 0.55 - band as f32 * 0.05);
                    c.line(a, b, color, None);
                    c.line(a, d, scale(color, 0.7), None);
                }
            }
        }
        // Spinning wireframe solids hover over the lanes: octahedra that tumble
        // as they pass, and brighten on the beat.
        // Heights vary from hovering just over the asphalt to high in the sky,
        // and each one bobs slowly so no two share a plane.
        for i in 0..6 {
            let z = 2. + (i as f32 * 14. - self.travel).rem_euclid(84.);
            let x = [-1.6, 2.4, 0.3, -2.6, 1.2, -0.6][i];
            let base = [0.4, -2.2, -5.2, -1.1, -3.8, -6.5][i];
            let y = base + (self.time * (0.5 + i as f32 * 0.13)).sin() * 0.5;
            // Under the cursor a solid swells, spins up and burns white.
            let r = 1.1 + (i % 3) as f32 * 0.25;
            let hot = self.aim(c).is_some_and(|(ux, uy)| {
                let (px, py) = c.project((x, y, z + 1.));
                let (ex, _) = c.project((x + r, y, z + 1.));
                let reach = (ex - px).abs().max(2.) * 1.3;
                ((ux - px) / c.aspect).hypot(uy - py) < reach / c.aspect
            });
            let spin = self.time * (0.9 + i as f32 * 0.2) * if hot { 3. } else { 1. };
            let (s, co) = spin.sin_cos();
            let r = if hot { r * 1.25 } else { r };
            let v = |px: f32, py: f32, pz: f32| {
                (x + px * co - pz * s, y + py, z + 1. + px * s + pz * co)
            };
            let top = v(0., -r, 0.);
            let bottom = v(0., r, 0.);
            let ring = [v(r, 0., 0.), v(0., 0., r), v(-r, 0., 0.), v(0., 0., -r)];
            c.offer(&[top, bottom, ring[0], ring[1], ring[2], ring[3]], || format!("PRISM {:02}", i + 1));
            let color = if hot {
                scale((230, 255, 235), 0.85 + self.sound.bass * 0.3)
            } else {
                scale(self.glow((140, 255, 160), z), 0.8 + self.sound.bass * 0.5)
            };
            for k in 0..4 {
                c.line(ring[k], ring[(k + 1) % 4], color, None);
                c.line(top, ring[k], color, None);
                c.line(bottom, ring[k], scale(color, 0.7), None);
            }
        }
    }
    fn circuit(&self, c: &mut Canvas) {
        // A continuous terrain under the camera: no lanes, median or road gap.
        let phase = self.travel % 3.;
        let index = (self.travel / 3.).floor() as i32;
        // Adjacent strips and bands share their world vertices. These heights
        // remain the same until travel reaches the next three-unit strip.
        #[cfg(test)]
        let cached = !self.exhaustive;
        #[cfg(not(test))]
        let cached = true;
        let mut terrain = self.terrain.borrow_mut();
        if cached && terrain.as_ref().is_none_or(|(at, _)| *at != index) {
            *terrain = Some((index, lunar_heights(index)));
        }
        let heights = cached.then(|| &terrain.as_ref().unwrap().1);
        for n in (-1..28).rev() {
            let raw_z = 1. + n as f32 * 3. - phase;
            let Some((z, far, t)) = lunar_visible_strip(raw_z) else {
                continue;
            };
            for band in -10..10 {
                let point = |b: i32, k: i32, zz: f32| {
                    let x = b as f32 * 2.;
                    let ground = |k: i32| match heights {
                        Some(rows) => rows[(k - index + 1) as usize][(b + 10) as usize],
                        None => lunar_ground(x, k as f32 * 3.),
                    };
                    let y = ground(k);
                    let y = if zz == z {
                        y + (ground(k + 1) - y) * t
                    } else {
                        y
                    };
                    (x, y, zz)
                };
                let a = point(band, index + n, z);
                let b = point(band + 1, index + n, z);
                let d = point(band, index + n + 1, far);
                let e = point(band + 1, index + n + 1, far);
                c.offer_quad([a, b, e, d], || format!("ALT {:.0}M", ((2.2 - (a.1 + b.1 + d.1 + e.1) * 0.25) * 100.).max(0.)));
                // Constant moonlight keeps terrain legible even in silence.
                let color = scale(
                    if band % 2 == 0 {
                        (65, 255, 105)
                    } else {
                        (45, 115, 255)
                    },
                    0.72 + 0.28 / (1. + z * 0.025),
                );
                c.face([a, b, e, d], scale(color, 0.10 + band.abs() as f32 * 0.012));
                c.line(a, b, color, None);
                c.line(a, d, scale(color, 0.9), None);
                c.line(b, d, scale(color, 0.65), None);
            }
        }
        let cx = c.cols as f32 * 0.68;
        let cy = c.horizon_y() - c.rows as f32 * 0.21;
        let radius = c.rows as f32 * 0.16;
        c.offer_screen((cx, cy), (radius * c.aspect, radius), 250., "MOON");
        // Only the moon's own rectangle: the rest of the sky is not it.
        let span = |centre: f32, reach: f32, size: usize| {
            ((centre - reach).floor().max(0.) as usize)..((centre + reach).ceil().max(0.) as usize + 1).min(size)
        };
        for y in span(cy, radius, c.rows) {
            for x in span(cx, radius * c.aspect, c.cols) {
                let nx = (x as f32 - cx) / (radius * c.aspect);
                let ny = (y as f32 - cy) / radius;
                let r2 = nx * nx + ny * ny;
                if r2 > 1. {
                    continue;
                }
                let surface = (1. - r2).sqrt();
                let light = (-0.48 * nx - 0.32 * ny + 0.8 * surface).max(0.);
                let mut shade = 0.28 + 0.72 * light;
                for (px, py, r) in [
                    (-0.35, -0.22, 0.22),
                    (0.25, 0.35, 0.28),
                    (0.38, -0.42, 0.15),
                    (-0.55, 0.42, 0.12),
                ] {
                    let d = ((nx - px).powi(2) + (ny - py).powi(2)).sqrt() / r;
                    if d < 1. {
                        shade *= 0.54 + 0.22 * d;
                    } else if d < 1.18 {
                        shade = (shade + 0.15).min(1.);
                    }
                }
                let palette = ['.', ':', 'o', 'O', '0', '@'];
                let ch = palette[(shade * 5.).clamp(0., 5.) as usize];
                c.r.dot(x as i32, y as i32, 250., ch, scale((205, 225, 255), shade));
            }
        }
        for i in 0..55 {
            let x = ((i * 73) % 101) as f32 / 100.;
            let y = ((i * 31) % 29) as f32 / 100.;
            c.r.dot(
                (x * c.cols as f32) as i32,
                (y * c.rows as f32) as i32,
                300.,
                '.',
                (100, 125, 155),
            );
        }
    }
    fn sunset(&self, c: &mut Canvas) {
        let cx = c.cols as f32 * 0.5;
        let cy = c.horizon_y() - c.rows as f32 * 0.18;
        let radius = c.rows as f32 * 0.205;
        c.offer_screen((cx, cy), (radius * c.aspect, radius), 300., "SUN");
        for y in 0..c.rows {
            let dy = (y as f32 - cy) / radius;
            if dy.abs() > 1. {
                continue;
            }
            let band = y as f32 / c.rows as f32;
            if dy > 0.05 && y % 4 == 0 {
                continue;
            }
            for x in 0..c.cols {
                let dx = (x as f32 - cx) / (radius * c.aspect);
                if dx * dx + dy * dy <= 1. {
                    let warm = ((dy + 1.) * 0.5).clamp(0., 1.);
                    let color = (
                        255,
                        (210. * (1. - warm)).max(20.) as u8,
                        (35. + warm * 170.) as u8,
                    );
                    c.r.dot(
                        x as i32,
                        y as i32,
                        300.,
                        if band < 0.3 { '▬' } else { '▄' },
                        scale(
                            color,
                            0.65 + self.sound.treble * 0.5 + self.sound.bass * 0.2,
                        ),
                    );
                }
            }
        }
        // Distant layered mountain silhouette, independent of the road grid.
        for i in 0..24 {
            let x = i as f32 / 24.;
            let nx = (i + 1) as f32 / 24.;
            let h = c.horizon_y() / c.rows as f32 - 0.01 - ((i * 17 % 9) as f32) * 0.009;
            let nh = c.horizon_y() / c.rows as f32 - 0.01 - (((i + 1) * 17 % 9) as f32) * 0.009;
            c.r.line(
                (x * c.cols as f32, h * c.rows as f32),
                (nx * c.cols as f32, nh * c.rows as f32),
                150.,
                (110, 35, 180),
                true,
            );
        }
        self.road(c, (230, 40, 185), true);
        for i in (0..10).rev() {
            let z = 2. + (i as f32 * 8. - self.travel).rem_euclid(80.);
            for side in [-1., 1.] {
                self.palm(c, side * 5.8, z);
            }
        }
        // Roadside life: cacti at fixed spots and grass that sways with treble.
        for n in (0..14).rev() {
            let world = ((self.travel / 7.).floor() as i32 + n) as u32;
            let z = 1.2 + (n as f32 * 7. - self.travel % 7.).rem_euclid(98.);
            let v = variant(world.wrapping_mul(2654435761));
            let side = if v < 0.5 { -1. } else { 1. };
            let x = side * (7.8 + v * 6. % 3.5);
            let h = 1.6 + variant(world.wrapping_add(7)) * 2.2;
            self.cactus(c, x, z, h);
        }
        for n in 0..40 {
            let z = 1. + (n as f32 * 2.4 - self.travel).rem_euclid(96.);
            let sway = (self.time * 3. + n as f32 * 1.7).sin() * (0.08 + self.sound.treble * 0.3);
            for side in [-1., 1.] {
                let x = side * (4.5 + (n % 3) as f32 * 0.35);
                let color = self.glow((80, 210, 110), z);
                for (dx, h) in [(-0.22, 0.45), (0., 0.7), (0.22, 0.5)] {
                    c.line((x, 2.2, z), (x + dx + sway, 2.2 - h, z), color, None);
                }
            }
        }
        // Stars above the mountains, avoiding the sun disc, twinkling on treble.
        for i in 0..60 {
            let x = ((i * 89) % 197) as f32 / 197.;
            let y = ((i * 37) % 47) as f32 / 47. * 0.32;
            let sx = (x - 0.5) * c.cols as f32 / (radius * c.aspect);
            let sy = (y * c.rows as f32 - cy) / radius;
            if sx * sx + sy * sy < 1.4 {
                continue;
            }
            let twinkle = 0.5 + 0.5 * (self.time * 2. + i as f32 * 0.9).sin();
            c.r.dot(
                (x * c.cols as f32) as i32,
                (y * c.rows as f32) as i32,
                300.,
                if twinkle > 0.85 { '+' } else { '.' },
                scale(
                    (255, 200, 230),
                    0.35 + twinkle * 0.3 + self.sound.treble * 0.35,
                ),
            );
        }
        // A flock of birds crosses the sunset, flapping in screen space.
        for bird in 0..5 {
            let t = (self.time * 0.045 + bird as f32 * 0.13) % 1.3;
            let bx = c.cols as f32 * (1.1 - t) + bird as f32 * 3.;
            let by = c.horizon_y() - c.rows as f32 * (0.27 + (bird % 3) as f32 * 0.03)
                + (self.time * 0.7 + bird as f32).sin() * 1.5;
            let flap = (self.time * 5. + bird as f32 * 1.3).sin() * 1.2;
            for dir in [-1., 1.] {
                c.r.line(
                    (bx, by),
                    (bx + dir * 2.5, by - flap),
                    200.,
                    (90, 30, 120),
                    true,
                );
            }
        }
        // The first convertible: low, wide neon rear view with two red tail
        // lamps and a cyan rear window, lamps pulsing with the bass.
        let z = 5.6;
        let x = self.gaze.0 * 0.25;
        let bob = (self.time * 9.).sin() * 0.012 * self.sound.bass;
        c.offer(&[(x - 1., 2.2, z), (x + 1., 1.04, z)], || "CONVERTIBLE".into());
        c.face(
            [
                (x - 1., 2.18 + bob, z),
                (x + 1., 2.18 + bob, z),
                (x + 0.75, 1.04 + bob, z),
                (x - 0.75, 1.04 + bob, z),
            ],
            (12, 17, 36),
        );
        c.face(
            [
                (x - 0.88, 2.03 + bob, z - 0.04),
                (x + 0.88, 2.03 + bob, z - 0.04),
                (x + 0.92, 1.65 + bob, z - 0.04),
                (x - 0.92, 1.65 + bob, z - 0.04),
            ],
            (95, 45, 145),
        );
        c.face(
            [
                (x - 0.68, 1.56 + bob, z - 0.05),
                (x + 0.68, 1.56 + bob, z - 0.05),
                (x + 0.49, 1.12 + bob, z - 0.05),
                (x - 0.49, 1.12 + bob, z - 0.05),
            ],
            (25, 105, 145),
        );
        let outline = self.glow((140, 195, 255), 1.);
        let lamp = scale((255, 70, 75), 0.75 + self.sound.bass * 0.5);
        for side in [-1., 1.] {
            c.line(
                (x + side * 0.92, 2.03 + bob, z - 0.08),
                (x + side * 0.92, 1.65 + bob, z - 0.08),
                outline,
                Some('▌'),
            );
            c.line(
                (x + side * 0.92, 1.65 + bob, z - 0.08),
                (x + side * 0.49, 1.1 + bob, z - 0.08),
                outline,
                None,
            );
            c.line(
                (x + side * 0.8, 1.86 + bob, z - 0.1),
                (x + side * 0.25, 1.86 + bob, z - 0.1),
                lamp,
                Some('█'),
            );
            c.line(
                (x + side * 0.86, 2.08 + bob, z - 0.1),
                (x + side * 0.65, 2.08 + bob, z - 0.1),
                (50, 65, 95),
                Some('█'),
            );
            // Exhaust haze drifting back on the beat (invisible when silent).
            c.line(
                (x + side * 0.6, 2.2 + bob, z - 0.12),
                (
                    x + side * (0.6 + self.sound.bass * 0.4),
                    2.24 + bob,
                    z - 0.9,
                ),
                scale((180, 120, 220), (self.sound.bass - 0.1).max(0.) * 0.9),
                Some('░'),
            );
        }
        c.line(
            (x - 0.49, 1.1 + bob, z - 0.08),
            (x + 0.49, 1.1 + bob, z - 0.08),
            (120, 235, 255),
            Some('▬'),
        );
        c.line(
            (x - 0.92, 1.65 + bob, z - 0.08),
            (x + 0.92, 1.65 + bob, z - 0.08),
            outline,
            Some('▬'),
        );
        c.line(
            (x - 0.86, 2.03 + bob, z - 0.08),
            (x + 0.86, 2.03 + bob, z - 0.08),
            (170, 200, 255),
            Some('▬'),
        );
    }
    fn cactus(&self, c: &mut Canvas, x: f32, z: f32, h: f32) {
        let color = self.glow((70, 200, 120), z);
        let top = 2.2 - h;
        c.offer(&[(x - 0.5, 2.2, z), (x + 0.5, top, z)], || "CACTUS".into());
        c.line((x, 2.2, z), (x, top, z), color, Some('▌'));
        for (side, at, len) in [(-1., 0.45, 0.55), (1., 0.6, 0.45)] {
            let y = 2.2 - h * at;
            let ax = x + side * 0.5;
            c.line((x, y, z), (ax, y, z), scale(color, 0.8), Some('▬'));
            c.line((ax, y, z), (ax, y - h * len, z), color, Some('▌'));
        }
        // A bloom on the crown lights up when the music plays.
        c.line(
            (x - 0.08, top - 0.1, z - 0.02),
            (x + 0.08, top - 0.1, z - 0.02),
            scale((255, 120, 200), (self.sound.treble - 0.1).max(0.) * 1.1),
            Some('*'),
        );
    }
    fn palm(&self, c: &mut Canvas, x: f32, z: f32) {
        let top = (x + if x < 0. { 0.45 } else { -0.45 }, -3.3, z);
        // The trunk and the heart of the crown: the whole crown was so wide
        // that a palm took the focus from everything around it.
        c.offer(&[(x - 0.3, 2.2, z), (x + 0.3, 2.2, z), (top.0 - 0.5, -3.6, z), (top.0 + 0.5, -3.6, z)], || "PALM".into());
        c.line((x, 2.2, z), top, self.glow((170, 55, 215), z), Some('▌'));
        for frond in 0..7 {
            let a = frond as f32 * std::f32::consts::PI / 3.5;
            let mut prev = top;
            for segment in 1..=5 {
                let t = segment as f32 / 5.;
                let p = (
                    top.0 + a.cos() * 2.2 * t,
                    top.1 - a.sin().abs() * 1.3 * t + 1.5 * t * t,
                    z + a.sin() * 1.1 * t,
                );
                c.line(prev, p, self.glow((205, 80, 255), z), None);
                prev = p;
            }
        }
    }
}
// Keep the foreground strip until its far edge passes the near plane.
// Dropping it when only the near edge crossed produced holes in the floor.
fn lunar_visible_strip(raw_z: f32) -> Option<(f32, f32, f32)> {
    let far = raw_z + 3.;
    if far <= 0.8 {
        return None;
    }
    let near = raw_z.max(0.8);
    Some((near, far, (near - raw_z) / 3.))
}
fn lunar_heights(index: i32) -> [[f32; 21]; 30] {
    std::array::from_fn(|row| {
        let world = (index + row as i32 - 1) as f32 * 3.;
        std::array::from_fn(|col| lunar_ground((col as i32 - 10) as f32 * 2., world))
    })
}
fn lunar_ground(x: f32, world_z: f32) -> f32 {
    // Original rolling neon ridges, extended continuously across the old road.
    let band = x.abs() * 0.5;
    let ridge = (world_z * std::f32::consts::TAU / 48. + band * 1.9).sin() * 0.5 + 0.5;
    2.2 - ridge * (band * 0.9 + 1.)
}
/// Whether `p` is inside the convex polygon `poly`, either winding.
fn inside(poly: &[(f32, f32); 4], p: (f32, f32)) -> bool {
    let mut sign = 0.;
    for i in 0..4 {
        let (a, b) = (poly[i], poly[(i + 1) % 4]);
        let c = (b.0 - a.0) * (p.1 - a.1) - (b.1 - a.1) * (p.0 - a.0);
        if c != 0. {
            if sign != 0. && c.signum() != sign {
                return false;
            }
            sign = c.signum();
        }
    }
    true
}
// Stateless scrambling varies placement without frame-to-frame jitter.
pub(crate) fn variant(mut n: u32) -> f32 {
    n ^= n >> 16;
    n = n.wrapping_mul(0x7feb352d);
    n ^= n >> 15;
    n = n.wrapping_mul(0x846ca68b);
    n ^= n >> 16;
    (n >> 8) as f32 / 16777216.
}
pub(crate) fn arrival_visibility(z: f32, far: f32, near: f32) -> f32 {
    let t = ((far - z) / (far - near)).clamp(0., 1.);
    t * t * (3. - 2. * t)
}
fn scale(c: Color, k: f32) -> Color {
    (
        (c.0 as f32 * k).clamp(0., 255.) as u8,
        (c.1 as f32 * k).clamp(0., 255.) as u8,
        (c.2 as f32 * k).clamp(0., 255.) as u8,
    )
}
struct Canvas {
    r: Raster,
    cols: usize,
    rows: usize,
    aspect: f32,
    gaze: (f32, f32),
    style: Style,
    solid_body: bool,
    /// The cell under the cursor, when there is one.
    aim: Option<(f32, f32)>,
    /// The nearest map element under the cursor, offered while drawing.
    focus: Option<Focus>,
}
/// A map element the cursor can rest on: its outline on screen, its depth
/// and its name.
struct Focus {
    poly: [(f32, f32); 4],
    z: f32,
    name: String,
}
impl Canvas {
    /// Offers an element, by points that bound it, to the cursor: the
    /// nearest whose screen box holds the cursor is focused after the frame.
    fn offer(&mut self, pts: &[P], name: impl FnOnce() -> String) {
        let Some((ux, uy)) = self.aim else { return };
        if pts.iter().any(|p| p.2 < 0.7) {
            return;
        }
        let z = pts.iter().map(|p| p.2).sum::<f32>() / pts.len() as f32;
        if self.focus.as_ref().is_some_and(|f| f.z <= z) {
            return;
        }
        let (mut x0, mut y0, mut x1, mut y1) = (f32::MAX, f32::MAX, f32::MIN, f32::MIN);
        for &p in pts {
            let (x, y) = self.project(p);
            (x0, y0, x1, y1) = (x0.min(x), y0.min(y), x1.max(x), y1.max(y));
        }
        // Small far things get a little slack, so they can be caught.
        let pad = 1.;
        if ux >= x0 - pad && ux <= x1 + pad && uy >= y0 - pad * 0.5 && uy <= y1 + pad * 0.5 {
            self.focus = Some(Focus { poly: [(x0, y0), (x1, y0), (x1, y1), (x0, y1)], z, name: name() });
        }
    }
    /// As `offer`, for something drawn straight on the screen, like a sun.
    fn offer_screen(&mut self, (cx, cy): (f32, f32), (rx, ry): (f32, f32), z: f32, name: &str) {
        let Some((ux, uy)) = self.aim else { return };
        if self.focus.as_ref().is_some_and(|f| f.z <= z) || (ux - cx).abs() > rx || (uy - cy).abs() > ry {
            return;
        }
        let (x0, y0, x1, y1) = (cx - rx, cy - ry, cx + rx, cy + ry);
        self.focus = Some(Focus { poly: [(x0, y0), (x1, y0), (x1, y1), (x0, y1)], z, name: name.into() });
    }
    /// As `offer`, for a patch of ground: the cursor must be inside the quad.
    fn offer_quad(&mut self, q: [P; 4], name: impl FnOnce() -> String) {
        let Some((ux, uy)) = self.aim else { return };
        if q.iter().any(|p| p.2 < 0.7) {
            return;
        }
        let z = q.iter().map(|p| p.2).sum::<f32>() * 0.25;
        if self.focus.as_ref().is_some_and(|f| f.z <= z) {
            return;
        }
        let poly = q.map(|p| self.project(p));
        if inside(&poly, (ux, uy)) {
            self.focus = Some(Focus { poly, z, name: name() });
        }
    }
    fn horizon_y(&self) -> f32 {
        self.rows as f32
            * (if self.style == Style::City {
                0.30
            } else {
                0.43
            } + self.gaze.1 * 0.025)
    }
    fn project(&self, p: P) -> (f32, f32) {
        let (x, y, z) = p;
        let f = self.rows as f32 * 0.78;
        (
            self.cols as f32 * 0.5 + (x - self.gaze.0 * 0.4) * f * self.aspect / z,
            self.horizon_y()
                + (y + if self.style == Style::Circuit {
                    1.3
                } else {
                    0.
                }) * f
                    / z,
        )
    }
    fn line(&mut self, a: P, b: P, color: Color, glyph: Option<char>) {
        if a.2 < 0.6 || b.2 < 0.6 {
            return;
        }
        // Subdivide depth-varying edges so a near endpoint cannot occlude
        // an entire distant road segment. Constant-depth edges remain one call.
        let count = ((a.2 - b.2).abs() / 0.8).ceil().clamp(1., 128.) as usize;
        let mix = |t: f32| {
            (
                a.0 + (b.0 - a.0) * t,
                a.1 + (b.1 - a.1) * t,
                a.2 + (b.2 - a.2) * t,
            )
        };
        for i in 0..count {
            let p = mix(i as f32 / count as f32);
            let q = mix((i + 1) as f32 / count as f32);
            self.r.line_with_glyph(
                self.project(p),
                self.project(q),
                p.2.min(q.2) - 0.015,
                color,
                true,
                glyph,
            );
        }
    }
    fn box3(&mut self, a: P, b: P, color: Color) {
        let p = [
            a,
            (b.0, a.1, a.2),
            (b.0, b.1, a.2),
            (a.0, b.1, a.2),
            (a.0, a.1, b.2),
            (b.0, a.1, b.2),
            b,
            (a.0, b.1, b.2),
        ];
        for (ids, k) in [
            ([0, 1, 2, 3], 0.25),
            ([0, 4, 5, 1], 0.40),
            ([0, 3, 7, 4], 0.16),
            ([1, 5, 6, 2], 0.32),
        ] {
            self.face(ids.map(|i| p[i]), scale(color, k));
        }
        for (a, b) in [
            (0, 1),
            (1, 2),
            (2, 3),
            (3, 0),
            (4, 5),
            (5, 6),
            (6, 7),
            (7, 4),
            (0, 4),
            (1, 5),
            (2, 6),
            (3, 7),
        ] {
            self.line(p[a], p[b], color, None);
        }
    }
    fn face(&mut self, p: [P; 4], color: Color) {
        if p.iter().any(|p| p.2 < 0.6) {
            return;
        }
        let q = p.map(|p| self.project(p));
        let cross = |a: (f32, f32), b: (f32, f32), c: (f32, f32)| {
            (b.0 - a.0) * (c.1 - a.1) - (b.1 - a.1) * (c.0 - a.0)
        };
        // Both triangles of the quad, with what each cell would recompute.
        let triangles = [[0, 1, 2], [0, 2, 3]].map(|ids| {
            let [a, b, c] = ids.map(|i| q[i]);
            let area = cross(a, b, c);
            (a, b, c, area.abs() >= 0.00001, area.recip(), ids.map(|i| p[i].2.recip()))
        });
        let lo = q
            .iter()
            .map(|p| p.1)
            .fold(f32::INFINITY, f32::min)
            .floor()
            .max(0.) as usize;
        let hi = q
            .iter()
            .map(|p| p.1)
            .fold(0., f32::max)
            .ceil()
            .min(self.rows as f32) as usize;
        for y in lo..hi {
            let yy = y as f32 + 0.5;
            let (mut low, mut high, mut hits) = (f32::INFINITY, f32::NEG_INFINITY, 0);
            for edge in 0..4 {
                let (a, b) = (q[edge], q[(edge + 1) % 4]);
                if (a.1 <= yy && b.1 > yy) || (b.1 <= yy && a.1 > yy) {
                    let hit = a.0 + (yy - a.1) * (b.0 - a.0) / (b.1 - a.1);
                    low = low.min(hit);
                    high = high.max(hit);
                    hits += 1;
                }
            }
            if hits < 2 {
                continue;
            }
            let left = low.floor().max(0.) as usize;
            let right = high.ceil().min(self.cols as f32) as usize;
            for x in left..right {
                let point = (x as f32 + 0.5, yy);
                for &(a, b, c, usable, per_area, inverse) in &triangles {
                    if !usable {
                        continue;
                    }
                    let u = cross(point, b, c) * per_area;
                    let v = cross(a, point, c) * per_area;
                    let w = 1. - u - v;
                    if u >= -0.001 && v >= -0.001 && w >= -0.001 {
                        let z = 1. / (u * inverse[0] + v * inverse[1] + w * inverse[2]);
                        self.r.dot(x as i32, y as i32, z, if self.solid_body { '▓' } else { '▪' }, color);
                        break;
                    }
                }
            }
        }
    }
    /// The fill as first written: every cell tests both triangles from scratch.
    #[cfg(test)]
    fn face_reference(&mut self, p: [P; 4], color: Color) {
        if p.iter().any(|p| p.2 < 0.6) {
            return;
        }
        let q = p.map(|p| self.project(p));

        let lo = q
            .iter()
            .map(|p| p.1)
            .fold(f32::INFINITY, f32::min)
            .floor()
            .max(0.) as usize;
        let hi = q
            .iter()
            .map(|p| p.1)
            .fold(0., f32::max)
            .ceil()
            .min(self.rows as f32) as usize;
        for y in lo..hi {
            let yy = y as f32 + 0.5;
            let mut hits = vec![];
            for edge in 0..4 {
                let (a, b) = (q[edge], q[(edge + 1) % 4]);
                if (a.1 <= yy && b.1 > yy) || (b.1 <= yy && a.1 > yy) {
                    hits.push(a.0 + (yy - a.1) * (b.0 - a.0) / (b.1 - a.1));
                }
            }
            if hits.len() < 2 {
                continue;
            }
            hits.sort_by(f32::total_cmp);
            let left = hits[0].floor().max(0.) as usize;
            let right = hits[hits.len() - 1].ceil().min(self.cols as f32) as usize;
            for x in left..right {
                let point = (x as f32 + 0.5, yy);
                for ids in [[0, 1, 2], [0, 2, 3]] {
                    let [a, b, c] = ids.map(|i| q[i]);
                    let cross = |a: (f32, f32), b: (f32, f32), c: (f32, f32)| {
                        (b.0 - a.0) * (c.1 - a.1) - (b.1 - a.1) * (c.0 - a.0)
                    };
                    let area = cross(a, b, c);
                    if area.abs() < 0.00001 {
                        continue;
                    }
                    let u = cross(point, b, c) / area;
                    let v = cross(a, point, c) / area;
                    let w = 1. - u - v;
                    if u >= -0.001 && v >= -0.001 && w >= -0.001 {
                        let z = 1. / (u / p[ids[0]].2 + v / p[ids[1]].2 + w / p[ids[2]].2);
                        self.r.dot(
                            x as i32,
                            y as i32,
                            z,
                            if self.solid_body { '▓' } else { '▪' },
                            color,
                        );
                        break;
                    }
                }
            }
        }
    }
}
/// Tells a frame from the one before it. Telling them apart wrongly would hold
/// back one frame out of 2^64, and the next one corrects it.
fn frame_print(cells: &[DirectCell]) -> u64 {
    cells.iter().fold(cells.len() as u64 + 1, crate::interactive_tunnel::frame_print_cell)
}
pub fn run(
    style: Style,
    cols: usize,
    rows: usize,
    aspect: f32,
    pointer: Arc<PointerState>,
    snapshot: Arc<Mutex<Vec<DirectCell>>>,
    generation: Arc<AtomicU64>,
    stop: Arc<AtomicBool>,
    wakeup: Arc<FrameWakeup>,
) -> anyhow::Result<()> {
    let audio = crate::AudioState::start(true);
    let mut scene = Scene::new(style);
    let mut cells = vec![];
    let mut shown = 0;
    let mut last = Instant::now();
    let mut seen = false;
    eprintln!("neon scene: {style:?}, audio reactive");
    while !stop.load(Ordering::Acquire) {
        let begin = Instant::now();
        let dt = (begin - last).as_secs_f32();
        last = begin;
        crate::fps_governor::note_tick();
        let sound = Sound::sample(&audio);
        if !seen && (sound.bass > 0.03 || sound.treble > 0.03) {
            eprintln!("neon scene: spectral audio received");
            seen = true;
        }
        scene.step(dt, pointer.sample(), sound);
        // A still frame costs the GPU and the battery nothing if never sent.
        // Fingerprint during hand-over, without reading the cell list again.
        let print = scene.draw_with_print(cols, rows, aspect, &mut cells);
        if print != shown {
            shown = print;
            let mut slot = snapshot.lock().unwrap();
            if stop.load(Ordering::Acquire) {
                break;
            }
            std::mem::swap(&mut *slot, &mut cells);
            generation.fetch_add(1, Ordering::Release);
            wakeup.notify()?;
        }
        let period = Duration::from_secs_f64(1. / crate::max_fps().unwrap_or(60.).clamp(1., 240.));
        if let Some(rest) = period.checked_sub(begin.elapsed()) {
            std::thread::sleep(rest);
        }
    }
    Ok(())
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn a_frame_print_only_matches_the_same_frame() {
        let cell = |x, ch, color| DirectCell { x, y: 3, ch, color, background: None, attrs: 1 };
        let frame = vec![cell(1, 'a', (1, 2, 3)), cell(2, 'b', (4, 5, 6))];
        assert_eq!(frame_print(&frame), frame_print(&frame.clone()));
        assert_ne!(frame_print(&frame), frame_print(&[]));
        for other in [
            vec![cell(1, 'a', (1, 2, 3))],
            vec![cell(1, 'a', (1, 2, 4)), cell(2, 'b', (4, 5, 6))],
            vec![cell(1, 'c', (1, 2, 3)), cell(2, 'b', (4, 5, 6))],
            vec![cell(1, 'a', (1, 2, 3)), cell(3, 'b', (4, 5, 6))],
            vec![cell(2, 'b', (4, 5, 6)), cell(1, 'a', (1, 2, 3))],
        ] {
            assert_ne!(frame_print(&frame), frame_print(&other));
        }
    }

    // cargo test --release clock_frame_cost -- --ignored --nocapture
    #[test]
    #[ignore = "manual cost report"]
    fn clock_frame_cost() {
        let (cols, rows, aspect) = (936, 279, 2.17);
        let names = std::env::var("CLOCKS").unwrap_or_else(|_| {
            "sand-clock sand-clock-chladni sand-clock-dunes sand-clock-galaxy".into()
        });
        for name in names.split_whitespace() {
            let mut scene = Scene::new(Style::parse(name).unwrap());
            let mut out = vec![];
            scene.draw(cols, rows, aspect, &mut out);
            let level: f32 = std::env::var("LEVEL").ok().and_then(|v| v.parse().ok()).unwrap_or(0.3);
            let sound = Sound { bass: level, treble: level * 0.7 };
            for _ in 0..200 {
                scene.step(0.01, Some((0.4, 0.5)), sound);
                scene.draw(cols, rows, aspect, &mut out);
            }
            let (mut step, mut draw, mut print) = (Duration::ZERO, Duration::ZERO, Duration::ZERO);
            let frames = 500;
            let mut sink = 0u64;
            for _ in 0..frames {
                let t = Instant::now();
                scene.step(0.01, Some((0.4, 0.5)), sound);
                step += t.elapsed();
                let t = Instant::now();
                scene.draw(cols, rows, aspect, &mut out);
                draw += t.elapsed();
                let t = Instant::now();
                sink ^= frame_print(&out);
                print += t.elapsed();
            }
            let ms = |d: Duration| d.as_secs_f64() * 1000. / frames as f64;
            eprintln!(
                "CLOCK {name:<20} step {:6.3} draw {:6.3} print {:6.3} total {:6.3} ms cells {} ({sink:x})",
                ms(step), ms(draw), ms(print), ms(step + draw + print), out.len()
            );
        }
    }
    // cargo test --release grid_frame_cost -- --ignored --nocapture
    #[test]
    #[ignore = "manual cost report"]
    fn grid_frame_cost() {
        use crate::gpu_grid::GpuGrid;
        let (cols, rows, aspect) = (936, 279, 2.17);
        let names = std::env::var("CLOCKS").unwrap_or_else(|_| {
            "sand-clock sand-clock-chladni sand-clock-dunes sand-clock-galaxy".into()
        });
        for name in names.split_whitespace() {
            let mut scene = Scene::new(Style::parse(name).unwrap());
            let mut grid = GpuGrid::with_profile(cols, rows, 8., crate::render_profile::RenderProfile::Clipped);
            grid.set_size(3456, 2234, 2.);
            let mut out = vec![];
            scene.draw(cols, rows, aspect, &mut out);
            let level: f32 = std::env::var("LEVEL").ok().and_then(|v| v.parse().ok()).unwrap_or(0.3);
            let sound = Sound { bass: level, treble: level * 0.7 };
            let frames: Vec<Vec<DirectCell>> = (0..300).map(|_| {
                scene.step(0.01, Some((0.4, 0.5)), sound);
                scene.draw(cols, rows, aspect, &mut out);
                out.clone()
            }).collect();
            for f in &frames[..50] { grid.fill(f).unwrap(); let _ = grid.damage_tiles(); }
            let (mut fill, mut damage, mut rects) = (Duration::ZERO, Duration::ZERO, Duration::ZERO);
            let mut n = 0;
            for f in &frames[50..] {
                let t = Instant::now();
                grid.fill(f).unwrap();
                fill += t.elapsed();
                let t = Instant::now();
                let tiles = grid.damage_tiles();
                damage += t.elapsed();
                let t = Instant::now();
                n += tiles.map(|t| grid.tiles_to_rects(&t).len()).unwrap_or(0);
                rects += t.elapsed();
            }
            let ms = |d: Duration| d.as_secs_f64() * 1000. / 250.;
            eprintln!("GRID {name:<20} fill {:6.3} damage {:6.3} rects {:6.3} ms  rects/frame {}", ms(fill), ms(damage), ms(rects), n / 250);
        }
    }
    #[test]
    fn lit_depths_are_not_remembered_across_frames() {
        let mut scene = Scene::new(Style::City);
        let loud = Sound { bass: 0.8, treble: 0.5 };
        for _ in 0..20 {
            scene.step(0.05, None, loud);
        }
        let first = scene.glow((200, 200, 200), 7.);
        assert_eq!(first, scene.glow((200, 200, 200), 7.));
        scene.step(0.5, None, Sound { bass: 0., treble: 0. });
        for _ in 0..20 {
            scene.step(0.05, None, Sound { bass: 0., treble: 0. });
        }
        assert_ne!(first, scene.glow((200, 200, 200), 7.));
    }
    #[test]
    fn the_faster_face_fill_paints_the_same_cells() {
        let canvas = || Canvas {
            r: Raster::new(310, 96),
            cols: 310,
            rows: 96,
            aspect: 1.75,
            gaze: (0.1, -0.2),
            style: Style::City,
            solid_body: false,
            aim: None,
            focus: None,
        };
        let quads: [[P; 4]; 3] = [
            [(4.2, 2.2, 6.), (8., 2.2, 6.), (8., -3., 6.), (4.2, -3., 6.)],
            [(4.2, 2.2, 6.), (4.2, -3., 6.), (4.2, -3., 10.5), (4.2, 2.2, 10.5)],
            [(-4., 2.2, 3.), (4., 2.2, 3.), (4., 2.2, 9.), (-4., 2.2, 9.)],
        ];
        for quad in quads {
            let (mut a, mut b) = (canvas(), canvas());
            a.face(quad, (10, 20, 30));
            b.face_reference(quad, (10, 20, 30));
            let (mut fast, mut slow) = (vec![], vec![]);
            a.r.finish(&mut fast);
            b.r.finish(&mut slow);
            assert!(fast.len() > 50);
            assert_eq!(fast, slow);
        }
    }

    #[test]
    #[ignore]
    fn neon_frame_cost() {
        use crate::interactive_matrix::Sound;
        // With and without a cursor: its lamp and rings are the extra work.
        for pointer in [None, Some((0.4, 0.5))] {
            for style in [Style::City, Style::Vector, Style::Circuit, Style::Sunset] {
                let mut scene = Scene::new(style);
                let loud = Sound { bass: 0.6, treble: 0.5 };
                let mut out = vec![];
                for _ in 0..240 {
                    scene.step(1. / 20., pointer, loud);
                }
                let (mut step, mut draw) = (Duration::ZERO, Duration::ZERO);
                let frames = 200;
                for i in 0..frames {
                    // A beat every half second, so rings are out.
                    let sound = if i % 10 == 0 { Sound { bass: 1., treble: 0.5 } } else { loud };
                    let t = Instant::now();
                    scene.step(1. / 20., pointer, sound);
                    step += t.elapsed();
                    let t = Instant::now();
                    scene.draw(310, 96, 1.75, &mut out);
                    draw += t.elapsed();
                }
                let ms = |d: Duration| d.as_secs_f64() * 1000. / frames as f64;
                eprintln!("COST {style:?} cursor {} step {:.3} ms draw {:.3} ms cells {}", pointer.is_some(), ms(step), ms(draw), out.len());
            }
        }
    }
    #[test]
    fn fused_handover_keeps_every_cell_and_fingerprint_exact() {
        for (cols, rows) in [(304, 72), (936, 279), (61, 23), (0, 23)] {
            for style in [Style::City, Style::Circuit, Style::Vector, Style::Sunset] {
                let mut scene = Scene::new(style);
                scene.serial = 0x5eed;
                let (mut cells, mut fused) = (vec![], vec![]);
                for frame in 0..120 {
                    let f = frame as f32;
                    let level = if frame % 60 < 30 { 0. } else { 0.8 };
                    scene.step(0.05, Some((0.5 + (f * 0.03).sin() * 0.45, 0.4)), Sound { bass: level, treble: level * 0.8 });
                    scene.draw(cols, rows, 2.17, &mut cells);
                    let print = scene.draw_with_print(cols, rows, 2.17, &mut fused);
                    assert_eq!(cells, fused, "{style:?} {cols}x{rows} frame{frame}");
                    assert_eq!(frame_print(&cells), print, "{style:?} {cols}x{rows} frame{frame}");
                }
            }
        }
    }
    #[test]
    fn no_neon_scene_shadows_a_showcase() {
        // The neon names are matched first: a showcase that shares one could
        // never be started.
        for name in crate::showcase_scenes::Kind::ALL {
            assert!(matches!(Style::parse(name), Some(Style::Showcase(_))), "{name} is taken by a neon scene");
        }
    }
    #[test]
    fn showcase_handover_retains_the_complete_frame_fingerprint() {
        for name in crate::showcase_scenes::Kind::ALL {
            let mut scene = Scene::new(Style::parse(name).unwrap());
            let (mut ordinary, mut fused) = (vec![], vec![]);
            for frame in 0..30 {
                scene.step(0.05, Some((0.4, 0.5)), Sound { bass: (frame % 2) as f32 * 0.8, treble: 0.4 });
                scene.draw(61, 23, 2.17, &mut ordinary);
                let print = scene.draw_with_print(61, 23, 2.17, &mut fused);
                assert_eq!(ordinary, fused, "{name} frame{frame}");
                assert_eq!(frame_print(&ordinary), print, "{name} frame{frame}");
            }
        }
    }
    #[test]
    #[ignore = "deterministic fused print cost, not physical efficiency evidence"]
    fn ascii_fused_print_cost() {
        for (cols, rows) in [(304, 72), (936, 279)] {
            for style in [Style::City, Style::Circuit] {
                for fused in [false, true, true, false] {
                    let mut scene = Scene::new(style);
                    scene.serial = 0x5eed;
                    let mut out = vec![];
                    let mut cost = Duration::ZERO;
                    for frame in 0..210 {
                        scene.step(0.05, Some((0.4, 0.5)), Sound::default());
                        let begin = Instant::now();
                        let print = if fused { scene.draw_with_print(cols, rows, 2.17, &mut out) }
                            else { scene.draw(cols, rows, 2.17, &mut out); frame_print(&out) };
                        let end = Instant::now();
                        std::hint::black_box(print);
                        if frame >= 60 { cost += end - begin; }
                    }
                    eprintln!("ASCII_FUSED {style:?} {cols}x{rows} fused={fused} total={:.4}ms", cost.as_secs_f64() * 1000. / 150.);
                }
            }
        }
    }
    #[test]
    fn shared_terrain_heights_leave_every_scene_cell_exact() {
        for (cols, rows) in [(304, 72), (936, 279), (61, 23)] {
            let mut fast = Scene::new(Style::Circuit);
            let mut slow = Scene::new(Style::Circuit);
            fast.serial = 0x5eed;
            slow.serial = fast.serial;
            slow.exhaustive = true;
            let (mut a, mut b) = (vec![], vec![]);
            for frame in 0..360 {
                let f = frame as f32;
                let pointer = Some((0.5 + (f * 0.071).sin() * 0.45, 0.5 + (f * 0.033).cos() * 0.45));
                let level = if frame % 120 < 60 { 0. } else { 0.7 };
                let sound = Sound { bass: level, treble: level * 0.8 };
                let dt = [0.05, 0.033, 0.1, 0.0083][frame % 4];
                fast.step(dt, pointer, sound);
                slow.step(dt, pointer, sound);
                if frame % 30 == 0 {
                    let travel = [0., 0.001, 2.9999, 3., 6143.9995, 6144.][frame / 30 % 6];
                    fast.travel = travel;
                    slow.travel = travel;
                }
                fast.draw(cols, rows, 2.17, &mut a);
                slow.draw(cols, rows, 2.17, &mut b);
                assert_eq!(a, b, "{cols}x{rows} frame {frame}");
            }
        }
    }
    #[test]
    #[ignore = "deterministic scene-only shortcut costs; not a hardware efficiency claim"]
    fn ascii_producer_shortcuts_frame_cost() {
        for (cols, rows) in [(304, 72), (936, 279)] {
            for style in [Style::City, Style::Circuit] {
                for exhaustive in [true, false] {
                    let mut scene = Scene::new(style);
                    scene.serial = 0x5eed;
                    scene.exhaustive = exhaustive;
                    let mut out = vec![];
                    let (mut draw, mut print) = (Duration::ZERO, Duration::ZERO);
                    let frames = 150;
                    for frame in 0..frames + 60 {
                        scene.step(0.05, Some((0.4, 0.5)), Sound::default());
                        let begin = Instant::now();
                        scene.draw(cols, rows, 2.17, &mut out);
                        let drawn = Instant::now();
                        std::hint::black_box(frame_print(&out));
                        let printed = Instant::now();
                        if frame >= 60 { draw += drawn - begin; print += printed - drawn; }
                    }
                    let ms = |d: Duration| d.as_secs_f64() * 1000. / frames as f64;
                    eprintln!("ASCII_SHORTCUT {style:?} {cols}x{rows} exhaustive={exhaustive} draw={:.4}ms print={:.4}ms", ms(draw), ms(print));
                }
            }
        }
    }
    #[test]
    fn lunar_foreground_stays_contiguous_through_every_scroll_phase() {
        for tick in 0..300 {
            let phase = tick as f32 / 100.;
            let mut edge = 0.8;
            for n in -1..28 {
                if let Some((near, far, t)) = lunar_visible_strip(1. + n as f32 * 3. - phase) {
                    assert!((near - edge).abs() < 0.0001);
                    assert!((0. ..=1.).contains(&t));
                    edge = far;
                }
            }
            for x in [-0.4, 0., 0.4] {
                assert!(lunar_ground(x, tick as f32) + 1.3 > 2.2);
            }
        }
    }
    #[test]
    fn births_are_hidden_and_placement_varies_before_reveal() {
        let mut s = Scene::new(Style::Sunset);
        s.serial = 123;
        for _ in 0..20 {
            s.step(
                0.05,
                None,
                Sound {
                    bass: 1.,
                    treble: 1.,
                },
            );
        }
        assert!(s.accents.len() > 1);
        assert!(s
            .accents
            .iter()
            .all(|a| arrival_visibility(a.z, 64., 44.) == 0.));
        assert!(s
            .accents
            .windows(2)
            .any(|a| (a[0].offset - a[1].offset).abs() > 0.1));
        assert_eq!(arrival_visibility(64., 64., 44.), 0.);
        assert!(arrival_visibility(63.9, 64., 44.) < 0.001);
        assert_eq!(arrival_visibility(44., 64., 44.), 1.);
    }
    #[test]
    fn musical_objects_persist_then_expire_and_population_is_bounded() {
        for style in [Style::City, Style::Vector, Style::Circuit, Style::Sunset] {
            let mut s = Scene::new(style);
            for _ in 0..2000 {
                s.step(
                    0.05,
                    None,
                    Sound {
                        bass: 1.,
                        treble: 1.,
                    },
                );
                assert!(s.accents.len() <= 32);
            }
            assert!(s.accents.len() > 5);
            let old_z = s.accents[0].z;
            s.step(0.05, None, Sound::default());
            if old_z > 1.2 {
                assert!((s.accents[0].z - (old_z - 0.3)).abs() < 0.001);
            }
            for _ in 0..400 {
                s.step(0.05, None, Sound::default());
            }
            assert!(s.accents.is_empty());
        }
    }
    #[test]
    fn sloped_faces_occlude_by_local_depth_not_average_depth() {
        let mut c = Canvas {
            r: Raster::new(174, 50),
            cols: 174,
            rows: 50,
            aspect: 1.75,
            gaze: (0., 0.),
            style: Style::Vector,
            solid_body: false,
            aim: None,
            focus: None,
        };
        let world = |sx: f32, sy: f32, z: f32| {
            (
                (sx - 87.) * z / (50. * 0.78 * 1.75),
                (sy - 21.5) * z / (50. * 0.78),
                z,
            )
        };
        c.r.dot(25, 20, 5., '@', (255, 0, 0));
        c.r.dot(95, 20, 5., '@', (255, 0, 0));
        c.face(
            [
                world(20., 10., 2.),
                world(100., 10., 20.),
                world(100., 30., 20.),
                world(20., 30., 2.),
            ],
            (0, 0, 255),
        );
        let mut out = vec![];
        c.r.finish(&mut out);
        assert_eq!(
            out.iter().find(|v| v.x == 25 && v.y == 20).unwrap().color,
            (0, 0, 255)
        );
        assert_eq!(
            out.iter().find(|v| v.x == 95 && v.y == 20).unwrap().color,
            (255, 0, 0)
        );
    }
    #[test]
    fn audio_changes_visible_colors_without_relying_on_camera_or_speed() {
        for style in [Style::City, Style::Vector, Style::Circuit, Style::Sunset] {
            let quiet = Scene::new(style);
            let mut music = Scene::new(style);
            // Modest input: the old multiplier was barely visible at this level.
            for _ in 0..30 {
                music.step(
                    0.02,
                    None,
                    Sound {
                        bass: 0.18,
                        treble: 0.12,
                    },
                );
            }
            music.travel = quiet.travel;
            music.time = quiet.time;
            music.accents.clear();
            music.ripples.clear();
            let (mut a, mut b) = (vec![], vec![]);
            quiet.draw(174, 50, 1.75, &mut a);
            music.draw(174, 50, 1.75, &mut b);
            // Grass sways and exhaust drifts with the music, so compare by
            // position rather than by index: colours must change broadly.
            let by_position = |cells: &[DirectCell]| {
                cells
                    .iter()
                    .map(|c| ((c.x, c.y), (c.ch, c.color)))
                    .collect::<std::collections::HashMap<_, _>>()
            };
            let (pa, pb) = (by_position(&a), by_position(&b));
            let changed = pa.iter().filter(|(k, v)| pb.get(k) != Some(v)).count();
            if style == Style::Circuit {
                // Moonlight is constant; sound populates crystals instead.
                assert_eq!(a.len(), b.len());
                assert_eq!(changed, 0);
                assert!(
                    a.iter()
                        .filter(|c| c.y > 30 && c.color.1.max(c.color.2) > 70)
                        .count()
                        > 100
                );
            } else {
                assert!(changed > a.len() / 4, "{style:?}: {changed}/{}", a.len());
            }
            for _ in 0..200 {
                music.step(0.02, None, Sound::default());
            }
            assert!(music.sound.bass < 0.001 && music.sound.treble < 0.001);
        }
    }
    #[test]
    fn sunset_sky_uses_the_same_horizon_as_the_road() {
        let mut c = Canvas {
            r: Raster::new(174, 50),
            cols: 174,
            rows: 50,
            aspect: 1.75,
            gaze: (0., -1.),
            style: Style::Sunset,
            solid_body: false,
            aim: None,
            focus: None,
        };
        let sky_a = c.horizon_y() - c.rows as f32 * 0.18;
        let road_a = c.project((0., 0., 80.)).1;
        c.gaze.1 = 1.;
        let sky_b = c.horizon_y() - c.rows as f32 * 0.18;
        let road_b = c.project((0., 0., 80.)).1;
        assert!((sky_b - sky_a - (road_b - road_a)).abs() < 0.0001);
        assert!(sky_b > sky_a);
    }
    #[test]
    #[ignore = "manual bounded playback diagnostic; reports levels only, no recording"]
    fn actual_scene_audio_levels() {
        let audio = crate::AudioState::start(true);
        let mut max = Sound::default();
        let mut sum = Sound::default();
        for _ in 0..150 {
            std::thread::sleep(Duration::from_millis(20));
            let s = Sound::sample(&audio);
            max.bass = max.bass.max(s.bass);
            max.treble = max.treble.max(s.treble);
            sum.bass += s.bass;
            sum.treble += s.treble;
        }
        eprintln!(
            "SCENE_AUDIO mean_bass={} mean_treble={} max_bass={} max_treble={}",
            sum.bass / 150.,
            sum.treble / 150.,
            max.bass,
            max.treble
        );
    }
    #[test]
    fn all_scenes_move_and_stay_unique_in_bounds() {
        for style in [Style::City, Style::Vector, Style::Circuit, Style::Sunset] {
            for (cols, rows) in [(1, 1), (17, 13), (174, 50), (312, 93)] {
                let mut s = Scene::new(style);
                let (mut a, mut b) = (vec![], vec![]);
                s.draw(cols, rows, 1.75, &mut a);
                for _ in 0..60 {
                    s.step(
                        0.02,
                        Some((0.9, 0.1)),
                        Sound {
                            bass: 0.7,
                            treble: 0.5,
                        },
                    );
                }
                s.draw(cols, rows, 1.75, &mut b);
                assert!(b
                    .iter()
                    .all(|c| c.x < cols && c.y < rows && !matches!(c.ch, ',' | ';' | '\n')));
                let keys = b
                    .iter()
                    .map(|c| (c.x, c.y))
                    .collect::<std::collections::HashSet<_>>();
                assert_eq!(keys.len(), b.len());
                if cols > 17 {
                    assert_ne!(a, b);
                    assert!(b.len() > 100);
                }
            }
        }
    }
    #[test]
    fn map_elements_come_into_focus_and_beats_ring_out() {
        let quiet = Sound { bass: 0., treble: 0. };
        // The cursor's own mark is one corner; a focus brings another.
        let named = |cells: &[DirectCell]| cells.iter().filter(|c| c.ch == '┌').count() >= 2;
        for style in [Style::City, Style::Vector, Style::Circuit, Style::Sunset] {
            if style != Style::City {
                // Somewhere on a grid of cursor spots, something is named.
                let mut seen = 0;
                for (px, py) in [(0.3, 0.75), (0.5, 0.6), (0.7, 0.8), (0.5, 0.25), (0.68, 0.2), (0.2, 0.5)] {
                    let mut s = Scene::new(style);
                    for _ in 0..80 {
                        s.step(0.02, Some((px, py)), quiet);
                    }
                    let mut cells = vec![];
                    s.draw(200, 60, 2., &mut cells);
                    assert!(cells.iter().any(|c| c.ch == '┌'), "{style:?}: the cursor is marked");
                    seen += named(&cells) as usize;
                    s.cursor = false;
                    s.draw(200, 60, 2., &mut cells);
                    assert!(!cells.iter().any(|c| c.ch == '┌'), "{style:?}: no cursor, no focus");
                }
                assert!(seen >= 3, "{style:?}: only {seen} spots focused something");
            }
            let mut s = Scene::new(style);
            for i in 0..40 {
                let bass = if i % 10 < 2 { 0.9 } else { 0. };
                s.step(0.02, None, Sound { bass, treble: 0. });
            }
            assert!(!s.ripples.is_empty(), "{style:?}: a beat rings out");
        }
    }
    #[test]
    fn the_tower_under_the_cursor_wakes_and_shows_its_name() {
        let quiet = Sound { bass: 0., treble: 0. };
        let mut found = false;
        // The cursor down the left side, over the towers there.
        for y in [0.35, 0.45, 0.55] {
            let mut s = Scene::new(Style::City);
            for _ in 0..80 {
                s.step(0.02, Some((0.12, y)), quiet);
            }
            let mut cells = vec![];
            s.draw(240, 80, 2., &mut cells);
            found |= cells.iter().any(|c| matches!(c.ch, '▀' | '▄'));
            s.cursor = false;
            s.draw(240, 80, 2., &mut cells);
            assert!(!cells.iter().any(|c| matches!(c.ch, '▀' | '▄')), "no cursor, no name");
        }
        assert!(found, "a tower under the cursor shows its name");
    }
    #[test]
    fn distant_road_does_not_overwrite_near_vehicle() {
        let mut c = Canvas {
            r: Raster::new(174, 50),
            cols: 174,
            rows: 50,
            aspect: 1.75,
            gaze: (0., 0.),
            style: Style::Sunset,
            solid_body: false,
            aim: None,
            focus: None,
        };
        let p = c.project((1., 0., 40.));
        let (x, y) = (p.0.round() as i32, p.1.round() as i32);
        c.r.dot(x, y, 5., '@', (255, 255, 255));
        c.line((1., 0., 1.), (1., 0., 80.), (255, 0, 0), None);
        let mut cells = vec![];
        c.r.finish(&mut cells);
        assert_eq!(
            cells
                .iter()
                .find(|c| c.x == x as usize && c.y == y as usize)
                .unwrap()
                .ch,
            '@'
        );
    }
    #[test]
    fn scenes_are_distinct_and_audio_adds_objects_without_accelerating() {
        let mut frames = vec![];
        for style in [Style::City, Style::Vector, Style::Circuit, Style::Sunset] {
            let mut a = Scene::new(style);
            let mut b = Scene::new(style);
            for _ in 0..60 {
                a.step(0.02, None, Sound::default());
                b.step(
                    0.02,
                    None,
                    Sound {
                        bass: 1.,
                        treble: 1.,
                    },
                );
            }
            assert_eq!(a.travel, b.travel);
            assert!(a.accents.is_empty() && !b.accents.is_empty());
            let mut f = vec![];
            a.draw(174, 50, 1.75, &mut f);
            assert!(!frames.contains(&f));
            frames.push(f);
        }
    }
}
