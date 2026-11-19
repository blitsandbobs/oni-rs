#!/bin/zsh
# Runs every scenario; prints ok / FAIL per file and exits non-zero if any
# failed. Run from oni-rs after `cargo build --release -p oni-view`; extra
# flags (e.g. --faithful) are passed on. Each scenario's level and start come
# from the command line in its header comment.
G=../generated/oni
fail=0
for f in scenarios/*.lua; do
  pos=$(grep -o -m1 -- '--char-pos [-0-9.,]*' $f | awk '{print $2}'); pos=${pos:-200,8,300}
  lvl=$(grep -o -m1 'levels/[0-9][0-9]' $f); lvl=${lvl:-levels/01}
  out=$(./target/release/oni-view $G/$lvl/environment.glb --character $G/common/characters/konoko_body_high/model.glb --anim $G/common/animations/KONOKOrun_lt.glb --char-pos $pos --script $f "$@" 2>&1)
  if [ $? -ne 0 ]; then echo "FAIL $f"; echo "$out" | tail -5; fail=1; else echo "ok   $f"; fi
done
exit $fail
