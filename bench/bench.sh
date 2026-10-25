#!/bin/bash
# bench.sh <label> <term|ours|idle> <bin> <fps> <secs>
set -u
export XDG_RUNTIME_DIR=/run/user/$(id -u)
export DBUS_SESSION_BUS_ADDRESS=unix:path=$XDG_RUNTIME_DIR/bus
export WAYLAND_DISPLAY=wayland-1
export HYPRLAND_INSTANCE_SIGNATURE=$(ls -t $XDG_RUNTIME_DIR/hypr | head -1)
export OMARCHY_PATH=/usr/share/omarchy
label=$1 kind=$2 bin=$3 fps=$4 secs=$5
D=~/.cache/ttfx-pr61
EFFECTS="matrix rain decrypt thunderstorm rings fireworks"
unit=ssb-$label-$$
WARM=15
hwfan=$(grep -l applesmc /sys/class/hwmon/hwmon*/name | head -1 | xargs dirname)/fan1_input
temp() { cat /sys/class/thermal/thermal_zone2/temp; }
case $kind in
  term)
    systemd-run --user --scope --unit=$unit --quiet \
      foot --app-id=org.omarchy.screensaver --config=$OMARCHY_PATH/default/foot/screensaver.ini \
      -e $D/loop.sh "$bin" "$fps" $EFFECTS >/dev/null 2>&1 & ;;
  ours)
    H=$D/ourhome; S=$H/.local/state/omarchy/audio-background
    rm -rf $H; mkdir -p $S
    arr=$(echo $EFFECTS | tr ' ' '\n' | sed 's/.*/"&"/' | paste -sd, -)
    printf '{"running":true,"audio":false,"show_fps":false,"boot_between":false,"effect":"matrix","effects":[%s],"intensity":5,"speed":5,"byline":"","restart":0,"intro_size":2,"rotate_secs":20,"resolution":%s,"reactivity":2,"ttfx_text":"OMARCHY","panel_opacity":0.6,"intro_beat_sync":true,"use_theme_colors":true,"transparent_background":false,"char_style":"native","boot_char_style":"native"}\n' "$arr" "${RES:-2}" > $S/state.json
    VTE=$HOME/.cache/vte-local/root/usr/lib; HOME=$H LD_LIBRARY_PATH=$VTE TTFX_BG_RENDERER=gpu TTFX_BG_MAX_FPS=$fps \
      systemd-run --user --scope --unit=$unit --quiet "$bin" --no-intro >/tmp/ours-$label.log 2>&1 & ;;
  idle) ;;
esac
sleep $WARM
cg=""
if [ $kind != idle ]; then
  cgp=$(systemctl --user show -p ControlGroup --value $unit.scope)
  cg=/sys/fs/cgroup$cgp
fi
cgu() { [ -n "$cg" ] && awk '/usage_usec/{print $2}' $cg/cpu.stat || echo 0; }
pidcpu() { [ -n "$1" ] && [ -r /proc/$1/stat ] && awk '{print $14+$15}' /proc/$1/stat || echo 0; }
foot=$(pgrep -x foot | while read p; do grep -qx "$p" $cg/cgroup.procs 2>/dev/null && echo $p; done | head -1)
hypr=$(pgrep -x Hyprland | head -1)
sys() { awk '/^cpu /{print $2+$3+$4+$7+$8, $2+$3+$4+$5+$6+$7+$8}' /proc/stat; }
t0=$(date +%s.%N); u0=$(cgu); f0=$(pidcpu "$foot"); h0=$(pidcpu $hypr); r0=$(cat /sys/class/drm/card1/power/rc6_residency_ms); read sb0 st0 <<<"$(sys)"
n=0; tsum=0; fsum=0; qsum=0
end=$((SECONDS + secs))
while ((SECONDS < end)); do
  sleep 2
  tsum=$((tsum + $(temp))); fsum=$((fsum + $(cat $hwfan)))
  q=$(cat /sys/devices/system/cpu/cpu*/cpufreq/scaling_cur_freq | awk '{s+=$1} END{print int(s/NR)}'); qsum=$((qsum + q)); n=$((n+1))
done
t1=$(date +%s.%N); u1=$(cgu); f1=$(pidcpu "$foot"); h1=$(pidcpu $hypr); r1=$(cat /sys/class/drm/card1/power/rc6_residency_ms); read sb1 st1 <<<"$(sys)"
tend=$(temp)
[ $kind != idle ] && systemctl --user stop $unit.scope 2>/dev/null
pkill -f "$D/loop.sh" 2>/dev/null
awk -v l=$label -v k=$kind -v fps=$fps -v t0=$t0 -v t1=$t1 -v u0=$u0 -v u1=$u1 -v f0=$f0 -v f1=$f1 -v h0=$h0 -v h1=$h1 -v r0=$r0 -v r1=$r1 \
  -v sb0=$sb0 -v sb1=$sb1 -v st0=$st0 -v st1=$st1 -v n=$n -v ts=$tsum -v fs=$fsum -v qs=$qsum -v te=$tend 'BEGIN{
  w=t1-t0; tot=(u1-u0)/1e6/w*100; ft=(f1-f0)/100/w*100; hy=(h1-h0)/100/w*100
  gpu=100-(r1-r0)/(w*1000)*100; if(gpu<0)gpu=0
  sysb=(sb1-sb0)/(st1-st0)*100
  printf "%-14s %-5s fps=%-3s total=%5.1f%% foot=%5.1f%% ttfx=%5.1f%% hypr=%5.1f%% gpu=%5.1f%% sys=%5.1f%% temp=%4.1f/%4.1fC fan=%4d freq=%4dMHz\n", l,k,fps,tot,ft,tot-ft,hy,gpu,sysb,ts/n/1000,te/1000,fs/n,qs/n/1000 }'
