#!/bin/sh
# Launch the Rust audio-reactive background for the current architecture.
# The repo ships one prebuilt binary per supported arch; this script picks the
# right one so the plugin "just works" after `omarchy plugin add` with no build
# step. Add a new arch by dropping bin/ttfx-bg-rs-<uname -m> and a case below.
set -e
# x86_64 draws with the GPU layer (TTFX_BG_RENDERER=gpu). On Hyprland every
# commit of a layer surface repaints the whole output, so the frame rate is
# the main cost lever: on a 2015 MacBook Pro (Iris 6100) each frame per second
# costs about 1% GPU busy and 0.2% CPU. TTFX_BG_MAX_FPS=auto measures the CPU
# cost per frame and picks the highest ceiling (20-60 fps) that keeps the
# process within TTFX_BG_CPU_BUDGET (default 8% of one core): about 24 fps on
# that MacBook, 60 fps on a current machine. Set a number to fix it instead.
# The remaining TTFX_BG_* settings tune the direct renderer it falls back to
# when EGL/GLES 3 is unavailable.
DIR=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)
ARCH=$(uname -m)
case "$ARCH" in
  aarch64|arm64) exec "$DIR/ttfx-bg-rs-aarch64" "$@" ;;
  x86_64|amd64)  export TTFX_BG_RENDERER=gpu; export TTFX_BG_MAX_FPS=${TTFX_BG_MAX_FPS:-auto}; export TTFX_BG_RASTER=retained; export GSK_RENDERER=vulkan; export TTFX_BG_OPAQUE=1; export TTFX_BG_TILECOPY=0; export TTFX_BG_WAKEUP=1; export TTFX_BG_DAMAGE=bounded; exec "$DIR/ttfx-bg-rs-x86_64" "$@" ;;
  *) echo "ttfx-bg: unsupported architecture: $ARCH" >&2; exit 1 ;;
esac
