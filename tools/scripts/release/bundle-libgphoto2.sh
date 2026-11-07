#!/bin/bash
# Puts libgphoto2 inside the macOS app (H-09, ADR 0004 amendments 7 and 8; the logic of the R-03
# spike's tools/spikes/gphoto-probe/bundle-test.sh, run on the real app binary). macOS only.
#
#   bundle-libgphoto2.sh bundle [binary]   copies the libraries the binary needs, and the two drivers
#                                          ptp2 and usb1, to apps/desktop/src-tauri/gphoto-libs,
#                                          rewrites the binary and the copies to the bundle's layout
#                                          and signs them ad hoc. Tauri's beforeBundleCommand runs it
#                                          (src-tauri/tauri.macos.conf.json), after the binary is
#                                          built and before it is bundled; the folder is then copied
#                                          to Contents/Frameworks/gphoto by the same file's
#                                          bundle.macOS.files.
#   bundle-libgphoto2.sh verify <App.app>  checks a built app: all seven libraries and both drivers
#                                          are in it, nothing points into Homebrew, signatures hold.
#
# Layout in the app (the app sets CAMLIBS and IOLIBS to the last two at start, see
# crates/gtp-capture/src/gphoto/paths.rs):
#   Contents/MacOS/<binary>                      loads @executable_path/../Frameworks/gphoto/*.dylib
#   Contents/Frameworks/gphoto/*.dylib           the seven libraries, loading each other @loader_path
#   Contents/Frameworks/gphoto/camlibs/ptp2.so   camera driver of every PTP camera, the D600 included
#   Contents/Frameworks/gphoto/iolibs/usb1.so    USB port driver
#
# A binary built without the `tether` feature does not load libgphoto2: the folder is then left
# empty and nothing is bundled.
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../../.." && pwd)"
OUT="$ROOT/apps/desktop/src-tauri/gphoto-libs"
NAME="grain-to-pixel-desktop"
APP_PREFIX="@executable_path/../Frameworks/gphoto"
# The libraries the app needs (the file name starts with one of these). A Homebrew update that adds
# one makes `bundle` print it and `verify` fail until it is added to docs/third-party-notices.md.
EXPECTED="libgphoto2.6 libgphoto2_port.12 libltdl.7 libexif.12 libusb-1.0.0 libjpeg.8 libintl.8"

[ "$(uname)" = "Darwin" ] || { echo "bundle-libgphoto2: macOS only" >&2; exit 1; }

is_system() { case "$1" in /usr/lib/*|/System/*) return 0 ;; *) return 1 ;; esac; }

# The libraries a file loads, as written in it (without its own install name).
deps_of() {
  local self
  self="$(basename "$1")"
  otool -L "$1" | tail -n +2 | awk '{print $1}' | while read -r dep; do
    [ "$(basename "$dep")" = "$self" ] || echo "$dep"
  done
}

# Where a file at $1 finds the libraries: the prefix to put before the library's file name.
prefix_for() {
  case "$1" in
    "$BIN") echo "$APP_PREFIX" ;;
    "$OUT/camlibs/"*|"$OUT/iolibs/"*) echo "@loader_path/.." ;;
    *) echo "@loader_path" ;;
  esac
}

# Copies what $1 needs into $OUT, rewrites $1 to use the copies, and does the same for each copy.
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
      cp -L "$dep" "$OUT/$base"
      chmod u+w "$OUT/$base"
      install_name_tool -id "@rpath/$base" "$OUT/$base" 2>/dev/null
      process "$OUT/$base"
    fi
    install_name_tool -change "$dep" "$(prefix_for "$file")/$base" "$file"
  done
}

find_binary() {
  local profile=release triple="${TAURI_ENV_TARGET_TRIPLE:-}" dir="${CARGO_TARGET_DIR:-$ROOT/target}" candidate
  [ "${TAURI_ENV_DEBUG:-false}" = "true" ] && profile=debug
  for candidate in "$dir/$triple/$profile/$NAME" "$dir/$profile/$NAME"; do
    if [ -f "$candidate" ]; then echo "$candidate"; return 0; fi
  done
  echo "bundle-libgphoto2: no built $NAME under $dir (profile $profile); pass the binary" >&2
  return 1
}

bundle() {
  BIN="${1:-$(find_binary)}"
  [ -f "$BIN" ] || { echo "bundle-libgphoto2: no such binary: $BIN" >&2; exit 1; }
  BIN="$(cd "$(dirname "$BIN")" && pwd)/$(basename "$BIN")"

  if ! otool -L "$BIN" | grep -q "libgphoto2"; then
    rm -rf "$OUT"
    mkdir -p "$OUT"
    echo "bundle-libgphoto2: $BIN does not load libgphoto2 (built without the tether feature): nothing to bundle"
    return 0
  fi
  if otool -L "$BIN" | grep -q "$APP_PREFIX/libgphoto2"; then
    if ls "$OUT"/libgphoto2*.dylib >/dev/null 2>&1; then
      echo "bundle-libgphoto2: $BIN was already rewritten and $OUT is there: nothing to do"
      return 0
    fi
    echo "bundle-libgphoto2: $BIN was rewritten but $OUT is gone; build the app again" >&2
    return 1
  fi

  PREFIX="$(brew --prefix libgphoto2)"
  rm -rf "$OUT"
  mkdir -p "$OUT/camlibs" "$OUT/iolibs"
  DONE="$(mktemp -t gp-done)"
  : > "$DONE"
  chmod u+w "$BIN"

  # The two drivers the D600 needs.
  local cam io
  cam="$(ls "$PREFIX"/lib/libgphoto2/*/ptp2.so | head -n 1)"
  io="$(ls "$PREFIX"/lib/libgphoto2_port/*/usb1.so 2>/dev/null | head -n 1 || true)"
  [ -n "$io" ] || io="$(ls "$(brew --prefix libgphoto2_port 2>/dev/null)"/lib/libgphoto2_port/*/usb1.so | head -n 1)"
  cp "$cam" "$OUT/camlibs/ptp2.so"
  cp "$io" "$OUT/iolibs/usb1.so"
  chmod u+w "$OUT/camlibs/ptp2.so" "$OUT/iolibs/usb1.so"

  process "$BIN"
  process "$OUT/camlibs/ptp2.so"
  process "$OUT/iolibs/usb1.so"
  rm -f "$DONE"

  # Rewriting breaks a signature and Apple Silicon refuses unsigned code: sign again, ad hoc
  # (Tauri signs the app itself after this, with the same identity).
  find "$OUT" -type f \( -name '*.dylib' -o -name '*.so' \) -exec codesign --force --sign - {} \;
  codesign --force --sign - "$BIN"

  echo "bundle-libgphoto2: libraries in $OUT:"
  ls "$OUT"
  du -sh "$OUT"
  check_expected "$OUT"
  check_homebrew_left "$BIN" "$OUT"/*.dylib "$OUT"/camlibs/*.so "$OUT"/iolibs/*.so
}

check_expected() {
  local dir="$1" lib missing=0
  for lib in $EXPECTED; do
    ls "$dir/$lib"*.dylib >/dev/null 2>&1 || { echo "bundle-libgphoto2: $lib is not in $dir" >&2; missing=1; }
  done
  # More libraries than the seven would need a notice and a source offer.
  local count
  count="$(ls "$dir"/*.dylib | wc -l | tr -d ' ')"
  if [ "$count" -ne 7 ]; then
    echo "bundle-libgphoto2: $count libraries found, the notices list seven (docs/third-party-notices.md, tools/scripts/release/gphoto-source.mjs)" >&2
    missing=1
  fi
  return "$missing"
}

check_homebrew_left() {
  local left=0 file
  for file in "$@"; do
    if otool -L "$file" | tail -n +2 | grep -E "/opt/homebrew|/usr/local"; then
      echo "  ^ in $file" >&2
      left=1
    fi
  done
  [ "$left" -eq 0 ] || { echo "bundle-libgphoto2: a path into Homebrew is left" >&2; return 1; }
}

verify() {
  local app="${1:-}" gp bin failed=0
  [ -d "$app/Contents" ] || { echo "usage: bundle-libgphoto2.sh verify <App.app>" >&2; exit 2; }
  gp="$app/Contents/Frameworks/gphoto"
  bin="$app/Contents/MacOS/$NAME"
  [ -f "$bin" ] || { echo "bundle-libgphoto2: no $bin" >&2; exit 1; }
  [ -f "$gp/camlibs/ptp2.so" ] && [ -f "$gp/iolibs/usb1.so" ] || { echo "bundle-libgphoto2: a driver is missing in $gp" >&2; failed=1; }
  check_expected "$gp" || failed=1
  check_homebrew_left "$bin" "$gp"/*.dylib "$gp"/camlibs/*.so "$gp"/iolibs/*.so || failed=1
  for f in "$gp"/*.dylib "$gp"/camlibs/*.so "$gp"/iolibs/*.so; do
    codesign --verify --strict "$f" 2>/dev/null || { echo "bundle-libgphoto2: bad signature: $f" >&2; failed=1; }
  done
  codesign --verify --deep --strict "$app" || { echo "bundle-libgphoto2: the app's signature does not verify" >&2; failed=1; }
  # A valid signature is not enough: under the hardened runtime, dyld refuses libraries of another
  # signer (the ad hoc ones here) unless library validation is off, and the app aborts at launch.
  codesign -d --entitlements - "$bin" 2>/dev/null | grep -q "com.apple.security.cs.disable-library-validation" \
    || { echo "bundle-libgphoto2: $bin lacks the disable-library-validation entitlement, so it cannot load the libraries (apps/desktop/src-tauri/entitlements.plist)" >&2; failed=1; }
  du -sh "$gp"
  [ "$failed" -eq 0 ] && echo "bundle-libgphoto2: $app carries libgphoto2 and nothing points into Homebrew"
  return "$failed"
}

case "${1:-}" in
  bundle) shift; bundle "$@" ;;
  verify) shift; verify "$@" ;;
  *) echo "usage: bundle-libgphoto2.sh bundle [binary] | verify <App.app>" >&2; exit 2 ;;
esac
