# omarchy-ttfx-layer

The [ttfx](https://github.com/omacom/ttfx) text effects drawn by the GPU on a
Wayland layer surface: one copy per output, below the windows as a background
or over everything as a screensaver. It is the `ttfx` source of
[omarchy-ambient](https://github.com/avillagran/omarchy-ambient), the live
background and screensaver engine for Omarchy.

```text
omarchy-ttfx-layer --output <name> --layer bottom|overlay --keyboard none|exclusive
                   [--effects a,b,c] [--rotate <seconds>] [--fps auto|<n>]
                   [--text <wordmark> | --text-file <UTF-8 path>] [--audio] [--namespace <ns>]
                   [--width <px> --height <px> --scale <n>] [--resolution <1-8>]
```

The effects run in-process on the vendored `ttfx` engine (`ttfx-src/`; on
x86-64 its NASM tiers are assembled when NASM is present). Glyphs come from
Pango and are cached, drawing goes through EGL and OpenGL ES, and only the
damaged region is presented. `--fps auto` picks the frame-rate ceiling from the
measured CPU cost of the effect, between 20 and 60 fps. `--text-file` draws a UTF-8 text such as Omarchy's
`~/.config/omarchy/branding/screensaver.txt`; `--audio` makes the effects react
to what is playing.

`omarchy-ttfx-layer --standalone --probe --capabilities` prints
`omarchy-ttfx-layer capabilities: text-file-v1 standalone-config-v1`, which is
how omarchy-ambient recognises a build it can use.

## Build

The crate is `bin/ttfx-bg-rs`; the binary takes its standalone mode from the
name it is started under, so install it as `omarchy-ttfx-layer`:

```sh
cd bin/ttfx-bg-rs
cargo build --release --no-default-features
install -Dm755 target/release/ttfx-bg-rs ~/.local/bin/omarchy-ttfx-layer
```

It needs Rust, GTK 4, gtk4-layer-shell, Pango and Cairo, and on x86-64 NASM 3
for the assembly tiers (without it the engine builds without them). The `vte`
feature, off above, adds the terminal renderer of the audio background plugin
and links libvte. `packaging/arch/PKGBUILD` builds the package for x86_64 and
aarch64.

## Tests

```sh
cd bin/ttfx-bg-rs
cargo test --release --no-default-features -- --test-threads=1
```

The tests share FreeType state, so they run one at a time.

`Panel.qml`, `bin/write_state.sh` and `bin/ttfx-bg-launch.sh` belong to the audio background plugin and stay because the binary's tests exercise them. `docs/` keeps the integration notes and the measurements behind the
renderer's defaults; `bench/` has the scripts used for them, including the
comparison with Omarchy's native screensaver.

## License

MIT. `ttfx-src/` keeps its own MIT license from omacom and the TerminalTextEffects
project it was ported from.
