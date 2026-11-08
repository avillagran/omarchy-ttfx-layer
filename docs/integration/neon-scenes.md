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

The pointer gently offsets the camera, and corner marks show where it rests.
It brings what it rests on into focus. In `neon-city` the tower under it
wakes: its windows light with a scan line climbing them and its name shows over
the roof in the pixel font, and the cars on the road come into focus like the
map elements of the other roads (CRUISER, TAXI, COUPE). On the other roads, map elements offer themselves
to the pointer while they are drawn (`Canvas::offer`, `offer_quad` for ground
patches, `offer_screen` for the sun, moon and planet); the nearest one under it
brightens, corner marks close in round it breathing with the bass, and its name
shows over it: palms, cacti, saguaros, shrubs, signs, the convertible and the
sun in `sunset-drive`; prisms, gate posts, ridges (with their height) and the planet
in `vector-highway`; ground patches (with their altitude), crystals and the
moon in `circuit-run`. The octahedron under the pointer also swells, spins up
and burns white. Each beat (bass well over its recent level) sends a ring of
light out from the pointer, or from the vanishing point when there is none.
Focus and rings only brighten cells already drawn (`Raster::lift`), so they
cost under 0.05 ms a frame at 310x96.

Scene objects are generated beyond the fog boundary, so they never appear
suddenly in view.

## Showcase scenes

These live in `showcase_scenes.rs`, behind `Style::Showcase`.

- `live-terminal`: an Omarchy Quattro 4.5 desktop in a terminal. A top bar shows
  workspaces, the real clock, the music level, CPU and memory; a foot shell types
  scripted Omarchy commands (updates, themes, packages, `fastfetch` with the
  Quattro mark). The journal pane and CPU bars are decorative, and memory is the
  real value, read once a second. The line under the cursor reads as selected.
- `data-globe`: rotating Earth from continent polygons, with bass-born traffic
  arcs between cities.
- `aquarium`: a fish tank with a schooling shoal, a castle, and a chest and a puffer that answer the bass
- `jellyfish`: bells pulsing on bass, plankton leaning toward the cursor.
- `cockpit`: window frame and HUD, asteroids, ringed planet, warp on bass.
  when the sand stops, and the time shows in the lower pile.

## Solid 3D scenes

These live in `scenes3d.rs`, behind `Style::Showcase`. A small perspective
camera, a depth-tested triangle rasterizer and a per-cell sphere ray tracer draw
on the same cell raster. Light becomes glyph density, and solid ground uses shade
blocks (`░▒▓█`) so relief reads at a glance.

- `event-horizon`: a black hole. The disk is ray traced per cell: each ray runs
  straight to its closest approach and leaves bent toward the hole, so the far
  side of the disk rises over the shadow and its underside shows below. Beaming
  brightens the approaching side. Bass feeds the disk and lights hot spots,
  treble drives polar jets, and now and then a star is torn into a stream that
  falls in. Stars, jets and the stream use a point-lens model. The cursor throws
  rocks and star dust in where it points (more on bass, a burst on each beat;
  without a cursor a beat throws a little dust in from the dark). They spiral
  in, heat as they near the hole, rocks stretch along their fall, and each one
  swallowed lights the disk where it lands and flares the photon ring.
- `supernova`: a granulated star that pulses on the bass (music hastens its end),
  collapses, flashes, and throws a filamentary shell with an equatorial ring and
  axial lobes round a pulsar whose beam flashes the nebula. Beats send shock
  rings through the shell. The gas falls back into a new star.
- `solar-system`: one planet per CPU core, up to twelve (more cores are averaged
  in runs). Load sets each planet's speed, size, heat and wake. The sun grows with total CPU, the asteroid belt is as dense
  as memory use, and swap adds comets. A line shows CPU, RAM, swap and load.
- `abyss`: a sand floor with ripples, caustics and fog, boulders, kelp, plankton,
  marine snow, three shoals (silver fish, large golden fish drawn whole when
  near, and lanternfish whose lights pulse with the treble and flare in a ring)
  and a manta. The cursor is a lamp that lights the
  sand, draws plankton and stirs sand as it moves. Beats send bioluminescent
  rings through the water and across the floor. The world repeats exactly every
  1024 units, so travel never loses precision.
- `load-canyon`: CPU history as terrain flowing away from the camera, one ridge
  per core and four samples a second. Height is the square root of load over the
  recent peak, so an idle core still shows relief beside a busy one. Memory use
  is a water line drawn where it meets the terrain, and busy cores throw sparks
  (more with the bass, bursts on beats) over dust and embers that drift back
  with the history. The cursor swings the camera nearly all the way round and
  from low over the ridges to high above, so the newest relief can be watched
  from afar, from the side or from behind.

The cyberpunk scenes live in `scenes3d/cyber.rs`. Large faces there are filled
as polygons clipped to the near plane, with attributes interpolated under
perspective, so window grids and floor lines hold still as the camera moves.
The rasterizer skips the shading of any cell already held by something nearer,
so scenes draw nearest first.

- `neon-avenue`: a flight down a rainy avenue of towers. Lit windows, shop neon,
  vertical signs, billboards with an equalizer or the machine's load, flying
  cars in four sky lanes, and a wireframe hologram over every crossing. The wet
  street mirrors the neon of the nearest side. CPU load lights more windows and
  fills the lanes; bass pulses the neon, glitches the holograms and sends a
  wave of light down the windows on each beat; a hard beat now and then
  strikes lightning; treble brings the rain and spins the holograms. The
  cursor steers the flight across the avenue and up the towers, and the facade
  it points at is scanned: rings of light sweep out from the spot, every window
  there wakes up and the building is named. The city repeats after 1024
  blocks, so the flight wraps without a seam.
- `packet-grid`: a synthwave grid towards a striped sun behind wireframe
  mountains. Each pulse of light is network traffic from `/proc/net/dev`
  (loopback left out): downloads run at the camera, uploads away, at a rate on
  a log scale from a trickle to 100 MB/s. Towers on either side keep the last
  twelve seconds of each direction. The grid is drawn per cell, a line one cell
  wide at any distance, its cross lines thinning out towards the horizon.
  Beats roll rings of light down the grid and pulse the sun, whose stripes run
  faster with the bass. The cursor moves the camera and marks the floor with a
  ring and a beam that draw passing packets in and count them.
- `silicon`: a processor on its board. One core per CPU (up to sixteen) rises
  and heats from blue to red with its load and throws heat as it works. Eight
  memory chips fill with use. Disk reads and writes (`/proc/diskstats`, whole
  disks only) and network traffic run along the traces, in towards the die or
  out from it. The clock ring turns faster with the bass, beats light the
  copper and spread rings over the board, and treble hurries the signals. The
  cursor turns the board and tilts the view; the core it rests on is outlined,
  named and throws more heat, and resting on the memory names it big.

Text in these three is drawn in a 3x5 pixel font of half blocks, sized to the
room it has: status lines grow with the grid, labels with how near their object
is, and either falls back to plain characters where the font would not fit.

Per-cell shading keeps libm out of the inner loops (`floor`, `ceil`, `fmod`
and `exp` are calls on baseline x86-64), the rasterizer walks only the span a
triangle covers on each row with depth and attributes set up as a + b·x, and
each scene draws nearest first. Work that depends on one coordinate is done
once per row or column: the abyss keeps its sand heights from frame to frame
and samples its light shafts once per lattice column, the canyon builds its
lattice in one pass and spends two triangles per lane wherever a lane is under
14 cells wide. On the M2 at 520x160 cells a frame of `neon-avenue` costs about
2.5 ms, `silicon` 1.1 ms and `packet-grid` 0.9 ms; on a Ryzen 5900X at the
355x98 grid of a 1080p display, `abyss` costs 1.7 ms and `load-canyon` 1.4 ms.

None of this needs the display's rate when nobody is looking closely: every
scene drops to 30 fps while tiled windows cover its output (it only shows in
the gaps and through translucent windows) or after 8 seconds with the cursor
still and no sound, and climbs back on the next movement or note. A grid finer
than `auto` is learned only while it costs under 30% of the CPU budget at the
display's rate, at most two steps, so a cheap scene on a fast machine stays
cheap instead of growing until it fills a quarter of a core.

`/proc/stat` is read four times a second, `/proc/meminfo` and `/proc/loadavg`
once a second, `/proc/net/dev` and `/proc/diskstats` twice a second, never per
frame.

The manual test `render_stills` writes stills (PPM) of any scene, for review and
for the selector posters:

    SCENES="event-horizon abyss" SNAPS=6,20 GRID=192x56 POSTER_DIR=stills \
      cargo test --release --no-default-features --bin ttfx-bg-rs -- \
      render_stills --ignored --nocapture

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

### Gestures and music events

The layer only learns where the pointer is, never a click, so every sand clock
reads gestures from its motion (`showcase_scenes/sand.rs`, `Hand`):

- Rest: how long the pointer has stayed within about a row (`Kit::dwell`,
  eased into `Kit::charge`); toys grow things with it, such as the ferrofluid
  hedgehog. It never bursts: only a shake does.
- Circle: about a turn within 1.4 seconds, keeping its distance from the centre,
  is a swirl (`Kit::swirl`, centre and turns per second). An out and back
  stroke also sweeps a turn round its middle, but passes through it, so it is
  not one. The classic clocks stir a vortex there.
- Shake: four fast sideways reversals within 0.7 seconds (`Kit::shake`). Only
  sideways, so a vertical zigzag across the digits is still a sweep. The
  classic clocks blow apart and pour back.

And events from the music (`Ear`): a beat is a rising edge of the bass; a drop
is the bass back after 1.5 seconds of music without it, or loud music within
0.6 seconds of a silence longer than 3 seconds; silence counts its seconds.

### Sand toys

Ten more clocks are built for play. Each is a `Play` in `showcase_scenes/sand/`
and gets the same `Kit` every frame: grid, wall clock, sound, beats, drops,
silence and gestures. In every one the time is sand: `SandText` (`sand/text.rs`)
makes each point of the digits a grain that the cursor scatters along its path
and that pours home a few seconds later, stirring with a circle and blowing
apart with a shake or a drop; each toy places it and paints the grains in its own look.
Other helpers: `face` (the sand clock's Cairo face fitted to any box), `stamp`
(the pixel font) and `ring`.

- `sand-clock-ferrofluid`: grains melt into one liquid (each spreads a soft
  blob into a density field) drawn as black chrome: dark flats, neon rims and
  a white flare facing the light. The cursor is a magnet that stretches the
  digits; resting grows a hedgehog of spines from the nearest fluid. Bass raises cones on a fixed lattice of sites along every
  edge, treble sparks on their tips, a drop makes it all bristle.
- `sand-clock-swarm`: nanobots hold the time in silence; with music most of
  them trace a live waveform (spread along its arc length) under a small clock,
  and a hard beat blows them apart. An escort follows the cursor as a hex
  shield that brightens at rest; a fast sweep makes the
  bots it passes give chase.
- `sand-clock-excavation`: brush sand off a desert floor to unearth relics in
  pixel art; one mostly bare gets breathing brackets and its name. The time is
  engraved on a stone tablet that glows through the sand. Bass gusts re-cover,
  beats ping a sonar from the tablet, a drop reveals every relic; a new set is
  buried after all are found.
- `sand-clock-glass`: dust settles on a pane in front of a neon city whose
  billboard shows the time; the cursor wipes it, leaving clumps, bass makes
  thick dust run in streaks, a shake knocks it off, resting polishes a shine.
- `sand-clock-foundry`: a chip with the time etched in copper; the cursor is a
  laser that fuses sand into routed traces (straight, square or diagonal, vias
  at turns) that cool from white to copper. Beats send data along them, treble
  sparks at vias; after six seconds of silence they crumble back to sand.
- `sand-clock-mandala`: rings of coloured sand with 6 to 16-fold mirror
  symmetry round the time; the cursor pours sand into every fold. Bass turns
  it, beats send rings out; each minute it is swept into the centre and bursts
  out as the new time, and a fresh pattern begins.
- `sand-clock-dust-devil`: a neon desert under a synthwave sun with the time on
  a billboard; a column of orbiting grains follows the cursor, born where it
  rests, wound up by circling, widened by bass and lit by treble lightning; a
  drop rolls a dust storm across.
- `sand-clock-spectrum`: a falling sand automaton pours into one tube per band
  of a pseudo-spectrum (low bands follow bass, high ones treble); piles keep an
  angle of repose and drain when their band goes quiet; the cursor blows them
  like a hair dryer.
- `sand-clock-zen`: a raked garden drawn as groove crests, each cell a line
  glyph along its groove, with server-block stones whose LEDs and meters follow
  the music. The cursor is a five-tine rake: grooves glow cyan and fade back
  into the comb over a minute. HH:MM is engraved in the sand and an unseen rake
  rewrites the changed digits each minute. Beats light the rings round the
  stones; resting rakes rings round the cursor; a shake smooths it flat and a
  comb sweeps the pattern back.
- `sand-clock-shore`: a bioluminescent beach at night. The cursor leaves
  alternating footprints that glow in wet sand; bass raises the waves that wash
  them off and leave a foam line sparkling with treble. HH:MM is pressed into the
  sand in the sand clocks' bold face; the wave each minute erases it and it is
  written again digit by digit.
  Beats pulse through the water, a drop sends a big wave.

`--resolution auto` picks the grid from the display: cell density follows its
scale, capped at about 360 columns, because the renderer's cost follows the size
of the grid rather than the cells a scene draws.

The manual test `showcase_frame_cost` reports step and draw time per scene:

    cargo test --release --no-default-features --bin ttfx-bg-rs -- \
      showcase_frame_cost --ignored --nocapture --test-threads=1
