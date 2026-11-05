#!/bin/bash
# R-03: one red, one green and one blue shot, each lit by the Scanlight and fired over USB by gphoto2.
# macOS. Needs the probe built (cargo build --release in this folder), gphoto2, and the camera free
# (see docs/research/scanlight-capture.md: kill ptpcamerad, in every logged-in account, first).
#
#   ./triplet.sh <serial port> [level 64] [settle seconds 1.5] [output folder triplet-out]
#   ./triplet.sh /dev/cu.usbmodem1101 64 1.5
#
# The light is lit by `scanlight-probe hold` in the background; the shot is taken after the settle
# time; then the hold is stopped (SIGINT: it switches the light off before it ends). The light is off
# at the end, also if a shot fails or the script is stopped.
set -u

PORT="${1:?usage: triplet.sh <serial port> [level] [settle seconds] [output folder]}"
LEVEL="${2:-64}"
SETTLE="${3:-1.5}"
OUT="${4:-triplet-out}"
HERE="$(cd "$(dirname "$0")" && pwd)"
PROBE="$HERE/target/release/scanlight-probe"

[ -x "$PROBE" ] || { echo "build the probe first: cargo build --release" >&2; exit 1; }
mkdir -p "$OUT"

HOLD=""
off() {
  if [ -n "$HOLD" ] && kill -0 "$HOLD" 2>/dev/null; then
    kill -INT "$HOLD" 2>/dev/null
    wait "$HOLD" 2>/dev/null
  fi
  HOLD=""
}
trap off EXIT INT TERM

for colour in red green blue; do
  "$PROBE" hold --port "$PORT" --colour "$colour" --level "$LEVEL" --seconds 30 &
  HOLD=$!
  sleep "$SETTLE"
  start=$(date +%s)
  if gphoto2 --capture-image-and-download --filename "$OUT/$colour.nef" --force-overwrite; then
    echo "$colour: shot taken, $(( $(date +%s) - start )) s from trigger to file"
  else
    echo "$colour: the shot failed" >&2
    off
    exit 1
  fi
  off
  sleep 1
done

echo "done: $OUT/red.nef $OUT/green.nef $OUT/blue.nef (light off)"
