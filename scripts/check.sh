#!/bin/zsh
# The commit gate, run in parallel: checks the formatting (rustfmt's default
# style), builds the release viewer, checks the docs are named and lowercase, then runs
# clippy and the tests (one after the other: cargo holds one lock on the
# build directory) alongside both scenario passes (fixed and --faithful).
# Prints each part's verdict, its failures, and how long it took; exits
# non-zero if any part failed.
#
#   scripts/check.sh            (from anywhere)
cd "${0:A:h}/.."
log=$(mktemp -d)
start=$SECONDS

if ! cargo fmt --all --check > $log/fmt.txt 2>&1; then
  echo "FAIL fmt (run: cargo fmt --all)"; grep "^Diff in" $log/fmt.txt | head -20
  rm -rf $log; exit 1
fi
echo "ok   fmt"

if ! cargo build --release -p oni > $log/build.txt 2>&1; then
  echo "FAIL build"; grep -E "^error" -A9 $log/build.txt | head -40
  rm -rf $log; exit 1
fi

# Docs: every `docs/…`, `scenarios/…`, `scripting/…` path a source or doc
# names must exist, and the docs' names are lowercase. (publish.sh names
# the docs since deleted on purpose: it leaves them out of the public
# history.)
missing=$(git ls-files -co --exclude-standard -- crates scenarios scripting scripts docs \
    README.md CLAUDE.md | grep -vE '^(target|generated)/|^scripts/publish\.sh$' \
  | xargs grep -ohE '\b(docs|scenarios|scripting)/[A-Za-z0-9_./-]+\.md\b' 2>/dev/null \
  | sort -u | while read f; do [ -e "$f" ] || echo "$f"; done)
upper=$(ls docs | grep '[A-Z]')
if [ -n "$missing$upper" ]; then
  echo "FAIL docs"; [ -n "$missing" ] && echo "named but missing: ${missing//$'\n'/ }"
  [ -n "$upper" ] && echo "not lowercase: ${upper//$'\n'/ }"
  rm -rf $log; exit 1
fi
echo "ok   docs"

# One part: its command's output to a file, its exit code and time after it.
part() {
  local name=$1; shift
  local t=$SECONDS
  "$@" > $log/$name.txt 2>&1
  echo "$? $((SECONDS - t))" > $log/$name.rc
}
( part clippy cargo clippy --workspace --all-targets -- -D warnings
  part test cargo test --workspace ) &
part fixed scenarios/run_all.sh &
part faithful scenarios/run_all.sh --faithful &
wait

fail=0
for name in clippy test fixed faithful; do
  read rc secs < $log/$name.rc
  if [ $rc -eq 0 ]; then
    printf "ok   %-8s %4d s\n" $name $secs
  else
    fail=1
    printf "FAIL %-8s %4d s\n" $name $secs
    case $name in
      clippy) grep -E "^(warning|error)" -A9 $log/clippy.txt | grep -v "block v0.1.6\|future-incompat" | head -40 ;;
      test)   grep -E "FAILED|panicked|^error" -A4 $log/test.txt | head -40 ;;
      *)      grep -v "^ok" $log/$name.txt | head -40 ;;
    esac
  fi
done
echo "all checks: $((SECONDS - start)) s"
rm -rf $log
exit $fail
