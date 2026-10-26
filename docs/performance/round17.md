# R17 — exact per-axis pixel lookup textures

Base ec31887 (optional async timing diagnostic), same binary ORIGINAL fullscreen
A vs AXIS_LOOKUP B; all other optimizations 0. Toggle exact
`TTFX_BG_GPU_AXIS_LOOKUP=0/1`, default0. No R5 quads/flat-state/R16/timeline
combination. If instanced quads are also requested, this route stays off.
Resources keep GPU_TIMING=0; optional query diagnostic separately after gates.

Evidence from coordinator: real Wayland/Iris original GPU command draw interval
p50~2.4ms vs R5~1.57ms; uploads~0.72us, no missing/disjoint. R5 removes the
per-pixel mapping algebra, but also changes geometry, so this does not isolate
the shader stage. R7 integer divisions and R11 vertex flat state had no global
advantage. This hypothesis changes mechanism: exact owner/local maps built at
resize, instead of dividing/ceil/floor/correcting per fragment. No assumed gain.

One RG32UI texture per axis stores (cell,local) for each pixel. CPU u64 arithmetic
computes cell=((pixel+1)*count-1)/size and local=pixel-floor(cell*size/count).
Two unsigned texture fetches replace the original fragment mapping. FG/BG and
glyph atlas retain their NORMALIZED samplers and existing decode/color math in
the same fragment stage; no half rounding approximation or R8 driver detection.
Raster geometry/scissor/Wayland damage/buffer-age union remain original.

Tradeoff: two dependent extra fetches and two texture bindings; upload/CPU build
only when the corresponding axis size or grid count changes. 2560+1600 requires
33280 bytes of lookup DATA, not driver allocation/RSS. Temporary CPU arrays are
released after upload. A one-axis change rebuilds/uploads just that axis. No
per-frame rebuild or CPU/GPU clock polling without optional logging. Map axes
that exceed actual GL_MAX_TEXTURE_SIZE, nonpositive dimensions/count or the
65536-pixel per-axis allocation bound use the exact reference mapping branch.
Grid/color textures retain their prior contracts; this does not extend the
baseline's maximum grid texture size. Fallback keeps the same normalized state.

Telemetry `axis_lookup` in submit (only existing presentation logging): active,
uploaded_bytes,rebuild_axes,payload_bytes,resize_cpu_ns,resize_wall_ns. Resize
CPU/wall are sampled only on real rebuilds with logging; no rebuild returns
null timings. Payload excludes allocator/driver/RSS. tools/axis-lookup-report.py
reports actual bytes/zero stable rebuilds vs missing/null. No FPS inferred from
lookup counters. Existing feedback reader and gpu-timer-report.py stay separate.

## Gates/build protocol

Use isolated x86_64 build with local NASM, no installation:

```sh
cargo build --release --locked --no-default-features --manifest-path bin/ttfx-bg-rs/Cargo.toml
cargo test --locked --no-default-features --manifest-path bin/ttfx-bg-rs/Cargo.toml
TTFX_BG_PRESENTATION_LOG=axis-gate-costs cargo test --locked --no-default-features --manifest-path bin/ttfx-bg-rs/Cargo.toml -- gpu_grid::equivalence_test --ignored --test-threads=1 --nocapture
PYTHONDONTWRITEBYTECODE=1 python3 tools/test-axis-lookup-report.py
```

Six explicit EGL gates: four inherited (including timer OFF/ON image gate), plus
`gpu_axis_lookup_matches_reference_pixels` and
`gpu_axis_lookup_preserves_buffer_age_and_fallback`. On Iris no software override,
no relaxed tolerance. gate-costs enables resize CPU/wall observation only in the
EGL fixture; it does not open a Wayland log/file. AXIS_RESIZE_GATE lines record
bytes/cost; not a resource benchmark.

Normal unit gates prove integer partitions including subpixel/empty cells,
1-pixel dimensions, large counts and u64 overflow safety, limits/fallback before
allocation. Exact RGBA gate: normalized high slots/flags/alpha/reset/resize;
full-panel/seams/degenerate/overlap and coherent pixels outside clips. Separate
three-previous-buffer fixture unions all changes since that buffer, actually
uses age2/3 (also longer), erase/blank/background changes, opacity fixed within
each sequence, geometry cuts. Force runtime max=0 to test reference fallback
without oversized real allocation; restore cached mapping without rebuild.
Resize two axes uploads both, then height-only change uploads just one; stable
and fallback frames assert zero uploaded bytes. This is not a live desktop
resize/lock/dismiss test.

Local ARM suite17+147+19=183 passed/12 ignored (helper subprocess excluded),
six EGL exact gates llvmpipe passed. Final release/gate logs recorded at freeze:
/tmp/ambient-r17-final-full.log, -final-gpu.log, -release.log. Reader stable-zero/
missing-null/resize/window regression passed. No Iris or resource result yet.

After Iris gates, original A vs LOOKUP1 B, seed12345 matrix/resolution2/audio0/
intensity5/speed5/cap60, all other toggles0; ABBA10s warm+20s resources without
logs/query, separate feedback both placements and all dismiss/restore checks.
Then optional GPU_TIMING1 vs0 same logging to observe draw interval and overhead,
not mixed into resource evidence. Resize diagnostics separately; stable warm
windows should show0 uploaded bytes/rebuilds. Confirm45 only if promising.
No watts/unique/60/temporal speedup claim; native sleep25ms+work remains <40.

Freeze verification: ARM release PASSED; final183/12 suite and six explicit
llvmpipe EGL gates PASSED. Resize fixture 511x383 first/509x337 second upload
7152/6768B; height-only509x339 uploads2712B/rebuild1, stable/fallback0. Local
resize thread-CPU43.792/46.417/23.250us (opaque sequence), clientwall43.585/
49.917/22.792us; not Iris prediction or whole-render CPU. Exact RGBA gates and
reader tests passed, git diff --check passed. No hardware result yet.
