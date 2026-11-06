#!/bin/bash
# R-03: one red, one green and one blue shot, each lit by the Scanlight and fired over USB by gphoto2.
# macOS. Needs the probe built (cargo build --release in this folder), gphoto2, and the camera free
# (see docs/research/scanlight-capture.md: kill ptpcamerad, in every logged-in account, first).
#
#   ./triplet.sh <serial port> [level 64] [settle seconds 1.5] [output folder triplet-out]
#   ./triplet.sh /dev/cu.usbmodem101 64 1.5           (ls /dev/cu.usbmodem* shows the port)
#   ./triplet.sh /dev/cu.usbmodem101 64,32,16 1.5     one level per colour: red, green, blue
#
# The app groups files into a triplet only if its shots are at most 5 s apart (MAX_SHOT_GAP_MS), so
# keep the settle time short (0.5 s or so) and the pause between colours, PAUSE, short (0.2 s).
#
# The light is lit by `scanlight-probe hold` in the background; a shot is taken only after the probe
# has said the light is on and the settle time has passed; then the hold is stopped (SIGINT: it
# switches the light off before it ends; if the signal is ignored the hold ends by itself after 30 s).
# The light is off at the end, also if a shot fails or the script is stopped.
set -u

PORT="${1:?usage: triplet.sh <serial port> [level or red,green,blue levels] [settle seconds] [output folder]}"
LEVELS="${2:-64}"
SETTLE="${3:-1.5}"
OUT="${4:-triplet-out}"
HERE="$(cd "$(dirname "$0")" && pwd)"
PROBE="$HERE/target/release/scanlight-probe"

[ -x "$PROBE" ] || { echo "build the probe first: cargo build --release" >&2; exit 1; }
[ -e "$PORT" ] || { echo "$PORT does not exist: is the Scanlight plugged into this Mac? (ls /dev/cu.usbmodem*)" >&2; exit 1; }
# One level for all three colours, or three: red,green,blue.
IFS=, read -r LEVEL_RED LEVEL_GREEN LEVEL_BLUE <<< "$LEVELS"
LEVEL_GREEN="${LEVEL_GREEN:-$LEVEL_RED}"
LEVEL_BLUE="${LEVEL_BLUE:-$LEVEL_RED}"
mkdir -p "$OUT"
LOG="$(mktemp -t triplet-hold)"

HOLD=""
off() {
  if [ -n "$HOLD" ] && kill -0 "$HOLD" 2>/dev/null; then
    kill -INT "$HOLD" 2>/dev/null
    wait "$HOLD" 2>/dev/null
  fi
  HOLD=""
}
trap 'off; rm -f "$LOG"' EXIT INT TERM

for colour in red green blue; do
  case "$colour" in red) LEVEL="$LEVEL_RED" ;; green) LEVEL="$LEVEL_GREEN" ;; blue) LEVEL="$LEVEL_BLUE" ;; esac
  : > "$LOG"
  "$PROBE" hold --port "$PORT" --colour "$colour" --level "$LEVEL" --seconds 30 > "$LOG" 2>&1 &
  HOLD=$!

  # Wait up to 5 s for the probe to say the light is on.
  for _ in $(seq 1 50); do
    grep -q "LIGHT ON" "$LOG" && break
    kill -0 "$HOLD" 2>/dev/null || break
    sleep 0.1
  done
  if ! grep -q "LIGHT ON" "$LOG"; then
    echo "$colour: the light did not come on, so no shot was taken:" >&2
    cat "$LOG" >&2
    off
    exit 1
  fi

  sleep "$SETTLE"
  start=$(date +%s)
  if gphoto2 --capture-image-and-download --filename "$OUT/$colour.nef" --force-overwrite; then
    echo "$colour: level $LEVEL, shot taken, $(( $(date +%s) - start )) s from trigger to file"
  else
    echo "$colour: the shot failed" >&2
    off
    exit 1
  fi
  off
  sleep "${PAUSE:-0.2}"
done

echo "done: $OUT/red.nef $OUT/green.nef $OUT/blue.nef (light off)"
