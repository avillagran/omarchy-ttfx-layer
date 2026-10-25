Tested this on old hardware: a 2015 MacBook Pro, i5-5257U (Broadwell, Iris 6100, Mesa), 2560x1600 at scale 2, fresh Omarchy 4.0.6, foot (the default terminal).

Setup: foot with the screensaver config (`--app-id=org.omarchy.screensaver`, fullscreen, 88x25 cells) running the same flags as `omarchy-screensaver` (`--canvas-width 0 --canvas-height 0 --reuse-canvas --anchor-canvas c --anchor-text c --no-eol --no-restore-cursor`), looping matrix, rain, decrypt, thunderstorm, rings and fireworks. Master is 921bd55, the PR is 68a5f8d, both built locally. 120 s per run after 15 s warm-up, two rounds in mirrored order, cooled down between runs. CPU is percent of one core, read from the scope's cgroup and /proc; GPU busy comes from i915 RC6 residency.

| | foot + ttfx | Hyprland | GPU busy | package temp avg / end |
|---|---|---|---|---|
| master, 120 fps | 133.2% | 30.0% | 46.8% | 81 / 89 °C |
| PR, 120 fps | 130.3% | 30.4% | 45.2% | 80 / 86 °C |
| master, 30 fps | 88.9% | 39.4% | 19.4% | 68 / 74 °C |
| PR, 30 fps | 76.8% | 36.5% | 17.3% | 68 / 72 °C |

Both rounds agreed within about 2 points.

The PR works as described: for matrix it cuts the bytes written from 73.1 MB to 1.6 MB over 8 s, with no scrolling or full repaints in the stream. On this machine foot's CPU barely follows, though. Per effect at 30 fps, foot went from 95.5% to 93.8% on matrix, 88.5% to 76.3% on decrypt, 86.2% to 81.1% on rings, and 91.5% to 73.2% on thunderstorm.

Nearly all of foot's time is in its four render threads, at about 21% each; the main thread that parses the output is at 2%. A fullscreen foot with a single cell changing 30 times a second already costs 24% for foot, 13% for Hyprland and 18% GPU busy. On slow CPUs, then, a terminal's fixed per-frame cost dominates and the frame rate matters more than the number of changed cells. Going from 120 to 30 fps saved more than this PR did, and the two combine.

So this looks like a good change. For older machines I'd also suggest a lower `--frame-rate` in `omarchy-screensaver`, which currently passes 120.
