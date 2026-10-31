# Neon Transit

`--interactive-tunnel` is a curved wireframe tunnel on the existing standalone
GPU glyph pipeline. It adds no compositor path, frame interpolation or logo.

Rectangular cross-sections are projected through a perspective camera, joined by
longitudinal rails, and bent and twisted along depth. Cyan and magenta walls,
amber floor chevrons, couriers, dust and ceiling light strips give independent
depth and speed cues. Line segments are clipped to the viewport before
rasterization, and the nearest section wins each cell's depth test.

Three staggered light bikes overtake the camera with hoop wheels, a rectangular
chassis and 14 world-unit trails, clipped at the near plane. The glyphs `▬` and
`▌` add solid accents to the ASCII wireframe.

The global cursor eases camera offset and roll, also over windows. Audio is always
on: treble launches brighter sections, bass adds packets of light and pulses the
ceiling strips, and travel speed stays constant. Silence keeps the base motion.
Desktop and live preview share this model. The mode needs a bottom layer with
keyboard `none` and excludes interactive Matrix and Eye.
