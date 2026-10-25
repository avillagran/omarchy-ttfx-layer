#!/bin/bash
# threads.sh <bin> <fps> <effect> : per-thread CPU of foot for 20 s
export XDG_RUNTIME_DIR=/run/user/$(id -u) DBUS_SESSION_BUS_ADDRESS=unix:path=/run/user/$(id -u)/bus WAYLAND_DISPLAY=wayland-1 OMARCHY_PATH=/usr/share/omarchy
export HYPRLAND_INSTANCE_SIGNATURE=$(ls -t $XDG_RUNTIME_DIR/hypr | head -1)
D=~/.cache/ttfx-pr61
systemd-run --user --scope --unit=thr-$$ --quiet foot --app-id=org.omarchy.screensaver --config=$OMARCHY_PATH/default/foot/screensaver.ini -e $D/loop.sh "$1" "$2" $3 >/dev/null 2>&1 &
sleep 8
foot=$(pgrep -nx foot)
snap() { for t in /proc/$foot/task/*; do echo "$(cat $t/comm) $(awk '{print $14+$15}' $t/stat)"; done | sort; }
a=$(snap); sleep 20; b=$(snap)
join <(echo "$a" | awk '{print $1"_"NR, $2}' | sort) <(echo "$b" | awk '{print $1"_"NR, $2}' | sort) | awk '{printf "%-20s %5.1f%%\n", $1, ($3-$2)/20}'
hyprctl clients -j | jq -r '.[] | select(.class=="org.omarchy.screensaver") | "window \(.size)"'
systemctl --user stop thr-$$.scope
