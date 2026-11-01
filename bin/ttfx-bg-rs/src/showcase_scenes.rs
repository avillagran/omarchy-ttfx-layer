//! Showcase backgrounds: a working terminal, a data globe, a fish tank, an
//! aquarium of jellyfish, a starship cockpit and clocks built from sand. All
//! share the cell raster, the
//! global cursor and the bass/treble pair; silence keeps a full animation.
use crate::{
    asm_direct_bridge::DirectCell, interactive_matrix::Sound, interactive_tunnel::Raster,
    neon_scenes::variant,
};
use std::f32::consts::{PI, TAU};
type Color = (u8, u8, u8);
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Kind {
    Terminal,
    Globe,
    Aquarium,
    Jellyfish,
    Cockpit,
    SandClock,
    ChladniClock,
    DunesClock,
    GalaxyClock,
}
impl Kind {
    pub(crate) fn parse(name: &str) -> Option<Self> {
        Some(match name {
            "live-terminal" => Self::Terminal,
            "data-globe" => Self::Globe,
            "aquarium" => Self::Aquarium,
            "jellyfish" => Self::Jellyfish,
            "cockpit" => Self::Cockpit,
            "sand-clock" => Self::SandClock,
            "sand-clock-chladni" => Self::ChladniClock,
            "sand-clock-dunes" => Self::DunesClock,
            "sand-clock-galaxy" => Self::GalaxyClock,
            _ => return None,
        })
    }
    #[cfg(test)]
    pub(crate) const ALL: [&'static str; 9] = [
        "live-terminal",
        "data-globe",
        "aquarium",
        "jellyfish",
        "cockpit",
        "sand-clock",
        "sand-clock-chladni",
        "sand-clock-dunes",
        "sand-clock-galaxy",
    ];
}
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
/// Cheap stateless noise in [0,1), continuous enough for terrain and clouds.
fn noise(x: f32, y: f32) -> f32 {
    let (xi, yi) = (x.floor(), y.floor());
    let (fx, fy) = (x - xi, y - yi);
    let h = |i: f32, j: f32| {
        variant((i as i32 as u32).wrapping_mul(73856093) ^ (j as i32 as u32).wrapping_mul(19349663))
    };
    let s = |t: f32| t * t * (3. - 2. * t);
    let a = h(xi, yi) + (h(xi + 1., yi) - h(xi, yi)) * s(fx);
    let b = h(xi, yi + 1.) + (h(xi + 1., yi + 1.) - h(xi, yi + 1.)) * s(fx);
    a + (b - a) * s(fy)
}
struct Rng(u32);
impl Rng {
    fn next(&mut self) -> f32 {
        self.0 = self.0.wrapping_mul(1664525).wrapping_add(1013904223);
        (self.0 >> 8) as f32 / 16777216.
    }
}
fn text(r: &mut Raster, x: i32, y: i32, z: f32, s: &str, color: Color) {
    for (i, ch) in s.chars().enumerate() {
        // The preview wire format reserves ',' and ';' as separators.
        let ch = match ch {
            ',' => '·',
            ';' => ':',
            c => c,
        };
        if ch != ' ' {
            r.dot(x + i as i32, y, z, ch, color);
        }
    }
}
// ------------------------------------------------------------------ terminal
struct Pane {
    lines: Vec<(String, Color)>,
}
impl Pane {
    fn push(&mut self, s: String, c: Color, keep: usize) {
        self.lines.push((s, c));
        if self.lines.len() > keep {
            self.lines.remove(0);
        }
    }
}
struct Terminal {
    main: Pane,
    log: Pane,
    script: usize,
    typed: f32,
    phase: f32,
    cpu: [f32; 8],
    mem: f32,
    wait: f32,
    rng: Rng,
}
const COMMANDS: [&str; 10] = [
    "omarchy-update",
    "cargo build --release",
    "omarchy-theme-set tokyo-night",
    "hyprctl monitors",
    "omarchy-pkg-add fs-uae",
    "git log --oneline -5",
    "omarchy-ambient start neon-city",
    "make -j8",
    "omarchy-screensaver",
    "neofetch",
];
const LOGS: [&str; 12] = [
    "hyprland: window mapped (ghostty)",
    "pipewire: stream started 48000 Hz",
    "omarchy-ambient: scene ready",
    "wireplumber: default sink set",
    "systemd: user session active",
    "omarchy-shell: plugin reloaded",
    "kernel: usb 2-1: new device",
    "omarchy-idle: activity detected",
    "walker: index refreshed",
    "waybar: module updated",
    "ghostty: font cache warm",
    "network: wlan0 roamed to ap-02",
];
impl Terminal {
    fn new() -> Self {
        Self {
            main: Pane { lines: vec![] },
            log: Pane { lines: vec![] },
            script: 0,
            typed: 0.,
            phase: 0.,
            cpu: [0.3; 8],
            mem: 0.42,
            wait: 0.,
            rng: Rng(rand::random()),
        }
    }
    fn output(&mut self, cmd: usize, rows: usize) {
        let keep = rows.saturating_sub(6).max(8);
        let green = (120, 230, 140);
        let white = (210, 220, 230);
        let dim = (110, 125, 140);
        let amber = (255, 190, 80);
        let cyan = (90, 210, 255);
        match cmd {
            0 => {
                for p in [
                    "hyprland 0.52.1-1",
                    "omarchy 3.4.0",
                    "linux 7.2.10",
                    "ghostty 1.3.0",
                ] {
                    self.main.push(format!("  upgrading {p}"), white, keep);
                }
                self.main.push("  :: done · 4 packages".into(), green, keep);
            }
            1 => {
                for c in ["ttfx-bg-rs", "omarchy-ambient-layer", "wl-layer"] {
                    self.main
                        .push(format!("   Compiling {c} v0.2.1"), green, keep);
                }
                self.main.push(
                    "    Finished release [optimized] in 21.4s".into(),
                    green,
                    keep,
                );
            }
            2 => self
                .main
                .push("  theme applied · 14 apps refreshed".into(), cyan, keep),
            3 => {
                self.main.push("Monitor eDP-1 (ID 0):".into(), white, keep);
                self.main
                    .push("    3456x2234@120 at 0x0 · scale 2.0".into(), dim, keep);
            }
            4 => {
                self.main
                    .push("resolving dependencies...".into(), dim, keep);
                self.main
                    .push("(1/1) installing fs-uae".into(), white, keep);
            }
            5 => {
                for (h, m) in [
                    ("9ec0155", "Remove tunnel ring gates"),
                    ("07b4121", "Light side facades"),
                    ("6e6adc5", "Rework neon scenes"),
                    ("92a05e3", "Raise lunar camera"),
                    ("b502f29", "Restore original neon ridges"),
                ] {
                    self.main.push(format!("{h} {m}"), amber, keep);
                }
            }
            6 => self.main.push(
                "  neon-city: audio reactive, layer bottom".into(),
                cyan,
                keep,
            ),
            7 => {
                for o in ["layer.o", "dismiss.o", "wayland-protocols.o"] {
                    self.main.push(format!("  CC  {o}"), dim, keep);
                }
                self.main
                    .push("  LD  omarchy-ambient-layer.so".into(), white, keep);
            }
            8 => self
                .main
                .push("  screensaver: dismissed by activity".into(), dim, keep),
            _ => {
                for l in [
                    "  ██████╗ ███╗   ███╗ █████╗ ██████╗  ██████╗██╗  ██╗██╗   ██╗",
                    " ██╔═══██╗████╗ ████║██╔══██╗██╔══██╗██╔════╝██║  ██║╚██╗ ██╔╝",
                    " ██║   ██║██╔████╔██║███████║██████╔╝██║     ███████║ ╚████╔╝ ",
                    " ██║   ██║██║╚██╔╝██║██╔══██║██╔══██╗██║     ██╔══██║  ╚██╔╝  ",
                    " ╚██████╔╝██║ ╚═╝ ██║██║  ██║██║  ██║╚██████╗██║  ██║   ██║   ",
                    "  ╚═════╝ ╚═╝     ╚═╝╚═╝  ╚═╝╚═╝  ╚═╝ ╚═════╝╚═╝  ╚═╝   ╚═╝   ",
                ] {
                    self.main.push(l.into(), cyan, keep);
                }
                self.main.push(
                    "  OS: Omarchy Linux  WM: Hyprland  Shell: omarchy-shell".into(),
                    white,
                    keep,
                );
            }
        }
    }
    fn step(&mut self, dt: f32, sound: &Sound, rows: usize) {
        self.phase += dt;
        let cmd = COMMANDS[self.script % COMMANDS.len()];
        if self.wait > 0. {
            self.wait -= dt;
        } else if self.typed < cmd.len() as f32 {
            // Typing speed follows the treble, never stalls in silence.
            self.typed += dt * (9. + sound.treble * 30.);
            if self.typed >= cmd.len() as f32 {
                self.wait = 0.5;
            }
        } else {
            let keep = rows.saturating_sub(6).max(8);
            self.main.push(format!("❯ {cmd}"), (235, 240, 245), keep);
            self.output(self.script % COMMANDS.len(), rows);
            self.script += 1;
            self.typed = 0.;
            self.wait = 1.2 + self.rng.next() * 1.5;
        }
        // Real load where available; otherwise a believable idle wobble.
        if (self.phase * 2.).fract() < dt * 2. {
            let n = self.rng.next();
            let msg = LOGS[(n * LOGS.len() as f32) as usize % LOGS.len()];
            self.log.push(
                format!("{:>6.1}  {msg}", self.phase),
                (120, 140, 160),
                rows / 2,
            );
        }
        for (i, c) in self.cpu.iter_mut().enumerate() {
            let target =
                0.15 + noise(self.phase * 0.7 + i as f32 * 3.1, i as f32) * 0.5 + sound.bass * 0.4;
            *c += (target - *c) * (1. - (-dt * 4.).exp());
        }
        // Once per second: this is a real file read, not worth doing per frame.
        let tick = self.phase as u32 != (self.phase - dt).max(0.) as u32;
        if let Some(meminfo) = tick
            .then(|| std::fs::read_to_string("/proc/meminfo").ok())
            .flatten()
        {
            let get = |k: &str| {
                meminfo
                    .lines()
                    .find(|l| l.starts_with(k))
                    .and_then(|l| l.split_whitespace().nth(1))
                    .and_then(|v| v.parse::<f32>().ok())
            };
            if let (Some(t), Some(a)) = (get("MemTotal:"), get("MemAvailable:")) {
                self.mem = 1. - a / t;
            }
        }
    }
    fn draw(&self, r: &mut Raster, cols: usize, rows: usize, sound: &Sound, gaze: (f32, f32)) {
        let split = cols * 62 / 100;
        let border = (60, 80, 100);
        let hot = |x: usize, y: usize| {
            // The pane under the cursor glows brighter.
            let px = ((gaze.0 + 1.) * 0.5 * cols as f32) as usize;
            let py = ((gaze.1 + 1.) * 0.5 * rows as f32) as usize;
            (px < split) == (x < split) && ((py < rows / 2) == (y < rows / 2) || x < split)
        };
        for y in 0..rows {
            r.dot(split as i32, y as i32, 5., '│', border);
        }
        for x in split..cols {
            r.dot(x as i32, (rows / 2) as i32, 5., '─', border);
        }
        r.dot(split as i32, (rows / 2) as i32, 4., '├', border);
        text(r, 1, 0, 4., " shell ", (20, 30, 40));
        for x in 1..8 {
            r.dot(x as i32, 0, 3., ' ', (0, 0, 0));
        }
        text(r, 2, 0, 3., "shell", (140, 200, 240));
        text(r, split as i32 + 2, 0, 3., "journal", (140, 200, 240));
        text(
            r,
            split as i32 + 2,
            (rows / 2) as i32 + 1,
            3.,
            "resources",
            (140, 200, 240),
        );
        let top = 2i32;
        let mut y = top;
        let lines_fit = rows.saturating_sub(4);
        let start = self
            .main
            .lines
            .len()
            .saturating_sub(lines_fit.saturating_sub(1));
        for (line, color) in &self.main.lines[start..] {
            let bright = if hot(1, y as usize) { 1. } else { 0.78 };
            let line: String = line.chars().take(split.saturating_sub(2)).collect();
            text(r, 1, y, 6., &line, scale(*color, bright));
            y += 1;
        }
        let cmd = COMMANDS[self.script % COMMANDS.len()];
        let typed: String = cmd.chars().take(self.typed as usize).collect();
        if (y as usize) < rows {
            text(r, 1, y, 6., "❯ ", (120, 230, 140));
            text(r, 3, y, 6., &typed, (235, 240, 245));
            let blink = (self.phase * 2.5).fract() < 0.6 || sound.bass > 0.3;
            if blink {
                r.dot(
                    3 + typed.chars().count() as i32,
                    y,
                    5.,
                    '█',
                    scale((235, 240, 245), 0.7 + sound.bass * 0.3),
                );
            }
        }
        let lx = split as i32 + 1;
        let fit = (rows / 2).saturating_sub(2);
        let start = self.log.lines.len().saturating_sub(fit);
        for (i, (line, color)) in self.log.lines[start..].iter().enumerate() {
            let line: String = line
                .chars()
                .take((cols - split).saturating_sub(2))
                .collect();
            text(r, lx, 2 + i as i32, 6., &line, *color);
        }
        let by = (rows / 2) as i32 + 3;
        let width = (cols - split).saturating_sub(14).max(4) as f32;
        for (i, c) in self.cpu.iter().enumerate() {
            let yy = by + i as i32;
            if yy as usize >= rows {
                break;
            }
            text(r, lx, yy, 6., &format!("cpu{i}"), (150, 165, 180));
            let n = (c * width) as usize;
            for k in 0..width as usize {
                let col = if k < n {
                    mix((90, 220, 140), (255, 110, 90), k as f32 / width)
                } else {
                    (35, 45, 55)
                };
                r.dot(
                    lx + 6 + k as i32,
                    yy,
                    6.,
                    if k < n { '▮' } else { '·' },
                    col,
                );
            }
            text(
                r,
                lx + 7 + width as i32,
                yy,
                6.,
                &format!("{:3.0}%", c * 100.),
                (150, 165, 180),
            );
        }
        let yy = by + 9;
        if (yy as usize) < rows {
            text(r, lx, yy, 6., "mem ", (150, 165, 180));
            let n = (self.mem * width) as usize;
            for k in 0..width as usize {
                r.dot(
                    lx + 6 + k as i32,
                    yy,
                    6.,
                    if k < n { '▮' } else { '·' },
                    if k < n { (120, 170, 255) } else { (35, 45, 55) },
                );
            }
            text(
                r,
                lx + 7 + width as i32,
                yy,
                6.,
                &format!("{:3.0}%", self.mem * 100.),
                (150, 165, 180),
            );
        }
        let yy = by + 11;
        if (yy as usize) < rows {
            // Audio spectrum as a little equaliser, honest about silence.
            text(r, lx, yy, 6., "audio", (150, 165, 180));
            for k in 0..width as usize {
                let f = k as f32 / width;
                let level = (sound.bass * (1. - f) + sound.treble * f)
                    * (0.6 + 0.4 * (self.phase * 7. + k as f32).sin().abs());
                let h = (level * 3.).clamp(0., 3.) as usize;
                let ch = [' ', '▂', '▄', '▆'][h];
                r.dot(
                    lx + 6 + k as i32,
                    yy,
                    6.,
                    ch,
                    mix((90, 160, 255), (255, 120, 200), f),
                );
            }
        }
    }
}
// --------------------------------------------------------------------- globe
// Coarse continent outlines as lat/lon polygons; enough for a recognisable
// Earth at terminal resolution without shipping a bitmap.
const CONTINENTS: [&[(f32, f32)]; 6] = [
    &[
        (70., -165.),
        (72., -100.),
        (75., -80.),
        (60., -60.),
        (45., -65.),
        (30., -80.),
        (25., -97.),
        (15., -90.),
        (8., -78.),
        (20., -105.),
        (35., -122.),
        (50., -128.),
        (60., -150.),
    ],
    &[
        (12., -72.),
        (10., -62.),
        (5., -50.),
        (-5., -35.),
        (-15., -39.),
        (-23., -42.),
        (-35., -54.),
        (-52., -70.),
        (-55., -72.),
        (-40., -74.),
        (-20., -71.),
        (-5., -80.),
        (5., -78.),
    ],
    &[
        (71., -9.),
        (70., 30.),
        (77., 105.),
        (70., 160.),
        (60., 165.),
        (50., 140.),
        (35., 122.),
        (22., 110.),
        (10., 105.),
        (8., 78.),
        (22., 70.),
        (25., 60.),
        (13., 44.),
        (30., 34.),
        (36., 10.),
        (37., -9.),
        (43., -9.),
        (48., -5.),
        (58., -5.),
    ],
    &[
        (83., -40.),
        (80., -20.),
        (70., -22.),
        (60., -44.),
        (70., -55.),
        (78., -70.),
    ],
    &[
        (37., -6.),
        (37., 10.),
        (32., 32.),
        (12., 43.),
        (-2., 42.),
        (-12., 40.),
        (-26., 33.),
        (-35., 20.),
        (-33., 18.),
        (-20., 12.),
        (-5., 10.),
        (5., 8.),
        (4., -8.),
        (15., -17.),
        (28., -13.),
    ],
    &[
        (-12., 130.),
        (-11., 137.),
        (-16., 146.),
        (-27., 153.),
        (-38., 148.),
        (-35., 137.),
        (-32., 116.),
        (-22., 114.),
        (-15., 124.),
    ],
];
const CITIES: [(f32, f32); 14] = [
    (-33.4, -70.6),
    (40.7, -74.0),
    (51.5, -0.1),
    (35.7, 139.7),
    (-23.5, -46.6),
    (19.4, -99.1),
    (52.5, 13.4),
    (1.3, 103.8),
    (-33.9, 151.2),
    (28.6, 77.2),
    (30.0, 31.2),
    (55.8, 37.6),
    (37.8, -122.4),
    (-1.3, 36.8),
];
struct Arc {
    from: usize,
    to: usize,
    age: f32,
}
struct Globe {
    points: Vec<(f32, f32, bool)>,
    arcs: Vec<Arc>,
    spin: f32,
    spawn: f32,
    rng: Rng,
}
fn land(lat: f32, lon: f32) -> bool {
    if lat < -68. {
        return true;
    }
    CONTINENTS.iter().any(|poly| {
        let mut inside = false;
        let n = poly.len();
        for i in 0..n {
            let (y1, x1) = poly[i];
            let (y2, x2) = poly[(i + 1) % n];
            if (y1 > lat) != (y2 > lat) && lon < x1 + (lat - y1) * (x2 - x1) / (y2 - y1) {
                inside = !inside;
            }
        }
        inside
    })
}
impl Globe {
    fn new() -> Self {
        let n = 2600;
        let golden = PI * (3. - 5f32.sqrt());
        let points = (0..n)
            .map(|i| {
                let y = 1. - (i as f32 + 0.5) / n as f32 * 2.;
                let lat = y.asin().to_degrees();
                let lon = ((i as f32 * golden) % TAU - PI).to_degrees();
                (lat, lon, land(lat, lon))
            })
            .collect();
        Self {
            points,
            arcs: vec![],
            spin: 0.,
            spawn: 0.,
            rng: Rng(rand::random()),
        }
    }
    fn step(&mut self, dt: f32, sound: &Sound) {
        self.spin = (self.spin + dt * 0.12) % TAU;
        for a in &mut self.arcs {
            a.age += dt * 0.35;
        }
        self.arcs.retain(|a| a.age < 1.6);
        self.spawn += dt * (0.5 + sound.bass * 4.);
        while self.spawn >= 1. {
            self.spawn -= 1.;
            if self.arcs.len() < 24 {
                let from = (self.rng.next() * CITIES.len() as f32) as usize % CITIES.len();
                let mut to = (self.rng.next() * CITIES.len() as f32) as usize % CITIES.len();
                if to == from {
                    to = (to + 1) % CITIES.len();
                }
                self.arcs.push(Arc { from, to, age: 0. });
            }
        }
    }
    fn place(&self, lat: f32, lon: f32, lift: f32, gaze: (f32, f32)) -> (f32, f32, f32) {
        let (la, lo) = (lat.to_radians(), lon.to_radians() + self.spin);
        let r = 1. + lift;
        let (x, y, z) = (
            r * la.cos() * lo.sin(),
            -r * la.sin(),
            r * la.cos() * lo.cos(),
        );
        // Cursor tilts the globe a little around the horizontal axis.
        let tilt = 0.35 + gaze.1 * 0.3;
        let (s, c) = tilt.sin_cos();
        (x, y * c - z * s, y * s + z * c)
    }
    fn draw(
        &self,
        r: &mut Raster,
        cols: usize,
        rows: usize,
        aspect: f32,
        sound: &Sound,
        gaze: (f32, f32),
        time: f32,
    ) {
        let cx = cols as f32 * 0.5 - gaze.0 * cols as f32 * 0.03;
        let cy = rows as f32 * 0.5;
        let radius = rows as f32 * 0.42;
        let project =
            |p: (f32, f32, f32)| (cx + p.0 * radius * aspect, cy + p.1 * radius, 2. - p.2);
        for i in 0..90 {
            let x = ((i * 97) % 211) as f32 / 211. * cols as f32;
            let y = ((i * 41) % 53) as f32 / 53. * rows as f32;
            let d = ((x - cx) / (radius * aspect)).powi(2) + ((y - cy) / radius).powi(2);
            if d > 1.3 {
                let t = 0.5 + 0.5 * (time * 1.7 + i as f32).sin();
                r.dot(
                    x as i32,
                    y as i32,
                    50.,
                    '.',
                    scale((120, 140, 170), 0.4 + t * 0.4),
                );
            }
        }
        for &(lat, lon, land) in &self.points {
            let p = self.place(lat, lon, 0., gaze);
            if p.2 < 0.05 {
                continue;
            }
            let (sx, sy, z) = project(p);
            let limb = p.2.powf(0.6);
            let (ch, color) = if land {
                let pulse = 0.8 + sound.treble * 0.5;
                ('@', scale((80, 220, 120), (0.35 + limb * 0.65) * pulse))
            } else {
                ('·', scale((40, 90, 160), 0.3 + limb * 0.5))
            };
            r.dot(
                sx.round() as i32,
                sy.round() as i32,
                z,
                if land && limb < 0.35 { 'o' } else { ch },
                color,
            );
        }
        for &(lat, lon) in &CITIES {
            let p = self.place(lat, lon, 0.01, gaze);
            if p.2 < 0.05 {
                continue;
            }
            let (sx, sy, z) = project(p);
            let blink = 0.6 + 0.4 * (time * 3. + lat).sin() + sound.treble * 0.5;
            r.dot(
                sx.round() as i32,
                sy.round() as i32,
                z - 0.05,
                '◉',
                scale((255, 220, 120), blink.min(1.3)),
            );
        }
        for a in &self.arcs {
            let (f, t) = (CITIES[a.from], CITIES[a.to]);
            let head = a.age.min(1.);
            let tail = (a.age - 0.6).max(0.);
            let steps = 40;
            for i in 0..=steps {
                let s = i as f32 / steps as f32;
                if s > head || s < tail {
                    continue;
                }
                // Straight-ish interpolation of lat/lon with a lift arch.
                let mut dlon = t.1 - f.1;
                if dlon > 180. {
                    dlon -= 360.;
                } else if dlon < -180. {
                    dlon += 360.;
                }
                let lat = f.0 + (t.0 - f.0) * s;
                let lon = f.1 + dlon * s;
                let lift = (s * PI).sin() * 0.22;
                let p = self.place(lat, lon, lift, gaze);
                if p.2 < -0.1 {
                    continue;
                }
                let (sx, sy, z) = project(p);
                let glow = 1. - ((head - s) * 3.).min(1.) * 0.6;
                r.dot(
                    sx.round() as i32,
                    sy.round() as i32,
                    z - 0.08,
                    if (s * 40.) as i32 == (head * 40.) as i32 {
                        '●'
                    } else {
                        '·'
                    },
                    scale((255, 170, 90), glow + sound.bass * 0.3),
                );
            }
        }
    }
}
/// Collects a static layer so it can be replayed instead of recomputed.
struct Bake(Vec<(i32, i32, f32, char, Color)>);
impl Bake {
    fn dot(&mut self, x: i32, y: i32, z: f32, ch: char, color: Color) {
        self.0.push((x, y, z, ch, color));
    }
}
// -------------------------------------------------------------------- aquarium
// The tank is one unit tall and `width` units wide; cells come only at draw.
const SURFACE: f32 = 0.07;
const SAND: f32 = 0.86;
const SCHOOL: usize = 28;
const CORAL_Z: f32 = 20.;
/// From the root of a blade to its tip.
const WEED: [(Color, Color); 4] = [
    ((14, 78, 58), (110, 215, 125)),
    ((10, 88, 92), (85, 225, 195)),
    ((62, 74, 20), (205, 195, 80)),
    ((84, 28, 58), (240, 110, 140)),
];
/// Where kelp grows, how tall, and which colour.
const KELP: [(f32, f32, usize); 8] = [
    (0.035, 0.46, 0),
    (0.335, 0.3, 2),
    (0.455, 0.52, 1),
    (0.55, 0.36, 0),
    (0.625, 0.44, 3),
    (0.785, 0.5, 1),
    (0.885, 0.34, 2),
    (0.955, 0.54, 0),
];
#[derive(Clone, Copy, PartialEq)]
enum Breed {
    Tetra,
    Clown,
    Tang,
    Angel,
    Puffer,
}
struct Fish {
    breed: Breed,
    x: f32,
    y: f32,
    vx: f32,
    vy: f32,
    /// -1 faces left, 1 right; in between the fish is seen turning.
    face: f32,
    wag: f32,
    size: f32,
    goal: (f32, f32),
}
struct Bubble {
    x: f32,
    y: f32,
    size: f32,
    phase: f32,
}
/// Sand the cursor lifts off the floor.
struct Dust {
    x: f32,
    y: f32,
    vx: f32,
    vy: f32,
    life: f32,
}
/// Direction away from the cursor and how strongly it is felt, 0 outside `radius`.
fn away(cursor: Option<(f32, f32)>, x: f32, y: f32, radius: f32) -> (f32, f32, f32) {
    let Some((cx, cy)) = cursor else {
        return (0., 0., 0.);
    };
    let (dx, dy) = (x - cx, y - cy);
    let d = (dx * dx + dy * dy).sqrt();
    if d >= radius {
        return (0., 0., 0.);
    }
    let k = 1. - d / radius;
    (dx / d.max(0.001), dy / d.max(0.001), k * k)
}
type AquariumKey = (usize, usize);
struct Aquarium {
    fish: Vec<Fish>,
    bubbles: Vec<Bubble>,
    dust: Vec<Dust>,
    /// Where the cursor is in the tank, while there is one.
    cursor: Option<(f32, f32)>,
    width: f32,
    time: f32,
    lid: f32,
    /// How swollen the puffer is, from its deflated size up to 1.
    puffed: f32,
    last_bass: f32,
    layer: std::cell::RefCell<(AquariumKey, Vec<(i32, i32, f32, char, Color)>)>,
    rng: Rng,
}
fn sand_top(x: f32) -> f32 {
    SAND + (x * 2.3).sin() * 0.018 + (x * 5.1 + 1.).sin() * 0.008
}
impl Aquarium {
    const CHEST: f32 = 0.71;
    const CASTLE: f32 = 0.2;
    fn new() -> Self {
        Self {
            fish: vec![],
            bubbles: vec![],
            dust: vec![],
            cursor: None,
            width: 1.8,
            time: 0.,
            lid: 0.,
            puffed: 0.,
            last_bass: 0.,
            layer: Default::default(),
            rng: Rng(rand::random()),
        }
    }
    fn stock(&mut self) {
        let width = self.width;
        let swimming = [
            (Breed::Clown, 0.034),
            (Breed::Clown, 0.028),
            (Breed::Clown, 0.031),
            (Breed::Tang, 0.04),
            (Breed::Tang, 0.033),
            (Breed::Angel, 0.04),
            (Breed::Angel, 0.034),
            (Breed::Puffer, 0.04),
        ];
        let tetras = (0..SCHOOL).map(|_| (Breed::Tetra, 0.0085));
        for (breed, size) in swimming.into_iter().chain(tetras) {
            let school = breed == Breed::Tetra;
            let (x, y) = if school {
                (width * (0.45 + self.rng.next() * 0.15), 0.3 + self.rng.next() * 0.15)
            } else {
                (width * (0.1 + self.rng.next() * 0.8), 0.2 + self.rng.next() * 0.5)
            };
            let dir = if self.rng.next() < 0.5 { 1. } else { -1. };
            self.fish.push(Fish {
                breed,
                x,
                y,
                vx: dir * 0.05,
                vy: 0.,
                face: dir,
                wag: self.rng.next() * TAU,
                size,
                goal: (x, y),
            });
        }
    }
    fn release_bubbles(&mut self, x: f32, y: f32, count: usize) {
        for _ in 0..count {
            if self.bubbles.len() < 140 {
                self.bubbles.push(Bubble {
                    x: x + (self.rng.next() - 0.5) * 0.03,
                    y,
                    size: 0.3 + self.rng.next() * 0.7,
                    phase: self.rng.next() * TAU,
                });
            }
        }
    }
    fn step(&mut self, dt: f32, sound: &Sound, cursor: Option<(f32, f32)>, width: f32) {
        self.time += dt;
        self.width = width.max(0.2);
        self.cursor = cursor.map(|(x, y)| ((x + 1.) * 0.5 * self.width, (y + 1.) * 0.5));
        if self.fish.is_empty() {
            self.stock();
        }
        // The chest answers the beat: it opens and lets its air out.
        let chest = (self.width * Self::CHEST, SAND - 0.05);
        let open = (sound.bass * 1.6 - 0.15).max(away(self.cursor, chest.0, chest.1, 0.3).2 * 2.5).clamp(0., 1.);
        self.lid += (open - self.lid) * (1. - (-dt * 7.).exp());
        if self.lid > 0.5 && self.rng.next() < dt * 6. {
            self.release_bubbles(chest.0, chest.1, 1);
        }
        if sound.bass > 0.4 && sound.bass > self.last_bass + 0.02 {
            self.release_bubbles(chest.0, chest.1, 3);
        }
        self.last_bass = sound.bass;
        if self.rng.next() < dt * 2.5 {
            self.release_bubbles(self.width * (Self::CASTLE + 0.035), SAND - 0.3, 1);
        }
        let (time, cursor) = (self.time, self.cursor);
        for b in &mut self.bubbles {
            let (ax, ay, k) = away(cursor, b.x, b.y, 0.16);
            b.y -= dt * (0.09 + b.size * 0.09) - ay * k * dt * 0.5;
            b.x += (time * 3. + b.phase).sin() * dt * 0.02 + ax * k * dt * 0.5;
        }
        self.bubbles.retain(|b| b.y > SURFACE);
        // Near the floor the cursor stirs the sand up.
        if let Some((x, y)) = cursor {
            if y > SAND - 0.14 && self.dust.len() < 90 {
                for _ in 0..(dt * 50.).ceil() as usize {
                    let spread = (self.rng.next() - 0.5) * 0.14;
                    self.dust.push(Dust {
                        x: x + spread,
                        y: sand_top((x + spread).max(0.)),
                        vx: spread * 1.4,
                        vy: -0.06 - self.rng.next() * 0.1,
                        life: 1. + self.rng.next(),
                    });
                }
            }
        }
        for d in &mut self.dust {
            d.vy += dt * 0.09;
            d.x += d.vx * dt;
            d.y += d.vy * dt;
            d.life -= dt;
        }
        self.dust.retain(|d| d.life > 0. && d.y < SAND + 0.02);
        self.swim(dt, sound);
        if let Some(puffer) = self.fish.iter().find(|f| f.breed == Breed::Puffer) {
            let scared = away(cursor, puffer.x, puffer.y, 0.34).2 * 2.;
            let wanted = sound.bass.max(scared).min(1.);
            self.puffed += (wanted - self.puffed) * (1. - (-dt * 6.).exp());
        }
    }
    fn swim(&mut self, dt: f32, sound: &Sound) {
        let width = self.width;
        let cursor = self.cursor;
        let drive = 1. + sound.bass * 0.8;
        let school: Vec<(f32, f32, f32, f32)> = self
            .fish
            .iter()
            .filter(|f| f.breed == Breed::Tetra)
            .map(|f| (f.x, f.y, f.vx, f.vy))
            .collect();
        let count = school.len().max(1) as f32;
        let centre = school.iter().fold((0., 0.), |s, f| (s.0 + f.0 / count, s.1 + f.1 / count));
        let flow = school.iter().fold((0., 0.), |s, f| (s.0 + f.2 / count, s.1 + f.3 / count));
        let (time, mut goals) = (self.time, vec![]);
        for _ in 0..self.fish.len() {
            goals.push((self.rng.next(), self.rng.next()));
        }
        for (f, roll) in self.fish.iter_mut().zip(goals) {
            let (mut ax, mut ay);
            let top_speed;
            if f.breed == Breed::Tetra {
                ax = (centre.0 - f.x) * 0.22 + (flow.0 - f.vx) * 1.2 + (time * 0.23).sin() * 0.05;
                ay = (centre.1 - f.y) * 0.3 + (flow.1 - f.vy) * 1.2 + (time * 0.37).cos() * 0.03;
                for &(x, y, _, _) in &school {
                    let (dx, dy) = (f.x - x, f.y - y);
                    let d = (dx * dx + dy * dy).sqrt();
                    if d > 0. && d < 0.075 {
                        ax += dx / d * 0.45 * (1. - d / 0.075);
                        ay += dy / d * 0.7 * (1. - d / 0.075);
                    }
                }
                let (dx, dy, k) = away(cursor, f.x, f.y, 0.22);
                ax += dx * k * 5.;
                ay += dy * k * 5.;
                top_speed = 0.16 * drive * (1. + k * 2.);
            } else {
                if (f.goal.0 - f.x).abs() + (f.goal.1 - f.y).abs() < 0.06 {
                    f.goal = (width * (0.08 + roll.0 * 0.84), SURFACE + 0.1 + roll.1 * (SAND - SURFACE - 0.22));
                }
                let (dx, dy) = (f.goal.0 - f.x, f.goal.1 - f.y);
                let d = (dx * dx + dy * dy).sqrt().max(0.001);
                ax = dx / d * 0.09;
                ay = dy / d * 0.05;
                // A fright also sends it to swim somewhere else.
                let (fx, fy, k) = away(cursor, f.x, f.y, 0.3);
                ax += fx * k * 2.2;
                ay += fy * k * 2.2;
                if k > 0.05 {
                    f.goal = (f.x + fx * 0.5, f.y + fy * 0.3);
                    f.goal.0 = f.goal.0.clamp(0.1, (width - 0.1).max(0.11));
                    f.goal.1 = f.goal.1.clamp(SURFACE + 0.1, SAND - 0.12);
                }
                top_speed = if f.breed == Breed::Puffer { 0.035 + k * 0.05 } else { 0.075 * drive * (1. + k * 3.) };
            }
            // The glass and the sand push back before a fish can reach them.
            let margin = 0.09;
            ax += ((margin - f.x).max(0.) - (f.x - (width - margin)).max(0.)) * 8.;
            ay += ((SURFACE + margin - f.y).max(0.) - (f.y - (SAND - margin)).max(0.)) * 8.;
            f.vx += ax * dt;
            f.vy += ay * dt;
            let speed = (f.vx * f.vx + f.vy * f.vy).sqrt();
            if speed > top_speed {
                f.vx *= top_speed / speed;
                f.vy *= top_speed / speed;
            }
            f.x = (f.x + f.vx * dt).clamp(0.02, (width - 0.02).max(0.03));
            f.y = (f.y + f.vy * dt).clamp(SURFACE + 0.02, SAND - 0.02);
            if f.vx.abs() > 0.008 {
                f.face += (f.vx.signum() - f.face) * (1. - (-dt * 3.5).exp());
            }
            f.wag += dt * (5. + speed * 45.);
        }
    }
    fn bake(&self, out: &mut Bake, cols: usize, rows: usize, aspect: f32) {
        let height = rows as f32;
        let across = |x: f32| x / self.width * cols as f32;
        // Gravel, with a light top and darker depth.
        for x in 0..cols {
            let top = (sand_top(x as f32 / cols as f32 * self.width) * height) as i32;
            for y in top..rows as i32 {
                let grain = variant((x as u32).wrapping_mul(7919) ^ (y as u32).wrapping_mul(104729));
                let depth = (y - top) as f32 / (rows as f32 * 0.14);
                if grain < 0.55 - depth * 0.2 || y == top {
                    let ch = if y == top { '~' } else if grain < 0.15 { '°' } else if grain < 0.35 { ':' } else { '·' };
                    let sand = mix((225, 195, 130), (120, 90, 70), depth);
                    out.dot(x as i32, y, 30., ch, scale(sand, 0.55 + grain * 0.5));
                }
            }
        }
        // A sunken castle: two towers joined by a wall with a dark gate.
        let stone = (118, 128, 150);
        let block = |out: &mut Bake, left: f32, right: f32, top: f32, bottom: f32| {
            for y in top as i32..bottom as i32 {
                for x in left as i32..right as i32 {
                    let brick = (y as f32 / (height * 0.018).max(1.)) as i32;
                    let seam = (x + brick * 3).rem_euclid(((height * 0.06 * aspect) as i32).max(3)) == 0;
                    let edge = x == left as i32 || x + 1 == right as i32;
                    let (ch, k) = if edge { ('█', 1.05) } else if seam { ('│', 0.55) } else { ('▒', 0.95) };
                    out.dot(x, y, 22., ch, scale(stone, k));
                }
            }
            let tooth = (height * 0.022 * aspect).max(2.);
            let mut x = left;
            while x + tooth <= right {
                for y in 0..(height * 0.018).max(1.) as i32 {
                    for dx in 0..tooth as i32 {
                        out.dot(x as i32 + dx, top as i32 - 1 - y, 22., '█', scale(stone, 1.1));
                    }
                }
                x += tooth * 2.;
            }
        };
        let centre = across(self.width * Self::CASTLE);
        let unit = height * aspect;
        let ground = SAND * height + height * 0.02;
        block(out, centre - unit * 0.16, centre - unit * 0.075, height * 0.5, ground);
        block(out, centre + unit * 0.075, centre + unit * 0.16, height * 0.46, ground);
        block(out, centre - unit * 0.075, centre + unit * 0.075, height * 0.62, ground);
        let (gate_w, gate_h) = (unit * 0.04, height * 0.15);
        for y in (ground - gate_h) as i32..ground as i32 {
            for x in (centre - gate_w) as i32..=(centre + gate_w) as i32 {
                let t = (x as f32 - centre) / gate_w;
                let arch = ground - gate_h * (1. - t * t).max(0.).sqrt();
                if y as f32 >= arch {
                    out.dot(x, y, 21., ' ', (0, 0, 0));
                }
            }
        }
        for (x, y) in [(-0.117, 0.57), (0.117, 0.53)] {
            let (wx, wy) = (centre + unit * x, height * y);
            for dy in 0..(height * 0.05) as i32 {
                for dx in -((unit * 0.008) as i32)..=(unit * 0.008) as i32 {
                    out.dot(wx as i32 + dx, wy as i32 + dy, 21., '█', (240, 190, 90));
                }
            }
        }
        // Rocks to break the floor line.
        for (site, size) in [(0.42, 0.075), (0.5, 0.05), (0.92, 0.09)] {
            let (cx, cy) = (cols as f32 * site, SAND * height + height * 0.03);
            let (rw, rh) = (height * size * aspect, height * size);
            for y in (cy - rh) as i32..cy as i32 {
                for x in (cx - rw) as i32..(cx + rw) as i32 {
                    let (u, v) = ((x as f32 - cx) / rw, (y as f32 - cy) / rh);
                    if u * u + v * v < 1. {
                        let light = (-v * 0.7 - u * 0.4 + noise(x as f32 * 0.25, y as f32 * 0.5) * 0.4).clamp(0., 1.);
                        let ch = if light > 0.62 { '▓' } else if light > 0.35 { '▒' } else { '░' };
                        out.dot(x, y, 24., ch, scale((105, 100, 125), 0.45 + light * 0.75));
                    }
                }
            }
        }
        // Branching corals: each branch forks twice on a fixed shape.
        for (site, tint, reach) in [
            (0.37, (255, 95, 130), 0.11),
            (0.58, (255, 150, 60), 0.09),
            (0.84, (200, 90, 230), 0.12),
            (0.97, (255, 110, 90), 0.08),
        ] {
            let x = cols as f32 * site;
            let mut stack = vec![(x, SAND * height, -PI / 2., height * reach, 0)];
            while let Some((x, y, angle, length, level)) = stack.pop() {
                let (ex, ey) = (x + angle.cos() * length * aspect, y + angle.sin() * length);
                let steps = (length.max(1.) * 2.5) as i32;
                for i in 0..=steps {
                    let t = i as f32 / steps.max(1) as f32;
                    let ch = if level == 3 { '*' } else if angle.cos().abs() < 0.3 { '|' } else if angle.cos() > 0. { '/' } else { '\\' };
                    out.dot((x + (ex - x) * t) as i32, (y + (ey - y) * t) as i32, CORAL_Z, ch, scale(tint, 0.6 + level as f32 * 0.14));
                }
                if level < 3 {
                    stack.push((ex, ey, angle - 0.55, length * 0.72, level + 1));
                    stack.push((ex, ey, angle + 0.5, length * 0.68, level + 1));
                }
            }
        }
    }
    /// One blade of weed: a solid ribbon with a light midrib and ruffled edges,
    /// or a plain tapering leaf. The current runs up it as a slow wave.
    #[allow(clippy::too_many_arguments)]
    fn frond(&self, r: &mut Raster, root: f32, bottom: f32, tall: f32, half: f32, lean: f32, phase: f32, shade: usize, z: f32, ribbon: bool, height: f32, aspect: f32, sound: &Sound) {
        let (deep, bright) = WEED[shade];
        let time = self.time;
        for i in 0..tall as i32 {
            let t = i as f32 / tall;
            let sway = (time * 0.8 + phase + t * 3.4).sin() * t.powf(1.3) * height * aspect * (0.03 + sound.bass * 0.035);
            let x = root + lean * t * t * height * aspect + sway;
            let wide = if ribbon {
                let ruffle = 1. + 0.22 * (i as f32 * 0.42 + time * 1.6 + phase).sin();
                half * (PI * t.powf(0.8)).sin().max(0.1).powf(0.6) * ruffle
            } else {
                half * (1. - t).powf(0.7)
            }
            .max(0.5);
            let y = (bottom - i as f32) as i32;
            for dx in -(wide.ceil() as i32)..=wide.ceil() as i32 {
                let u = dx as f32 / wide;
                if u.abs() > 1. {
                    continue;
                }
                let light = 0.5 + 0.75 * (1. - u * u) - u * 0.18;
                let tone = mix(deep, bright, t * 0.8 + 0.1);
                let rib = ribbon && u.abs() < 0.14;
                let vein = (i + dx.abs() * 2).rem_euclid(7) == 0;
                let (ch, c) = if rib {
                    ('█', scale(mix(tone, (235, 250, 190), 0.45), light))
                } else if u.abs() > 0.74 {
                    ('▒', scale(tone, light))
                } else if vein {
                    ('░', scale(tone, light))
                } else {
                    ('█', scale(tone, light))
                };
                r.dot((x + dx as f32) as i32, y, z, ch, c);
            }
        }
    }
    fn draw_fish(&self, r: &mut Raster, f: &Fish, depth: f32, cols: usize, rows: usize, aspect: f32) {
        let height = rows as f32;
        let (cx, cy) = (f.x / self.width * cols as f32, f.y * height);
        if f.breed == Breed::Puffer {
            return self.draw_puffer(r, f, depth, (cx, cy), height, aspect);
        }
        let tall = if f.breed == Breed::Angel { 1.5 } else { 1. };
        let half_h = (f.size * height * tall).max(1.);
        let turn = f.face.abs().max(0.22);
        let half_l = (f.size * height * aspect * 1.7 * turn).max(2.);
        let side = if f.face < 0. { -1. } else { 1. };
        let bend = (f.size * height * 0.3).max(0.5);
        let colour = |u: f32, v: f32| -> Color {
            match f.breed {
                Breed::Clown => {
                    let band = (u - 0.42).abs() < 0.11 || (u + 0.2).abs() < 0.11;
                    let rim = (u - 0.42).abs() < 0.15 || (u + 0.2).abs() < 0.15;
                    if band { (250, 245, 235) } else if rim { (35, 25, 20) } else { (255, 118, 28) }
                }
                Breed::Tang => {
                    if v.abs() < 0.4 && u.abs() < 0.75 && (u * 3. + v * 2.).sin() > 0.75 { (15, 20, 50) } else { (35, 95, 235) }
                }
                Breed::Angel => {
                    if (u * 7.5).sin() > 0.62 { (30, 28, 34) } else { mix((250, 225, 120), (205, 215, 225), v.abs()) }
                }
                _ => if v > 0.05 && u < 0.45 { (255, 55, 70) } else { (70, 225, 255) },
            }
        };
        let columns = (half_l * 1.42) as i32;
        for i in -columns..=(half_l as i32) {
            let u = i as f32 / half_l;
            let sway = (f.wag - u * 2.4).sin() * bend * (1. - u) * 0.5;
            let x = (cx + side * i as f32) as i32;
            if u < -1. {
                // The tail spreads from the end of the body.
                let spread = half_h * 0.95 * (-u - 1.) / 0.42;
                let tail = if f.breed == Breed::Tang { (250, 215, 55) } else { scale(colour(-0.9, 0.), 0.85) };
                for y in (-spread) as i32..=spread as i32 {
                    let ch = if y.abs() as f32 >= spread - 1. { if (y as f32) < 0. { '/' } else { '\\' } } else { '=' };
                    let ch = match (side < 0., ch) { (true, '/') => '\\', (true, '\\') => '/', (_, c) => c };
                    r.dot(x, (cy + sway + y as f32) as i32, depth, ch, tail);
                }
                continue;
            }
            let shape = (1. - u * u).max(0.).sqrt() * (1. + u * 0.18);
            let half = half_h * shape;
            let top = (-half).floor() as i32;
            for y in top..=half.ceil() as i32 {
                let v = y as f32 / half.max(1.);
                if v.abs() > 1. {
                    continue;
                }
                let light = 1.08 - (v + 0.35).abs() * 0.5;
                let ch = if v.abs() > 0.85 {
                    '▓'
                } else if (i + y * 3).rem_euclid(5) == 0 {
                    if side > 0. { ')' } else { '(' }
                } else {
                    '█'
                };
                r.dot(x, (cy + sway + y as f32) as i32, depth, ch, scale(colour(u, v), light));
            }
            // Dorsal and belly fins follow the body and ripple with it.
            let fin_top = if f.breed == Breed::Angel { 0.7 } else { 0.42 };
            if u.abs() < 0.62 {
                let reach = half_h * fin_top * (1. - (u / 0.62).powi(2)) * (0.8 + (f.wag + u * 6.).sin() * 0.2);
                for y in 1..=reach as i32 {
                    r.dot(x, (cy + sway - half - y as f32) as i32, depth + 0.1, '^', scale(colour(u, -1.), 0.7));
                    if f.breed == Breed::Angel {
                        r.dot(x, (cy + sway + half + y as f32) as i32, depth + 0.1, 'v', scale(colour(u, 1.), 0.7));
                    }
                }
            }
        }
        if turn > 0.45 {
            let (ex, ey) = (cx + side * half_l * 0.6, cy - half_h * 0.22);
            r.dot(ex as i32, ey as i32, depth - 0.2, '●', (250, 250, 250));
            if half_l > 6. {
                r.dot((ex + side) as i32, ey as i32, depth - 0.3, '•', (10, 10, 20));
            }
        }
    }
    fn draw_puffer(&self, r: &mut Raster, f: &Fish, depth: f32, (cx, cy): (f32, f32), height: f32, aspect: f32) {
        // The beat fills it with water and it deflates between hits.
        let radius = (f.size * height * (1. + self.puffed * 0.95)).max(1.5);
        let side = if f.face < 0. { -1. } else { 1. };
        for y in (-radius) as i32 - 1..=radius as i32 + 1 {
            let reach = radius * aspect;
            for x in (-reach) as i32 - 2..=reach as i32 + 2 {
                let (u, v) = (x as f32 / reach, y as f32 / radius);
                let d = (u * u + v * v).sqrt();
                let (px, py) = ((cx + x as f32) as i32, (cy + y as f32) as i32);
                if d < 1. {
                    let belly = v > 0.25;
                    let spot = !belly && noise(u * 4. + f.x * 0., v * 4.) > 0.68;
                    let c = if belly { (245, 240, 215) } else if spot { (120, 90, 30) } else { (240, 190, 55) };
                    r.dot(px, py, depth, if d > 0.82 { '▓' } else { '█' }, scale(c, 1.05 - d * 0.3));
                } else if d < 1.28 && (((v.atan2(u) * 11.).sin()) > 0.75) {
                    r.dot(px, py, depth + 0.1, '*', scale((240, 200, 90), 0.8));
                }
            }
        }
        let tail = (cx - side * radius * aspect * 1.25) as i32;
        for y in -1..=1 {
            r.dot(tail, (cy + y as f32 + (f.wag).sin()) as i32, depth, '#', (240, 190, 55));
        }
        let (ex, ey) = (cx + side * radius * aspect * 0.5, cy - radius * 0.3);
        r.dot(ex as i32, ey as i32, depth - 0.2, '●', (250, 250, 250));
        r.dot((ex + side) as i32, ey as i32, depth - 0.3, '•', (10, 10, 20));
        r.dot((cx + side * radius * aspect) as i32, (cy + radius * 0.1) as i32, depth - 0.2, 'o', (200, 110, 60));
    }
    fn draw(&self, r: &mut Raster, cols: usize, rows: usize, aspect: f32, sound: &Sound) {
        let height = rows as f32;
        let time = self.time;
        {
            let mut layer = self.layer.borrow_mut();
            if layer.0 != (cols, rows) {
                layer.0 = (cols, rows);
                let mut bake = Bake(std::mem::take(&mut layer.1));
                bake.0.clear();
                self.bake(&mut bake, cols, rows, aspect);
                layer.1 = bake.0;
            }
            // The cursor is a lantern on everything that stands still. Corals
            // only move for the music, swaying from the foot on the bass.
            let to_tank = self.width / cols.max(1) as f32;
            for &(x, y, z, ch, color) in &layer.1 {
                let (tx, ty) = (x as f32 * to_tank, y as f32 / height);
                let k = away(self.cursor, tx, ty, 0.26).2;
                let lean = if z == CORAL_Z {
                    (time * 2.2 + tx * 3.).sin() * sound.bass * (SAND - ty).max(0.) * 0.35 / to_tank
                } else {
                    0.
                };
                r.dot(x + lean as i32, y, z, ch, if k > 0. { scale(color, 1. + k * 1.7) } else { color });
            }
        }
        // The surface, and light falling through it.
        for x in 0..cols {
            let t = x as f32 * 0.03;
            let wave = (t + time * 0.8).sin() * 0.5 + (t * 2.3 - time * 1.1).sin() * 0.3;
            // The cursor raises a swell that rings out across the surface.
            let tx = x as f32 / cols as f32 * self.width;
            let swell = self.cursor.map_or(0., |(cx, cy)| {
                let d = (tx - cx).abs();
                (1. - d / 0.45).max(0.) * (d * 28. - time * 5.).cos() * (1. - cy * 0.8)
            });
            let y = SURFACE * height + (wave + swell * 1.6) * (height * 0.012).max(0.6);
            let glint = 0.5 + ((t * 5. + time * 1.5).sin() * 0.5 + 0.5) * 0.6 + sound.treble * 0.4 + swell.abs() * 0.5;
            r.dot(x as i32, y as i32, 45., '~', scale((130, 215, 245), glint));
            if x % 3 != 0 {
                r.dot(x as i32, y as i32 - 1, 46., '-', scale((70, 150, 200), glint * 0.5));
            }
        }
        for ray in 0..7 {
            let base = cols as f32 * (ray as f32 + 0.5) / 7. + (time * 0.13 + ray as f32 * 2.).sin() * cols as f32 * 0.04;
            let lean = (0.26 + ray as f32 * 0.018) * aspect - 0.2;
            let glow = 0.14 + ((time * 0.4 + ray as f32 * 1.7).sin() * 0.5 + 0.5) * 0.14 + sound.treble * 0.2;
            let wide = (height * 0.06 * aspect) as i32;
            let mut y = (SURFACE * height) as i32 + 1;
            while (y as f32) < SAND * height {
                let fade = 1. - y as f32 / (SAND * height);
                for dx in (0..wide).step_by(2) {
                    let x = base + y as f32 * lean + dx as f32;
                    let across = 1. - (dx as f32 / wide as f32 * 2. - 1.).abs();
                    let lit = away(self.cursor, x / cols as f32 * self.width, y as f32 / height, 0.3).2;
                    if variant((x as u32).wrapping_mul(31) ^ (y as u32).wrapping_mul(977) ^ (time * 2.) as u32) < 0.55 {
                        r.dot(x as i32, y, 50., '·', scale((120, 205, 235), glow * fade * across * 8. * (1. + lit * 4.)));
                    }
                }
                y += 2;
            }
        }
        // Kelp ribbons stand behind, tufts of eelgrass in front of them.
        for (i, &(site, tall, shade)) in KELP.iter().enumerate() {
            let root = cols as f32 * site;
            let bottom = sand_top(site * self.width) * height + 1.;
            let (phase, z) = (i as f32 * 1.9, 13. + (i % 4) as f32 * 2.);
            self.frond(r, root, bottom, height * tall, height * 0.017 * aspect, 0.05, phase, shade, z, true, height, aspect, sound);
            let beside = root + height * 0.03 * aspect;
            self.frond(r, beside, bottom, height * tall * 0.68, height * 0.013 * aspect, -0.06, phase + 2., shade, z + 0.5, true, height, aspect, sound);
        }
        for tuft in 0..12u32 {
            let site = variant(tuft * 2654435 + 29);
            let root = cols as f32 * site;
            let bottom = sand_top(site * self.width) * height + 1.;
            for blade in 0..3 {
                let spread = blade as f32 - 1.;
                let tall = height * (0.1 + variant(tuft * 97 + blade) * 0.09);
                self.frond(r, root + spread * height * 0.012 * aspect, bottom, tall, height * 0.006 * aspect, spread * 0.05, tuft as f32 + spread, 0, 7.5 + blade as f32 * 0.1, false, height, aspect, sound);
            }
        }
        // Chest: the lid lifts on the beat and the gold shows.
        let unit = height * aspect;
        let (cx, base) = (cols as f32 * Self::CHEST, SAND * height + height * 0.015);
        let (half, body) = (unit * 0.055, height * 0.055);
        for y in (base - body) as i32..base as i32 {
            for x in (cx - half) as i32..(cx + half) as i32 {
                let strap = ((x as f32 - cx) / half).abs() > 0.82 || (x as f32 - cx).abs() < 1.5;
                let (ch, c) = if strap { ('█', (205, 165, 60)) } else { ('▒', (125, 72, 38)) };
                r.dot(x, y, 10., ch, c);
            }
        }
        let lift = self.lid * height * 0.05;
        if self.lid > 0.08 {
            for x in (cx - half * 0.8) as i32..(cx + half * 0.8) as i32 {
                let shine = 0.7 + (x as f32 * 0.9 + time * 7.).sin() * 0.3;
                r.dot(x, (base - body) as i32, 9., '*', scale((255, 215, 90), shine + sound.treble * 0.4));
            }
        }
        let lid_rows = (height * 0.03).max(1.) as i32;
        for y in 0..lid_rows {
            let inset = (y as f32 * aspect * 0.7) as i32;
            for x in (cx - half) as i32 + (lid_rows - 1 - y).min(inset)..(cx + half) as i32 - (lid_rows - 1 - y).min(inset) {
                let py = (base - body - lift) as i32 - lid_rows + y;
                r.dot(x, py, 9.5, '▓', (150, 90, 45));
            }
        }
        for b in &self.bubbles {
            let (x, y) = ((b.x / self.width * cols as f32) as i32, (b.y * height) as i32);
            let ch = if b.size > 0.75 { 'O' } else if b.size > 0.45 { 'o' } else { '°' };
            r.dot(x, y, 5., ch, scale((185, 235, 255), 0.6 + b.size * 0.5 + sound.bass * 0.3));
        }
        for d in &self.dust {
            let (x, y) = ((d.x / self.width * cols as f32) as i32, (d.y * height) as i32);
            r.dot(x, y, 6., if d.life > 1. { '°' } else { '·' }, scale((225, 195, 130), 0.3 + d.life * 0.4));
        }
        for i in 0..70u32 {
            let (sx, sy) = (variant(i * 37 + 1), variant(i * 91 + 7));
            let x = sx * cols as f32 + (time * 0.2 + i as f32).sin() * height * 0.05;
            let y = ((sy + time * 0.006).rem_euclid(1.) * (SAND - SURFACE) + SURFACE) * height;
            let (ax, ay, k) = away(self.cursor, x / cols as f32 * self.width, y / height, 0.2);
            let (x, y) = (x + ax * k * height * aspect * 0.15, y + ay * k * height * 0.15);
            r.dot(x as i32, y as i32, 35., '·', scale((130, 190, 200), 0.3 + sound.treble * 0.5 + k));
        }
        // Nearer fish are larger; the school swims behind them.
        let mut order: Vec<&Fish> = self.fish.iter().collect();
        order.sort_by(|a, b| a.size.total_cmp(&b.size));
        for (i, f) in order.iter().enumerate() {
            self.draw_fish(r, f, 8. - i as f32 * 0.04, cols, rows, aspect);
        }
    }
}
// ------------------------------------------------------------------ jellyfish
struct Jelly {
    x: f32,
    y: f32,
    size: f32,
    phase: f32,
    hue: f32,
    drift: f32,
    /// How startled it is by the cursor, 0 to 1.
    fright: f32,
    /// Sideways drag on the tentacles while it flees.
    trail: f32,
}
struct Plankton {
    x: f32,
    y: f32,
    home: (f32, f32),
}
struct Jellyfish {
    jellies: Vec<Jelly>,
    plankton: Vec<Plankton>,
    time: f32,
    rng: Rng,
}
impl Jellyfish {
    fn new() -> Self {
        let mut rng = Rng(rand::random());
        let jellies = (0..7)
            .map(|i| Jelly {
                x: rng.next(),
                y: rng.next(),
                size: 0.55 + rng.next() * 0.8,
                phase: rng.next() * TAU,
                hue: i as f32 / 7.,
                drift: rng.next() - 0.5,
                fright: 0.,
                trail: 0.,
            })
            .collect();
        let plankton = (0..140)
            .map(|_| {
                let (x, y) = (rng.next(), rng.next());
                Plankton { x, y, home: (x, y) }
            })
            .collect();
        Self {
            jellies,
            plankton,
            time: 0.,
            rng,
        }
    }
    fn step(&mut self, dt: f32, sound: &Sound, cursor: Option<(f32, f32)>) {
        self.time += dt;
        let cursor = cursor.map(|(x, y)| ((x + 1.) * 0.5, (y + 1.) * 0.5));
        for j in &mut self.jellies {
            // Near the cursor it pulses hard, lights up and swims away.
            let (ax, ay, k) = away(cursor, j.x, j.y, 0.32);
            j.x += ax * k * dt * 0.3;
            j.y += ay * k * dt * 0.3;
            j.fright += ((k * 3.).min(1.) - j.fright) * (1. - (-dt * 5.).exp());
            j.trail += (-ax * k * 2.5 - j.trail) * (1. - (-dt * 3.).exp());
            // Bell contraction on the bass drives a push upward.
            j.phase += dt * (0.9 + sound.bass * 1.5 + j.fright * 7.);
            let push = (j.phase.sin().max(0.)).powi(2);
            j.y -= dt * (0.012 + push * 0.05) / j.size;
            j.x += dt * j.drift * 0.02 + (self.time * 0.3 + j.phase).sin() * dt * 0.01;
            if j.y < -0.25 {
                j.y = 1.2;
                j.x = self.rng.next();
            }
            j.x = j.x.rem_euclid(1.);
        }
        let (tx, ty) = cursor.unwrap_or((-9., -9.));
        for (i, p) in self.plankton.iter_mut().enumerate() {
            let k = i as f32;
            // Each mote wanders around its own home and only leans toward the
            // cursor when it is close, so the cloud never collapses or drifts.
            let (dx, dy) = (tx - p.x, ty - p.y);
            let near = (1. - (dx * dx + dy * dy).sqrt() / 0.22).max(0.);
            let wander_x = (self.time * 0.4 + k).sin() * 0.012;
            let wander_y = (self.time * 0.3 + k * 1.3).cos() * 0.012 - 0.003;
            p.x += (wander_x + (p.home.0 - p.x) * 0.15 + dx * near * 0.6) * dt;
            p.y += (wander_y + (p.home.1 - p.y) * 0.15 + dy * near * 0.6) * dt;
            p.home.1 = (p.home.1 - dt * 0.004).rem_euclid(1.);
            p.x = p.x.rem_euclid(1.);
            p.y = p.y.rem_euclid(1.);
        }
    }
    fn draw(&self, r: &mut Raster, cols: usize, rows: usize, aspect: f32, sound: &Sound) {
        // Caustic light bands on the sea floor, slow and soft.
        for x in 0..cols {
            for band in 0..3 {
                let yy = rows as f32 * (0.82 + band as f32 * 0.06)
                    + (x as f32 * 0.09 + self.time * (0.5 + band as f32 * 0.2) + band as f32).sin()
                        * 1.6
                    + (x as f32 * 0.21 - self.time * 0.8).sin() * 0.8;
                let g = 0.25
                    + 0.25 * (x as f32 * 0.17 + self.time * 1.1).sin().abs()
                    + sound.treble * 0.3;
                r.dot(x as i32, yy as i32, 40., '~', scale((60, 150, 190), g));
            }
        }
        for (i, p) in self.plankton.iter().enumerate() {
            let t = 0.4 + 0.6 * (self.time * 2. + i as f32).sin().abs();
            r.dot(
                (p.x * cols as f32) as i32,
                (p.y * rows as f32) as i32,
                30.,
                if t > 0.9 { '°' } else { '·' },
                scale((150, 220, 230), 0.3 + t * 0.4 + sound.treble * 0.3),
            );
        }
        let mut order: Vec<&Jelly> = self.jellies.iter().collect();
        order.sort_by(|a, b| a.size.partial_cmp(&b.size).unwrap());
        for j in order {
            let cx = j.x * cols as f32;
            let cy = j.y * rows as f32;
            let contract = 1. - j.phase.sin().max(0.) * 0.25;
            let rw = rows as f32 * 0.09 * j.size * aspect * contract;
            let rh = rows as f32 * 0.07 * j.size / contract.sqrt();
            let tint = mix(
                (190, 120, 255),
                (120, 230, 255),
                (j.hue * 2.).rem_euclid(1.),
            );
            let tint = mix(tint, (255, 180, 220), j.hue * 0.5);
            let depth = 10. - j.size;
            let bright = 0.45 + j.size * 0.35 + sound.bass * 0.35 + j.fright * 0.6;
            // Bell: an upper half ellipse of rim glyphs with a lighter inner glow.
            for i in 0..=48 {
                let a = PI + i as f32 / 48. * PI;
                let (x, y) = (cx + a.cos() * rw, cy + a.sin() * rh);
                let ch = if a.cos().abs() > 0.85 {
                    '('
                } else if a.sin() < -0.9 {
                    '~'
                } else {
                    '^'
                };
                let ch = if a.cos() > 0.85 { ')' } else { ch };
                r.dot(
                    x.round() as i32,
                    y.round() as i32,
                    depth,
                    ch,
                    scale(tint, bright),
                );
                if i % 4 == 0 {
                    let (ix, iy) = (cx + a.cos() * rw * 0.55, cy + a.sin() * rh * 0.55);
                    r.dot(
                        ix.round() as i32,
                        iy.round() as i32,
                        depth + 0.1,
                        '·',
                        scale(tint, bright * 0.5),
                    );
                }
            }
            for i in 0..=(rw * 2.) as i32 {
                let x = cx - rw + i as f32;
                r.dot(
                    x.round() as i32,
                    cy.round() as i32,
                    depth,
                    '~',
                    scale(tint, bright * 0.8),
                );
            }
            // Tentacles: sine ribbons trailing below, lagging behind the pulse.
            let count = 4 + (j.size * 3.) as usize;
            for t in 0..count {
                let ox = -rw * 0.8 + rw * 1.6 * t as f32 / (count - 1).max(1) as f32;
                let len = rh * (3. + (t % 3) as f32) * (0.8 + j.size * 0.3);
                let steps = len as i32;
                for s in 0..steps {
                    let f = s as f32 / len;
                    let sway =
                        (self.time * 1.4 - f * 5. + j.phase + t as f32).sin() * rw * 0.25 * f;
                    let x = cx + ox * (1. + f * 0.3) + sway + j.trail * rw * 1.2 * f * f;
                    let y = cy + s as f32;
                    let ch = if s % 5 == 0 {
                        '°'
                    } else if f > 0.75 {
                        '.'
                    } else {
                        ':'
                    };
                    r.dot(
                        x.round() as i32,
                        y as i32,
                        depth + 0.2,
                        ch,
                        scale(tint, bright * (0.8 - f * 0.6)),
                    );
                }
            }
        }
    }
}
// -------------------------------------------------------------------- cockpit
struct Rock {
    x: f32,
    y: f32,
    z: f32,
    r: f32,
    spin: f32,
    seed: u32,
}
struct Cockpit {
    stars: Vec<(f32, f32, f32)>,
    rocks: Vec<Rock>,
    time: f32,
    warp: f32,
    rng: Rng,
}
impl Cockpit {
    fn new() -> Self {
        let mut rng = Rng(rand::random());
        let stars = (0..220)
            .map(|_| {
                (
                    rng.next() * 2. - 1.,
                    rng.next() * 2. - 1.,
                    0.2 + rng.next() * 0.8,
                )
            })
            .collect();
        let rocks = (0..14)
            .map(|i| Rock {
                x: rng.next() * 8. - 4.,
                y: rng.next() * 5. - 2.5,
                z: 4. + i as f32 * 4.,
                r: 0.25 + rng.next() * 0.6,
                spin: rng.next() * TAU,
                seed: rng.0,
            })
            .collect();
        Self {
            stars,
            rocks,
            time: 0.,
            warp: 0.,
            rng,
        }
    }
    fn step(&mut self, dt: f32, sound: &Sound, gaze: (f32, f32)) {
        self.time += dt;
        self.warp += (sound.bass - self.warp) * (1. - (-dt * 3.).exp());
        let speed = 3. + self.warp * 10.;
        for s in &mut self.stars {
            s.2 -= dt * speed * 0.08;
            if s.2 < 0.05 {
                s.2 = 1.;
                s.0 = self.rng.next() * 2. - 1.;
                s.1 = self.rng.next() * 2. - 1.;
            }
        }
        for k in &mut self.rocks {
            k.z -= dt * speed;
            k.spin += dt * 0.8;
            // Steering drifts the field opposite to the cursor, like a yoke.
            k.x -= gaze.0 * dt * 0.8;
            k.y -= gaze.1 * dt * 0.5;
            if k.z < 0.6 {
                k.z = 56. + self.rng.next() * 8.;
                k.x = self.rng.next() * 8. - 4.;
                k.y = self.rng.next() * 5. - 2.5;
                k.r = 0.25 + self.rng.next() * 0.6;
                k.seed = self.rng.0;
            }
        }
    }
    fn draw(
        &self,
        r: &mut Raster,
        cols: usize,
        rows: usize,
        aspect: f32,
        sound: &Sound,
        gaze: (f32, f32),
    ) {
        let (cx, cy) = (cols as f32 * 0.5, rows as f32 * 0.47);
        let f = rows as f32 * 0.9;
        // Ringed planet far away, drifting with the steering.
        let px = cx + cols as f32 * 0.22 - gaze.0 * cols as f32 * 0.08;
        let py = cy - rows as f32 * 0.16 - gaze.1 * rows as f32 * 0.06;
        let pr = rows as f32 * 0.11;
        let span = |c: f32, r: f32, max: usize| {
            ((c - r).max(0.) as usize)..((c + r + 2.).clamp(0., max as f32) as usize)
        };
        for y in span(py, pr, rows) {
            for x in span(px, pr * aspect, cols) {
                let nx = (x as f32 - px) / (pr * aspect);
                let ny = (y as f32 - py) / pr;
                let d = nx * nx + ny * ny;
                if d <= 1. {
                    let light = ((1. - d).sqrt() * 0.7 - nx * 0.4 + 0.3).max(0.1);
                    let band = ((ny * 6. + nx * 0.5).sin() * 0.5 + 0.5) * 0.3;
                    r.dot(
                        x as i32,
                        y as i32,
                        90.,
                        if light > 0.6 {
                            '0'
                        } else if light > 0.35 {
                            'o'
                        } else {
                            '.'
                        },
                        scale((230, 170, 110), light * 0.8 + band),
                    );
                }
            }
        }
        for i in 0..=100 {
            let a = i as f32 / 100. * TAU;
            let (x, y) = (
                px + a.cos() * pr * 2.1 * aspect,
                py + a.sin() * pr * 0.35 + a.cos() * pr * 0.05,
            );
            let front = a.sin() > 0.;
            if front || (x - px).abs() > pr * aspect * 1.02 {
                r.dot(
                    x.round() as i32,
                    y.round() as i32,
                    if front { 85. } else { 95. },
                    '=',
                    scale((210, 190, 160), 0.55),
                );
            }
        }
        // Star streaks lengthen with warp.
        for &(sx, sy, sz) in &self.stars {
            let (x, y) = (
                cx + (sx - gaze.0 * 0.3) * f * aspect / sz,
                cy + (sy - gaze.1 * 0.2) * f / sz,
            );
            let len = 1. + self.warp * 8. / sz;
            let dx = (x - cx) / ((x - cx).abs() + (y - cy).abs() + 0.01);
            let dy = (y - cy) / ((x - cx).abs() + (y - cy).abs() + 0.01);
            for k in 0..len as i32 + 1 {
                let b = (1. - k as f32 / (len + 1.)) * (0.4 + (1. - sz) * 0.6);
                r.dot(
                    (x - dx * k as f32).round() as i32,
                    (y - dy * k as f32 * 0.5).round() as i32,
                    80.,
                    if len > 3. { '-' } else { '.' },
                    scale((220, 230, 255), b),
                );
            }
        }
        // Asteroids: tumbling irregular wireframe polygons with depth.
        for k in &self.rocks {
            let n = 7;
            let mut pts = vec![];
            for i in 0..n {
                let a = i as f32 / n as f32 * TAU + k.spin;
                let wob = 0.7 + variant(k.seed.wrapping_add(i as u32 * 31)) * 0.6;
                let (x, y) = (k.x + a.cos() * k.r * wob, k.y + a.sin() * k.r * wob * 0.8);
                pts.push((cx + x * f * aspect / k.z, cy + y * f / k.z));
            }
            let fade = (0.25 + 1.5 / k.z).min(1.);
            let color = scale((200, 180, 150), fade * (0.8 + sound.treble * 0.4));
            for i in 0..n {
                r.line(pts[i], pts[(i + 1) % n], k.z, color, true);
            }
            if k.z < 6. {
                let (mx, my) = (cx + k.x * f * aspect / k.z, cy + k.y * f / k.z);
                r.dot(mx as i32, my as i32, k.z - 0.01, '#', scale(color, 0.6));
            }
        }
        // Cockpit frame: a trapezoid window, struts, and a HUD at the bottom.
        let frame = scale((90, 140, 170), 0.9 + sound.treble * 0.3);
        let (l, rr, t, b) = (
            cols as f32 * 0.06,
            cols as f32 * 0.94,
            rows as f32 * 0.08,
            rows as f32 * 0.84,
        );
        r.line((l, b), (l + cols as f32 * 0.08, t), 1., frame, true);
        r.line((rr, b), (rr - cols as f32 * 0.08, t), 1., frame, true);
        r.line(
            (l + cols as f32 * 0.08, t),
            (rr - cols as f32 * 0.08, t),
            1.,
            frame,
            true,
        );
        r.line((cx, t), (cx, t + 2.), 1., frame, true);
        for y in 0..t as i32 {
            for x in 0..cols as i32 {
                r.dot(
                    x,
                    y,
                    0.5,
                    if (x + y) % 7 == 0 { '·' } else { ' ' },
                    (25, 35, 45),
                );
            }
        }
        for y in b as i32..rows as i32 {
            for x in 0..cols as i32 {
                r.dot(x, y, 0.5, ' ', (0, 0, 0));
            }
        }
        for x in 0..cols as i32 {
            r.dot(x, b as i32, 0.4, '═', frame);
        }
        // HUD readouts react to the music: speed, shields, cabin lights.
        let speed = 300. + self.warp * 1800. + (self.time * 2.).sin() * 15.;
        text(
            r,
            l as i32,
            b as i32 + 1,
            0.3,
            &format!("VEL {speed:5.0} km/s"),
            (120, 220, 255),
        );
        text(
            r,
            l as i32,
            b as i32 + 2,
            0.3,
            &format!("WARP {:4.2}", 1. + self.warp * 3.),
            if self.warp > 0.4 {
                (255, 160, 90)
            } else {
                (120, 220, 255)
            },
        );
        let bar = (cols as f32 * 0.25) as usize;
        text(
            r,
            (cx - bar as f32 * 0.5) as i32 - 8,
            b as i32 + 1,
            0.3,
            "SHIELD",
            (120, 220, 255),
        );
        for k in 0..bar {
            let lit = k as f32 / bar as f32
                <= 0.78 + (self.time * 0.7).sin() * 0.05 + sound.treble * 0.15;
            r.dot(
                (cx - bar as f32 * 0.5) as i32 + k as i32,
                b as i32 + 1,
                0.3,
                if lit { '▮' } else { '·' },
                if lit { (90, 230, 140) } else { (40, 50, 60) },
            );
        }
        text(
            r,
            (cx - bar as f32 * 0.5) as i32 - 8,
            b as i32 + 2,
            0.3,
            "CABIN",
            (120, 220, 255),
        );
        for k in 0..bar {
            let f2 = k as f32 / bar as f32;
            let on = sound.bass * (1. - f2) + sound.treble * f2 > (k % 5) as f32 * 0.12;
            r.dot(
                (cx - bar as f32 * 0.5) as i32 + k as i32,
                b as i32 + 2,
                0.3,
                if on { '▪' } else { '·' },
                if on { (255, 200, 120) } else { (40, 50, 60) },
            );
        }
        let dest = ["ORION", "VEGA", "OMARCHY-1", "SIRIUS"][((self.time / 20.) as usize) % 4];
        text(
            r,
            rr as i32 - 18,
            b as i32 + 1,
            0.3,
            &format!("DEST {dest:>9}"),
            (120, 220, 255),
        );
        text(
            r,
            rr as i32 - 18,
            b as i32 + 2,
            0.3,
            &format!(
                "T+{:02}:{:02}",
                (self.time / 60.) as u32 % 60,
                self.time as u32 % 60
            ),
            (120, 220, 255),
        );
        // Crosshair follows the cursor across the window.
        let (hx, hy) = (
            cx + gaze.0 * cols as f32 * 0.3,
            cy + gaze.1 * rows as f32 * 0.25,
        );
        let hud = scale((120, 255, 200), 0.8 + sound.bass * 0.4);
        for d in [-4, -3, 3, 4] {
            r.dot(hx as i32 + d, hy as i32, 0.2, '─', hud);
        }
        for d in [-2, 2] {
            r.dot(hx as i32, hy as i32 + d, 0.2, '│', hud);
        }
        r.dot(hx as i32, hy as i32, 0.2, '+', hud);
    }
}
const DIGITS: [[&str; 5]; 11] = [
    ["###", "# #", "# #", "# #", "###"],
    ["  #", "  #", "  #", "  #", "  #"],
    ["###", "  #", "###", "#  ", "###"],
    ["###", "  #", "###", "  #", "###"],
    ["# #", "# #", "###", "  #", "  #"],
    ["###", "#  ", "###", "  #", "###"],
    ["###", "#  ", "###", "# #", "###"],
    ["###", "  #", "  #", "  #", "  #"],
    ["###", "# #", "###", "# #", "###"],
    ["###", "# #", "###", "  #", "###"],
    ["   ", " # ", "   ", " # ", "   "],
];
// ----------------------------------------------------------------- sand clock
/// What music moves is measured in cells, so it has to grow with the grid to
/// look the same on screen at any resolution.
fn music_reach(rows: usize) -> f32 {
    (rows as f32 / 80.).max(0.6)
}
fn local_hms() -> (u32, u32, u32) {
    // SAFETY: plain libc calls with valid, owned buffers.
    unsafe {
        let t = libc::time(std::ptr::null_mut());
        let mut tm: libc::tm = std::mem::zeroed();
        libc::localtime_r(&t, &mut tm);
        (tm.tm_hour as u32, tm.tm_min as u32, tm.tm_sec as u32)
    }
}
struct SandGrain {
    x: f32,
    y: f32,
    vx: f32,
    vy: f32,
    slot: usize,
    /// Position inside the slot's pool; it fixes which glyph point it follows.
    index: usize,
    target: (f32, f32),
    /// While `time` is below this the grain is scattered and drifts freely.
    loose_until: f32,
    shade: f32,
    phase: f32,
    speed: f32,
    /// Fixed offset from the glyph point, so the sand never forms a grid.
    jitter: (f32, f32),
    size: f32,
}
/// Glyph points in scan order, relative to the character's own origin.
#[derive(Default)]
struct SandFont {
    glyphs: std::collections::HashMap<char, Vec<(i32, i32)>>,
    advance: i32,
    x0: i32,
    y0: i32,
}
impl SandFont {
    /// Rasterize a bold monospace face into grid cells. Cells are taller than
    /// wide, so the face is stretched by the cell aspect to keep its shapes.
    fn cairo(cols: usize, rows: usize, aspect: f32) -> Option<Self> {
        use gtk4::cairo::{
            Context, FontOptions, FontSlant, FontWeight, Format, HintMetrics, HintStyle,
            ImageSurface,
        };
        const CHARS: &str = "0123456789:";
        let aspect = aspect as f64;
        let measure = ImageSurface::create(Format::A8, 8, 8).ok()?;
        let cr = Context::new(&measure).ok()?;
        cr.select_font_face("monospace", FontSlant::Normal, FontWeight::Bold);
        cr.set_font_size(100.);
        let unit = cr.text_extents("0").ok()?.x_advance() / 100.;
        let size = (rows as f64 * 0.72).min(cols as f64 * 0.9 / 8. / (unit * aspect));
        let advance = (size * unit * aspect).round().max(1.) as i32;
        let height = (size * 1.3).ceil().max(1.) as i32;
        let mut font = Self {
            advance,
            x0: (cols as i32 - advance * 8) / 2,
            y0: (rows as i32 - height) / 2,
            ..Self::default()
        };
        let sample_x = (aspect.round() as usize).max(1);
        for ch in CHARS.chars() {
            let mut surface = ImageSurface::create(Format::A8, advance, height).ok()?;
            {
                let cr = Context::new(&surface).ok()?;
                cr.scale(aspect, 1.);
                // Hinting assumes square pixels and leaves spikes once stretched.
                let mut options = FontOptions::new().ok()?;
                options.set_hint_style(HintStyle::None);
                options.set_hint_metrics(HintMetrics::Off);
                cr.set_font_options(&options);
                cr.select_font_face("monospace", FontSlant::Normal, FontWeight::Bold);
                cr.set_font_size(size);
                let e = cr.font_extents().ok()?;
                cr.move_to(0., (height as f64 - e.height()) / 2. + e.ascent());
                cr.show_text(&ch.to_string()).ok()?;
            }
            surface.flush();
            let stride = surface.stride() as usize;
            let data = surface.data().ok()?;
            let mut points = vec![];
            for y in 0..height as usize {
                for x in 0..advance as usize {
                    // Cells are taller than wide: skip columns for a square,
                    // sparse lattice where each grain can be seen moving.
                    if x % sample_x == 0 && data[y * stride + x] > 120 {
                        points.push((x as i32, y as i32));
                    }
                }
            }
            if points.is_empty() {
                return None;
            }
            font.glyphs.insert(ch, points);
        }
        Some(font)
    }
    /// Without a usable font, fall back to the block digits.
    fn blocks(cols: usize, rows: usize) -> Self {
        let kx = ((cols as f32 * 0.82) / 31.).floor().max(1.) as i32;
        let ky = ((rows as f32 * 0.55) / 5.).floor().max(1.) as i32;
        let mut font = Self {
            advance: 4 * kx,
            x0: (cols as i32 - 31 * kx) / 2,
            y0: (rows as i32 - 5 * ky) / 2,
            ..Self::default()
        };
        for ch in "0123456789:".chars() {
            let d = ch.to_digit(10).map(|d| d as usize).unwrap_or(10);
            let mut points = vec![];
            for y in 0..5 * ky {
                for x in 0..3 * kx {
                    if DIGITS[d][(y / ky) as usize].as_bytes()[(x / kx) as usize] == b'#' {
                        points.push((x, y));
                    }
                }
            }
            font.glyphs.insert(ch, points);
        }
        font
    }
    fn cells(&self, ch: char, slot: usize) -> Vec<(f32, f32)> {
        let ox = self.x0 + slot as i32 * self.advance;
        self.glyphs[&ch]
            .iter()
            .map(|&(x, y)| ((ox + x) as f32, (self.y0 + y) as f32))
            .collect()
    }
}
/// The time built from sand, moved like a particle clock: each
/// of the eight character slots keeps a stable pool of grains. On a change
/// every grain of that slot follows the glyph point with its own index, so
/// the sand pours across the digit. The cursor scatters just what it touches.
struct SandClock {
    grains: Vec<SandGrain>,
    shown: Vec<char>,
    font: SandFont,
    cols: usize,
    rows: usize,
    time: f32,
    cursor: Option<(f32, f32)>,
    backdrop: SandBackdrop,
    back: Vec<BackGrain>,
    figure: usize,
    figure_clock: f32,
    /// Per cell: plate displacement and unit gradient of the current figure.
    field: Vec<[f32; 3]>,
    field_figure: usize,
    time_key: u32,
    rng: Rng,
}
impl SandClock {
    fn new(backdrop: SandBackdrop) -> Self {
        Self {
            backdrop,
            back: vec![],
            figure: 0,
            figure_clock: 0.,
            field: vec![],
            field_figure: usize::MAX,
            time_key: u32::MAX,
            grains: vec![],
            shown: vec![],
            font: SandFont::default(),
            cols: 0,
            rows: 0,
            time: 0.,
            cursor: None,
            rng: Rng(rand::random()),
        }
    }
    /// ParticleClockAssignment.assign: pools grow to fit the glyph and never
    /// shrink or get recreated; grain i follows glyph point i % n.
    fn retarget(&mut self, slot: usize, ch: char) {
        let cells = self.font.cells(ch, slot);
        let have = self.grains.iter().filter(|g| g.slot == slot).count();
        for index in have..cells.len() {
            let jitter = ((self.rng.next() - 0.5) * 0.8, (self.rng.next() - 0.5) * 0.8);
            let (x, y) = cells[index];
            self.grains.push(SandGrain {
                x: x + jitter.0,
                y: y + jitter.1,
                vx: 0.,
                vy: 0.,
                slot,
                index,
                target: (0., 0.),
                loose_until: 0.,
                shade: 0.55 + self.rng.next() * 0.45,
                phase: self.rng.next() * TAU,
                speed: 0.6 + self.rng.next() * 0.8,
                jitter,
                size: 0.5 + self.rng.next(),
            });
        }
        for g in self.grains.iter_mut().filter(|g| g.slot == slot) {
            let (x, y) = cells[g.index % cells.len()];
            g.target = (x + g.jitter.0, y + g.jitter.1);
        }
    }
    fn build(&mut self, text: &[char]) {
        self.grains.clear();
        for (slot, &ch) in text.iter().enumerate() {
            self.retarget(slot, ch);
        }
        for g in &mut self.grains {
            (g.x, g.y) = g.target;
        }
    }
    fn step(
        &mut self,
        dt: f32,
        sound: &Sound,
        pointer: Option<(f32, f32)>,
        cols: usize,
        rows: usize,
        aspect: f32,
    ) {
        self.time += dt;
        let (h, m, s) = local_hms();
        let key = h * 3600 + m * 60 + s;
        let resized = self.cols != cols || self.rows != rows;
        let text: Vec<char> = if key != self.time_key || resized {
            self.time_key = key;
            format!("{h:02}:{m:02}:{s:02}").chars().collect()
        } else {
            self.shown.clone()
        };
        if self.cols != cols || self.rows != rows {
            self.cols = cols;
            self.rows = rows;
            self.font =
                SandFont::cairo(cols, rows, aspect).unwrap_or_else(|| SandFont::blocks(cols, rows));
            self.build(&text);
            self.init_backdrop(cols, rows);
            self.shown = text.clone();
        }
        for slot in 0..text.len() {
            if text[slot] != self.shown[slot] {
                self.retarget(slot, text[slot]);
            }
        }
        self.shown = text;
        // The cursor only touches grains along the path it moved this frame.
        let cursor = pointer.map(|(x, y)| (x * cols as f32, y * rows as f32));
        if let (Some(a), Some(b)) = (self.cursor, cursor) {
            let (sx, sy) = (b.0 - a.0, b.1 - a.1);
            let len2 = sx * sx + sy * sy * 4.;
            if len2 > 0.04 {
                let (cvx, cvy) = (
                    (sx / dt.max(0.001)).clamp(-110., 110.),
                    (sy / dt.max(0.001)).clamp(-55., 55.),
                );
                let radius = (rows as f32 * 0.07).max(2.5);
                let hit = |x: f32, y: f32| {
                    let t = (((x - a.0) * sx + (y - a.1) * sy * 4.) / len2).clamp(0., 1.);
                    let dx = x - (a.0 + sx * t);
                    let dy = (y - (a.1 + sy * t)) * 2.;
                    let d = (dx * dx + dy * dy).sqrt();
                    (d < radius).then(|| {
                        let k = 1. - d / radius;
                        (
                            (cvx * 0.5 + dx / (d + 0.5) * 16.) * k,
                            (cvy * 0.5 + dy / (d + 0.5) * 8.) * k,
                        )
                    })
                };
                for g in &mut self.grains {
                    if let Some((vx, vy)) = hit(g.x, g.y) {
                        g.vx += vx;
                        g.vy += vy;
                        g.loose_until = self.time + 2.5 + self.rng.next();
                    }
                }
                for g in &mut self.back {
                    if let Some((vx, vy)) = hit(g.sx, g.sy) {
                        g.vx += vx;
                        g.vy += vy;
                    }
                }
            }
        }
        self.cursor = cursor;
        let friction = (-dt * 3.5).exp();
        // 14% of the way per 60 Hz frame; the same curve at any frame rate.
        let frames = dt * 60.;
        let pull = 1. - 0.86f32.powf(frames);
        let time = self.time;
        for g in &mut self.grains {
            if time < g.loose_until {
                // Scattered sand slides to a stop where it was thrown.
                g.vx *= friction;
                g.vy *= friction;
                g.x += g.vx * dt;
                g.y += g.vy * dt;
            } else {
                g.vx = 0.;
                g.vy = 0.;
                let (ex, ey) = (g.target.0 - g.x, g.target.1 - g.y);
                let d = (ex * ex + ey * ey).sqrt();
                if d > 0.3 {
                    // The sideways sway fades as the grain lands.
                    let sway = (d / 3.).min(1.) * frames;
                    g.x += ex * pull + (time * g.speed + g.phase).sin() * 0.14 * sway;
                    g.y += ey * pull + (time * g.speed * 0.9 + g.phase).sin() * 0.07 * sway;
                } else {
                    (g.x, g.y) = g.target;
                }
            }
            g.x = g.x.clamp(0., cols as f32 - 1.);
            g.y = g.y.clamp(0., rows as f32 - 1.);
        }
        self.step_backdrop(dt, sound, cols, rows, aspect);
    }
    fn draw(&self, r: &mut Raster, sound: &Sound) {
        self.draw_backdrop(r, sound);
        // The digits tremble on the bass, keeping their shape.
        let reach = music_reach(self.rows);
        let tremble = sound.bass * 0.9 * reach;
        let halo = reach.round().max(1.) as i32;
        for g in &self.grains {
            let ch = if g.size > 1.15 {
                '●'
            } else if g.size > 0.8 {
                '•'
            } else {
                '·'
            };
            // Every grain catches the light on its own beat; treble adds to it.
            let glint = (self.time * (1.2 + g.speed * 1.6) + g.phase).sin().max(0.).powi(6);
            let sparkle = glint * (0.7 + sound.treble * 0.9);
            let light = g.shade * (1.05 + sound.treble * 0.3 + sound.bass * 0.25) + sparkle * 0.6;
            let x = (g.x + (self.time * 41. + g.phase).sin() * tremble).round() as i32;
            let y = (g.y + (self.time * 37. + g.phase * 2.).sin() * tremble * 0.5).round() as i32;
            let colour = mix((255, 205, 120), (255, 250, 232), sparkle + sound.bass * 0.3);
            r.dot(x, y, 2. - g.size * 0.5, ch, scale(colour, light));
            if g.size > 0.8 {
                let glow = scale((255, 150, 55), 0.15 + sound.bass * 0.25 + sparkle * 0.3);
                for (dx, dy) in [(-2, 0), (2, 0), (0, -1), (0, 1)] {
                    r.dot(x + dx * halo, y + dy * halo, 3., '░', glow);
                }
            }
        }
    }
}
// ------------------------------------------------------- sand clock backdrops
#[derive(Clone, Copy, PartialEq)]
enum SandBackdrop {
    None,
    Chladni,
    Dunes,
    Galaxy,
}
struct BackGrain {
    x: f32,
    y: f32,
    vx: f32,
    vy: f32,
    /// Chladni: unused. Dunes: layer. Galaxy: orbit radius.
    a: f32,
    /// Dunes: vertical speed. Galaxy: orbit angle.
    b: f32,
    shade: f32,
    size: f32,
    /// Where the grain is on screen, for drawing and for the cursor.
    sx: f32,
    sy: f32,
}
const CHLADNI_MODES: [(f32, f32); 8] = [
    (1., 2.),
    (2., 3.),
    (1., 4.),
    (3., 4.),
    (2., 5.),
    (3., 5.),
    (1., 5.),
    (4., 5.),
];
/// Displacement of a square plate; sand gathers where this is zero.
fn chladni(u: f32, v: f32, (n, m): (f32, f32)) -> f32 {
    (n * PI * u).cos() * (m * PI * v).cos() - (m * PI * u).cos() * (n * PI * v).cos()
}
fn dune_ridge(layer: usize, x: f32, time: f32, rows: usize) -> f32 {
    let (base, drift) = ([0.72, 0.82, 0.92][layer], [1.5, 3., 5.][layer]);
    let w = x + time * drift + layer as f32 * 70.;
    rows as f32 * (base + (w * 0.021).sin() * 0.05 + (w * 0.053 + 1.3).sin() * 0.018)
}
impl SandClock {
    fn init_backdrop(&mut self, cols: usize, rows: usize) {
        self.back.clear();
        if self.backdrop == SandBackdrop::None {
            return;
        }
        let count = (cols * rows / 6).clamp(40, 5000);
        // On the dunes these are only the sand in the air: too many hide the ridges.
        let count = if self.backdrop == SandBackdrop::Dunes {
            count / 6
        } else {
            count
        };
        let reach = rows as f32 * 0.72;
        for i in 0..count {
            let (x, y) = (self.rng.next() * cols as f32, self.rng.next() * rows as f32);
            let (a, b) = match self.backdrop {
                SandBackdrop::Dunes => ((i % 3) as f32, 0.),
                SandBackdrop::Galaxy => {
                    let r = reach * self.rng.next().sqrt();
                    let arm = (i % 3) as f32 * TAU / 3.;
                    (
                        r,
                        arm + (1. + r / 5.).ln() * 4.6 + (self.rng.next() - 0.5) * 0.7,
                    )
                }
                _ => (0., 0.),
            };
            self.back.push(BackGrain {
                x,
                y,
                vx: 0.,
                vy: 0.,
                a,
                b,
                shade: 0.5 + self.rng.next() * 0.5,
                size: 0.5 + self.rng.next(),
                sx: x,
                sy: y,
            });
        }
    }
    /// The figure only changes every few seconds, so its trigonometry runs
    /// once per figure instead of six times per grain per frame.
    fn bake_field(&mut self, cols: usize, rows: usize, aspect: f32) {
        let mode = CHLADNI_MODES[self.figure];
        let side = rows as f32 * aspect;
        self.field.clear();
        self.field.reserve(cols * rows);
        for y in 0..rows {
            for x in 0..cols {
                let (u, v) = ((x as f32 + 0.5) / side, (y as f32 + 0.5) / rows as f32);
                let e = 0.01;
                let du = (chladni(u + e, v, mode) - chladni(u - e, v, mode)) / (2. * e);
                let dv = (chladni(u, v + e, mode) - chladni(u, v - e, mode)) / (2. * e);
                let len = (du * du + dv * dv).sqrt().max(0.001);
                self.field.push([chladni(u, v, mode), du / len, dv / len]);
            }
        }
        self.field_figure = self.figure;
    }
    fn step_backdrop(&mut self, dt: f32, sound: &Sound, cols: usize, rows: usize, aspect: f32) {
        let time = self.time;
        let friction = (-dt * 3.).exp();
        let (w, h) = (cols as f32, rows as f32);
        let reach = music_reach(rows);
        match self.backdrop {
            SandBackdrop::None => {}
            SandBackdrop::Chladni => {
                // Treble hurries the next figure; silence lets it hold.
                self.figure_clock += dt * (1. + sound.treble * 7.);
                if self.figure_clock >= 14. {
                    self.figure_clock = 0.;
                    self.figure = (self.figure + 1) % CHLADNI_MODES.len();
                }
                let shake = (2.2 + sound.bass * 16.) * reach;
                if self.field_figure != self.figure {
                    self.bake_field(cols, rows, aspect);
                }
                for g in &mut self.back {
                    let cell = g.y as usize * cols + g.x as usize;
                    let [f, nx, ny] = self.field[cell.min(self.field.len() - 1)];
                    let speed = 9. * reach * (f.abs() * 3.).min(1.) * -f.signum();
                    let rattle = shake * (0.3 + f.abs()) * dt * 7.;
                    g.x += (nx * speed * aspect + g.vx) * dt
                        + (self.rng.next() - 0.5) * rattle * aspect;
                    g.y += (ny * speed + g.vy) * dt + (self.rng.next() - 0.5) * rattle;
                    g.vx *= friction;
                    g.vy *= friction;
                    g.x = g.x.rem_euclid(w);
                    g.y = g.y.rem_euclid(h);
                    (g.sx, g.sy) = (g.x, g.y);
                }
            }
            SandBackdrop::Dunes => {
                let wind = (9. + sound.bass * 60.) * reach;
                for g in &mut self.back {
                    let layer = g.a as usize;
                    g.b += dt * 14. * reach;
                    g.x += (wind * (0.6 + g.shade * 0.6) + g.vx) * dt;
                    g.y += (g.b + g.vy) * dt;
                    g.vx *= friction;
                    g.vy *= friction;
                    if g.x >= w || g.y > dune_ridge(layer, g.x, time, rows) + 1. {
                        // Lifted off a crest again, more of it when the bass hits.
                        g.x = self.rng.next() * w;
                        g.y = dune_ridge(layer, g.x, time, rows);
                        g.b = -(5. + self.rng.next() * 9. + sound.bass * 18.) * reach;
                        g.vx = 0.;
                        g.vy = 0.;
                    }
                    (g.sx, g.sy) = (g.x, g.y);
                }
            }
            SandBackdrop::Galaxy => {
                let (cx, cy) = (w * 0.5, h * 0.5);
                let settle = (-dt * 0.8).exp();
                for g in &mut self.back {
                    g.b += dt * (0.05 + 0.5 / (1. + g.a * 0.09 / reach)) * (1. + sound.bass * 1.2);
                    g.x = (g.x + g.vx * dt) * settle;
                    g.y = (g.y + g.vy * dt) * settle;
                    g.vx *= friction;
                    g.vy *= friction;
                    // The bass travels outward through the arms as a ring.
                    let ring = 1. + sound.bass * 0.24 * (g.a * 0.12 / reach - time * 3.).sin().max(0.);
                    let r = g.a * ring;
                    g.sx = cx + r * g.b.cos() * aspect + g.x;
                    g.sy = cy + r * g.b.sin() * 0.72 + g.y;
                }
            }
        }
    }
    fn draw_backdrop(&self, r: &mut Raster, sound: &Sound) {
        let (cols, rows) = (self.cols, self.rows);
        let time = self.time;
        if self.backdrop == SandBackdrop::Dunes {
            for layer in 0..3 {
                let tint = [(80, 72, 185), (118, 88, 215), (165, 108, 240)][layer];
                let light = 0.62 + sound.treble * 0.3;
                for x in 0..cols {
                    let top = dune_ridge(layer, x as f32, time, rows) as usize;
                    for y in top..rows {
                        let seed = (x as u32).wrapping_mul(7349) ^ (y as u32).wrapping_mul(9151);
                        let depth = (y - top) as f32;
                        // A bright crest fading down, solid enough to hide the
                        // dune behind it so each ridge reads as a silhouette.
                        let chance = if depth < 2. {
                            0.9
                        } else {
                            0.42 - depth * 0.035
                        };
                        let grain = x % 2 == 0 && variant(seed) < chance;
                        let z = 9. - layer as f32;
                        if grain {
                            let k = (1.5 - depth * 0.09).max(0.45);
                            r.dot(x as i32, y as i32, z, '·', scale(tint, light * k));
                        } else {
                            r.dot(x as i32, y as i32, z, ' ', (0, 0, 0));
                        }
                    }
                }
            }
        }
        let (base, glow) = match self.backdrop {
            SandBackdrop::Chladni => ((70, 170, 235), (170, 235, 255)),
            SandBackdrop::Dunes => ((170, 150, 235), (225, 210, 255)),
            _ => ((150, 90, 255), (255, 120, 205)),
        };
        for (i, g) in self.back.iter().enumerate() {
            let twinkle = (time * 3. + i as f32).sin() > 0.97 - sound.treble * 0.2;
            let ch = if self.backdrop == SandBackdrop::Galaxy && twinkle {
                '+'
            } else if g.size > 1.25 {
                '•'
            } else {
                '·'
            };
            let light = (0.5 + g.shade * 0.35 + sound.treble * 0.25 + sound.bass * 0.12).min(1.);
            let tone = if self.backdrop == SandBackdrop::Galaxy {
                mix(base, glow, g.a / (self.rows as f32 * 0.72))
            } else {
                mix(base, glow, sound.treble * 0.6)
            };
            r.dot(
                g.sx.round() as i32,
                g.sy.round() as i32,
                6.,
                ch,
                scale(tone, light),
            );
        }
    }
}
// ------------------------------------------------------------------- scene
pub(crate) struct Showcase {
    kind: Kind,
    time: f32,
    gaze: (f32, f32),
    sound: Sound,
    terminal: Option<Terminal>,
    globe: Option<Globe>,
    aquarium: Option<Aquarium>,
    jelly: Option<Jellyfish>,
    cockpit: Option<Cockpit>,
    sand_clock: Option<SandClock>,
    size: std::cell::Cell<Option<(usize, usize, f32)>>,
    raster: std::cell::RefCell<Option<Raster>>,
}
impl Showcase {
    pub(crate) fn new(kind: Kind) -> Self {
        Self {
            kind,
            time: 0.,
            gaze: (0., 0.),
            sound: Sound::default(),
            terminal: (kind == Kind::Terminal).then(Terminal::new),
            globe: (kind == Kind::Globe).then(Globe::new),
            aquarium: (kind == Kind::Aquarium).then(Aquarium::new),
            jelly: (kind == Kind::Jellyfish).then(Jellyfish::new),
            cockpit: (kind == Kind::Cockpit).then(Cockpit::new),
            sand_clock: match kind {
                Kind::SandClock => Some(SandClock::new(SandBackdrop::None)),
                Kind::ChladniClock => Some(SandClock::new(SandBackdrop::Chladni)),
                Kind::DunesClock => Some(SandClock::new(SandBackdrop::Dunes)),
                Kind::GalaxyClock => Some(SandClock::new(SandBackdrop::Galaxy)),
                _ => None,
            },
            size: std::cell::Cell::new(None),
            raster: std::cell::RefCell::new(None),
        }
    }
    pub(crate) fn step(&mut self, dt: f32, pointer: Option<(f32, f32)>, sound: Sound) {
        let dt = dt.clamp(0., 0.1);
        let drive = |v: f32| 1. - (-3. * v.clamp(0., 1.)).exp();
        self.sound.follow(
            Sound {
                bass: drive(sound.bass),
                treble: drive(sound.treble),
            },
            dt,
        );
        self.time += dt;
        let target = pointer
            .map(|(x, y)| ((x - 0.5) * 2., (y - 0.5) * 2.))
            .unwrap_or((0., 0.));
        let k = 1. - (-dt * 4.).exp();
        self.gaze.0 += (target.0.clamp(-1., 1.) - self.gaze.0) * k;
        self.gaze.1 += (target.1.clamp(-1., 1.) - self.gaze.1) * k;
        // The clocks and the tank build themselves from the real grid.
        let Some((cols, rows, aspect)) = self.size.get() else {
            return;
        };
        let sound = self.sound;
        let gaze = self.gaze;
        if let Some(t) = &mut self.terminal {
            t.step(dt, &sound, rows);
        }
        if let Some(g) = &mut self.globe {
            g.step(dt, &sound);
        }
        if let Some(a) = &mut self.aquarium {
            a.step(dt, &sound, pointer.map(|_| gaze), cols as f32 / (rows as f32 * aspect).max(1.));
        }
        if let Some(j) = &mut self.jelly {
            j.step(dt, &sound, pointer.map(|_| gaze));
        }
        if let Some(c) = &mut self.cockpit {
            c.step(dt, &sound, gaze);
        }
        if let Some(c) = &mut self.sand_clock {
            c.step(dt, &sound, pointer, cols, rows, aspect);
        }
    }
    pub(crate) fn draw(&self, cols: usize, rows: usize, aspect: f32, out: &mut Vec<DirectCell>) {
        out.clear();
        if cols == 0 || rows == 0 {
            return;
        }
        let aspect = aspect.clamp(1., 4.);
        self.size.set(Some((cols, rows, aspect)));
        // The depth and cell buffers are about a megabyte: reuse them.
        let mut slot = self.raster.borrow_mut();
        let r = match slot.take() {
            Some(r) if r.fits(cols, rows) => r,
            _ => Raster::new(cols, rows),
        };
        let r = slot.insert(r);
        match self.kind {
            Kind::Terminal => {
                self.terminal
                    .as_ref()
                    .unwrap()
                    .draw(r, cols, rows, &self.sound, self.gaze)
            }
            Kind::Globe => self.globe.as_ref().unwrap().draw(
                r,
                cols,
                rows,
                aspect,
                &self.sound,
                self.gaze,
                self.time,
            ),
            Kind::Aquarium => {
                self.aquarium
                    .as_ref()
                    .unwrap()
                    .draw(r, cols, rows, aspect, &self.sound)
            }
            Kind::Jellyfish => {
                self.jelly
                    .as_ref()
                    .unwrap()
                    .draw(r, cols, rows, aspect, &self.sound)
            }
            Kind::Cockpit => {
                self.cockpit
                    .as_ref()
                    .unwrap()
                    .draw(r, cols, rows, aspect, &self.sound, self.gaze)
            }
            Kind::SandClock | Kind::ChladniClock | Kind::DunesClock | Kind::GalaxyClock => {
                self.sand_clock.as_ref().unwrap().draw(r, &self.sound)
            }
        }
        r.recycle(out);
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{Duration, Instant};

    #[test]
    fn fish_stay_in_the_tank_and_the_school_flees_the_cursor() {
        let mut tank = Aquarium::new();
        let loud = Sound { bass: 0.8, treble: 0.4 };
        for frame in 0..6000 {
            tank.step(1. / 30., &loud, Some((0.6, -0.2)), 1.8);
            for f in &tank.fish {
                assert!(f.x > 0. && f.x < 1.8 && f.y > SURFACE && f.y < SAND, "frame {frame}");
                assert!(f.face.abs() <= 1.0001);
            }
            assert!(tank.bubbles.len() <= 140);
        }
        assert_eq!(tank.fish.len(), SCHOOL + 8);
        let cursor = tank.cursor.unwrap();
        let close = tank.fish.iter().filter(|f| f.breed == Breed::Tetra).filter(|f| {
            ((f.x - cursor.0).powi(2) + (f.y - cursor.1).powi(2)).sqrt() < 0.05
        });
        assert_eq!(close.count(), 0);
        assert!(!tank.bubbles.is_empty());
    }

    #[test]
    fn everything_in_the_tank_answers_the_cursor() {
        let quiet = Sound { bass: 0., treble: 0. };
        let frame = |tank: &Aquarium| {
            let mut r = Raster::new(240, 70);
            tank.draw(&mut r, 240, 70, 1.75, &quiet);
            let mut cells = vec![];
            r.finish(&mut cells);
            cells
        };
        let mut tank = Aquarium::new();
        tank.step(1. / 30., &quiet, None, 1.8);
        // The chest opens and the sand rises with no music at all.
        for _ in 0..60 {
            tank.step(1. / 30., &quiet, Some((Aquarium::CHEST * 2. - 1., 0.72)), 1.8);
        }
        assert!(tank.lid > 0.5, "{}", tank.lid);
        assert!(!tank.dust.is_empty());
        let puffer = tank.fish.iter().position(|f| f.breed == Breed::Puffer).unwrap();
        let spot = (tank.fish[puffer].x / 1.8 * 2. - 1., tank.fish[puffer].y * 2. - 1.);
        for _ in 0..30 {
            tank.step(1. / 30., &quiet, Some(spot), 1.8);
        }
        assert!(tank.puffed > 0.4, "{}", tank.puffed);
        // Standing things catch the light.
        tank.fish.clear();
        tank.bubbles.clear();
        tank.dust.clear();
        tank.time = 0.;
        tank.cursor = None;
        let dark = frame(&tank);
        tank.cursor = Some((1.8 * 0.37, 0.75));
        let lit = frame(&tank);
        let moved = dark.iter().filter(|c| !lit.contains(c)).count();
        assert!(moved > 50, "{moved}");
    }

    #[test]
    fn jellyfish_flee_light_up_and_trail_from_the_cursor() {
        let quiet = Sound { bass: 0., treble: 0. };
        let mut sea = Jellyfish::new();
        sea.jellies[0].x = 0.5;
        sea.jellies[0].y = 0.5;
        let (mut x, y) = (0.5, 0.5);
        for _ in 0..40 {
            sea.step(1. / 30., &quiet, Some((-0.2, 0.)));
            x = sea.jellies[0].x;
        }
        assert!(x > 0.52, "{x} {y}");
        assert!(sea.jellies[0].fright > 0.3);
        assert!(sea.jellies[0].trail < 0.);
        for _ in 0..120 {
            sea.step(1. / 30., &quiet, None);
        }
        assert!(sea.jellies[0].fright < 0.05);
    }

    #[test]
    fn corals_sway_with_the_music_and_not_with_the_cursor() {
        let at = |tank: &Aquarium, bass: f32| {
            let mut r = Raster::new(240, 70);
            tank.draw(&mut r, 240, 70, 1.75, &Sound { bass, treble: 0. });
            let mut cells = vec![];
            r.finish(&mut cells);
            cells.into_iter().filter(|c| matches!(c.ch, '*')).map(|c| (c.x, c.y)).collect::<Vec<_>>()
        };
        let mut tank = Aquarium::new();
        tank.time = 3.;
        let still = at(&tank, 0.);
        assert!(!still.is_empty());
        tank.cursor = Some((0.65, 0.7));
        assert_eq!(still, at(&tank, 0.));
        assert_ne!(still, at(&tank, 0.9));
    }

    #[test]
    fn weed_ignores_the_cursor() {
        let quiet = Sound { bass: 0., treble: 0. };
        let mut tank = Aquarium::new();
        let weed = |tank: &Aquarium| {
            let mut r = Raster::new(240, 70);
            tank.frond(&mut r, 100., 60., 40., 4., 0.05, 1., 0, 13., true, 70., 1.75, &quiet);
            let mut cells = vec![];
            r.finish(&mut cells);
            cells
        };
        let free = weed(&tank);
        tank.cursor = Some((0.75, 0.6));
        assert!(!free.is_empty());
        assert_eq!(free, weed(&tank));
    }

    #[test]
    fn every_showcase_moves_stays_in_bounds_and_is_unique() {
        for name in Kind::ALL {
            let kind = Kind::parse(name).unwrap();
            for (cols, rows) in [(1, 1), (17, 13), (174, 50), (312, 93)] {
                let mut s = Showcase::new(kind);
                let (mut a, mut b) = (vec![], vec![]);
                s.draw(cols, rows, 1.75, &mut a);
                for _ in 0..90 {
                    s.step(
                        0.05,
                        Some((0.8, 0.2)),
                        Sound {
                            bass: 0.6,
                            treble: 0.5,
                        },
                    );
                }
                s.draw(cols, rows, 1.75, &mut b);
                assert!(
                    b.iter()
                        .all(|c| c.x < cols && c.y < rows && !matches!(c.ch, ',' | ';' | '\n')),
                    "{name}"
                );
                let keys = b
                    .iter()
                    .map(|c| (c.x, c.y))
                    .collect::<std::collections::HashSet<_>>();
                assert_eq!(keys.len(), b.len(), "{name}");
                if cols > 17 {
                    assert_ne!(a, b, "{name}");
                    assert!(b.len() > 100, "{name}: {}", b.len());
                }
            }
        }
    }
    #[test]
    fn silence_keeps_animating_and_music_changes_the_frame() {
        for name in Kind::ALL {
            let kind = Kind::parse(name).unwrap();
            let mut quiet = Showcase::new(kind);
            let (mut a, mut b, mut c) = (vec![], vec![], vec![]);
            for _ in 0..40 {
                quiet.step(0.05, None, Sound::default());
            }
            quiet.draw(174, 50, 1.75, &mut a);
            for _ in 0..40 {
                quiet.step(0.05, None, Sound::default());
            }
            quiet.draw(174, 50, 1.75, &mut b);
            assert_ne!(a, b, "{name} froze in silence");
            for _ in 0..40 {
                quiet.step(
                    0.05,
                    None,
                    Sound {
                        bass: 0.9,
                        treble: 0.9,
                    },
                );
            }
            quiet.draw(174, 50, 1.75, &mut c);
            assert_ne!(b, c, "{name} ignores music");
            assert!(quiet.sound.bass > 0.5);
        }
    }
    #[test]
    #[ignore = "manual cost report, run with --release --nocapture"]
    fn showcase_frame_cost() {
        let loud = Sound {
            bass: 0.6,
            treble: 0.5,
        };
        for name in Kind::ALL {
            let mut s = Showcase::new(Kind::parse(name).unwrap());
            let mut frame = vec![];
            s.draw(300, 90, 1.75, &mut frame);
            for _ in 0..240 {
                s.step(1. / 60., Some((0.4, 0.5)), loud);
            }
            let (mut step, mut draw) = (Duration::ZERO, Duration::ZERO);
            let frames = 300;
            for i in 0..frames {
                let pointer = (0.3 + (i as f32 * 0.03).sin() * 0.2, 0.5);
                let t = Instant::now();
                s.step(1. / 60., Some(pointer), loud);
                step += t.elapsed();
                let t = Instant::now();
                s.draw(300, 90, 1.75, &mut frame);
                draw += t.elapsed();
            }
            let ms = |d: Duration| d.as_secs_f64() * 1000. / frames as f64;
            println!(
                "COST {name:<20} step {:7.3} ms  draw {:7.3} ms  cells {}",
                ms(step),
                ms(draw),
                frame.len()
            );
        }
    }
    #[test]
    fn sand_backdrops_move_with_music_and_chladni_sand_finds_the_nodes() {
        let step = |c: &mut SandClock, s: Sound| c.step(0.05, &s, None, 174, 50, 1.75);
        let loud = Sound {
            bass: 1.,
            treble: 1.,
        };
        // Silence: Chladni sand gathers on the zero lines of the plate.
        let mut c = SandClock::new(SandBackdrop::Chladni);
        step(&mut c, Sound::default());
        let mode = CHLADNI_MODES[0];
        let spread = |c: &SandClock| {
            let side = 50. * 1.75;
            c.back
                .iter()
                .map(|g| chladni(g.x / side, g.y / 50., mode).abs())
                .sum::<f32>()
                / c.back.len() as f32
        };
        let start = spread(&c);
        for _ in 0..100 {
            step(&mut c, Sound::default());
        }
        assert!(spread(&c) < start * 0.5, "{} vs {start}", spread(&c));
        // Treble hurries the figures along.
        let quiet = c.figure;
        for _ in 0..100 {
            step(
                &mut c,
                Sound {
                    bass: 0.,
                    treble: 1.,
                },
            );
        }
        assert_ne!(c.figure, quiet);
        // Bass throws dune sand further and opens the galaxy ring.
        let mut calm = SandClock::new(SandBackdrop::Dunes);
        let mut windy = SandClock::new(SandBackdrop::Dunes);
        for _ in 0..40 {
            step(&mut calm, Sound::default());
            step(&mut windy, loud);
        }
        let height = |c: &SandClock| {
            c.back
                .iter()
                .map(|g| dune_ridge(g.a as usize, g.x, c.time, 50) - g.y)
                .sum::<f32>()
        };
        assert!(height(&windy) > height(&calm));
        let mut g = SandClock::new(SandBackdrop::Galaxy);
        step(&mut g, Sound::default());
        let at_rest: Vec<_> = g.back.iter().map(|b| b.a).collect();
        for _ in 0..40 {
            step(&mut g, loud);
        }
        assert!(g.back.iter().zip(&at_rest).all(|(b, a)| b.a == *a));
        assert!(g.back.iter().all(|b| b.sx.is_finite() && b.sy.is_finite()));
    }
    #[test]
    fn sand_clock_pours_within_its_digit_and_scatters_only_what_is_touched() {
        let mut c = Showcase::new(Kind::SandClock);
        let mut frame = vec![];
        c.draw(174, 50, 1.75, &mut frame);
        c.step(0.02, None, Sound::default());
        let home = |clock: &SandClock, slot: Option<usize>| {
            clock
                .grains
                .iter()
                .filter(|g| slot.map_or(true, |s| g.slot == s))
                .filter(|g| (g.x - g.target.0).abs() < 0.6 && (g.y - g.target.1).abs() < 0.6)
                .count()
        };
        let clock = c.sand_clock.as_ref().unwrap();
        let total = clock.grains.len();
        assert_eq!(home(clock, None), total, "the time starts formed");
        // A real font: digits differ, and an eight has more sand than a one.
        assert!(clock.font.glyphs[&'8'].len() > clock.font.glyphs[&'1'].len());
        let first_slot = home(clock, Some(0));
        let others: Vec<_> = clock
            .grains
            .iter()
            .filter(|g| g.slot != 4)
            .map(|g| g.target)
            .collect();
        // A change retargets every grain of that slot by index and no other slot.
        let clock = c.sand_clock.as_mut().unwrap();
        let digit = if clock.shown[4] == '1' { '8' } else { '1' };
        clock.retarget(4, digit);
        let cells = clock.font.cells(digit, 4);
        assert!(clock.grains.iter().filter(|g| g.slot == 4).all(|g| {
            let (x, y) = cells[g.index % cells.len()];
            (g.target.0 - x - g.jitter.0).abs() < 0.001
                && (g.target.1 - y - g.jitter.1).abs() < 0.001
        }));
        let same: Vec<_> = clock
            .grains
            .iter()
            .filter(|g| g.slot != 4)
            .map(|g| g.target)
            .collect();
        assert_eq!(others, same, "an unchanged digit was retargeted");
        let (left, right) = (
            (clock.font.x0 + 4 * clock.font.advance) as f32 - 3.,
            (clock.font.x0 + 5 * clock.font.advance) as f32 + 3.,
        );
        // It eases in with no burst: grains stay in the digit and land by 0.7 s.
        for _ in 0..42 {
            c.step(0.016, None, Sound::default());
            let clock = c.sand_clock.as_ref().unwrap();
            assert!(clock
                .grains
                .iter()
                .filter(|g| g.slot == 4)
                .all(|g| g.x >= left && g.x <= right));
        }
        let clock = c.sand_clock.as_ref().unwrap();
        assert_eq!(
            home(clock, Some(4)),
            clock.grains.iter().filter(|g| g.slot == 4).count()
        );
        // Sweeping across the minutes leaves the hours untouched.
        for i in 0..60 {
            let x = 0.45 + i as f32 * 0.0025;
            let y = 0.5 + (i as f32 * 0.7).sin() * 0.22;
            c.step(0.016, Some((x, y)), Sound::default());
        }
        let clock = c.sand_clock.as_ref().unwrap();
        assert_eq!(home(clock, Some(0)), first_slot, "untouched digit moved");
        assert!(home(clock, None) < total - 20, "cursor scattered nothing");
        for _ in 0..300 {
            c.step(0.016, Some((0.6, 0.5)), Sound::default());
        }
        let clock = c.sand_clock.as_ref().unwrap();
        assert!(home(clock, None) > total * 97 / 100, "sand never returned");
    }
    #[test]
    fn the_land_mask_has_continents() {
        assert!(land(40.7, -74.0) && land(51.5, -0.1) && land(-23.5, -46.6));
        assert!(!land(0., -150.) && !land(-50., 0.));
    }
}
