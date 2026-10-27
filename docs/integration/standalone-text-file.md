# Standalone literal branding input

Isolated auxiliary based on validated runtime2307875. CLI opt-in only; no default,
profile, renderer, simulation, pacing, sleep, plugin preference or ASM ABI changes.
V1/compact/research snapshots and installations remain untouched.

## Contract

`omarchy-ttfx-layer --text-file PATH` reads UTF-8 once during option parsing,
before monitor lookup, signals, frame governor or worker startup. File contents
are passed directly to the ASM input parser, with no trim, wordmark expansion,
per-line centering or glyph restyling. Empty input stays empty; a literal
`OMARCHY` file stays the seven characters. Engine ANSI parsing, anchoring,
clipping and animation semantics still apply. It is not a pixel-preserving
positioning API or file watcher. Text-independent native effects (Matrix/rain)
do not consume it. File content is retained for effect rotation; restart reloads.

Explicit `--text` wins in either argument order, including empty `--text`.
The ignored file is not opened. Existing `--text` canvas/wordmark behavior and
absence defaultOMARCHY remain. File-only errors (missing path, missing/unreadable
file/directory, invalid UTF-8) abort with a readable error naming the option and
path; no fallback to the default wordmark. Explicit integration fallback can
choose `--text` instead. The launcher owns path selection; the renderer does not
infer Omarchy paths or read a branding preference from the plugin.

The old entry point wraps the new optional-input route with None. Only
standalone passes the file override; no Config/persisted schema field is added.
No vendored ttfx or sources/ttfx changes. Probe stays `omarchy-ttfx-layer 1`:
that marker alone does NOT prove --text-file support in an older binary, which
may ignore unknown flags. Integration must use this candidate build explicitly
until capability/version negotiation is handled separately.

## Local validation

Normal full suite193passed/17ignored (17library+157binary+19integration;
subprocess helpers not counted twice). Focused standalone8passed/1ignored.
Explicit `text_file_literal_input_reaches_real_asm`1passed: parses a real
multiline ASCII file, sends the same production direct-input selection into real
ASM print, checks emitted glyphs and relative x/y alignment. It uses virtual
clock and seed12345, no Wayland surface and no CPU benchmark; unavailable ASM
fails rather than silently passing. Parser tests cover UTF-8, tabs, CRLF, leading/
trailing spaces/newlines, empty input, literal OMARCHY, precedence in both orders,
ignored unreadable file, missing path/file, directory and invalid UTF-8.
Local ARM release passed. No x86 or real compositor branding claim yet.
Historical intermittent ARM parallel F_GETFD failures remain unresolved; no
frame_wakeup changes and current passing suite is not a fix claim.

## Reproduce/build x86 in an isolated target

Use one Cargo job at a time, local NASM/normal existing dependencies on x86.
No install or original worktree changes are needed:

```sh
export CARGO_BUILD_JOBS=1 CARGO_INCREMENTAL=0
export CARGO_TARGET_DIR=/your/isolated/target
cargo test --locked --no-default-features --manifest-path bin/ttfx-bg-rs/Cargo.toml
cargo test --locked --no-default-features --manifest-path bin/ttfx-bg-rs/Cargo.toml -- text_file_literal_input_reaches_real_asm --ignored --test-threads=1 --nocapture
cargo build --release --locked --no-default-features --manifest-path bin/ttfx-bg-rs/Cargo.toml
cp "$CARGO_TARGET_DIR/release/ttfx-bg-rs" /your/artifacts/omarchy-ttfx-layer
/your/artifacts/omarchy-ttfx-layer --probe
```

Use correct standalone argv0/name; do not invoke the controller binary with
`--version`. A missing --text-file path errors before compositor startup. For a
coordinator-operated real preview, pass the chosen output/layer/keyboard and
`--effects decrypt,rings --text-file "$HOME/.config/omarchy/branding/screensaver.txt"`.
Check actual file alignment/branding, rotation, dismiss and restoration separately.
No remote execution, performance sample or publication was done by this thread.

## Pre-existing standalone/plugin coupling — not changed here

`IntensityWatcher` and `SpeedWatcher` refresh from `read_config()` every250ms,
replacing cfg fallback values. `LiveReactivity` also refreshes from read_config;
ASM audio callbacks call current before the disabled-audio guard, so audiofalse
does not eliminate that read. ASM effect completion consults read_config.running.
That function reads `~/.local/state/omarchy/audio-background/state.json` with
legacy fallback `~/.config/omarchy/plugins/io.github.avillagran.omarchy-audio-background/state.json`.
Consequently this candidate still inherits hidden plugin settings; implementing
file branding does NOT complete configuration isolation from the plugin.

Proposed separate minimal follow-up, not implemented: a producer-scoped config
policy for standalone, supplying fixed cfg intensity/speed/reactivity and using
its stop flag for liveness; controller workers retain existing live watchers.
Cover all four reads, not just speed/intensity. Test conflicting plugin state,
absent/malformed state, rotation and stop, and prove no plugin reads for the fixed
policy. Such a change can alter cadence/resources and requires its own validation;
it is deliberately excluded from this branding-only commit and v1 perf claims.
