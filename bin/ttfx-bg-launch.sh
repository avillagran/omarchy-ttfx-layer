#!/bin/sh
# Launch the Rust audio-reactive background for the current architecture.
# The repo ships one prebuilt binary per supported arch; this script picks the
# right one so the plugin "just works" after `omarchy plugin add` with no build
# step. Add a new arch by dropping bin/ttfx-bg-rs-<uname -m> and a case below.
set -e
DIR=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)
ARCH=$(uname -m)
case "$ARCH" in
  aarch64|arm64) exec "$DIR/ttfx-bg-rs-aarch64" "$@" ;;
  x86_64|amd64)  export TTFX_BG_RENDERER=direct; export TTFX_BG_RASTER=retained; export GSK_RENDERER=vulkan; export TTFX_BG_OPAQUE=1; export TTFX_BG_TILECOPY=0; export TTFX_BG_WAKEUP=1; export TTFX_BG_DAMAGE=bounded; exec "$DIR/ttfx-bg-rs-x86_64" "$@" ;;
  *) echo "ttfx-bg: unsupported architecture: $ARCH" >&2; exit 1 ;;
esac
