//! Six showcase backgrounds: a working terminal, a data globe, a city seen
//! from above in the rain, an aquarium of jellyfish, a starship cockpit and a
//! particle hourglass that writes the time. All share the cell raster, the
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
    CityRain,
    Jellyfish,
    Cockpit,
    Hourglass,
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
            "city-rain" => Self::CityRain,
            "jellyfish" => Self::Jellyfish,
            "cockpit" => Self::Cockpit,
            "hourglass" => Self::Hourglass,
            "sand-clock" => Self::SandClock,
            "sand-clock-chladni" => Self::ChladniClock,
            "sand-clock-dunes" => Self::DunesClock,
            "sand-clock-galaxy" => Self::GalaxyClock,
            _ => return None,
        })
    }
    #[cfg(test)]
    pub(crate) const ALL: [&'static str; 10] = [
        "live-terminal",
        "data-globe",
        "city-rain",
        "jellyfish",
        "cockpit",
        "hourglass",
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
// ------------------------------------------------------------------ city rain
struct Car {
    road: usize,
    pos: f32,
    dir: f32,
    speed: f32,
}
struct CityRain {
    cars: Vec<Car>,
    time: f32,
    flash: f32,
    rng: Rng,
}
// Variable block sizes repeat over a period so the city never looks tiled.
const COL_W: [f32; 6] = [44., 30., 58., 36., 50., 32.];
const ROW_H: [f32; 5] = [18., 11., 24., 14., 20.];
const STREET: f32 = 5.;
fn span_at(v: f32, sizes: &[f32]) -> (usize, f32, f32) {
    let period: f32 = sizes.iter().sum::<f32>() + STREET * sizes.len() as f32;
    let mut t = v.rem_euclid(period);
    let lap = (v.div_euclid(period)) as i32;
    for (i, s) in sizes.iter().enumerate() {
        if t < s + STREET {
            return ((lap * 97 + i as i32) as usize, t, *s);
        }
        t -= s + STREET;
    }
    (0, 0., sizes[0])
}
impl CityRain {
    fn new() -> Self {
        let mut rng = Rng(rand::random());
        let cars = (0..36)
            .map(|i| Car {
                road: i % 12,
                pos: rng.next(),
                dir: if i % 2 == 0 { 1. } else { -1. },
                speed: 0.03 + rng.next() * 0.04,
            })
            .collect();
        Self {
            cars,
            time: 0.,
            flash: 0.,
            rng,
        }
    }
    fn step(&mut self, dt: f32, sound: &Sound) {
        self.time += dt;
        for c in &mut self.cars {
            c.pos = (c.pos + c.dir * dt * c.speed * (1. + sound.bass * 1.5)).rem_euclid(1.);
        }
        self.flash = (self.flash - dt * 2.5).max(0.);
        if sound.treble > 0.55 && self.rng.next() < dt * 1.5 {
            self.flash = 1.;
        }
    }
    fn draw(&self, r: &mut Raster, cols: usize, rows: usize, sound: &Sound, gaze: (f32, f32)) {
        let pan_x = gaze.0 * cols as f32 * 0.05 + self.time * 1.2;
        let pan_y = gaze.1 * rows as f32 * 0.05 + self.time * 0.3;
        let base = 0.75 + self.flash * 0.8;
        // Streets first, then every block as a solid, outlined building or park.
        for y in 0..rows {
            for x in 0..cols {
                let wx = x as f32 + pan_x;
                let wy = y as f32 + pan_y;
                let (ci, fx, bw) = span_at(wx, &COL_W);
                let (ri, fy, bh) = span_at(wy, &ROW_H);
                let on_street = fx >= bw || fy >= bh;
                if on_street {
                    let ch = if fx >= bw && fy >= bh {
                        ' '
                    } else if fx >= bw {
                        if (fx - bw) as i32 == 2 && (wy as i32) % 2 == 0 {
                            '¦'
                        } else {
                            ' '
                        }
                    } else if (fy - bh) as i32 == 2 && (wx as i32) % 4 < 2 {
                        '-'
                    } else {
                        ' '
                    };
                    if ch != ' ' {
                        r.dot(
                            x as i32,
                            y as i32,
                            8.,
                            ch,
                            scale((120, 125, 140), base * 0.8),
                        );
                    }
                    continue;
                }
                let seed =
                    variant((ci as u32).wrapping_mul(2654435761) ^ (ri as u32).wrapping_mul(40503));
                if seed < 0.16 {
                    // Park: tree clumps, a pond and winding paths.
                    let t = noise(wx * 0.3, wy * 0.6);
                    let pond = ((fx - bw * 0.5) / (bw * 0.25)).powi(2)
                        + ((fy - bh * 0.5) / (bh * 0.3)).powi(2)
                        < 1.;
                    let (ch, c) = if pond {
                        (
                            '~',
                            scale(
                                (60, 120, 200),
                                base * (0.6 + 0.3 * (self.time * 2. + wx).sin().abs()),
                            ),
                        )
                    } else if t > 0.7 {
                        ('♣', scale((60, 170, 80), base))
                    } else if t > 0.58 {
                        ('"', scale((50, 130, 70), base))
                    } else if ((wx * 0.3 + wy * 0.5).sin() * 4.).fract().abs() < 0.08 {
                        ('·', scale((140, 130, 100), base * 0.7))
                    } else {
                        (' ', (0, 0, 0))
                    };
                    if ch != ' ' {
                        r.dot(x as i32, y as i32, 9., ch, c);
                    }
                    continue;
                }
                // Building: inset roof with a lit parapet on the top/left edges
                // (the light comes from the upper left), roof texture, vents.
                let inset = 1.;
                let (rx, ry) = (fx - inset, fy - inset);
                let (rw, rh) = (bw - 2. * inset, bh - 2. * inset);
                if rx < 0. || ry < 0. || rx >= rw || ry >= rh {
                    continue;
                }
                let height = 0.3 + seed * 0.7;
                let roof = mix((45, 55, 80), (90, 120, 140), height);
                let edge_x = rx < 1. || rx >= rw - 1.;
                let edge_y = ry < 1. || ry >= rh - 1.;
                let lit = rx < 1. || ry < 1.;
                let (ch, c) = if edge_x || edge_y {
                    (
                        if edge_x && edge_y {
                            '+'
                        } else if edge_y {
                            '═'
                        } else {
                            '║'
                        },
                        scale(roof, base * if lit { 1.8 } else { 1.1 }),
                    )
                } else {
                    let s2 = variant(
                        (ci as u32)
                            .wrapping_mul(7)
                            .wrapping_add((ri as u32).wrapping_mul(13))
                            .wrapping_mul(99991),
                    );
                    let unit = ((rx / rw - 0.5) * 2.).abs() < 0.35 + s2 * 0.3
                        && ((ry / rh - 0.5) * 2.).abs() < 0.3 + s2 * 0.3;
                    if unit && (rx as i32 + ry as i32 * 2) % 5 == 0 {
                        (
                            '▪',
                            scale((255, 215, 130), base * (0.35 + sound.treble * 0.5)),
                        )
                    } else if (rx + ry * 3. + s2 * 7.) as i32 % 9 == 0 {
                        ('·', scale(roof, base * 0.9))
                    } else {
                        ('░', scale(roof, base * 0.55))
                    }
                };
                r.dot(x as i32, y as i32, 9., ch, c);
            }
        }
        // Traffic lights where the streets cross, red/green per crossing.
        let (px, py) = (pan_x, pan_y);
        let mut sx = -(px.rem_euclid(1.));
        let mut xi = 0;
        while sx < cols as f32 + 60. {
            let (ci, fx, bw) = span_at(sx + px, &COL_W);
            let step = bw + STREET - fx;
            let cross_x = sx + bw - fx;
            let mut sy = -(py.rem_euclid(1.));
            while sy < rows as f32 + 30. {
                let (ri, fy, bh) = span_at(sy + py, &ROW_H);
                let cross_y = sy + bh - fy;
                let green = ((self.time * 0.4) + ci.wrapping_add(ri) as f32 * 0.9).sin() > 0.;
                let c = if green { (80, 240, 120) } else { (255, 70, 70) };
                r.dot(
                    (cross_x + 1.) as i32,
                    (cross_y + 1.) as i32,
                    6.,
                    '●',
                    scale(c, base),
                );
                r.dot(
                    (cross_x + STREET - 2.) as i32,
                    (cross_y + STREET - 2.) as i32,
                    6.,
                    '●',
                    scale(c, base),
                );
                sy += bh + STREET - fy;
            }
            sx += step;
            xi += 1;
            if xi > 40 {
                break;
            }
        }
        // Cars run down the street centre lines, with a headlight cone ahead.
        for c in &self.cars {
            let head = scale((255, 245, 210), 0.8 + sound.bass * 0.4);
            let tail = scale((255, 70, 60), 0.7 + sound.bass * 0.5);
            let beam = scale((255, 240, 180), 0.25);
            if c.road % 2 == 0 {
                // Vertical avenue: find the (road/2)-th street column on screen.
                let n = c.road / 2;
                let period: f32 = COL_W.iter().sum::<f32>() + STREET * COL_W.len() as f32;
                let mut xw = 0.;
                for i in 0..n % COL_W.len() {
                    xw += COL_W[i] + STREET;
                }
                xw += COL_W[n % COL_W.len()] + 1. + (n / COL_W.len()) as f32 * period;
                let x = (xw - pan_x).rem_euclid(period * 2.);
                if x >= cols as f32 {
                    continue;
                }
                let x = x as i32 + if c.dir > 0. { 1 } else { 0 };
                let y = (c.pos * (rows as f32 + 10.)) as i32 - 5;
                let d = c.dir as i32;
                r.dot(x, y, 4., '▮', head);
                r.dot(x, y - d, 4., '▮', scale(head, 0.6));
                r.dot(x, y - 2 * d, 4., '▮', tail);
                for k in 1..4 {
                    r.dot(x, y + d * k, 4.5, '·', scale(beam, 1.2 - k as f32 * 0.3));
                }
            } else {
                let n = c.road / 2;
                let period: f32 = ROW_H.iter().sum::<f32>() + STREET * ROW_H.len() as f32;
                let mut yw = 0.;
                for i in 0..n % ROW_H.len() {
                    yw += ROW_H[i] + STREET;
                }
                yw += ROW_H[n % ROW_H.len()] + 1. + (n / ROW_H.len()) as f32 * period;
                let y = (yw - pan_y).rem_euclid(period * 2.);
                if y >= rows as f32 {
                    continue;
                }
                let y = y as i32 + if c.dir > 0. { 1 } else { 0 };
                let x = (c.pos * (cols as f32 + 20.)) as i32 - 10;
                let d = c.dir as i32;
                for k in 0..3 {
                    r.dot(x - d * k, y, 4., '▬', scale(head, 1. - k as f32 * 0.25));
                }
                r.dot(x - 3 * d, y, 4., '▬', tail);
                for k in 1..6 {
                    r.dot(x + d * k, y, 4.5, '·', scale(beam, 1.2 - k as f32 * 0.2));
                }
            }
        }
        // Rain: diagonal streaks, denser with treble, splashes on roofs.
        let drops = 180 + (sound.treble * 260.) as usize;
        for i in 0..drops {
            let h = (i as u32).wrapping_mul(2654435761);
            let sx = ((h >> 5) & 4095) as f32 / 4096. * (cols as f32 + 20.);
            let speed = 28. + ((h >> 17) & 255) as f32 / 255. * 18.;
            let sy = (self.time * speed + ((h >> 20) & 4095) as f32) % (rows as f32 + 6.) - 3.;
            let x = sx - sy * 0.35 - 10.;
            if x >= 0. && x < cols as f32 && sy >= 0. && sy < rows as f32 {
                r.dot(
                    x as i32,
                    sy as i32,
                    3.,
                    '╱',
                    scale((175, 210, 245), 0.6 + sound.treble * 0.4 + self.flash * 0.5),
                );
            }
            if i % 3 == 0 {
                let ty = ((h >> 9) & 4095) as f32 / 4096. * rows as f32;
                let tx = ((h >> 21) & 2047) as f32 / 2048. * cols as f32;
                let life = (self.time * 3. + i as f32 * 0.37).fract();
                if life < 0.25 {
                    r.dot(
                        tx as i32,
                        ty as i32,
                        2.5,
                        if life < 0.12 { '°' } else { 'o' },
                        scale((170, 200, 230), 0.5 - life),
                    );
                }
            }
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
    fn step(&mut self, dt: f32, sound: &Sound, gaze: (f32, f32)) {
        self.time += dt;
        for j in &mut self.jellies {
            // Bell contraction on the bass drives a push upward.
            j.phase += dt * (0.9 + sound.bass * 1.5);
            let push = (j.phase.sin().max(0.)).powi(2);
            j.y -= dt * (0.012 + push * 0.05) / j.size;
            j.x += dt * j.drift * 0.02 + (self.time * 0.3 + j.phase).sin() * dt * 0.01;
            if j.y < -0.25 {
                j.y = 1.2;
                j.x = self.rng.next();
            }
            j.x = j.x.rem_euclid(1.);
        }
        let (tx, ty) = ((gaze.0 + 1.) * 0.5, (gaze.1 + 1.) * 0.5);
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
            let bright = 0.45 + j.size * 0.35 + sound.bass * 0.35;
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
                    let x = cx + ox * (1. + f * 0.3) + sway;
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
        for y in 0..rows {
            for x in 0..cols {
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
// ------------------------------------------------------------------ hourglass
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
struct Grain {
    x: f32,
    y: f32,
    vx: f32,
    vy: f32,
}
/// Off-screen frame: the glass is painted upright, then blitted rotated.
struct Cells {
    cols: usize,
    rows: usize,
    cells: Vec<Option<(f32, char, Color)>>,
}
impl Cells {
    fn dot(&mut self, x: i32, y: i32, z: f32, ch: char, color: Color) {
        if x < 0 || y < 0 || x as usize >= self.cols || y as usize >= self.rows {
            return;
        }
        let cell = &mut self.cells[y as usize * self.cols + x as usize];
        if cell.map_or(true, |c| z < c.0) {
            *cell = Some((z, ch, color));
        }
    }
}
/// Sand as per-column stacks. The top bulb stacks hang from the glass wall
/// downward toward the neck; the bottom bulb stacks grow up from the floor.
/// Both obey an angle of repose, so funnels and cones form by themselves.
struct Hourglass {
    cols: usize,
    rows: usize,
    top: Vec<i32>,
    bottom: Vec<i32>,
    falling: Vec<Grain>,
    time: f32,
    clock: String,
    cycle: f32,
    release: f32,
    /// Rotation progress 0..1; negative when the glass stands still.
    flip: f32,
    /// Sand dropping onto the funnel after a turn, 0..1; negative otherwise.
    settle: f32,
    /// Seconds with no grain moving through the neck.
    idle: f32,
    floor_cache: Vec<(i32, i32)>,
    rng: Rng,
}
impl Hourglass {
    fn new() -> Self {
        Self {
            cols: 0,
            rows: 0,
            top: vec![],
            bottom: vec![],
            falling: vec![],
            time: 0.,
            clock: String::new(),
            cycle: 0.,
            release: 0.,
            flip: -1.,
            settle: -1.,
            idle: 0.,
            floor_cache: vec![],
            rng: Rng(rand::random()),
        }
    }
    fn read_clock() -> String {
        let (h, m, _) = local_hms();
        format!("{h:02}:{m:02}")
    }
    fn geometry(cols: usize, rows: usize, aspect: f32) -> (f32, f32, f32, f32) {
        let h = rows as f32 * 0.86;
        let w = (h * 0.5 * aspect).min(cols as f32 * 0.8);
        (cols as f32 * 0.5, rows as f32 * 0.5, w * 0.5, h * 0.5)
    }
    /// Half width of the glass at a given row.
    fn half(y: f32, cy: f32, hw: f32, hh: f32) -> f32 {
        let t = ((y - cy).abs() / hh).min(1.);
        hw * (0.05 + 0.95 * t.powf(1.35))
    }
    /// Lowest glass row of the top bulb and the highest of the bottom bulb
    /// inside the given column, the "floor" for each stack.
    fn floors(&self, col: usize, _cx: f32, _cy: f32, _hw: f32, _hh: f32) -> (i32, i32) {
        self.floor_cache[col]
    }
    fn compute_floors(col: usize, cx: f32, cy: f32, hw: f32, hh: f32) -> (i32, i32) {
        let dx = (col as f32 - cx).abs();
        // Solve half(y) >= dx for the row closest to the neck.
        let mut top_floor = (cy - 1.) as i32;
        let mut bottom_floor = (cy + 1.) as i32;
        for y in (cy - hh) as i32..=(cy) as i32 {
            if Self::half(y as f32, cy, hw, hh) > dx + 0.5 {
                top_floor = top_floor.max(y);
            }
        }
        for y in (cy) as i32..=(cy + hh) as i32 {
            if Self::half(y as f32, cy, hw, hh) > dx + 0.5 {
                bottom_floor = bottom_floor.min(y);
                break;
            }
        }
        // Recompute top floor as the lowest inside row (walk down).
        let mut tf = (cy - hh) as i32 - 1;
        for y in (cy - hh) as i32..(cy) as i32 {
            if Self::half(y as f32, cy, hw, hh) > dx + 0.5 {
                tf = y;
            }
        }
        (tf, bottom_floor)
    }
    fn refill(&mut self, cx: f32, cy: f32, hw: f32, hh: f32) {
        self.cycle = 0.;
        self.falling.clear();
        self.top = vec![0; self.cols];
        self.bottom = vec![0; self.cols];
        // Fill the top bulb up to a flat surface at 35% from its top.
        let surface = (cy - hh * 0.8) as i32;
        for col in 0..self.cols {
            let (tf, _) = self.floors(col, cx, cy, hw, hh);
            if tf >= surface {
                self.top[col] = tf - surface + 1;
            }
        }
    }
    fn step(&mut self, dt: f32, sound: &Sound, cols: usize, rows: usize, aspect: f32) {
        self.time += dt;
        self.cycle += dt;
        if self.clock.is_empty() || (self.time as u32) % 20 == 0 && self.time.fract() < dt {
            self.clock = Self::read_clock();
        }
        let (cx, cy, hw, hh) = Self::geometry(cols, rows, aspect);
        if self.cols != cols || self.rows != rows {
            self.cols = cols;
            self.rows = rows;
            self.floor_cache = (0..cols)
                .map(|c| Self::compute_floors(c, cx, cy, hw, hh))
                .collect();
            self.refill(cx, cy, hw, hh);
        }
        if self.flip >= 0. {
            // The whole glass turns 180 degrees with the sand held in place;
            // only once it stands inverted does gravity take over.
            self.flip += dt / 2.6;
            if self.flip >= 1. {
                self.flip = -1.;
                self.settle = 0.;
                self.turn_over();
            }
            return;
        }
        if self.settle >= 0. {
            self.settle += dt / 0.7;
            if self.settle >= 1. {
                self.settle = -1.;
                self.cycle = 0.;
                self.idle = 0.;
            }
            return;
        }
        let shake = sound.bass * 2. + sound.treble;
        // Release grains from the neck columns of the top stacks.
        let neck: Vec<usize> = (0..cols).filter(|&c| (c as f32 - cx).abs() <= 1.).collect();
        self.release += dt * (28. + shake * 30.);
        let mut released = false;
        while self.release >= 1. {
            self.release -= 1.;
            let mut best = None;
            for &c in &neck {
                if self.top[c] > 0 && best.map_or(true, |b: usize| self.top[c] > self.top[b]) {
                    best = Some(c);
                }
            }
            if let Some(c) = best {
                released = true;
                self.top[c] -= 1;
                self.falling.push(Grain {
                    x: c as f32 + (self.rng.next() - 0.5) * 0.6,
                    y: cy,
                    vx: (self.rng.next() - 0.5) * (1. + shake * 3.),
                    vy: 4. + self.rng.next() * 3.,
                });
            }
        }
        // Angle of repose in both bulbs: a column passes grains to a lower
        // neighbour when its surface stands more than one cell above it.
        let passes = 4 + (shake * 6.) as usize;
        for _ in 0..passes {
            for col in 1..cols.saturating_sub(1) {
                let (tf, _) = self.floors(col, cx, cy, hw, hh);
                if self.top[col] > 0 {
                    let surf = tf - self.top[col];
                    for n in [col - 1, col + 1] {
                        let (nf, _) = self.floors(n, cx, cy, hw, hh);
                        if nf < (cy - hh) as i32 {
                            continue;
                        }
                        let nsurf = nf - self.top[n];
                        // Sand flows toward the neck: the deeper hole wins.
                        if nsurf > surf + 1 && self.top[col] > 0 && self.rng.next() < 0.5 {
                            self.top[col] -= 1;
                            self.top[n] += 1;
                        }
                    }
                }
            }
            for col in 1..cols.saturating_sub(1) {
                if self.bottom[col] == 0 {
                    continue;
                }
                let (_, bf) = self.floors(col, cx, cy, hw, hh);
                let cap = (cy + hh) as i32 - bf;
                let surf = (cy + hh) as i32 - self.bottom[col];
                for n in [col - 1, col + 1] {
                    let (_, nbf) = self.floors(n, cx, cy, hw, hh);
                    let ncap = (cy + hh) as i32 - nbf;
                    if ncap <= 0 {
                        continue;
                    }
                    let nsurf = (cy + hh) as i32 - self.bottom[n];
                    if nsurf > surf + 1 && self.bottom[n] < ncap && self.rng.next() < 0.5 {
                        self.bottom[col] -= 1;
                        self.bottom[n] += 1;
                    }
                }
                let _ = cap;
            }
        }
        // Falling grains land on the bottom stacks.
        let bottom_y = (cy + hh) as i32;
        let mut landed = vec![];
        for (i, g) in self.falling.iter_mut().enumerate() {
            g.vy += dt * 30.;
            g.vx *= 0.96;
            g.x += g.vx * dt;
            g.y += g.vy * dt;
            let col = (g.x.round() as i32).clamp(0, cols as i32 - 1) as usize;
            let surf = bottom_y - self.bottom[col];
            if g.y >= surf as f32 - 0.5 {
                self.bottom[col] += 1;
                landed.push(i);
            }
        }
        for i in landed.into_iter().rev() {
            self.falling.swap_remove(i);
        }
        // When the sand stops running, the hourglass turns itself over.
        if released || !self.falling.is_empty() {
            self.idle = 0.;
        } else {
            self.idle += dt;
            if self.idle > 1.2 {
                self.flip = 0.;
            }
        }
    }
    /// After 180 degrees each bulb is the other one, mirrored left to right.
    fn turn_over(&mut self) {
        let cols = self.cols;
        let mut top = vec![0; cols];
        let mut bottom = vec![0; cols];
        for col in 0..cols {
            let m = (cols - col).min(cols - 1);
            top[m] += self.bottom[col];
            bottom[m] += self.top[col];
        }
        self.top = top;
        self.bottom = bottom;
    }
    fn draw(&self, r: &mut Raster, cols: usize, rows: usize, aspect: f32, sound: &Sound) {
        let (cx, cy, _, _) = Self::geometry(cols, rows, aspect);
        let mut frame = Cells {
            cols,
            rows,
            cells: vec![None; cols * rows],
        };
        self.paint(&mut frame, cols, rows, aspect, sound);
        if self.flip < 0. {
            for y in 0..rows {
                for x in 0..cols {
                    if let Some((z, ch, color)) = frame.cells[y * cols + x] {
                        r.dot(x as i32, y as i32, z, ch, color);
                    }
                }
            }
            return;
        }
        // Rotate in screen space, sampling backwards so no holes appear.
        let t = self.flip;
        let eased = t * t * (3. - 2. * t);
        let (sin, cos) = (eased * PI).sin_cos();
        for y in 0..rows {
            for x in 0..cols {
                let dx = (x as f32 - cx) / aspect;
                let dy = y as f32 - cy;
                let sx = (cx + (dx * cos + dy * sin) * aspect).round() as i32;
                let sy = (cy - dx * sin + dy * cos).round() as i32;
                if sx < 0 || sy < 0 || sx as usize >= cols || sy as usize >= rows {
                    continue;
                }
                if let Some((z, ch, color)) = frame.cells[sy as usize * cols + sx as usize] {
                    // Slanted glyphs would point the wrong way mid-turn.
                    let ch = match ch {
                        '╱' | '╲' | '┃' if sin.abs() > 0.05 => '▪',
                        c => c,
                    };
                    r.dot(x as i32, y as i32, z, ch, color);
                }
            }
        }
    }
    fn paint(&self, r: &mut Cells, cols: usize, rows: usize, aspect: f32, sound: &Sound) {
        let (cx, cy, hw, hh) = Self::geometry(cols, rows, aspect);
        // Digits: a large 6x10 time inside the bottom sand, as brighter grains.
        let chars: Vec<char> = self.clock.chars().collect();
        let gw = chars.len() as i32 * 8 - 2;
        let x0 = cx as i32 - gw / 2;
        let y0 = (cy + hh) as i32 - 13;
        let digit_cell = |x: i32, y: i32| {
            if y < y0 || y >= y0 + 10 || x < x0 {
                return false;
            }
            let i = ((x - x0) / 8) as usize;
            let rx = (x - x0) % 8;
            if i >= chars.len() || rx >= 6 {
                return false;
            }
            let d = chars[i].to_digit(10).map(|d| d as usize).unwrap_or(10);
            DIGITS[d][((y - y0) / 2) as usize].as_bytes()[(rx / 2) as usize] == b'#'
        };
        let bottom_y = (cy + hh) as i32;
        let ceiling = (2. * cy) as i32 - bottom_y;
        // The digits sink back into plain sand before the glass starts to tip.
        let digits = if self.flip >= 0. {
            (1. - self.flip * 5.).max(0.)
        } else if self.settle >= 0. {
            0.
        } else {
            1.
        };
        for col in 0..cols.min(self.bottom.len()) {
            let (tf, _) = self.floors(col, cx, cy, hw, hh);
            // Fresh from a turn, each column still hangs at the ceiling and
            // drops onto the funnel.
            let drop = if self.settle >= 0. {
                let gap = (tf - self.top[col] + 1 - ceiling).max(0) as f32;
                (gap * (1. - self.settle * self.settle)) as i32
            } else {
                0
            };
            for k in 0..self.top[col] {
                let y = tf - k - drop;
                let depth = k as f32 / 12.;
                r.dot(
                    col as i32,
                    y,
                    1.,
                    if k == self.top[col] - 1 { '▒' } else { '▓' },
                    scale(mix((255, 200, 110), (200, 120, 60), depth.min(1.)), 0.85),
                );
            }
            for k in 0..self.bottom[col] {
                let y = bottom_y - k;
                let hot = digit_cell(col as i32, y) && digits > 0.;
                let sand = scale(
                    mix((200, 120, 60), (255, 190, 100), (k as f32 / 14.).min(1.)),
                    0.8 + sound.bass * 0.2,
                );
                let c = if hot {
                    mix(
                        sand,
                        scale((255, 240, 170), 1. + sound.treble * 0.2),
                        digits,
                    )
                } else {
                    sand
                };
                r.dot(
                    col as i32,
                    y,
                    1.,
                    if hot {
                        '█'
                    } else if k == self.bottom[col] - 1 {
                        '▒'
                    } else {
                        '▓'
                    },
                    c,
                );
            }
        }
        for g in &self.falling {
            r.dot(
                g.x.round() as i32,
                g.y.round() as i32,
                0.9,
                if g.vy > 9. { '¦' } else { '·' },
                (255, 215, 130),
            );
        }
        // Glass and frame drawn last, on top.
        let glass = scale((160, 200, 230), 0.8 + sound.treble * 0.2);
        for y in (cy - hh) as i32..=(cy + hh) as i32 {
            let half = Self::half(y as f32, cy, hw, hh);
            let (l, rr) = ((cx - half).round() as i32, (cx + half).round() as i32);
            r.dot(l, y, 0.5, if (y as f32) > cy { '╱' } else { '╲' }, glass);
            r.dot(rr, y, 0.5, if (y as f32) > cy { '╲' } else { '╱' }, glass);
        }
        let wood = (150, 105, 70);
        for x in (cx - hw) as i32 - 2..=(cx + hw) as i32 + 2 {
            for dy in [-2, -1] {
                r.dot(
                    x,
                    (cy - hh) as i32 + dy,
                    0.4,
                    if dy == -2 { '▀' } else { '█' },
                    wood,
                );
            }
            for dy in [1, 2] {
                r.dot(
                    x,
                    (cy + hh) as i32 + dy,
                    0.4,
                    if dy == 2 { '▄' } else { '█' },
                    wood,
                );
            }
        }
        for x in [(cx - hw) as i32 - 3, (cx + hw) as i32 + 3] {
            for y in (cy - hh) as i32 - 2..=(cy + hh) as i32 + 2 {
                r.dot(x, y, 0.4, '┃', scale(wood, 0.8));
            }
        }
    }
}
// ----------------------------------------------------------------- sand clock
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
        // With a backdrop the digits tremble on the bass, keeping their shape.
        let tremble = if self.backdrop == SandBackdrop::None {
            0.
        } else {
            sound.bass * 0.9
        };
        for g in &self.grains {
            let ch = if g.size > 1.15 {
                '●'
            } else if g.size > 0.8 {
                '•'
            } else {
                '·'
            };
            let light = g.shade * (0.95 + sound.treble * 0.2 + sound.bass * 0.1);
            r.dot(
                (g.x + (self.time * 41. + g.phase).sin() * tremble).round() as i32,
                (g.y + (self.time * 37. + g.phase * 2.).sin() * tremble * 0.5).round() as i32,
                2. - g.size * 0.5,
                ch,
                scale((255, 205, 120), light),
            );
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
        match self.backdrop {
            SandBackdrop::None => {}
            SandBackdrop::Chladni => {
                // Treble hurries the next figure; silence lets it hold.
                self.figure_clock += dt * (1. + sound.treble * 7.);
                if self.figure_clock >= 14. {
                    self.figure_clock = 0.;
                    self.figure = (self.figure + 1) % CHLADNI_MODES.len();
                }
                let shake = 2.2 + sound.bass * 16.;
                if self.field_figure != self.figure {
                    self.bake_field(cols, rows, aspect);
                }
                for g in &mut self.back {
                    let cell = g.y as usize * cols + g.x as usize;
                    let [f, nx, ny] = self.field[cell.min(self.field.len() - 1)];
                    let speed = 9. * (f.abs() * 3.).min(1.) * -f.signum();
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
                let wind = 9. + sound.bass * 60.;
                for g in &mut self.back {
                    let layer = g.a as usize;
                    g.b += dt * 14.;
                    g.x += (wind * (0.6 + g.shade * 0.6) + g.vx) * dt;
                    g.y += (g.b + g.vy) * dt;
                    g.vx *= friction;
                    g.vy *= friction;
                    if g.x >= w || g.y > dune_ridge(layer, g.x, time, rows) + 1. {
                        // Lifted off a crest again, more of it when the bass hits.
                        g.x = self.rng.next() * w;
                        g.y = dune_ridge(layer, g.x, time, rows);
                        g.b = -(5. + self.rng.next() * 9. + sound.bass * 18.);
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
                    g.b += dt * (0.05 + 0.5 / (1. + g.a * 0.09)) * (1. + sound.bass * 1.2);
                    g.x = (g.x + g.vx * dt) * settle;
                    g.y = (g.y + g.vy * dt) * settle;
                    g.vx *= friction;
                    g.vy *= friction;
                    // The bass travels outward through the arms as a ring.
                    let ring = 1. + sound.bass * 0.24 * (g.a * 0.12 - time * 3.).sin().max(0.);
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
    city: Option<CityRain>,
    jelly: Option<Jellyfish>,
    cockpit: Option<Cockpit>,
    hourglass: Option<Hourglass>,
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
            city: (kind == Kind::CityRain).then(CityRain::new),
            jelly: (kind == Kind::Jellyfish).then(Jellyfish::new),
            cockpit: (kind == Kind::Cockpit).then(Cockpit::new),
            hourglass: (kind == Kind::Hourglass).then(Hourglass::new),
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
        // The hourglass needs the real grid before it pours its sand.
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
        if let Some(c) = &mut self.city {
            c.step(dt, &sound);
        }
        if let Some(j) = &mut self.jelly {
            j.step(dt, &sound, gaze);
        }
        if let Some(c) = &mut self.cockpit {
            c.step(dt, &sound, gaze);
        }
        if let Some(h) = &mut self.hourglass {
            h.step(dt, &sound, cols, rows, aspect);
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
            Kind::CityRain => {
                self.city
                    .as_ref()
                    .unwrap()
                    .draw(r, cols, rows, &self.sound, self.gaze)
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
            Kind::Hourglass => {
                self.hourglass
                    .as_ref()
                    .unwrap()
                    .draw(r, cols, rows, aspect, &self.sound)
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
    fn hourglass_sand_settles_into_the_time_and_land_mask_has_continents() {
        let mut h = Showcase::new(Kind::Hourglass);
        let mut frame = vec![];
        h.draw(174, 50, 1.75, &mut frame);
        for _ in 0..1200 {
            h.step(0.05, None, Sound::default());
        }
        let hg = h.hourglass.as_ref().unwrap();
        let settled: i32 = hg.bottom.iter().sum();
        let remaining: i32 = hg.top.iter().sum();
        assert!(
            settled > 60,
            "settled {settled} remaining {remaining} falling {}",
            hg.falling.len()
        );
        let peak = hg.bottom.iter().max().copied().unwrap_or(0);
        let edge = hg.bottom[hg.cols / 2 - 12];
        assert!(
            peak > edge,
            "cone should peak at the centre: {peak} vs {edge}"
        );
        assert_eq!(hg.clock.len(), 5);
        // Once the sand stops running the glass turns over, and none is lost.
        let mut h = Showcase::new(Kind::Hourglass);
        h.draw(174, 50, 1.75, &mut frame);
        h.step(0.05, None, Sound::default());
        let hg = h.hourglass.as_ref().unwrap();
        let total: i32 =
            hg.top.iter().sum::<i32>() + hg.bottom.iter().sum::<i32>() + hg.falling.len() as i32;
        let mut flipped = false;
        let mut sideways = 0;
        for _ in 0..8000 {
            h.step(0.05, None, Sound::default());
            let hg = h.hourglass.as_ref().unwrap();
            if hg.flip >= 0. {
                flipped = true;
                if (hg.flip - 0.5).abs() < 0.03 {
                    // At the halfway point the glass really lies on its side.
                    h.draw(174, 50, 1.75, &mut frame);
                    let (lo, hi) = frame
                        .iter()
                        .fold((usize::MAX, 0), |m, c| (m.0.min(c.y), m.1.max(c.y)));
                    let (xl, xh) = frame
                        .iter()
                        .fold((usize::MAX, 0), |m, c| (m.0.min(c.x), m.1.max(c.x)));
                    sideways = (xh - xl) as i32 - (hi - lo) as i32 * 2;
                }
            }
            let hg = h.hourglass.as_ref().unwrap();
            if flipped && hg.flip < 0. {
                let now: i32 = hg.top.iter().sum::<i32>() + hg.bottom.iter().sum::<i32>();
                assert_eq!(now, total, "sand lost in the turn");
                assert!(
                    hg.top.iter().sum::<i32>() > 100,
                    "sand must be on top again"
                );
                assert!(sideways > 0, "glass did not rotate: {sideways}");
                break;
            }
        }
        assert!(flipped, "hourglass never turned over");
        assert!(land(40.7, -74.0) && land(51.5, -0.1) && land(-23.5, -46.6));
        assert!(!land(0., -150.) && !land(-50., 0.));
    }
}
