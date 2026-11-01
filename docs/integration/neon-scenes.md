# Named scenes

Select a scene with `--neon-scene NAME`. All of them are procedural geometry on
the existing standalone glyph renderer. Audio is always on and comes from the
playback monitor. The same scene runs as a background on the bottom layer or as a
screensaver on the overlay layer, whichever layer and keyboard mode the caller asks
for. Named scenes exclude interactive Matrix, Eye and Tunnel; unknown or missing
names fail before startup.
The live selector uses the same models. Silence keeps full motion, and music adds
to it without changing travel speed. A frame identical to the last one is not
published.

## Journeys

- `neon-city`: towers with front and street-side windows. Treble lights more
  windows, bass climbs the floors, and music objects light whole towers. Cars run
  in the lanes and leave past the camera.
- `vector-highway`: wireframe planet, rolling hills and tumbling octahedra.
- `circuit-run` (Lunar Drift): continuous neon terrain under a moon. Music raises
  crystals along the way.
- `sunset-drive`: striped sun, palms, cacti, swaying grass, birds and a rear-view
  convertible. Music adds signs, saguaros and lit shrubs.

The pointer gently offsets the camera. Scene objects are generated beyond the fog
boundary, so they never appear suddenly in view.

## Showcase scenes

These live in `showcase_scenes.rs`, behind `Style::Showcase`.

- `live-terminal`: a shell that types scripted Omarchy commands. The journal pane
  and CPU bars are decorative, and memory is the real value, read once a second.
- `data-globe`: rotating Earth from continent polygons, with bass-born traffic
  arcs between cities.
- `aquarium`: a fish tank with a schooling shoal, a castle, and a chest and a puffer that answer the bass
- `jellyfish`: bells pulsing on bass, plankton leaning toward the cursor.
- `cockpit`: window frame and HUD, asteroids, ringed planet, warp on bass.
  when the sand stops, and the time shows in the lower pile.

## Sand clocks

`sand-clock` draws HH:MM:SS from sand. Each character keeps a stable pool that grows
and never shrinks, and grain i follows glyph point i % n, so only changed characters
move. A grain covers 14% of the remaining distance per 60 Hz frame with a fading
sway. Digits are a bold monospace face rasterized with Cairo, or block digits if no
font is usable. The cursor scatters only the grains on its path, and each returns
after 2.5 to 3.5 seconds.

Three variants add a backdrop, and their digits tremble on the bass:

- `sand-clock-chladni`: grains seek the zero lines of a vibrating plate. Bass
  rattles them and treble advances the figure.
- `sand-clock-dunes`: ridge layers, with wind and lifted sand on bass.
- `sand-clock-galaxy`: a three-arm spiral with a bass ring and treble stars.

`--resolution auto` picks the grid from the display: cell density follows its
scale, capped at about 360 columns, because the renderer's cost follows the size
of the grid rather than the cells a scene draws.

The manual test `showcase_frame_cost` reports step and draw time per scene:

    cargo test --release --no-default-features --bin ttfx-bg-rs -- \
      showcase_frame_cost --ignored --nocapture --test-threads=1
