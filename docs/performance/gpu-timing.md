# Optional asynchronous GPU diagnostic (not a performance round)

Base R5 77159c4; original and CLIPPED_QUADS routes remain independently selectable.
No optimization/default change and no measured GPU benefit. R15 stays frozen.

Only `TTFX_BG_GPU_TIMING=1` AND `TTFX_BG_PRESENTATION_LOG` enable this probe in
Wayland. With either absent, no timer creation/query/disjoint polling occurs.
Three EXT timestamp queries bracket GPU commands before upload, after upload,
and after draw; differences estimate upload and draw completion intervals.
This excludes EGL presentation, compositor work, CPU fill/clip construction, and
physical display latency. No GPU/CPU absolute clock comparison. Timestamp query
boundaries can perturb scheduling: compare probe 0/1 with otherwise identical
logging in a separate diagnostic run; do not treat query runs as resource ABBA.

Validate EXT_disjoint_timer_query, actual context renderer/version, all six used
entrypoints and useful timestamp counter bits. Capability surfaceless is not
proof of the Wayland context. Zero bits/missing capability/function/GL errors
produce unavailable, not invented zero durations. Errors disable measurement.
No driver detection or fallback normalization hacks.

At most 64 frames/192 query objects. At the next actual draw, test availability
of the oldest frame's final timestamp; same-target earlier queries are ordered
available by the extension. If unavailable, return immediately without reads,
spinning, sleeps, glFinish or waiting. Read only available 64-bit results. Check
disjoint before and after collection before publishing anything; conservatively
invalidate all pending/candidate frames on disjoint. Counter wrap/order failures
are invalid, not durations. Saturation skips measuring the new frame, not drawing.
On exit delete queries in the current context, report unread frames, never drain
with a wait. Query storage is bounded; logging failure follows existing probe.

Events: gpu_timer_capability, gpu_elapsed (gpu_timing_frame, gpu_upload_us,
gpu_draw_us, gpu_total_us), unavailable/disjoint/saturated/invalid/end. Submit
also carries gpu_timing_frame. tools/gpu-timer-report.py joins deferred results
to the actual submit elapsed window, not their readback arrival; missing results
and absent stages remain explicit. GPU intervals are not isolated shader stages,
fragment counts, total GPU power or compositor timing. Do not sum percentiles.

Build/test isolated, x86 needs local NASM, no install:

```sh
cargo build --release --locked --no-default-features --manifest-path bin/ttfx-bg-rs/Cargo.toml
cargo test --locked --no-default-features --manifest-path bin/ttfx-bg-rs/Cargo.toml
cargo test --locked --no-default-features --manifest-path bin/ttfx-bg-rs/Cargo.toml -- gpu_grid::equivalence_test --ignored --test-threads=1 --nocapture
python3 tools/gpu-timer-report.py /path/presentation.jsonl --skip 10 --seconds 20
```

Four explicit EGL gates: three previous exact RGBA tests plus
`gpu_timer_is_async_and_preserves_reference_pixels` (probe OFF/ON same images,
capability and deferred valid samples required). Gate readback is allowed ONLY
as the image oracle, not how runtime queries complete. Production timer has no
ReadPixels/Finish/wait. Unit fake driver tests unavailable-not-read, bounded
storage/recycling, and disjoint both before/after candidate reads. Benchmark
only after real Iris gate/context/capability validation. Diagnose baseline0 and
CLIPPED_QUADS1 separately, no R15/other candidate combination, both placements.

Primary contract: https://registry.khronos.org/OpenGL/extensions/EXT/EXT_disjoint_timer_query.txt

Local validation: ARM release passed, full17+145+19=181 passed/10 ignored
(helper subprocess excluded); four explicit EGL tests passed on llvmpipe.
Reader late-arrival/window/missing/null regression passed with bytecode disabled.
No Iris/Wayland timings or overhead measured yet. Raw logs:
/tmp/ambient-gpu-timer-final-full.log, /tmp/ambient-gpu-timer-final-egl.log,
/tmp/ambient-gpu-timer-release.log. Earlier accidental Python bytecode cache
preserved outside worktree in own gpu-timing-python-cache/report-import-1 artifact.
