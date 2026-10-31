# Interactive Matrix

Opt in with `omarchy-ttfx-layer --interactive-matrix`. This is a new rain simulation using the existing clipped GPU
glyph renderer/cache, not an equivalence-preserving change to the TTFX Matrix.
It reads no branding and does not change ordinary effect behavior.

ASCII rule: audio reactivity is on for this mode, without requiring `--audio`.
The existing system-output monitor analyzer separates bass and treble bands.
Bass controls extra top-origin streams across the whole output, independently
of the mouse. Treble sets a pale color at each strip’s first visible entry; that
base palette is latched for its complete lifetime. Existing strips are never
recolored by later audio. The default motion adds a smoothed bass impulse, at most 1.4x base fall speed;
`motion=steady` in the source config keeps constant speed for comparison. Both
models preserve the bass-density and frozen treble-color contract. Mouse emphasis
only boosts brightness locally, preserving each strip’s base hue. Silence keeps
the mouse-driven baseline animation.
No microphone capture.

Motion preference is read once at producer/preview start from
`~/.config/omarchy/ambient/matrix-reactive.conf`, optional `motion=reactive|steady`.
`TTFX_BG_MATRIX_MOTION` overrides it for isolated comparisons. The preview and
background use the same preference and smoothed spectral drive.

A read-only Hyprland IPC tracker samples the global pointer at 30 Hz, including
when an application window covers the background. It uses the Unix command
socket directly, without spawning `hyprctl` per sample; output geometry is
refreshed every two seconds. Logical coordinates account for output origin,
scale and rotation. Leaving an output stops new streams there.

The pointer selects nearby vertical rain lanes. Those lanes brighten gently;
additional streams (36 per second in a 24-column band) start just above the top edge with the same speed range and trail
shape as baseline rain, never at the pointer position. Leaving fades the lane
emphasis and stops spawning; existing streams finish falling. No radial halo.

The layer remains entirely click-through, with no keyboard grab, pointer grab,
or logo. It uses one bounded tracker per output. It requires Hyprland, and does not promise global input on other compositors.

The field uses a reused grid; added streams are capped at 384. Simulation time
uses elapsed seconds and the existing adaptive FPS governor, with a 60 FPS cap.

Feature handshake: `--standalone --probe --interactive-matrix-probe` returns
`omarchy-ttfx-layer interactive-matrix 1`. Existing capability reply is unchanged.
Rejects other explicit effects.

Bass birth rate is proportional to the smoothed low-band envelope, with a
bounded pool. Treble alone does not add streams; bass alone does not lighten
them. Combined bands naturally produce more pale new streams. Color latching
applies to baseline respawns and extra mouse/bass streams, sampled at first
visible entry. Normal head/tail fade remains part of the fixed strip palette.
