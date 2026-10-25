#!/bin/bash
# Two rounds in mirrored order so thermal drift cancels out.
D=~/.cache/ttfx-pr61; OUT=$D/results.txt; SECS=${SECS:-120}
B=$D/base/target/release/ttfx; P=$D/pr/target/release/ttfx; O=$HOME/.cache/ttfx-gpu/bin/ttfx-bg-rs/target/release/ttfx-bg-rs
cool() { local d=$((SECONDS+180)); while ((SECONDS<d)) && (( $(cat /sys/class/thermal/thermal_zone2/temp) > ${BASE_T:-60000} + 4000 )); do sleep 5; done; sleep 10; }
run() { $D/bench.sh "$@" | tee -a $OUT; cool; }
echo "== $(date -Is) secs=$SECS" >> $OUT
sleep 30; BASE_T=$(cat /sys/class/thermal/thermal_zone2/temp)
run idle idle - - 60
for round in 1 2; do
  if [ $round = 1 ]; then order="A B C D E"; else order="E D C B A"; fi
  for v in $order; do
    case $v in
      A) run master-120 term $B 120 $SECS ;;
      B) run pr61-120 term $P 120 $SECS ;;
      C) run pr61-30 term $P 30 $SECS ;;
      D) run master-30 term $B 30 $SECS ;;
      E) run ours-30 ours $O 30 $SECS ;;
    esac
  done
done
echo "== done $(date -Is)" >> $OUT
