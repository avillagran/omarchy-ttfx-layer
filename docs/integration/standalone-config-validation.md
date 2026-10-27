# Standalone config validation — e918c256

Runtime/source tested: `e918c25643f44552658889f283d07a49e28bb8eb`.
This results document is a docs-only descendant; it does not alter or replace
that source snapshot. No further runtime changes, gates or reruns at closure.
No publication or installation. A clean two-architecture release is **blocked**
by the unresolved ARM normal-parallel SIGSEGV described below.

## Implemented scope

Producer-scoped fixed Config before native/ASM/audio work; read_config's early
scoped return prevents indirect plugin file reads. Running follows worker stop.
Controller live watchers/cache/default paths are preserved. Omarchy theme files
remain allowed. Literal text-file support from d8d5146 is retained.

`--standalone --probe --capabilities` must return exactly
`omarchy-ttfx-layer capabilities: text-file-v1 standalone-config-v1` plus newline.
Capabilities wins over probe; probe alone keeps `omarchy-ttfx-layer 1`.
Use the combined handshake with a timeout for old-binary detection.

## ARM local — results remain separate

- Focused11passed/2ignored, including four isolated HOME fixtures and real native
  Matrix→ASM rings→Matrix/stop, with plugin-read-branch counters0 per worker.
- FULL NORMAL with default parallelism: exit101, test binary killed by SIGSEGV.
  This is a failed gate; no successful full normal-parallel result is claimed.
- Diagnostic FULL SERIAL:17library+160binary+19integration=196passed/18ignored;
  subprocess helper results are not duplicated. This does not resolve the failed
  parallel run and is not a substitute for it.
- Explicit real ASM literal-file input gate1passed.
- Nine exact EGL gates passed on Apple M2 Pro (G14S B1), not Iris.
- ARM release build passed; no new performance measurement or acceptance claim.

Systemd core PID422794/TID422929 localizes the fault to libfreetype/FT_Load_Glyph,
Cairo/Pango, GpuGrid::rasterize_mask/slot_for/fill, in the pre-existing
`general_unicode_styles_collisions_reset_resize_and_slot_order_match` test.
Another pre-existing glyph test was also rasterizing in a different thread.
The standalone fixture parent was waiting for its child. This identifies the
crashing stack and concurrent work, **not the cause**. Rasterization/test source
is unchanged from d8d5146 and2307875; that fact alone does not exonerate this
change or prove a baseline race. It is distinct from earlier ARM parallel
F_GETFD assertion incidents, which also remain unresolved.

Bounded comparison without source edits: existing d8d5146 main-test binary ran
its normal tests once with default parallelism,157passed/17ignored; baseline
glyph group with8threads2passed/1ignored; e918 glyph group serial2passed/1ignored.
The failure was not reproduced. No root cause, baseline reproduction or fix
has been established. No further diagnostic loops were run at closure.
Original systemd core remains preserved; it was not extracted, uploaded or
deleted. Core info, exact crashed binary, baseline binary provenance/hashes and
all diagnostic logs are retained in the external review artifact directory.

## x86 coordinator report — not rerun by this thread

- Release, FULL NORMAL196passed/18ignored, real ASM literal input1passed and nine
  exact Iris6100 EGL gates passed.
- Four modern/legacy/conflicting/malformed/missing configuration fixture runs
  (four fixture kinds: modern conflict, legacy conflict, malformed, missing)
  passed real native→ASM→native/stop with plugin_reads0.
- Real compositor smoke passed both placements: launcher text selection→GPU,
  real user branding file, decrypt/rings/thunderstorm rotation, motion dismiss.
  All697background/748screensaver submits used defaultclipped/cache/backend with
  fallbacknull. This is functional evidence, not an all-effects performance or
  end-user visual-acceptance claim.
- First harness checked hashes before plugin restoration and failed that order
  check. That incident is preserved. Corrected retry passed, four config hashes
  unchanged after restoration. Final engine none/unlocked/idlefalse/stayAwaketrue,
  usual layers only and BANK_FREE reported verified.

x86 release SHA256:
`b7a6dd15d768d3c0198d509cfb75a710d17347112fde703465e4609d429812af`.
Coordinator log:
`~/.local/state/omarchy-ambient-pr-review/2026-10-27/hardware/build-e918c256.log`.
No x86 result is relabeled as ARM evidence.

## Preserved artifacts

Root:
`~/.local/state/omarchy-ambient-dev-artifacts/standalone-config/e918c25643f44552658889f283d07a49e28bb8eb/`
contains source.tar, manifest.json, logs/ and aarch64/omarchy-ttfx-layer.

Source tar5908480B SHA256:
`c3e2cfd48da33f463ee28bbb80972f9a30e3d712efa6cf8035d51ee55690d250`.
ARM binary3493480B SHA256:
`e2ec4eee42b94a69072825d90047bc623269f9511c63fd8df026d2f9f1dacd24`.
This artifact binary was copied outside the shared Cargo target; it is not
installed. Its build success does not waive the ARM normal-parallel failure.

Immutable review tar/manifests remain under sibling `review-e918c256.../`, with
`arm-diagnostic/` containing the failed log, core info, preserved crashed binary,
provenance and bounded comparison logs. No raw core memory is included in source
archives. V1, compact, previous snapshots, originals, installations and sources/
ttfx remain untouched. No60fps/watts/unique-content or transferred v1 resource
claim; fixed configuration can change sessions that formerly inherited plugin
settings. Local PR review only; no sending/publication was performed here.
