#!/bin/bash
# screensaver-ab.sh: the native omarchy-screensaver against omarchy-ambient's
# ttfx source, full screen, same text, same effect, nobody at the keyboard.
#
#   screensaver-ab.sh series [secs]        A/B/C/C/B/A per effect, then fps, then report
#   screensaver-ab.sh run <arm> <effect> <secs>
#   screensaver-ab.sh fps <arm> <effect> [secs]
#   screensaver-ab.sh dismiss <arm>        a pointer motion must end it
#   screensaver-ab.sh report
#
# Arms: native (omarchy-screensaver, 120 ticks gated by the frame callback),
# ours (omarchy-ambient screensaver ttfx, fps=auto), ours60 (fps=60).
# Everything is written under $OUT; nothing of the user's is changed.
set -u
export LANG=C.UTF-8 OMARCHY_PATH=${OMARCHY_PATH:-/usr/share/omarchy}
export $(systemctl --user show-environment | grep -E '^(WAYLAND_DISPLAY|HYPRLAND_INSTANCE_SIGNATURE|XDG_RUNTIME_DIR|DBUS_SESSION_BUS_ADDRESS)=')

NATIVE=${NATIVE:-$HOME/bench/omarchy-screensaver/target/release/omarchy-launch-screensaver}
OURS_BIN=${OURS_BIN:-$HOME/.cache/ambient-native-pr/e918c25643f44552658889f283d07a49e28bb8eb/review-bin}
VPOINTER=${VPOINTER:-$HOME/.cache/bg-live-int/build/hardware/vpointer}
BRANDING=${BRANDING:-$HOME/.config/omarchy/branding/screensaver.txt}
OUT=${OUT:-$HOME/bench/results/$(date +%F)}
EFFECTS=${EFFECTS:-matrix thunderstorm rain}
SEED=${SEED:-7} WARM=${WARM:-15}
mkdir -p "$OUT"

# Counters; each one is optional so the script also runs on other machines.
# GPU busy: i915 RC6 residency on Intel, agx_stats busy_ns on Apple silicon.
RC6=$(ls /sys/class/drm/card*/power/rc6_residency_ms 2>/dev/null | head -1)
AGX=$(ls /sys/class/drm/card*/device/agx_stats 2>/dev/null | head -1)
# Temperature: the package sensor on x86, the GPU sensor from agx_stats on Apple silicon.
TZ=$(grep -lx x86_pkg_temp /sys/class/thermal/thermal_zone*/type 2>/dev/null | head -1); TZ=${TZ%/type}
[ -n "$TZ" ] || [ -n "$AGX" ] || TZ=$(ls -d /sys/class/thermal/thermal_zone0 2>/dev/null)
SMC=$(grep -lxE 'applesmc|macsmc_hwmon' /sys/class/hwmon/hwmon*/name 2>/dev/null | head -1); SMC=${SMC%/name}
FAN=${SMC:+$SMC/fan1_input}
# Power, in microwatts, where the SMC reports it (Apple silicon: total system and heatpipe).
PW_SYS=$(grep -lx 'Total System Power' $SMC/power*_label 2>/dev/null | head -1); PW_SYS=${PW_SYS:+${PW_SYS%_label}_input}
PW_SOC=$(grep -lx 'Heatpipe Power' $SMC/power*_label 2>/dev/null | head -1); PW_SOC=${PW_SOC:+${PW_SOC%_label}_input}
temp() {
  if [ -n "$TZ" ]; then cat "$TZ/temp" 2>/dev/null || echo 0
  elif [ -n "$AGX" ]; then awk '/^temperature_raw/{r=$2} /^temperature_scale/{s=$2} END{print (s>0)?int(r*10/s):0}' "$AGX"
  else echo 0; fi
}
fan() { cat "$FAN" 2>/dev/null || echo 0; }
rc6() { cat "$RC6" 2>/dev/null || echo 0; }
agx() { [ -n "$AGX" ] && awk '/^busy_ns/{print $2}' "$AGX" || echo 0; }
pw() { cat "${1:-/nonexistent}" 2>/dev/null || echo 0; }
freq() { cat /sys/devices/system/cpu/cpu*/cpufreq/scaling_cur_freq 2>/dev/null | awk '{s+=$1} END{print NR?int(s/NR):0}'; }
sys() { awk '/^cpu /{print $2+$3+$4+$7+$8, $2+$3+$4+$5+$6+$7+$8}' /proc/stat; }
pidcpu() { [ -n "${1:-}" ] && [ -r /proc/$1/stat ] && awk '{print $14+$15}' /proc/$1/stat || echo 0; }
log() { echo "$(date +%T) $*" | tee -a "$OUT/log.txt" >&2; }

layers() { hyprctl layers -j | jq -r '[..|objects|select(.namespace?)|.namespace]|map(select(test("screensaver|ambient")))|length'; }
clean() {
  [ "$(layers)" = 0 ] && ! pgrep -f '[o]rg.omarchy.screensaver' >/dev/null &&
    [ "$(PATH=$OURS_BIN:$HOME/.local/bin:$PATH omarchy-ambient status 2>/dev/null)" = none ]
}
wait_for() { local i; for ((i = 0; i < $2 * 10; i++)); do "$1" && return 0; sleep 0.1; done; return 1; }

start() { # arm effect unit [extra env...]
  local arm=$1 e=$2 unit=$3; shift 3
  case $arm in
    native)
      env "$@" systemd-run --user --scope --quiet --unit="$unit" \
        "$NATIVE" force --effect "$e" --seed "$SEED" -i "$BRANDING" >"$OUT/$unit.stderr" 2>&1 & ;;
    ours|ours60)
      local cfg=$OUT/config-$arm-$e fps=auto
      [ "$arm" = ours60 ] && fps=60
      mkdir -p "$cfg/omarchy/ambient" "$OUT/data"
      printf 'effects=%s\nrotate=3600\nfps=%s\nresolution=2\naudio=0\n' "$e" "$fps" >"$cfg/omarchy/ambient/ttfx.conf"
      env "$@" XDG_CONFIG_HOME="$cfg" XDG_DATA_HOME="$OUT/data" PATH="$OURS_BIN:$HOME/.local/bin:$PATH" \
        systemd-run --user --scope --quiet --unit="$unit" omarchy-ambient screensaver ttfx >"$OUT/$unit.stderr" 2>&1 & ;;
    *) echo "unknown arm $arm" >&2; return 1 ;;
  esac
  covered() { [ "$(layers)" -ge "$(hyprctl monitors -j | jq length)" ]; }
  wait_for covered 20 || { log "$unit: no layer after 20 s"; return 1; }
}
stop() { systemctl --user stop "$1.scope" 2>/dev/null; wait_for clean 10 || log "$1: not clean after stop"; }
active() { systemctl --user is-active --quiet "$1.scope"; }

run() { # arm effect secs
  local arm=$1 e=$2 secs=$3 unit=ssab-$1-$2-$$
  wait_for clean 5 || { log "not clean before $unit"; return 1; }
  local cg=""
  if [ "$arm" != idle ]; then
    start "$arm" "$e" "$unit" || return 1
    sleep "$WARM"
    cg=/sys/fs/cgroup$(systemctl --user show -p ControlGroup --value "$unit.scope")
  fi
  cgu() { [ -n "$cg" ] && awk '/usage_usec/{print $2}' "$cg/cpu.stat" 2>/dev/null || echo 0; }
  local hypr=$(pgrep -x Hyprland | head -1) gliff=$(pgrep -x gliff-server | head -1)
  local t0 u0 h0 g0 r0 a0 sb0 st0 n=0 tsum=0 fsum=0 qsum=0 psum=0 ssum=0 end
  t0=$(date +%s.%N); u0=$(cgu); h0=$(pidcpu "$hypr"); g0=$(pidcpu "$gliff"); r0=$(rc6); a0=$(agx); read sb0 st0 <<<"$(sys)"
  end=$((SECONDS + secs))
  while ((SECONDS < end)); do
    sleep 2; tsum=$((tsum + $(temp))); fsum=$((fsum + $(fan))); qsum=$((qsum + $(freq)))
    psum=$((psum + $(pw "$PW_SYS"))); ssum=$((ssum + $(pw "$PW_SOC"))); n=$((n + 1))
  done
  local t1 u1 h1 g1 r1 a1 sb1 st1 tend mem ok=1
  t1=$(date +%s.%N); u1=$(cgu); h1=$(pidcpu "$hypr"); g1=$(pidcpu "$gliff"); r1=$(rc6); a1=$(agx); read sb1 st1 <<<"$(sys)"
  tend=$(temp); mem=$(cat "$cg/memory.peak" 2>/dev/null || echo 0)
  if [ "$arm" != idle ]; then active "$unit" || ok=0; stop "$unit"; fi
  # The rate our governor settled on, from its own log; the native has no governor.
  local gov=0
  [ -f "$OUT/$unit.stderr" ] && gov=$(sed -nE 's/.*fps governor: .*-> ([0-9]+) fps.*/\1/p; s/.*starting at ([0-9]+) fps.*/\1/p' "$OUT/$unit.stderr" | tail -1)
  awk -v arm="$arm" -v e="$e" -v secs="$secs" -v ok=$ok -v t0=$t0 -v t1=$t1 -v u0=$u0 -v u1=$u1 -v h0=$h0 -v h1=$h1 \
    -v g0=$g0 -v g1=$g1 -v r0=$r0 -v r1=$r1 -v a0=$a0 -v a1=$a1 -v sb0=$sb0 -v sb1=$sb1 -v st0=$st0 -v st1=$st1 \
    -v n=$n -v ts=$tsum -v fs=$fsum -v qs=$qsum -v ps=$psum -v ss=$ssum -v te=$tend -v mem=$mem -v gov=${gov:-0} 'BEGIN{
    w=t1-t0; tot=(u1-u0)/1e6/w*100; hy=(h1-h0)/100/w*100; gl=(g1-g0)/100/w*100
    if(r1>0){gpu=100-(r1-r0)/(w*1000)*100; if(gpu<0)gpu=0} else if(a1>0){gpu=(a1-a0)/(w*1e9)*100} else gpu=-1
    sysb=(sb1-sb0)/(st1-st0)*100
    printf "{\"arm\":\"%s\",\"effect\":\"%s\",\"secs\":%d,\"ok\":%d,\"cpu\":%.1f,\"hypr\":%.1f,\"gliff\":%.1f,\"gpu\":%.1f,\"sys\":%.1f,\"temp_avg\":%.1f,\"temp_end\":%.1f,\"fan\":%d,\"mhz\":%d,\"rss_peak_mb\":%.0f,\"gov_fps\":%d,\"power_w\":%.1f,\"soc_w\":%.1f,\"at\":\"%s\"}\n",
      arm,e,secs,ok,tot,hy,gl,gpu,sysb,ts/n/1000,te/1000,fs/n,qs/n/1000,mem/1048576,gov,ps/n/1e6,ss/n/1e6,strftime("%FT%T") }' | tee -a "$OUT/runs.jsonl"
}

fps() { # arm effect [secs]: wl_surface commits per second, with the protocol log on
  local arm=$1 e=$2 secs=${3:-20} unit=ssfps-$1-$2-$$
  wait_for clean 5 || return 1
  start "$arm" "$e" "$unit" WAYLAND_DEBUG=1 || return 1
  sleep 10
  local c0 c1
  c0=$(grep -c 'wl_surface[@#][0-9]*\.commit(' "$OUT/$unit.stderr"); sleep "$secs"
  c1=$(grep -c 'wl_surface[@#][0-9]*\.commit(' "$OUT/$unit.stderr")
  local ok=1; active "$unit" || ok=0
  stop "$unit"
  awk -v a="$arm" -v e="$e" -v ok=$ok -v c0=$c0 -v c1=$c1 -v s=$secs 'BEGIN{printf "{\"arm\":\"%s\",\"effect\":\"%s\",\"ok\":%d,\"fps\":%.1f}\n",a,e,ok,(c1-c0)/s}' | tee -a "$OUT/fps.jsonl"
}

dismiss() { # arm: a pointer motion ends it within 3 s
  local arm=$1 unit=ssdis-$1-$$ begin
  wait_for clean 5 || return 1
  start "$arm" matrix "$unit" || return 1
  sleep 4
  begin=$(date +%s%N); "$VPOINTER" motion
  gone() { ! active "$unit"; }
  if wait_for gone 3; then log "dismiss $arm: ok, $(( ($(date +%s%N) - begin) / 1000000 )) ms"; wait_for clean 5; return 0; fi
  log "dismiss $arm: FAILED"; stop "$unit"; return 1
}

# Stay awake while measuring, the way the bar's indicator does; the user's own
# choice is put back afterwards.
idle_off() {
  stay_awake_was=$(omarchy-toggle-idle status 2>/dev/null | jq -r .enabled)
  omarchy-toggle-idle stay-awake >/dev/null
}
idle_on() { [ "${stay_awake_was:-false}" = true ] || omarchy-toggle-idle allow-idle >/dev/null; }

cool() { # back to the base temperature, 5 min at most
  local d=$((SECONDS + 300))
  while ((SECONDS < d)) && (( $(temp) > BASE_T + 4000 )); do sleep 5; done
  sleep 10
}

series() {
  local secs=${1:-60}
  [ "$(omarchy-shell lock isLocked 2>/dev/null)" = false ] || { log "session locked"; exit 1; }
  # Nothing may wake, lock or draw during the runs: stay awake, and pause the
  # audio-background plugin when its layer is up. Both are put back at the end.
  PLUGIN=io.github.avillagran.omarchy-audio-background plugin_was_on=false
  hyprctl layers -j | jq -e '[..|objects|select(.namespace? == "ttfx-bg-gpu")]|length>0' >/dev/null && plugin_was_on=true
  restore() {
    idle_on
    $plugin_was_on && omarchy-shell shell setPluginEnabled "$PLUGIN" true >/dev/null
    log "idle service and plugin restored"
  }
  trap restore EXIT
  idle_off
  $plugin_was_on && { omarchy-shell shell setPluginEnabled "$PLUGIN" false >/dev/null; sleep 3; }
  wait_for clean 10 || { log "something is already on screen"; exit 1; }
  {
    echo "machine: $(hostname) $(uname -m) $(grep -m1 'model name' /proc/cpuinfo | cut -d: -f2)"
    echo "monitors: $(hyprctl monitors -j | jq -r '.[]|"\(.name) \(.width)x\(.height)@\(.refreshRate|floor) scale \(.scale)"' | paste -sd';')"
    echo "native: $($NATIVE --help 2>&1 | head -1) $(git -C "$(dirname "$NATIVE")/../.." log -1 --format=%h 2>/dev/null)"
    echo "ours: $(PATH=$OURS_BIN:$PATH omarchy-ttfx-layer --standalone --probe --capabilities 2>/dev/null) renderer=$(sha256sum "$(readlink -f "$OURS_BIN/omarchy-ttfx-layer")" | cut -c1-12) engine=$(sha256sum "$(readlink -f "$OURS_BIN/omarchy-ambient")" | cut -c1-12)"
    echo "text: $(sha256sum "$BRANDING" | cut -c1-12) seed=$SEED warm=$WARM secs=$secs effects=$EFFECTS gliff=$(pgrep -x gliff-server >/dev/null && echo running || echo no)"
  } | tee "$OUT/setup.txt" >&2
  log "cooling to a base temperature"
  local d=$((SECONDS + 600)); while ((SECONDS < d)) && (( $(temp) > 66000 )); do sleep 5; done
  sleep 30; BASE_T=$(temp); log "base temperature $((BASE_T / 1000)) C, fan $(fan)"
  for arm in native ours; do dismiss "$arm" || exit 1; cool; done
  run idle idle "$secs"
  for e in $EFFECTS; do
    for arm in native ours ours60 ours60 ours native; do run "$arm" "$e" "$secs"; cool; done
  done
  for e in $EFFECTS; do for arm in native ours ours60; do fps "$arm" "$e"; cool; done; done
  report
}

report() {
  python3 - "$OUT" <<'PY'
import json, statistics, sys
from pathlib import Path
out = Path(sys.argv[1])
runs = [json.loads(l) for l in (out / "runs.jsonl").read_text().splitlines() if l.strip()]
fps = {}
if (out / "fps.jsonl").exists():
    for l in (out / "fps.jsonl").read_text().splitlines():
        if l.strip():
            d = json.loads(l); fps[(d["arm"], d["effect"])] = d["fps"]
effects = []
for r in runs:
    if r["effect"] not in effects and r["effect"] != "idle": effects.append(r["effect"])
arms = ["idle", "native", "ours", "ours60"]
keys = [("cpu", "CPU %"), ("hypr", "Hyprland %"), ("gpu", "GPU busy %"), ("temp_end", "temp end C"), ("fan", "fan rpm"), ("power_w", "system W"), ("soc_w", "heatpipe W"), ("rss_peak_mb", "RSS MB")]
keys = [k for k in keys if any(r.get(k[0], 0) not in (0, -1, None) for r in runs)]
lines = []
for e in effects:
    lines.append(f"\n## {e}\n")
    lines.append("| | " + " | ".join(a for a in arms) + " |")
    lines.append("|---|" + "---|" * len(arms))
    for k, label in keys:
        row = []
        for a in arms:
            v = [r[k] for r in runs if r["arm"] == a and (r["effect"] == e or a == "idle") and r["ok"]]
            row.append(f"{statistics.median(v):.1f}" if v else "-")
        lines.append(f"| {label} | " + " | ".join(row) + " |")
    row = [f"{fps[(a, e)]:.1f}" if (a, e) in fps else "-" for a in arms]
    lines.append("| presented fps | " + " | ".join(row) + " |")
    row = []
    for a in arms:
        v = [r.get("gov_fps", 0) for r in runs if r["arm"] == a and r["effect"] == e and r["ok"] and r.get("gov_fps")]
        row.append(f"{statistics.median(v):.0f}" if v else "-")
    lines.append("| governor fps | " + " | ".join(row) + " |")
    row = [str(sum(1 for r in runs if r["arm"] == a and r["effect"] == e and r["ok"])) for a in arms]
    lines.append("| valid runs | " + " | ".join(row) + " |")
bad = [r for r in runs if not r["ok"]]
if bad: lines.append(f"\n{len(bad)} run(s) ended early (input or crash): " + ", ".join(f"{r['arm']}/{r['effect']}" for r in bad))
text = "\n".join(lines)
(out / "report.md").write_text(text + "\n")
print(text)
PY
}

case ${1:-} in
  series) series "${2:-60}" ;;
  run) run "$2" "$3" "${4:-60}" ;;
  fps) fps "$2" "$3" "${4:-20}" ;;
  dismiss) dismiss "$2" ;;
  report) report ;;
  *) sed -n '2,13p' "$0"; exit 1 ;;
esac
