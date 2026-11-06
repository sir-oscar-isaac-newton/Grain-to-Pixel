#!/bin/bash
# R-03: can libgphoto2 be shipped inside an app? Builds a throwaway copy of the probe with every
# non-system library it needs, the two drivers the D600 uses (the camera driver ptp2 and the USB port
# driver), install names rewritten so nothing points at Homebrew, and ad-hoc signed; then takes a
# picture from the copy. macOS only, with the camera free and awake.
#
#   cargo build --release
#   ./bundle-test.sh [probe binary, default target/release/gphoto-probe] [output folder, default /tmp/gp-bundle]
#
# Prints the sizes and every library of the copy, and fails if a path into Homebrew is left.
set -euo pipefail

BIN="${1:-target/release/gphoto-probe}"
OUT="${2:-/tmp/gp-bundle}"
PREFIX="$(brew --prefix libgphoto2)"

[ -f "$BIN" ] || { echo "build the probe first: cargo build --release" >&2; exit 1; }

rm -rf "$OUT"
mkdir -p "$OUT/bin" "$OUT/lib/camlibs" "$OUT/lib/iolibs"
cp "$BIN" "$OUT/bin/gphoto-probe"
chmod u+w "$OUT/bin/gphoto-probe"

# The libraries of the copy, one file name per line (bash 3.2 has no maps).
DONE="$(mktemp -t gp-done)"
: > "$DONE"

is_system() { case "$1" in /usr/lib/*|/System/*) return 0 ;; *) return 1 ;; esac; }

# The libraries a file loads, as written in it (without its own install name).
deps_of() {
  local self
  self="$(basename "$1")"
  otool -L "$1" | tail -n +2 | awk '{print $1}' | while read -r dep; do
    [ "$(basename "$dep")" = "$self" ] || echo "$dep"
  done
}

# Where a library at $1 finds its neighbours in lib/: the prefix to put before the file name.
prefix_for() {
  case "$1" in
    "$OUT/bin/"*) echo "@executable_path/../lib" ;;
    "$OUT/lib/camlibs/"*|"$OUT/lib/iolibs/"*) echo "@loader_path/.." ;;
    *) echo "@loader_path" ;;
  esac
}

# Copies what $1 needs into lib/, rewrites $1 to use the copies, and does the same for each copy.
process() {
  local file="$1" dep base
  for dep in $(deps_of "$file"); do
    is_system "$dep" && continue
    case "$dep" in
      /*) ;;
      *) echo "warning: $file loads $dep, a path this script does not resolve" >&2; continue ;;
    esac
    base="$(basename "$dep")"
    if ! grep -qx "$base" "$DONE"; then
      echo "$base" >> "$DONE"
      cp -L "$dep" "$OUT/lib/$base"
      chmod u+w "$OUT/lib/$base"
      install_name_tool -id "@rpath/$base" "$OUT/lib/$base" 2>/dev/null
      process "$OUT/lib/$base"
    fi
    install_name_tool -change "$dep" "$(prefix_for "$file")/$base" "$file"
  done
}

# The two drivers the D600 needs.
CAM="$(ls "$PREFIX"/lib/libgphoto2/*/ptp2.so | head -n 1)"
IO="$(ls "$(brew --prefix libgphoto2)"/lib/libgphoto2_port/*/usb1.so 2>/dev/null | head -n 1 || true)"
[ -n "$IO" ] || IO="$(ls "$(brew --prefix libgphoto2_port 2>/dev/null)"/lib/libgphoto2_port/*/usb1.so | head -n 1)"
cp "$CAM" "$OUT/lib/camlibs/ptp2.so"
cp "$IO" "$OUT/lib/iolibs/usb1.so"
chmod u+w "$OUT/lib/camlibs/ptp2.so" "$OUT/lib/iolibs/usb1.so"

process "$OUT/bin/gphoto-probe"
process "$OUT/lib/camlibs/ptp2.so"
process "$OUT/lib/iolibs/usb1.so"

# Ad-hoc signing: install_name_tool invalidates signatures, and Apple Silicon refuses unsigned code.
find "$OUT" -type f \( -name '*.dylib' -o -name '*.so' -o -path '*/bin/*' \) -exec codesign --force --sign - {} \; 2>/dev/null

echo "--- the copy"
du -sh "$OUT"
du -sh "$OUT/lib/camlibs" "$OUT/lib/iolibs"
ls "$OUT/lib"
echo "--- libraries still pointing outside the copy (should be system ones only):"
LEFT=0
for f in "$OUT/bin/gphoto-probe" "$OUT"/lib/*.dylib "$OUT"/lib/camlibs/*.so "$OUT"/lib/iolibs/*.so; do
  if otool -L "$f" | grep -E "/opt/homebrew|/usr/local" ; then
    echo "  ^ in $f"
    LEFT=1
  fi
done
[ "$LEFT" -eq 0 ] && echo "none"

echo "--- a picture from the copy"
CAMLIBS="$OUT/lib/camlibs" IOLIBS="$OUT/lib/iolibs" "$OUT/bin/gphoto-probe" capture "$OUT/test.nef"
rm -f "$DONE"
[ "$LEFT" -eq 0 ] || { echo "the copy still points into Homebrew" >&2; exit 1; }
echo "ok: the copy works on its own"
