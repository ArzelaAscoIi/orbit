#!/bin/sh
# Runs Orbit's command line for the herdr plugin: ORBIT_BIN when set, else
# the installed app, else `orbit` on PATH.
orbit="${ORBIT_BIN:-}"
[ -z "$orbit" ] && [ -x /Applications/Orbit.app/Contents/MacOS/orbit ] && orbit=/Applications/Orbit.app/Contents/MacOS/orbit
[ -z "$orbit" ] && orbit="$(command -v orbit)"
if [ -z "$orbit" ]; then
  echo "Orbit is not installed (no /Applications/Orbit.app, no orbit on PATH)." >&2
  exit 1
fi
case "$1" in
  pane)
    # Opens the plugin pane through herdr itself.
    exec "${HERDR_BIN_PATH:-herdr}" plugin pane open --plugin orbit --entrypoint "$2" --focus
    ;;
  open-clicked)
    # A link clicked in a herdr pane: Orbit opens it, then comes to the front.
    "$orbit" open "${HERDR_PLUGIN_CLICKED_URL:-}" && open -a Orbit
    ;;
  *)
    exec "$orbit" "$@"
    ;;
esac
