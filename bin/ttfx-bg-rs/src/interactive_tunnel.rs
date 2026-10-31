//! Projected wireframe tunnel: geometry, depth-tested ASCII and spectral light.
use crate::{
    asm_direct_bridge::DirectCell,
    frame_wakeup::FrameWakeup,
    interactive_matrix::{PointerState, Sound},
};
use std::{
    sync::{
        atomic::{AtomicBool, AtomicU64, Ordering},
        Arc, Mutex,
    },
    time::{Duration, Instant},
};

pub(crate) struct Tunnel {
    time: f32,
    travel: f32,
    gaze: (f32, f32),
    sound: Sound,
    sparks: Vec<(f32, f32, f32)>,
    spawn: f32,
    serial: u32,
}
impl Tunnel {
    pub(crate) fn new() -> Self {
        Self {
            time: 0.,
            travel: 0.,
            gaze: (0., 0.),
            sound: Sound::default(),
            sparks: vec![],
            spawn: 0.,
            serial: rand::random(),
        }
    }
    pub(crate) fn step(&mut self, dt: f32, pointer: Option<(f32, f32)>, sound: Sound) {
        let dt = dt.clamp(0., 0.1);
        self.time = (self.time + dt) % 628.;
        self.sound.follow(sound, dt);
        self.travel = (self.travel + dt * 3.8) % 1000.;
        for p in &mut self.sparks {
            p.2 -= dt * 9.;
        }
        self.sparks.retain(|p| p.2 > 0.8);
        let drive = 1. - (-3. * (self.sound.bass + self.sound.treble)).exp();
        self.spawn += dt * drive * 7.;
        while self.spawn >= 1. {
            self.spawn -= 1.;
            if self.sparks.len() < 40 {
                let h = self.serial.wrapping_mul(747796405).wrapping_add(2891336453);
                self.sparks.push((
                    if h & 1 == 0 { -3.12 } else { 3.12 },
                    ((h >> 8) & 65535) as f32 / 65535. * 3.4 - 1.7,
                    64. + ((h >> 24) & 255) as f32 / 255. * 8.,
                ));
            }
            self.serial = self.serial.wrapping_add(1);
        }
        let target = pointer
            .map(|(x, y)| ((x - 0.5) * 2., (y - 0.5) * 2.))
            .unwrap_or((0., 0.));
        let k = 1. - (-dt * 5.).exp();
        self.gaze.0 += (target.0.clamp(-1., 1.) - self.gaze.0) * k;
        self.gaze.1 += (target.1.clamp(-1., 1.) - self.gaze.1) * k;
    }
    fn project(&self, x: f32, y: f32, z: f32, cols: usize, rows: usize, aspect: f32) -> (f32, f32) {
        let bend_x = (z * 0.075 + self.time * 0.23).sin() * z * 0.12;
        let bend_y = (z * 0.052 + self.time * 0.17).cos() * z * 0.055;
        let roll = (z * 0.027 + self.time * 0.13).sin() * 0.24 + self.gaze.0 * 0.08;
        let (s, c) = roll.sin_cos();
        let xx = x * c - y * s;
        let yy = x * s + y * c;
        let f = rows as f32 * 0.88;
        (
            cols as f32 * 0.5 + (xx + bend_x - self.gaze.0 * 0.75) * f * aspect / z,
            rows as f32 * 0.5 + (yy + bend_y - self.gaze.1 * 0.45) * f / z,
        )
    }
    pub(crate) fn draw(&self, cols: usize, rows: usize, aspect: f32, cells: &mut Vec<DirectCell>) {
        cells.clear();
        if cols == 0 || rows == 0 {
            return;
        }
        let mut raster = Raster {
            cols,
            rows,
            depth: vec![f32::INFINITY; cols * rows],
            cells: vec![None; cols * rows],
        };
        let spacing = 1.8;
        let phase = self.travel % spacing;
        // More than 20 true perspective cross-sections, connected by longitudinal rails.
        for n in (0..24).rev() {
            let z = 0.65 + n as f32 * spacing - phase;
            if z < 0.65 {
                continue;
            }
            let next = z + spacing;
            let pulse = (1. - ((z - (self.time * 12.) % 42.).abs() / 3.).min(1.)).max(0.)
                * self.sound.treble;
            for side in 0..4 {
                let edge = |t: f32| match side {
                    0 => (-3.2 + 6.4 * t, -2.2),
                    1 => (3.2, -2.2 + 4.4 * t),
                    2 => (3.2 - 6.4 * t, 2.2),
                    _ => (-3.2, 2.2 - 4.4 * t),
                };
                let color = if side == 0 || side == 2 {
                    (30., 210., 255.)
                } else {
                    (185., 55., 255.)
                };
                let intensity = (0.18 + 0.82 / (1. + z * 0.065) + pulse * 0.65).min(1.4);
                let rgb = |gain: f32| {
                    (
                        (color.0 * gain).min(255.) as u8,
                        (color.1 * gain).min(255.) as u8,
                        (color.2 * gain).min(255.) as u8,
                    )
                };
                let (a, b) = (edge(0.), edge(1.));
                raster.line(
                    self.project(a.0, a.1, z, cols, rows, aspect),
                    self.project(b.0, b.1, z, cols, rows, aspect),
                    z,
                    rgb(intensity),
                    true,
                );
                for lane in 0..=4 {
                    let p = edge(lane as f32 / 4.);
                    let gain = if lane == 0 || lane == 4 {
                        intensity
                    } else {
                        intensity * 0.32
                    };
                    raster.line(
                        self.project(p.0, p.1, z, cols, rows, aspect),
                        self.project(p.0, p.1, next, cols, rows, aspect),
                        z,
                        rgb(gain),
                        false,
                    );
                }
            }
        }
        // Ceiling light strips along the bore blink in step with the bass.
        for n in 0..12 {
            let z = 1. + (n as f32 * 3.6 - self.travel).rem_euclid(43.);
            let on =
                0.35 + self.sound.bass * 0.65 * (0.5 + 0.5 * (self.time * 6. + n as f32).sin());
            let project = |x, y, z| self.project(x, y, z, cols, rows, aspect);
            raster.line_with_glyph(
                project(-0.6, -2.15, z),
                project(0.6, -2.15, z),
                z - 0.01,
                ((255. * on) as u8, (240. * on) as u8, (200. * on) as u8),
                true,
                Some('▬'),
            );
        }
        // Dust streaks past the camera give the bore a sense of speed.
        for i in 0..28 {
            let h = (i as u32).wrapping_mul(2654435761);
            let x = ((h >> 4) & 1023) as f32 / 1023. * 5.6 - 2.8;
            let y = ((h >> 14) & 1023) as f32 / 1023. * 3.6 - 1.8;
            let z =
                0.9 + (((h >> 24) & 255) as f32 / 255. * 30. - self.travel * 2.2).rem_euclid(30.);
            let project = |x, y, z| self.project(x, y, z, cols, rows, aspect);
            raster.line_with_glyph(
                project(x, y, z),
                project(x, y, z + 0.6),
                z,
                (70, 80, 110),
                false,
                Some('.'),
            );
        }
        // Fast amber couriers: moving depth, no screen-space confetti.
        for i in 0..6 {
            let z = 0.8 + (i as f32 * 2.31 - self.travel * 1.7).rem_euclid(40.);
            let x = if i % 2 == 0 { -3.17 } else { 3.17 };
            let y = (i % 5) as f32 * 0.9 - 1.8;
            raster.line(
                self.project(x, y, z, cols, rows, aspect),
                self.project(x, y, z + 0.7, cols, rows, aspect),
                z,
                (255, 145 + (self.sound.treble * 100.) as u8, 45),
                false,
            );
        }
        // Musical packets are born far away and persist through an approach.
        for &(x, y, z) in &self.sparks {
            let fade = crate::neon_scenes::arrival_visibility(z, 42., 28.);
            if fade == 0. {
                continue;
            }
            let light = |c: (u8, u8, u8)| {
                (
                    (c.0 as f32 * fade) as u8,
                    (c.1 as f32 * fade) as u8,
                    (c.2 as f32 * fade) as u8,
                )
            };
            let project = |x, y, z| self.project(x, y, z, cols, rows, aspect);
            for dy in [-0.12, 0.12] {
                raster.line_with_glyph(
                    project(x, y + dy, z),
                    project(x, y + dy, z + 1.3),
                    z,
                    light((255, 200, 80)),
                    true,
                    Some('▬'),
                );
            }
            raster.line(
                project(x, y - 0.12, z),
                project(x, y + 0.12, z),
                z - 0.01,
                light((245, 250, 255)),
                true,
            );
        }
        // Amber navigation chevrons on the floor give a second, grounded speed cue.
        for n in 0..10 {
            let z = 1.2 + (n as f32 * 4.2 - self.travel).rem_euclid(42.);
            let project = |x, y, z| self.project(x, y, z, cols, rows, aspect);
            for sign in [-1., 1.] {
                raster.line(
                    project(sign * 0.55, 2.17, z + 0.9),
                    project(0., 2.17, z),
                    z,
                    (255, 175, 50),
                    true,
                );
            }
        }
        // Light bikes overtake the camera: their depth INCREASES, unlike scenery.
        for bike in 0..3 {
            let age = (self.time + bike as f32 * 3.6) % 12.;
            if age > 9.5 {
                continue;
            }
            let z = 0.7 + age * 3.8;
            let lane = if bike % 2 == 0 { -1.65 } else { 1.65 };
            let color = if bike == 1 {
                (255, 150, 25)
            } else {
                (65, 245, 255)
            };
            let project = |x, y, z| self.project(x, y, z, cols, rows, aspect);
            // Rear-facing hoop wheels, front/back separation and a low chassis.
            for wheel_z in [z, z + 1.15] {
                for segment in 0..16 {
                    let a = segment as f32 * std::f32::consts::TAU / 16.;
                    let b = (segment + 1) as f32 * std::f32::consts::TAU / 16.;
                    raster.line(
                        project(lane + a.cos() * 0.25, 1.83 + a.sin() * 0.34, wheel_z),
                        project(lane + b.cos() * 0.25, 1.83 + b.sin() * 0.34, wheel_z),
                        wheel_z - 0.02,
                        color,
                        true,
                    );
                }
            }
            for side in [-0.26, 0.26] {
                raster.line_with_glyph(
                    project(lane + side, 1.65, z),
                    project(lane + side, 1.44, z + 1.2),
                    z - 0.04,
                    color,
                    true,
                    Some('▬'),
                );
                raster.line(
                    project(lane + side, 1.44, z + 1.2),
                    project(lane, 1.28, z + 0.55),
                    z - 0.05,
                    color,
                    true,
                );
            }
            // Trail remains behind the bike, clipped at the camera near plane.
            let tail = (z - 14.).max(0.65);
            raster.line(
                project(lane, 2.12, tail),
                project(lane, 2.12, z),
                tail,
                (color.0 / 2, color.1 / 2, color.2 / 2),
                true,
            );
            raster.line_with_glyph(
                project(lane, 1.95, tail),
                project(lane, 1.95, z),
                tail,
                color,
                false,
                Some('▌'),
            );
        }
        cells.extend(raster.cells.into_iter().flatten());
    }
}
pub(crate) struct Raster {
    cols: usize,
    rows: usize,
    depth: Vec<f32>,
    cells: Vec<Option<DirectCell>>,
}
impl Raster {
    pub(crate) fn new(cols: usize, rows: usize) -> Self {
        Self {
            cols,
            rows,
            depth: vec![f32::INFINITY; cols * rows],
            cells: vec![None; cols * rows],
        }
    }
    pub(crate) fn fits(&self, cols: usize, rows: usize) -> bool {
        self.cols == cols && self.rows == rows
    }
    /// Hand over the frame and leave the buffers ready for the next one.
    pub(crate) fn recycle(&mut self, out: &mut Vec<DirectCell>) {
        out.clear();
        out.extend(self.cells.iter_mut().filter_map(Option::take));
        self.depth.fill(f32::INFINITY);
    }
    pub(crate) fn finish(self, out: &mut Vec<DirectCell>) {
        out.clear();
        out.extend(self.cells.into_iter().flatten());
    }
    pub(crate) fn dot(&mut self, x: i32, y: i32, z: f32, ch: char, color: (u8, u8, u8)) {
        if x < 0 || y < 0 || x as usize >= self.cols || y as usize >= self.rows {
            return;
        }
        let i = y as usize * self.cols + x as usize;
        if z < self.depth[i] {
            self.depth[i] = z;
            self.cells[i] = Some(DirectCell {
                x: x as usize,
                y: y as usize,
                ch,
                color,
                background: None,
                attrs: 1,
            });
        }
    }

    pub(crate) fn line(
        &mut self,
        a: (f32, f32),
        b: (f32, f32),
        z: f32,
        color: (u8, u8, u8),
        ring: bool,
    ) {
        self.line_with_glyph(a, b, z, color, ring, None);
    }
    pub(crate) fn line_with_glyph(
        &mut self,
        a: (f32, f32),
        b: (f32, f32),
        z: f32,
        color: (u8, u8, u8),
        ring: bool,
        glyph: Option<char>,
    ) {
        if ![a.0, a.1, b.0, b.1].iter().all(|v| v.is_finite()) {
            return;
        }
        // Clip before walking: near geometry can project far beyond the viewport.
        let (dx, dy) = (b.0 - a.0, b.1 - a.1);
        let (mut lo, mut hi) = (0f32, 1f32);
        for (p, q) in [
            (-dx, a.0),
            (dx, self.cols as f32 - 1. - a.0),
            (-dy, a.1),
            (dy, self.rows as f32 - 1. - a.1),
        ] {
            if p == 0. {
                if q < 0. {
                    return;
                }
            } else {
                let t = q / p;
                if p < 0. {
                    lo = lo.max(t);
                } else {
                    hi = hi.min(t);
                }
            }
        }
        if lo > hi {
            return;
        }
        let start = (a.0 + dx * lo, a.1 + dy * lo);
        let end = (a.0 + dx * hi, a.1 + dy * hi);
        let steps = ((end.0 - start.0).abs().max((end.1 - start.1).abs()).ceil() as usize).max(1);
        let ch = if ring {
            if dy.abs() < dx.abs() * 0.35 {
                '='
            } else if dx.abs() < dy.abs() * 0.35 {
                '|'
            } else if dx * dy > 0. {
                '\\'
            } else {
                '/'
            }
        } else {
            '.'
        };
        let ch = glyph.unwrap_or(ch);
        for i in 0..=steps {
            let t = i as f32 / steps as f32;
            let x = (start.0 + (end.0 - start.0) * t)
                .round()
                .clamp(0., self.cols as f32 - 1.) as usize;
            let y = (start.1 + (end.1 - start.1) * t)
                .round()
                .clamp(0., self.rows as f32 - 1.) as usize;
            let index = y * self.cols + x;
            if z < self.depth[index] {
                self.depth[index] = z;
                self.cells[index] = Some(DirectCell {
                    x,
                    y,
                    ch,
                    color,
                    background: None,
                    attrs: 1,
                });
            }
        }
    }
}
pub fn run(
    cols: usize,
    rows: usize,
    aspect: f32,
    pointer: Arc<PointerState>,
    snapshot: Arc<Mutex<Vec<DirectCell>>>,
    generation: Arc<AtomicU64>,
    stop: Arc<AtomicBool>,
    wakeup: Arc<FrameWakeup>,
) -> anyhow::Result<()> {
    let sound = crate::AudioState::start(true);
    let mut tunnel = Tunnel::new();
    let mut cells = vec![];
    let mut last = Instant::now();
    let mut seen = false;
    while !stop.load(Ordering::Acquire) {
        let begin = Instant::now();
        let dt = (begin - last).as_secs_f32();
        last = begin;
        crate::fps_governor::note_tick();
        let input = Sound::sample(&sound);
        if !seen && (input.bass > 0.03 || input.treble > 0.03) {
            eprintln!("ASCII tunnel: spectral audio received");
            seen = true;
        }
        tunnel.step(dt, pointer.sample(), input);
        tunnel.draw(cols, rows, aspect, &mut cells);
        {
            let mut slot = snapshot.lock().unwrap();
            if stop.load(Ordering::Acquire) {
                break;
            }
            std::mem::swap(&mut *slot, &mut cells);
            generation.fetch_add(1, Ordering::Release);
            wakeup.notify()?;
        }
        let period = Duration::from_secs_f64(1. / crate::max_fps().unwrap_or(60.).clamp(1., 60.));
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
    fn bounded_unique_ascii_geometry_and_motion() {
        for (cols, rows) in [(1, 1), (17, 13), (174, 50), (310, 96)] {
            let mut t = Tunnel::new();
            let (mut a, mut b) = (vec![], vec![]);
            t.draw(cols, rows, 1.75, &mut a);
            for _ in 0..40 {
                t.step(0.016, Some((1., 0.)), Sound::default());
            }
            t.draw(cols, rows, 1.75, &mut b);
            assert!(b.iter().all(|c| c.x < cols
                && c.y < rows
                && (c.ch.is_ascii() || matches!(c.ch, '▬' | '▌'))
                && c.background.is_none()));
            let mut keys = b.iter().map(|c| (c.x, c.y)).collect::<Vec<_>>();
            keys.sort();
            keys.dedup();
            assert_eq!(keys.len(), b.len());
            if cols > 17 {
                assert_ne!(a, b);
                assert!(b.len() > 100);
            }
        }
    }
    #[test]
    fn audio_populates_without_speed_changes_and_returns() {
        let (mut a, mut b) = (Tunnel::new(), Tunnel::new());
        for _ in 0..30 {
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
        assert!(a.sparks.is_empty() && !b.sparks.is_empty());
        assert!(b.sound.treble > 0.9);
        for _ in 0..100 {
            b.step(0.05, None, Sound::default());
        }
        assert!(b.sound.bass < 0.001);
    }
    #[test]
    fn perspective_and_clipping_are_finite() {
        let t = Tunnel::new();
        let near = t.project(3.2, 0., 2., 174, 50, 1.75);
        let far = t.project(3.2, 0., 30., 174, 50, 1.75);
        assert!((near.0 - 87.).abs() > (far.0 - 87.).abs());
        let mut r = Raster {
            cols: 10,
            rows: 10,
            depth: vec![f32::INFINITY; 100],
            cells: vec![None; 100],
        };
        r.line((-1e9, 5.), (1e9, 5.), 1., (1, 2, 3), true);
        assert!(r.cells.iter().flatten().count() <= 10);
    }
}
