#!/bin/bash
# footcpu.sh <label> <cmd...>: foot + Hyprland CPU for 20 s running cmd fullscreen
export XDG_RUNTIME_DIR=/run/user/$(id -u) DBUS_SESSION_BUS_ADDRESS=unix:path=/run/user/$(id -u)/bus WAYLAND_DISPLAY=wayland-1 OMARCHY_PATH=/usr/share/omarchy
export HYPRLAND_INSTANCE_SIGNATURE=$(ls -t $XDG_RUNTIME_DIR/hypr | head -1)
l=$1; shift
systemd-run --user --scope --unit=fc-$$ --quiet foot --app-id=org.omarchy.screensaver --config=$OMARCHY_PATH/default/foot/screensaver.ini -e "$@" >/dev/null 2>&1 &
sleep 8
foot=$(pgrep -nx foot); hy=$(pgrep -x Hyprland)
c() { awk '{print $14+$15}' /proc/$1/stat; }
a=$(c $foot); h=$(c $hy); r=$(cat /sys/class/drm/card1/power/rc6_residency_ms); sleep 20
awk -v l=$l -v f=$(( $(c $foot) - a )) -v h=$(( $(c $hy) - h )) -v r=$(( $(cat /sys/class/drm/card1/power/rc6_residency_ms) - r )) 'BEGIN{printf "%-22s foot=%5.1f%% hypr=%5.1f%% gpu=%5.1f%%\n", l, f/20, h/20, 100-r/200}'
systemctl --user stop fc-$$.scope 2>/dev/null
