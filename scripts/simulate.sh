#!/usr/bin/env bash
# Plays a simulated user's steps on the real board, read-only, and saves a
# frame per `shot` step (src/app/review.rs `simulate` lists the steps).
#
#   scripts/simulate.sh <steps.json> <out-dir> [--generic] [--email someone@company] [--teams WEB,API]
#
# Safe by construction: ORBIT_READ_ONLY=1 refuses every Linear mutation and
# GitHub merge and starts no Claude session or shell (src/main.rs), and the
# run uses a throwaway copy of the dev data, never your real data folder.
# --generic runs with no profile; --email is whose profile section applies.
set -euo pipefail
cd "$(dirname "$0")/.."
steps="$1"; out="$2"; shift 2
generic=""; email=""; teams=""
while [ $# -gt 0 ]; do
  case "$1" in
    --generic) generic=1 ;;
    --email) email="$2"; shift ;;
    --teams) teams="$2"; shift ;;
  esac
  shift
done

cargo build --release --quiet
data=target/sim-data
rm -rf "$data" "$out" && mkdir -p "$data" "$out"
# The board's state without the heavy parts (repos, the Laya model).
rsync -a --exclude work --exclude laya --exclude repos target/dev-data/ "$data/"
python3 - "$data/settings.json" "$generic" "$teams" <<'PY'
import json, os, sys
p, generic, teams = sys.argv[1:4]
s = json.load(open(p)) if os.path.exists(p) else {}
if generic:
    s["profile"] = "generic"
if teams:
    s["teams"] = teams.split(",")
json.dump(s, open(p, "w"))
PY

set -a; [ -f .env ] && source .env; set +a
export ORBIT_NO_KEYCHAIN=1 ORBIT_READ_ONLY=1 ORBIT_DATA="$data"
export ORBIT_SIMULATE="$steps" ORBIT_SIM_OUT="$out"
[ -n "$email" ] && export ORBIT_SIM_EMAIL="$email"
target/release/orbit &
pid=$!
# macOS stops drawing a covered window: keep it frontmost until done.
for _ in $(seq 1 600); do
  [ -f "$out/done" ] && break
  kill -0 "$pid" 2>/dev/null || break
  osascript -e 'tell application "System Events" to set frontmost of (first process whose name is "orbit") to true' 2>/dev/null || true
  sleep 0.5
done
kill "$pid" 2>/dev/null || true
wait "$pid" 2>/dev/null || true
for ppm in "$out"/*.ppm; do
  [ -f "$ppm" ] || continue
  sips -s format png "$ppm" --out "${ppm%.ppm}.png" >/dev/null && rm -f "$ppm"
done
cat "$out/steps.log" 2>/dev/null || echo "no steps.log: the run did not finish" >&2
ls "$out"
