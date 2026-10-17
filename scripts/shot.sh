#!/usr/bin/env bash
# Screenshots one view of the app without anyone at the screen.
#
#   scripts/shot.sh <foryou|stories|intake|sessions|customers> [claude] [delay]
#
# Writes target/shots/<view>[-claude].png. Uses an isolated data dir, so it
# never touches your real sessions or caches. Set ORBIT_TEAMS (in
# .env) to skip the team picker. macOS stops drawing an
# occluded window, so the script keeps the window frontmost until the
# frame is saved; expect it to flash up for a few seconds.
set -euo pipefail
cd "$(dirname "$0")/.."

view="${1:-foryou}"
tab="${2:-}"
delay="${3:-6}"
name="$view${tab:+-$tab}"
mkdir -p target/shots
ppm="target/shots/$name.ppm"
png="target/shots/$name.png"
rm -f "$ppm"

cargo build --release --quiet
ORBIT_DATA="${ORBIT_DATA:-target/dev-data}" \
ORBIT_VIEW="$view" \
ORBIT_TAB="$tab" \
ORBIT_SCREENSHOT="$ppm" \
ORBIT_SCREENSHOT_DELAY="$delay" \
  target/release/orbit &
pid=$!

# Keep the window frontmost until the frame lands.
for _ in $(seq 1 $((delay * 2 + 20))); do
  [ -f "$ppm" ] && break
  osascript -e 'tell application "System Events" to set frontmost of (first process whose name is "orbit") to true' 2>/dev/null || true
  sleep 0.5
done
wait "$pid" 2>/dev/null || true

[ -f "$ppm" ] || { echo "no frame saved (window never drew?)" >&2; exit 1; }
sips -s format png "$ppm" --out "$png" >/dev/null
rm -f "$ppm"
echo "$png"
