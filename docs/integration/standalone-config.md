# Standalone producer configuration and capabilities — review snapshot

Isolated auxiliary `~/Work/ttfx-ambient-standalone-config`, branch
`feat/ambient-standalone-config`, based on frozen text-file d8d5146 (v1 ancestor
2307875). No existing frozen worktree, source installation or renderer is edited.
This is configuration/compatibility work, not an optimization or a performance
claim. Text-file parsing and literal engine input remain unchanged.

## Producer policy

A thread-local RAII guard is entered by the standalone's actual rotation worker,
before AudioState/native watchers/ASM callbacks. Its immutable startup Config is
returned at the top of read_config, before plugin paths or LAST_GOOD_CONFIG are
accessed. The returned running flag is derived from that worker's stop flag.
This covers indirect intensity/speed watchers, reactivity even in audiofalse
callbacks, and post-effect running checks. No per-frame poll or new timer is
added. Intensity/speed/reactivity retain standalone defaults5/5/2; a change in
plugin settings no longer changes these values. The parent remains responsible
for rotating/stopping workers and layer lifetime.

The guard is non-Send, scoped to its owner thread, restores any previous policy
on drop, and cannot affect controller threads or their configuration cache.
Without a guard read_config follows its original modern/legacy state-file paths.
No Config serialization or new plugin preference is added. Omarchy's existing
read_theme_colors path and ThemeWatcher remain active: current/theme.name and
the selected Omarchy theme's colors.toml are allowed; plugin state is not.
Auto-degrade remains disabled in the direct route as before.

## Safe capability handshake

Use `--standalone --probe --capabilities` with a timeout and require stdout to
contain exactly this single line (plus newline):

```
omarchy-ttfx-layer capabilities: text-file-v1 standalone-config-v1
```

Capabilities takes precedence over probe and argument/file parsing. It returns
before signal setup, governor, monitor lookup, theme/audio/worker/Wayland startup.
Probe alone is unchanged: `omarchy-ttfx-layer 1`. An older probe-capable renderer
safely returns that old line for the combined invocation, which MUST be rejected
as lacking these features. Never invoke capabilities alone on an unknown old
binary, because it may ignore the option and start its normal runtime. No GPU
identification or filesystem capability inference is involved.

## Review validation status

This is an immutable REVIEW snapshot, not final freeze or acceptance. Local ARM
focused tests are still linking when this snapshot is made; full normal, explicit
ASM input, EGL and release are pending for this implementation. The descriptions
below identify implemented tests, not passed results. Earlier d8d5146 gates do not
validate this new policy. Results will be recorded separately without amending
this source snapshot.

## Gates and reproduction

Run one Cargo operation at a time in an isolated target, with normal dependencies
and local NASM on x86. No SSH/install or benchmark is required:

```sh
export CARGO_BUILD_JOBS=1 CARGO_INCREMENTAL=0
export CARGO_TARGET_DIR=/your/isolated/target
cargo test --locked --no-default-features --manifest-path bin/ttfx-bg-rs/Cargo.toml
cargo test --locked --no-default-features --manifest-path bin/ttfx-bg-rs/Cargo.toml -- text_file_literal_input_reaches_real_asm --ignored --test-threads=1 --nocapture
cargo test --locked --no-default-features --manifest-path bin/ttfx-bg-rs/Cargo.toml -- gpu_grid::equivalence_test --ignored --test-threads=1 --nocapture
cargo build --release --locked --no-default-features --manifest-path bin/ttfx-bg-rs/Cargo.toml
cp "$CARGO_TARGET_DIR/release/ttfx-bg-rs" /your/artifacts/omarchy-ttfx-layer
/your/artifacts/omarchy-ttfx-layer --standalone --probe --capabilities
/your/artifacts/omarchy-ttfx-layer --probe
```

Normal tests include a child process with isolated HOME for each modern-conflict,
legacy-conflict, malformed and absent plugin state. Conflict is runningfalse,
intensity0,speed100,reactivity5. The controller path is checked first (including
unchanged paths and live values); then scoped watchers/audiofalse callbacks and
stop are checked. Actual standalone producer helper runs Matrix→real ASM rings→
Matrix with shared slot and real publication/stop for every fixture. A test-only
counter checks entry to the actual read_config plugin-I/O branch: zero in every
worker. The counter and fixtures are absent from the product binary. Config
fallback tests are not proof of arbitrary external syscall behavior; static
routing and production-helper wiring identify the paths under test. Real ASM
is required; an unavailable build fails this fixture, not a fake fallback pass.
Omarchy theme reading is checked with a fixture selected through its own paths.
No test reads or writes the user's actual HOME plugin/theme files.

Test scope does not include a new real Wayland/controller UI smoke, x86/Iris or
physical performance measurement until separately reported by the coordinator.
Existing historical ARM parallel F_GETFD failures remain unresolved; no wakeup
code is changed. Source-level default pacing is preserved, but removing hidden
plugin speed/intensity inputs can intentionally change sessions that previously
inherited conflicting state; do not claim old CPU/cadence results apply unchanged.

## Recorded results

The pending review status above describes immutable runtime snapshot e918c256.
See [standalone-config-validation.md](standalone-config-validation.md) for the
separate x86 passes, ARM serial/EGL/build passes and unresolved ARM normal-parallel
SIGSEGV. The failed parallel gate remains failed; no two-architecture clean release
is claimed. This link is documentation only.
