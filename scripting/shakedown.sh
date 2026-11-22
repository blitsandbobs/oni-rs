#!/bin/zsh
# Runs every level's own scripts headless, from its start and from save points
# 1–4, with the player idle, 8 at a time; prints one line per run — script
# errors, crashes, where the player ended — and the commands skipped. Run from
# oni-rs; it builds the `checked` profile first (release speed, overflow
# checks and debug assertions on).
#
#   scripting/shakedown.sh [ticks]      (default 3600: a minute each)
#   scripting/shakedown.sh --tour       (from each level's start, the player
#                                        put in every trigger volume and at
#                                        every console, twice over)
#   options: --saves (a tour from save points 1-4 too), --faithful
#
# A run's full report: $TMPDIR/oni-shakedown-{idle,tour}/<level>_<save>.txt.
G=../generated/oni
if [ "$1" = "--one" ]; then
  l=${2%%:*}; s=${2##*:}; out=$3/${l}_$s.txt
  tour=; if [ "$5" = tour ]; then
    tour=$3/tour_$l.lua; uv run -q python -I scripting/tour_stops.py $G/levels/$l $tour
  fi
  SHAKE_TOUR=$tour SHAKE_TICKS=$4 SHAKE_SAVE=${s:#0} ./target/checked/oni $G/levels/$l/environment.glb \
    --character $G/common/characters/konoko_body_high/model.glb \
    --anim $G/common/animations/KONOKOrun_lt.glb --char-pos 0,0,0 ${=6} \
    --script scripting/shakedown.lua > $out 2>&1
  echo "exit $?" >> $out
  exit 0
fi
cargo build -q --profile checked -p oni || exit 1
mode=idle; saves="0 1 2 3 4"; extra=
while [ $# -gt 0 ]; do
  case $1 in
    --tour) mode=tour; saves=0 ;;
    --saves) saves="0 1 2 3 4" ;;
    --faithful) extra=--faithful ;;
    *) break ;;
  esac
  shift
done
ticks=${1:-3600}
dir=${TMPDIR:-/tmp}/oni-shakedown-$mode${extra:+-faithful}
rm -rf $dir; mkdir -p $dir
for l in $(ls $G/levels); do for s in ${=saves}; do echo $l:$s; done; done |
  xargs -P 8 -I{} "$0" --one {} $dir $ticks $mode "$extra"
bad=0
for f in $dir/*.txt; do
  n=${f:t:r}; e=$(grep -c '^SCRIPT-ERROR\|^STEP-ERROR\|^BEGIN-ERROR\|^TOUR-ERROR\|^LOST\|^READY never\|panicked' $f)
  done_=$(grep -c '^SHAKE-DONE' $f); x=$(grep '^exit' $f | cut -c6-)
  p=$(grep '^PLAYER' $f | cut -c8-); m=$(grep '^START' $f | grep -o 'moved [0-9]*, fell [-0-9]*')
  if [ "$e" != 0 ] || [ "$done_" != 1 ] || [ "$x" != 0 ]; then st="FAIL"; bad=1; else st="ok"; fi
  printf "%-4s %-6s %-26s %s\n" $st $n "$p" "$m"
  grep '^SCRIPT-ERROR\|^STEP-ERROR\|^BEGIN-ERROR\|^TOUR-ERROR\|^LOST\|^READY never\|panicked' $f | head -6 | sed 's/^/       /'
done
echo "skipped commands (runs reaching them):"
cat $dir/*.txt | grep '^STUBBED' | cut -c9- | tr ' ' '\n' | grep -v '^$' | sort | uniq -c | sort -rn | awk '{printf "  %s (%s)", $2, $1} END {print ""}'
exit $bad
