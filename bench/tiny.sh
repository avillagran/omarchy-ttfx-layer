#!/bin/bash
# One cell changes per frame at 30 fps: foot's fixed per-frame cost.
printf '\033]11;rgb:00/00/00\007\033[?25l\033[2J'
i=0; while true; do printf '\033[20;20H%d' $((i%10)); i=$((i+1)); sleep 0.033; done
