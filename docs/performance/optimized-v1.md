# Optimized renderer v1 — hardware acceptance candidate

Isolated worktree `~/Work/ttfx-ambient-optimized-v1`, branch
`perf/ambient-optimized-v1`. Base R26 d984bc9 (ancestor ec31887) plus R17 mapping
48de42e; inherited clipped geometry is the R5 implementation. All experimental
snapshots remain untouched. This candidate combines previously measured CPU and
GPU opportunities; their independent gains MUST NOT be added or assumed to
survive combination. No install, promotion, hardware acceptance or 60fps claim.

## One control, same binary

`TTFX_BG_RENDER_PROFILE=reference|clipped|axis` is read once per Wayland layer,
then copied into its grid and pipeline. **Absence selects clipped**, the provisional
v1 choice after both same-binary screenings. Explicit reference, non-Unicode and
all other invalid values select **reference** safely. Axis remains explicit.
Values are exact and case sensitive; no automatic GPU identity selection.

| Profile | Glyph slot lookup | GPU mapping |
|---|---|---|
| reference | original HashMap | original fullscreen fragment mapping |
| clipped | R26 bounded 256-entry/full-key cache | R5 cell quads restricted to each original clip |
| axis | R26 same cache | R17 fullscreen with exact per-pixel RG32UI owner/local maps |

Clipped and axis are alternatives, never active together. Cache handles Unicode
and normalized styles, owns no slots, never caches None/errors, and is cleared
before atlas reset/retry. Collisions fall back to the original map. Table entry
payload is 4096B, not total memory/RSS; adversarial working sets can regress.
Axis payload at 2560x1600 is 33280B plus resize scratch, not driver bytes/RSS.
Clipped computes per-clip ranges and emits actual intersecting cell geometry.
Normalized RGBA8 cell colors and R8 glyph sampling/precision remain unchanged.

All historical optimization activation env vars are ignored, including native
SWAP/SGR_CACHE and GPU GLYPH_SLOT_CACHE/FAST_GLYPHS/CLIPPED_QUADS/CELL_QUADS/
AXIS_LOOKUP/UPLOAD_DIRTY, and later trials (absent from this base). WL_DRAW,
WL_CLIP, WL_DAMAGE and WL_DEFER_CONSUME cannot change this candidate's rendering,
damage rectangle budget or pacing. No dozens of resets are needed. Normal
configuration (effect, cap, resolution, intensity, speed, audio, opacity, font,
placement) remains in force. Explicit diagnostics PRESENTATION_LOG/PRODUCER_LOG/
GPU_TIMING/WL_STATS and optional NATIVE_SEED remain supported. GPU queries require
both presentation logging and GPU_TIMING=1; resources must use no logs/query.

## Fallback and lifetime

Optional backend compile/link or axis initialization error builds the reference
GPU program, retaining the requested profile's R26 cache. If the reference also
fails, the existing error propagates. Axis bounds/capability limits (including
actual MAX_TEXTURE_SIZE, five fragment texture units, 65536 axis pixels) use the
reference mapping. GL upload errors are checked only on axis rebuild and latch
reference mapping for that geometry; a resize/grid change permits retry. No
GetError loop, busy wait, timer-query polling or retry upload in steady frames.
The fault-injection upload gate tests error response, not a real Iris OOM.
Unsafe clipped integer products use its embedded reference mapping branch.
Fallback neither drops glyphs nor reduces resolution/quality.

Pipeline records its owning current EGL context. Normal Wayland teardown drops
pipeline before EGL. Same-context Drop releases program, VAO, FG/BG/atlas and
axis textures; optional timer finish deletes queries without waiting. Drop with
another/no current context performs no GL deletion: owning-context destruction
reclaims those objects. No arbitrary GL calls after context teardown. Tests
cover repeated same-context creation/deletion and foreign-context object safety.

## Observable route, no new polling

Existing submit JSON includes `render_profile`:
`{profile,cache,cache_payload_bytes,backend,fallback}`. Backend is reference,
clipped or axis; fallback is null or one of backend_initialization_failed,
unsafe_cell_geometry, axis_limits, axis_upload_gl_error. Cache is none or
`glyph-slot-256`. Existing `glyph_slot_cache_payload_bytes`, `axis_lookup`, actual
upload bytes/draw totals and wp_presentation feedback fields remain compatible.
Candidate startup records render_profile_requested. Metadata is only serialized
with existing logging; there is no file/metric sampler in normal operation.

## Build and gates (x86_64, local NASM, no install)

```sh
cargo build --release --locked --no-default-features --manifest-path bin/ttfx-bg-rs/Cargo.toml
cargo test --locked --no-default-features --manifest-path bin/ttfx-bg-rs/Cargo.toml
cargo test --locked --no-default-features --manifest-path bin/ttfx-bg-rs/Cargo.toml -- gpu_grid::equivalence_test --ignored --test-threads=1 --nocapture
PYTHONDONTWRITEBYTECODE=1 python3 tools/test-axis-lookup-report.py
```

EGL must print the actual renderer on Iris; no software override/tolerance.
Combined gates compare both optimized profiles against original exact RGBA:
Unicode, all style bits/hidden/blank, high slots/BG flags, alpha/opaque, reset,
resize, full-panel/subpixel seams, overlaps, coherent pixels outside clips and
three actual prior buffers with union damage including age2/3. Fallback and
resource lifetime gates are separate. Normal gates compare full slot maps,
pending glyph mask bytes/order, invalid styles, collisions/reset/overflow/retry,
seeded native publications and profile selection under poisoned legacy env.
Explicit timer gate validates OFF/ON image equality and availability; it is not
a resource benchmark. All ignored CPU profiles remain diagnostics, not round
acceptance. No x86/Iris result is claimed until coordinator supplies it.

## Hardware protocol and decision

Same frozen binary, seed12345, matrix/resolution2/audio0/intensity5/speed5/cap60.
Initial screening ran reference vs clipped and reference vs axis independently,
both placements. Final-SHA confirmation compares A explicit reference against
B with the variable UNSET (expected clipped), both placements,45s OFF ABBA and
separate feedback; six-effect smoke runs on that same final binary/default.
Resource ABBA: 10s warmup +20s sample, no presentation/producer/stats logs, timing0.
Separate feedback AB20s both placements: existing presentation log only, timing0,
producer log unset. Verify effective route on EVERY submit: cache0/reference A,
4096B plus chosen backend B, fallback null expected at native panel size. Keep
strict cooling three readings <=60C, abort >=85C, restore on every exit; excluded
thermal samples cannot decide candidate causality. Dismiss movement/key/click/
scroll and restore/lock/cursor/single-instance semantics remain hardware gates.
A promising winner repeats45s ABBA +feedback before choosing any default.
Optional GPU timing/producer/RSS diagnostics are separate from resource evidence.
Do not sum independent gains or count this as another numbered round.

Producer/simulation/publication/pacing implementation remains original (R16,
R19, R2 and R15 not active). Native Matrix still requests25ms sleep AFTER work,
thus <40 updates/s; lower contention may alter observed wall cadence and must be
reported. Fixed-step publication/RNG equality does not prove exact wall-time
speed equivalence. Presented feedback != unique content; unique fps and watts
remain unknown. RC6 active is a GPU proxy, not measured electrical consumption.
Amiga uses another pipeline and gets no benefit/claim from these profiles.

Existing ARM risk: R26 saw two parallel full-suite FD assertions in
frame_wakeup.rs (F_GETFD after final drop, descriptor reuse suspected only);
serial and x86 normal suites passed, no fix claimed. Current candidate normal
suite and explicit EGL results are recorded below at freeze. No frame_wakeup edit.

After hardware selection: minimal product cleanup can remove inactive historical
trial code/tests/docs from the chosen renderer, retain explicit reference
rollback and safe fallback; no broad refactor or default promotion before results.

## First freeze evidence (26aa717,2026-10-26,aarch64)

Release `--locked --no-default-features` passed (optimized, 2m01s), without
compiler warnings. Standalone `--probe` passed.
Normal parallel full suite: **189 passed /16 ignored**, 17 library +153 binary
+19 integration tests; subprocess fixture summaries are not counted twice.
Nine explicit EGL gates passed **Apple M2 Pro (G14S B1)**, no software override.
This is not x86/Iris evidence. Reader stable-zero/missing-null/resize/window gates
passed. Logs `/tmp/ambient-opt-v1-full-final.log`, `-egl-freeze.log`, `-release.log`.

Preserved setup failure: first EGL run8/9 passed, lifecycle gate stopped at
`eglMakeCurrent(other)` before testing foreign-context deletion. The fixture had
selected a minimal second EGLConfig not guaranteed compatible with its owner's
RGBA8 pbuffer. It now reuses Sandbox's exact EGLConfig; all asserts remain and
all nine final gates passed. No production cleanup bug is inferred from that
setup failure. Earlier log `/tmp/ambient-opt-v1-egl.log` remains intact.
R26's separate historical ARM FD failures remain unresolved despite this pass.

For the isolated x86 delivery, after the coordinator's build and gates, copy the
release executable as `omarchy-ttfx-layer` into an artifact directory owned by
this trial (not the installed engine/plugin path), then record `sha256sum` and
the frozen source SHA together. The executable name selects standalone mode;
`--standalone --probe` must print `omarchy-ttfx-layer 1`. No prebuilt x86 checksum
can be supplied from this ARM host. Smoke matrix,rain,decrypt,rings,thunderstorm,
fireworks in both placements is functional validation, separate from Matrix
ABBA, queries and capture. First-freeze x86 binary hash and screening are recorded below. Final-default SHA,
x86 rebuild/hash,45s acceptance and smoke remain pending final validation.


## Provisional default decision after screening (not final acceptance)

Coordinator tested26aa717 x86 release/full189passed16ignored and9 exact Iris6100
EGL gates successfully. That binary SHA256 is
`b619d6c5465bfb2af76f5dfcb63543525f95ee0ac053a83546e692121e844269`;
it is NOT the final-default binary. Source archive SHA256
`a568615d0b32ac9dab1532b63b6515dee53080e645fa99cd640723aac42e1eb7`.

Matrix equal-parameter resource screening20s OFF ABBA, background/screensaver:

| Profile | CPU A→B (one core%) | RC6 active A→B (%) |
|---|---|---|
| clipped/background |11.550→11.065|48.525→45.765|
| clipped/screensaver |12.030→11.570|48.930→45.555|
| axis/background |11.495→11.040|48.350→46.435|
| axis/screensaver |12.205→11.390|48.485→46.185|

Clipped feedback37.60/37.60background,37.60/37.65screensaver;axis37.55/37.60
and37.60/37.65. Each profile:four full windows,0discarded,p99~33.358–33.359ms,
every submit effective route/cache4096/fallbacknull verified,6dismisspassed,
max78C,restorationverified. No causal watt/unique or global dominance claim.
Sources ROOT=`~/.local/state/omarchy-ambient-bench/2026-10-26`,
`optimized-v1/clipped-20-summary.json`, `optimized-v1/axis-20-summary.json`,
`opt-v1-clipped-20/results` and `opt-v1-axis-20/results`.

User/coordinator provisionally chose clipped: greater GPU-proxy reduction in
both placements, no axis map/resize storage, retained CPU reduction~3.8–4.2%.
Axis remains a functional explicit alternative with a CPU/GPU tradeoff; these
screenings do not prove universal superiority. Only default selection, invalid
input handling, constructor tests and docs change after26aa717. Producer,
pacing,cache,backends,GL lifetime and fallbacks are unchanged.

Acceptance requires rebuilding the NEW frozen SHA on x86, normal suite and9
exact Iris gates, then reference vs UNSET-default45s resources/feedback in both
placements with route validation, six-effect rotation/dismiss smoke and restore.
No installation/promotion before that evidence. If confirmation fails, default
must revert in a NEW local commit. Performance scope is Matrix; smoke does not
prove catalog performance or benefit to Amiga.60fps/watts/unique remain pending.

## Default-candidate local gates (2026-10-26, aarch64)

Release build passed (optimized,1m39s), with no compiler warnings.
Normal parallel full suite: **190 passed /16 ignored** (17 library +154 binary
+19 integration; subprocess fixtures excluded from totals). Nine exact EGL gates
passed Apple M2 Pro (G14S B1), including actual default constructors under poisoned
historical switches, empty/invalid values, and a non-Unicode environment value.
Absent selects clipped/cache4096; explicit reference and non-Unicode select
reference/cache0. Reader regression gates also passed. These local results do
not replace the final x86/Iris rebuild,45s confirmation or functional smoke.
Logs `/tmp/ambient-opt-v1-default-{full,egl,release}.log` are separate from26aa717.
Historical ARM FD intermittency remains unresolved; no descriptor code changed.
