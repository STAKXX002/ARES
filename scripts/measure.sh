#!/usr/bin/env bash
# Measure ARES wall time and peak memory on a synthetic set of repositories.
#
# Usage: scripts/measure.sh [NUM_REPOS]   (default 100)
#
# Creates NUM_REPOS small git repositories in a temp directory (every fifth
# one dirty), runs `ares repos` on them, reports peak resident memory and
# elapsed time, then removes the temp directory. Read the numbers as a
# baseline for your machine, not as a guarantee.
set -euo pipefail

N="${1:-100}"
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cargo build --release --manifest-path "$ROOT/Cargo.toml" >/dev/null 2>&1
BIN="$ROOT/target/release/ares"

TMP="$(mktemp -d)"
trap 'rm -rf "$TMP"' EXIT

for i in $(seq 1 "$N"); do
  d="$TMP/repo$i"
  mkdir "$d"
  git -C "$d" init -q -b main
  echo "$i" > "$d/f.txt"
  git -C "$d" add f.txt
  git -C "$d" -c user.name=m -c user.email=m@m commit -q -m init
  if [ $((i % 5)) -eq 0 ]; then echo dirty > "$d/untracked.txt"; fi
done

echo "repositories: $N"
echo "binary size:  $(stat -c %s "$BIN") bytes"
if [ -x /usr/bin/time ]; then
  /usr/bin/time -f "wall time:    %e s\npeak RSS:     %M KB" "$BIN" repos "$TMP" >/dev/null
else
  echo "/usr/bin/time not found; install the 'time' package for RSS numbers"
  time "$BIN" repos "$TMP" >/dev/null
fi
