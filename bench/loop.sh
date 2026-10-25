#!/bin/bash
# Runs inside foot exactly like omarchy-screensaver, but with a fixed effect
# sequence so every variant renders the same content.
bin=$1 fps=$2; shift 2
printf '\033]11;rgb:00/00/00\007'
deadline=$((SECONDS + 2))
while ((SECONDS < deadline)) && [[ $(stty size 2>/dev/null) == "24 80" ]]; do sleep 0.02; done
while true; do
  for e in "$@"; do
    "$bin" -i ~/.config/omarchy/branding/screensaver.txt \
      --frame-rate "$fps" --canvas-width 0 --canvas-height 0 --reuse-canvas --anchor-canvas c --anchor-text c \
      --no-eol --no-restore-cursor "$e"
  done
done
