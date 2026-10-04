#!/usr/bin/env bash
# S5 (vig-os/stepv#9): build stepv.app — the Quick Look preview and thumbnail
# extensions, the CLI, the kernel and its OCCT dylibs — without Xcode.
#
# The Command Line Tools' swiftc and macOS SDK are enough: an app extension
# is a bundle with an NSExtension Info.plist whose executable links
# _NSExtensionMain as its entry point. Layout:
#
#   stepv.app/Contents/
#     Info.plist                  UTI declarations for STEP/IGES/BREP
#     MacOS/stepv-host            the container app (registers the extensions)
#     MacOS/stepv, stepv-occt     the CLI and its kernel, for Terminal use
#     Frameworks/                 libstepvocct.dylib (the kernel as a library)
#                                 and its OCCT dylibs, out of /nix/store: ONE
#                                 copy, shared by the CLI and both extensions
#     PlugIns/StepvThumbnail.appex, StepvPreview.appex: Swift, statically
#                                 linking libstepv_capi.a (the Rust renderer +
#                                 header reader) and loading the kernel
#                                 IN-PROCESS: the extension sandbox forbids
#                                 exec, but not loading a dylib
#
# Signing: STEPV_SIGN_IDENTITY (default "-", ad-hoc: runs on this machine).
# For distribution set it to a "Developer ID Application: …" identity; the
# hardened runtime and a secure timestamp are then enabled, and the result is
# ready for scripts/notarize-macos.sh. Output: target/macos/stepv.app.
set -euo pipefail
root=$(cd "$(dirname "$0")/.." && pwd)
src="$root/macos"
out="$root/target/macos"
app="$out/stepv.app"
identity=${STEPV_SIGN_IDENTITY:--}
version=$(sed -n 's/^version = "\(.*\)"/\1/p' "$root/Cargo.toml" | head -1)
arch=$(uname -m)
bin=${STEPV_BIN:-$root/target/release/stepv}
occt=${STEPV_OCCT:-$root/target/kernel/stepv-occt}
lib_occt=${STEPV_OCCT_LIB:-$root/target/kernel/libstepvocct.dylib}
lib_capi=${STEPV_CAPI_LIB:-$root/target/release/libstepv_capi.a}
for f in "$bin" "$occt" "$lib_occt" "$lib_capi"; do
  [ -e "$f" ] || { echo "build-macos-app: missing $f — run: cargo build --release --workspace && just kernel" >&2; exit 1; }
done

rm -rf "$app"
mkdir -p "$app/Contents/MacOS" "$app/Contents/PlugIns"
sed "s/@VERSION@/$version/g" "$src/Host/Info.plist" > "$app/Contents/Info.plist"
# Licence obligations travel with the binary (NOTICE; OCCT's LGPL + exception).
mkdir -p "$app/Contents/Resources/licenses"
cp "$root/NOTICE" "$root/LICENSE" "$app/Contents/Resources/"
cp "$root"/licenses/*.txt "$app/Contents/Resources/licenses/"

# ── Swift: Apple's toolchain, with a clean environment so the nix dev shell's
#    SDKROOT/CC/LD settings cannot leak into it. ──
swiftc() {
  env -i PATH=/usr/bin:/bin HOME="$HOME" TMPDIR="${TMPDIR:-/tmp}" /usr/bin/xcrun swiftc \
    -O -swift-version 5 -target "$arch-apple-macos14.0" \
    -sdk "$(env -i PATH=/usr/bin:/bin /usr/bin/xcrun --sdk macosx --show-sdk-path)" "$@"
}
swiftc "$src/Host/main.swift" -framework AppKit -o "$app/Contents/MacOS/stepv-host"

appex() { # name, frameworks...
  local name=$1; shift
  local dir="$app/Contents/PlugIns/$name.appex/Contents"
  mkdir -p "$dir/MacOS"
  local fwflags=(); for f in "$@"; do fwflags+=(-framework "$f"); done
  # libstepv_capi.a's native deps (cargo --print native-static-libs):
  # -liconv -lSystem -lc -lm, all in the SDK.
  swiftc -module-name "$name" -parse-as-library -application-extension \
    -import-objc-header "$src/Shared/Bridge.h" \
    "$src"/Shared/*.swift "$src"/"${name#Stepv}"/*.swift \
    "$lib_capi" -liconv -L "$(dirname "$lib_occt")" -lstepvocct \
    -Xlinker -rpath -Xlinker @executable_path/../../../../Frameworks \
    -Xlinker -e -Xlinker _NSExtensionMain "${fwflags[@]}" -o "$dir/MacOS/$name"
  sed "s/@VERSION@/$version/g" "$src/${name#Stepv}/Info.plist" > "$dir/Info.plist"
}
appex StepvThumbnail QuickLookThumbnailing SceneKit AppKit
appex StepvPreview QuickLookUI SceneKit AppKit

# ── CLI + kernel, and every /nix/store dylib they need, made relocatable,
#    INSIDE each extension: a Quick Look extension's sandbox cannot see the
#    containing app's files, so a shared copy in stepv.app/Contents/MacOS reads
#    as "doesn't exist" from the extension. ──
relocate() { # $1: a Mach-O; $2: the Frameworks dir it resolves against
  local f=$1 fw=$2 dep base
  while read -r dep; do
    base=$(basename "$dep")
    if [ ! -e "$fw/$base" ]; then
      cp "$dep" "$fw/$base"
      chmod u+w "$fw/$base"
      install_name_tool -id "@rpath/$base" "$fw/$base" 2>/dev/null
      relocate "$fw/$base" "$fw"
    fi
    install_name_tool -change "$dep" "@rpath/$base" "$f" 2>/dev/null
  done < <(otool -L "$f" | tail -n +2 | awk '{print $1}' | grep '^/nix/store/' || true)
}
# ONE copy of the kernel library and OCCT, in the app's Contents/Frameworks,
# shared by both extensions (rpath @executable_path/../../../../Frameworks)
# and the bundled CLI. Loading a dylib from the containing app is allowed in
# the extension sandbox; only exec is not.
fw="$app/Contents/Frameworks"
mkdir -p "$fw"
install -m644 "$lib_occt" "$fw/libstepvocct.dylib"
chmod u+w "$fw/libstepvocct.dylib"
relocate "$fw/libstepvocct.dylib" "$fw"
# The CLI for Terminal use: `stepv.app/Contents/MacOS/stepv`, with its kernel.
install -m755 "$bin" "$app/Contents/MacOS/stepv"
install -m755 "$occt" "$app/Contents/MacOS/stepv-occt"
for exe in stepv stepv-occt; do
  relocate "$app/Contents/MacOS/$exe" "$fw"
  install_name_tool -add_rpath "@executable_path/../Frameworks" "$app/Contents/MacOS/$exe" 2>/dev/null || true
done
for lib in "$fw"/*.dylib; do
  install_name_tool -add_rpath "@loader_path" "$lib" 2>/dev/null || true
done

# Nothing in the bundle may still point into /nix/store.
leaks=$(find "$app" -type f \( -name '*.dylib' -o -perm -u+x \) -exec sh -c 'otool -L "$1" 2>/dev/null | tail -n +2 | grep "/nix/store/"' _ {} \; || true)
[ -z "$leaks" ] || { echo "build-macos-app: unrelocated /nix/store references:" >&2; echo "$leaks" >&2; exit 1; }

# ── Sign inside-out. ──
sign_opts=(--force --sign "$identity")
[ "$identity" = "-" ] || sign_opts+=(--options runtime --timestamp)
codesign "${sign_opts[@]}" "$app"/Contents/Frameworks/*.dylib
for x in "$app"/Contents/PlugIns/*.appex; do
  codesign "${sign_opts[@]}" --entitlements "$src/extension.entitlements" "$x"
done
codesign "${sign_opts[@]}" "$app/Contents/MacOS/stepv" "$app/Contents/MacOS/stepv-occt"
codesign "${sign_opts[@]}" "$app"
codesign --verify --deep --strict "$app"

du -sh "$app" | awk '{print "build-macos-app: " $2 " (" $1 ")"}'
