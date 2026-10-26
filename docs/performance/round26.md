# R26 — bounded general glyph-slot cache

Isolated ec31887 base, branch perf/ambient-round26-glyph-cache in
~/Work/ttfx-ambient-round26-glyph-cache. Exact toggle
`TTFX_BG_GPU_GLYPH_SLOT_CACHE=0/1`, default0; original/rest0, no R12 combination.
This is not the rejected RowView research (83f0f4f, preserved separately, not
counted as a round). R12 measured native fill lookup cost; its gains are not
attributed to this general cache. Materiality must be measured anew.

256 direct-mapped entries, full key=(char as u64)<<8 | normalized style,
u64MAX impossible sentinel. Hash mixes key and shifts then multiplicative high
8 bits; selects bucket only. Hit requires complete key equality. Collision
falls back to original HashMap, then replaces bucket. HashMap alone owns slot
allocation/order/capacity/pending glyph masks. No cached None/errors/slot0.
Unnormalized style bits bypass cache entirely and retain original HashMap keys.
Atlas reset/resize clears all entries before fill retry; no stale slot reuse.
Color/ANSI parsing/Strings/RNG/simulation/geometry/shaders/uploads unchanged.

4KiB entry payload (16B Rust Entry ×256), one Box allocation only B; no growth.
Box/allocator/Grid object overhead excluded from payload, not RSS or total
memory. A has no table allocation. CPU costs include branch/hash/key compare;
adversarial collisions can regress to baseline map plus cache overhead. Counters
hit/miss are cfg(test) only; normal binary has no counters/polling/timestamps.
Existing submit logs expose glyph_slot_cache_payload_bytes nullA/4096B, existing
feedback reader sufficient; no installation/new report reader.

## Gates/build protocol

Serial x86_64 with local NASM, no install or software override:

```sh
cargo build --release --locked --no-default-features --manifest-path bin/ttfx-bg-rs/Cargo.toml
cargo test --locked --no-default-features --manifest-path bin/ttfx-bg-rs/Cargo.toml
cargo test --locked --no-default-features --manifest-path bin/ttfx-bg-rs/Cargo.toml -- gpu_grid::equivalence_test --ignored --test-threads=1 --nocapture
cargo test --locked --no-default-features --manifest-path bin/ttfx-bg-rs/Cargo.toml --config 'profile.test.package.ttfx-bg-rs.opt-level=3' -- actual_general_glyph_slot_cache_cpu_profile --ignored --test-threads=1 --nocapture
```

Normal oracles compare planes, FULL original HashMap, pending mask bytes/order,
atlas reset, collisions/key equality/slot reuse after reset, Unicode maximum,
all valid styles and invalid style bypass, working sets above255 slots, resize,
overflow retry/error and subsequent smaller frame. GPU gates compare original
and cached route exact normalized RGBA/seams/clips/alpha/BG/high slots/resize,
including coherent prior content outside damage. No tolerance.

Optional opt3 actual fill profile alternating A/B:128 seeded native frames,
startup32/steady96 separately; Unicode512/styles,1536/styles,adversarial24
same-bucket keys, changing theme colors/backgrounds, reversed publication order.
Full plane/map/mask oracle each frame. Counters/payload actual, not a gain based
on cache hit counts alone; test instrumentation itself not normal performance.
No summing percentiles, no hardware benefit inferred from ARM profile.

## Hardware

After exact Iris gates, seed12345 matrix/resolution2/audio0/intensity5/speed5/
cap60, all other toggles0 incl FAST_GLYPHS0/query0/timeline0, producer/ASM logs
unset. Off ABBA10s warm+20sample both placements, separate feedbackAB20 with
route4096B, dismiss/restore and strict cooling gates. Promising→45confirm and
producer/memory separately if cadence changes. No default promotion or install.
Feedback is presentation, unique null, refresh59.972Hz. Native25ms+work caps
updates below40/s; lower overhead can incidentally accelerate playback, so
reduced CPU alone does not prove temporal equivalence or no greater consumption.
RC6 active is a proxy, not watts.

Local validation boundary: first parallel full failed an existing descriptor test
(frame_wakeup.rs211 F_GETFD left1/right-1 after final drop) in main; second
parallel full failed same assertion in lib, which does not include this cache.
Logs /tmp/ambient-r26-cache-full.log and -full-retry.log preserved. Descriptor
reuse by another concurrent test is a static suspicion, not proven cause. No
frame_wakeup edit or tolerance change. Full SERIAL --test-threads=1 passed
17+148+19=184 tests/12ignored (helper subprocess not duplicated), five exact EGL
gates passed Apple M2 Pro G14S B1. This does NOT resolve the failure or establish
parallel suite green; normal x86 full gate is still required. Explicit serial
run is a separate diagnostic if the existing parallel failure repeats.

Final local opt3 profile: native startup p50/p95us70.626/152.458→25.916/56.000;
steady80.792/115.917→28.709/35.500. Native hits301432/misses21368. Unicode512
styles steady11.000/13.708→9.125/9.541;1536 styles40.292/46.291→38.417/40.875.
Adversarial24 steady1.833/1.958→1.958/2.042 (regression). Adversarial512
startup20.125/26.459→20.917/25.917, steady12.416/25.750→11.042/25.791; tiny hit
count95/miss49057, variable timings/HashMap layout, no universal collision gain.
All synthetic frames vary colors/backgrounds and reverse key order; cache keys
remain glyph/style. No broad speed claim from hit counts or these profiles.
Logs /tmp/ambient-r26-cache-{profile-final,full,full-retry,full-serial,gpu,release}.log.
Release completion checked at freeze. Hardware x86/Iris/resources pending.
