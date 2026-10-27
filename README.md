# Omarchy Audio Background

[![Demo del Omarchy Audio Background en X / Twitter](preview.png)](https://x.com/avillagran/status/2094998179628409318?s=20)

> **NOTE:** There are bugs. Please report them!

Animated, audio-reactive **desktop background** for [Omarchy](https://omarchy.org) / Hyprland, controlled from the Omarchy bar. It renders as a real Wayland `layer-shell` background that lives *below* app windows and the bar.

## Install

```sh
omarchy plugin add https://github.com/avillagran/omarchy-audio-background.git --enable --yes
```

Then restart the shell so the bar widget and service load:

```sh
omarchy-restart-shell
```

Left-click the ♪ bar icon to open the config panel.

## What it does

- One Rust binary opens a `wlr-layer-shell` background surface per monitor and draws the active effect with the GPU (EGL/OpenGL ES, partial damage, frame-rate capped). The old Vte terminal renderer is an optional build feature.
- Audio is captured from the default sink monitor (`parec`) and analyzed into volume, beat and per-band energy. The animation speed, brightness and color react live to the music.
- The config panel writes `state.json`; the background polls it every ~700 ms, so changes apply without a shell restart.

## Effects

`matrix` · `rain` · `wave` · `bars` · `donut` · `fire` · `starfield` · `life`

Plus a vendored catalog from [`ttfx`](https://github.com/omacom/ttfx): `beams`, `blackhole`, `bubbles`, `burn`, `colorshift`, `fireworks`, `rings`, `synthgrid`, `thunderstorm`, `vhstape`, `swarm`, `spray`.

Enable any subset in the panel; with more than one enabled the active effect rotates every 20 s.

## Configuration

Key settings in the panel:

- **Enabled** — turn the background on/off.
- **React to audio** — make the effect move with system audio.
- **Use theme colors** — pick the palette from the active Omarchy theme instead of the built-in effect colors (default on).
- **Effect** — choose and enable/disable individual effects.
- **Intensity** — 0–10, animation speed and trail length.
- **Audio reactivity** — how strongly audio affects speed/brightness/color.
- **Resolution** — render at lower or higher cell density.
- **Intro text** — word shown during the boot splash (default `OMARCHY`).
- **Intro size** — 1–3 scale for the boot splash.
- **Rotate seconds** — how long each effect stays when multiple are enabled.
- **Panel opacity** — transparency of the config card.

## Running as a systemd service

Use this **or** the built-in plugin service, not both:

```sh
cp ttfx-bg.service ~/.config/systemd/user/
systemctl --user daemon-reload
systemctl --user enable --now ttfx-bg.service
```

The unit uses `%h` and `bin/ttfx-bg-launch.sh`, so it works on any install with no edits.

## Local development

```sh
git clone https://github.com/avillagran/omarchy-audio-background.git
cd omarchy-audio-background
ln -sfn "$PWD" ~/.config/omarchy/plugins/io.github.avillagran.omarchy-audio-background
omarchy-shell shell rescanPlugins
```

The prebuilt binaries in `bin/` cover the supported architectures (`aarch64`, `x86_64`). To rebuild manually:

```sh
cd bin/ttfx-bg-rs
cargo build --release --no-default-features
cp target/release/ttfx-bg-rs ../ttfx-bg-rs-$(uname -m)
```

On x86_64 the build needs NASM 3.0 or newer (`pacman -S nasm`); without it the effect engine falls back to a Rust path that cannot render, so the build stops with a clear error. If `nasm` is not on `PATH`, point `NASM=/path/to/nasm` at it. `--no-default-features` leaves out the Vte terminal renderer, which otherwise pulls in `vte4`; use `cargo build --release` to keep it.

Runtime knobs (set in `bin/ttfx-bg-launch.sh`): `TTFX_BG_MAX_FPS` (`auto` by default: 20-60 fps chosen from the measured CPU cost per frame against `TTFX_BG_CPU_BUDGET`, default 0.08 of one core; or a fixed number), `TTFX_BG_RENDERER` (`gpu`, `direct` or `vte`), `TTFX_BG_WL_DAMAGE` (`full`, `bbox`, `all` or a maximum number of damage rectangles; default 2) and `TTFX_BG_WL_STATS=1` to log per-second render statistics to `/tmp/ttfx-bg-debug.log`.

## As a source for omarchy-bg-live

The same binary runs on its own as `omarchy-ttfx-layer` (a symlink or copy
with that name, or `--standalone`): one output, its own layer surface, no
controller lock. omarchy-bg-live's `ttfx` source uses it to
show these effects as the background or as the screensaver.

```sh
omarchy-ttfx-layer --output eDP-1 --layer overlay --keyboard exclusive \
  --effects matrix,decrypt,rings --rotate 30 --fps auto
```

`--layer`, `--keyboard`, `--output`, the size and `--namespace` default to
the `OMARCHY_BG_LIVE_*` variables omarchy-bg-live sets. `--probe` prints
`omarchy-ttfx-layer 1` and exits, so callers can tell this build from older
ones. With `--fps auto` the ceiling also respects `TTFX_BG_GPU_BUDGET`
(default 0.30) where the kernel reports GPU load (Intel RC6, AMD
`gpu_busy_percent`).

### Literal Omarchy branding file (standalone only)

```sh
omarchy-ttfx-layer --output eDP-1 --layer overlay --keyboard exclusive \
  --effects decrypt,rings --text-file "$HOME/.config/omarchy/branding/screensaver.txt"
```

`--text-file` reads a UTF-8 file once, before renderer startup, and passes its
contents directly to the existing ASM input parser. It preserves spaces, line
breaks and UTF-8 bytes without wordmark expansion or per-line recentering. The
engine's existing ANSI parsing, anchoring, clipping and effect behavior remain.
Even a file containing only `OMARCHY` is literal. An empty file stays empty.
Native Matrix/rain and other non-text effects do not animate this input.

Precedence is independent of argument order: explicit `--text` wins and retains
its existing canvas/wordmark behavior; an ignored `--text-file` is not read.
Otherwise `--text-file` wins over the normal `OMARCHY` default. A missing argument,
unreadable file or invalid UTF-8 produces a startup error naming the flag/path;
there is no silent substitution. Without either option the old behavior remains.
No default branding path is selected by the renderer: the Omarchy/source launcher
supplies it. The file is not watched or reread during rotation; restart to reload.

Standalone producers now keep their startup config fixed: intensity/speed/
reactivity do not read the audio-background plugin, and liveness follows their
stop flag rather than the plugin's running flag. Controller workers retain live
plugin settings. Omarchy theme colors still use the existing Omarchy theme paths.
This configuration correction is separate from the measured v1 optimization;
no new performance result is claimed.

For safe feature detection, invoke:

```sh
omarchy-ttfx-layer --standalone --probe --capabilities
```

This build prints exactly
`omarchy-ttfx-layer capabilities: text-file-v1 standalone-config-v1` and exits
before file/monitor/audio/Wayland startup. Capabilities takes priority over probe.
Older builds with probe support safely print `omarchy-ttfx-layer 1` for that
combined handshake; that response is not feature support. Compare the full line
with a timeout; do not use `--capabilities` alone to test an unknown old build.
`--probe` alone still prints its original line.

## Files

- `bin/ttfx-bg-rs/src/main.rs` — state polling, renderer lifecycle, monitor tracking.
- `bin/ttfx-bg-rs/src/wl_layer.rs`, `gpu_grid.rs`, `occlusion.rs` — Wayland layer surface, GPU cell grid renderer, pause when covered/locked/screen off.
- `bin/ttfx-bg-rs/src/fps_governor.rs`, `standalone.rs` — automatic frame-rate ceiling; the `omarchy-ttfx-layer` mode.
- `bin/ttfx-bg-rs/src/` + `ttfx-src/` — Rust source and vendored effect engine.
- `BarWidget.qml` — bar icon.
- `Panel.qml` — configuration panel.
- `Service.qml` — plugin service entry point.
- `manifest.json` — plugin manifest.
- `ttfx-bg.service` — optional systemd user service.

## Requirements

- Omarchy / Hyprland on Wayland.
- Same-arch prebuilt binary included; rebuild needs Rust, `gtk4`, `gtk4-layer-shell`, `pkg-config` and, on x86_64, `nasm`. `vte4` only for the optional Vte renderer.

## Renderer optimization candidate (isolated, not installed)

`TTFX_BG_RENDER_PROFILE=reference|clipped|axis` selects the same-binary
hardware acceptance candidate. An absent variable selects `clipped`, provisionally
chosen from Matrix screening. Explicit `reference` and invalid values (including
non-Unicode) select reference; `axis` remains an explicit alternative.
`clipped` and `axis` include the bounded Unicode glyph-slot cache and select
mutually exclusive GPU mapping backends. Historical optimization trial flags
cannot override this control. Explicit `reference` is the rollback path.

See [the v1 build, exact-image gates and A/B protocol](docs/performance/optimized-v1.md).
Performance evidence motivating these components is limited to Matrix at equal
resolution/parameters; the combined candidate still requires hardware acceptance.
Other effects retain their functional contract and require rotation/dismissal
smoke checks. No performance gain for the wider catalog or Amiga, measured watts,
unique-content 60fps, or installed/accepted optimization is claimed. Final-SHA45s confirmation and
catalog smoke checks remain pending; this default can be reverted if they fail.
