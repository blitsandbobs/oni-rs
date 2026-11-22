#!/bin/zsh
# Runs every scenario, 8 at a time (-j N to change); prints ok / FAIL per
# file, in file order, and exits non-zero if any failed. Run from oni-rs after
# `cargo build --release -p oni`; extra flags (e.g. --faithful) are passed
# on. Each scenario's level and start come from the command line in its header
# comment.
#
#   scenarios/run_all.sh [-j N] [flags…]
G=../generated/oni

# (Internal: one scenario into its own report file.)
if [ "$1" = "--one" ]; then
  f=$2; report="$3/${f:t}.txt"; shift 3
  pos=$(grep -o -m1 -- '--char-pos [-0-9.,]*' $f | awk '{print $2}'); pos=${pos:-200,8,300}
  lvl=$(grep -o -m1 'levels/[0-9][0-9]' $f); lvl=${lvl:-levels/01}
  out=$(./target/release/oni $G/$lvl/environment.glb --character $G/common/characters/konoko_body_high/model.glb --anim $G/common/animations/KONOKOrun_lt.glb --char-pos $pos --script $f "$@" 2>&1)
  if [ $? -ne 0 ]; then { echo "FAIL $f"; echo "$out" | tail -5; } > $report; exit 1; fi
  echo "ok   $f" > $report
  exit 0
fi

jobs=8
if [ "$1" = "-j" ]; then jobs=$2; shift 2; fi
dir=$(mktemp -d)
# (The player's saved game and options left alone: an empty save folder.)
export ONI_SAVE_DIR=$dir/save
for f in scenarios/*.lua; do echo $f; done |
  xargs -P $jobs -I{} "$0" --one {} "$dir" "$@"
fail=0
for f in scenarios/*.lua; do
  r="$dir/${f:t}.txt"
  cat $r
  grep -q '^FAIL' $r && fail=1
done
rm -rf $dir
exit $fail
