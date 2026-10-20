#!/usr/bin/env bash
# The README's demo: plays a simulated user (src/app/review.rs) on Acme,
# the made-up company of demo mode (src/demo.rs), and turns the frames into
# docs/demo.gif. No account or key needed, nothing real is shown or sent.
#
#   scripts/demo-gif.sh [steps.json] [out-dir]
#
# Needs Pillow for the GIF (scripts/gif.py): `python3 -m pip install pillow`,
# or PYTHON=<a venv's python>. The window must stay in front while it plays.
set -euo pipefail
cd "$(dirname "$0")/.."
steps="${1:-tests/sim/demo.json}"
out="${2:-target/demo}"
cargo build --release --quiet
data=target/demo-data
# A run that lost the front (another Orbit came forward) stalls: try again.
for _try in 1 2 3; do
  rm -rf "$data" "$out" && mkdir -p "$data" "$out"
  echo '{"teams": ["WEB", "API"], "profile": "acme", "notifications": false}' > "$data/settings.json"
  ORBIT_DEMO=1 ORBIT_NO_KEYCHAIN=1 ORBIT_DATA="$data" \
    ORBIT_SIZE=${ORBIT_SIZE:-1440x900} ORBIT_SIMULATE="$steps" ORBIT_SIM_OUT="$out" \
    target/release/orbit &
  pid=$!
  for _ in $(seq 1 240); do
    [ -f "$out/done" ] && break
    kill -0 "$pid" 2>/dev/null || break
    osascript -e 'tell application "System Events" to set frontmost of (first process whose name is "orbit") to true' 2>/dev/null || true
    sleep 0.5
  done
  kill "$pid" 2>/dev/null || true
  wait "$pid" 2>/dev/null || true
  [ -f "$out/done" ] && break
done
for ppm in "$out"/*.ppm; do
  [ -f "$ppm" ] || continue
  sips -s format png "$ppm" --out "${ppm%.ppm}.png" >/dev/null && rm -f "$ppm"
done
cat "$out/steps.log"
if [ "$out" = target/demo ]; then
  "${PYTHON:-python3}" scripts/gif.py docs/demo.gif "$out"/*.png
fi
