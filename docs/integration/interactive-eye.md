# Interactive eye

`--interactive-eye` draws a monochrome eye from ASCII glyphs through the existing
GPU pipeline, as a click-through background or as a screensaver. It
blinks every 2.8 to 6 seconds, eases its gaze toward the global cursor, also over
windows and toward adjacent monitors, and drifts between safe screen positions.

Audio is always on and comes from the default-sink monitor, never the microphone.
A soft envelope drives pupil size, iris light and glints, and leaves headroom for
beats at sustained levels. Silence decays to zero while blink and gaze continue.

Directional shading, density-ranked glyphs and corneal glints with gaze parallax
give it depth. Behind it, dim stars, orbiting polygons and sine ribbons drift in
silence and respond moderately to music. Backdrop cells use `attrs` 0, so they
are told apart from the eye and never overwrite it.

Feature probe: `--standalone --probe --interactive-eye-probe` returns
`omarchy-ttfx-layer interactive-eye 1`; the ordinary capability reply is
unchanged. It excludes interactive Matrix.
Source-specific auto resolution chooses a coarse per-output grid for this scene.
