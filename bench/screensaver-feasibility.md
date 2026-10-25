# Using the GPU layer renderer as the Omarchy screensaver

Status: analysis only (2026-10-25), separate project.

## How the stock screensaver works (Omarchy 4.0.6)
- `shell/plugins/services/idle/Service.qml` runs `omarchy-launch-screensaver`
  150 s after the last input, then `omarchy-system-lock` at 300 s.
- `omarchy-launch-screensaver` opens one fullscreen terminal per monitor
  (class `org.omarchy.screensaver`) running `omarchy-screensaver`, which loops
  `ttfx --frame-rate 120 --random-effect`.
- Dismissal: `omarchy-screensaver` does `read -n1 -t 1` (a key typed into the
  terminal) or exits when the screensaver window loses focus. Mouse motion
  produces no terminal input, so the mouse only dismisses it indirectly, when
  the compositor moves focus. That is the problem the user sees.
- The idle service tracks `openwindow`/`closewindow` for that class: closing
  the window before the lock deadline cancels the pending lock.

## What our renderer would do instead
- One `zwlr_layer_shell` surface per output on the Overlay layer, fullscreen,
  `keyboard_interactivity = exclusive`, drawn by the existing GPU grid.
- Add `wl_seat` listeners: any `wl_pointer.motion/button/axis` or
  `wl_keyboard.key` event exits the process. Pointer focus lands on the
  overlay layer because it covers everything, so mouse motion works directly.
- Hide the cursor the way the stock script does (`hl.config cursor.invisible`)
  and restore it on exit.
- Exit on session lock (the occlusion module already detects LOCK) so the
  lock screen is never fought.
- Cost on mbp-intel: ~24 fps at 5-6% CPU / 23% GPU instead of 77% CPU for the
  terminal path (foot renders every frame on the CPU and Hyprland uploads it).

## Integration points
- Idle service calls `omarchy-launch-screensaver` through `bash -lc`; PATH
  there starts with `/usr/local/sbin:/usr/local/bin:/usr/bin`, and
  `~/.local/bin` is not ahead of `/usr/bin`, so a user wrapper does not
  override it. Options: (a) `omarchy-toggle-enabled screensaver-off` to silence
  the stock one and have a plugin service watch `omarchy-shell idle status`
  / the Hyprland idle events to start ours; (b) an upstream change letting
  `omarchy-launch-screensaver` run a user-configured command.
- The idle service's "screensaver dismissed → cancel lock" logic keys on a
  window class. With a layer there is no window; mouse motion wakes the
  IdleMonitor anyway, which cancels the cycle, so behaviour is preserved.
- Hyprland repaints the whole layer per commit (see hyprland-layer-damage.md);
  irrelevant for a fullscreen screensaver.

## Effort
About a day: `--screensaver` mode in the binary (overlay layer, input
listeners, cursor hide, multi-output), launch script, idle integration, tests
on mbp-intel for wake latency and lock interplay.
