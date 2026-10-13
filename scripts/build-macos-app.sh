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
#     Frameworks/                 the CLI kernel's OCCT dylibs, out of
#                                 /nix/store
#     PlugIns/StepvThumbnail.appex, StepvPreview.appex: Swift, statically
#                                 linking libstepv_capi.a (the Rust renderer +
#                                 header reader), and no kernel: a Quick Look
#                                 extension may not spawn one (#79), so its
#                                 supervisor service does (#64)
#       Contents/XPCServices/StepvSupervisor.xpc (#64), in each extension:
#                                 the service that runs the kernel as its
#                                 child; Helpers/stepv-occt (app-sandbox +
#                                 inherit) and Frameworks/, its own copy of
#                                 the OCCT dylibs (51 MB): its sandbox reads
#                                 its own bundle, not the app's Frameworks
#
# Signing: STEPV_SIGN_IDENTITY (default "-", ad-hoc: runs on this machine).
# For distribution set it to a "Developer ID Application: …" identity; the
# hardened runtime and a secure timestamp are then enabled, and the result is
# ready for scripts/notarize-macos.sh. Ad-hoc, the hardened runtime stays off:
# with it, library validation refuses the bundled dylibs ("different Team
# IDs"). Developer ID with it on is #97. Output: target/macos/stepv.app.
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
lib_capi=${STEPV_CAPI_LIB:-$root/target/release/libstepv_capi.a}
for f in "$bin" "$occt" "$lib_capi"; do
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
    "$lib_capi" -liconv \
    -Xlinker -e -Xlinker _NSExtensionMain "${fwflags[@]}" -o "$dir/MacOS/$name"
  sed "s/@VERSION@/$version/g" "$src/${name#Stepv}/Info.plist" > "$dir/Info.plist"
}
appex StepvThumbnail QuickLookThumbnailing SceneKit AppKit
appex StepvPreview QuickLookUI SceneKit AppKit

# ── #64: each extension's supervisor service, which runs the kernel as its
#    child (macos/Shared/StepvSupervisor.swift). Its own copy of stepv-occt
#    in Contents/Helpers, signed to inherit the service's sandbox, and its own
#    copy of the OCCT dylibs in Contents/Frameworks (below): the service's
#    sandbox reads its own bundle and nothing of the app around it ("file
#    system sandbox blocked open()" on stepv.app/Contents/Frameworks). ──
supervisor() { # the appex's name; its bundle id from its Info.plist
  local appex="$app/Contents/PlugIns/$1.appex/Contents"
  local id; id=$(/usr/libexec/PlistBuddy -c 'Print :CFBundleIdentifier' "$appex/Info.plist")
  local x="$appex/XPCServices/StepvSupervisor.xpc/Contents"
  mkdir -p "$x/MacOS" "$x/Helpers"
  swiftc -module-name StepvSupervisor -application-extension \
    "$src/Shared/StepvFrames.swift" "$src/Shared/StepvSupervisor.swift" "$src/Supervisor/main.swift" \
    -o "$x/MacOS/StepvSupervisor"
  sed -e "s/@ID@/$id.supervisor/" -e "s/@VERSION@/$version/g" "$src/Supervisor/Info.plist" > "$x/Info.plist"
  install -m755 "$occt" "$x/Helpers/stepv-occt"
}
supervisor StepvThumbnail
supervisor StepvPreview

# ── The kernels, and every /nix/store dylib they need, made relocatable:
#    the CLI's against the app's Frameworks, each supervisor's against its
#    own (a sandboxed service cannot read the app around it). ──
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
# The CLI kernel's OCCT, in the app's Contents/Frameworks (the supervisors'
# kernels each have their own copy, below: #101).
fw="$app/Contents/Frameworks"
mkdir -p "$fw"
# The CLI for Terminal use: `stepv.app/Contents/MacOS/stepv`, with its kernel.
install -m755 "$bin" "$app/Contents/MacOS/stepv"
install -m755 "$occt" "$app/Contents/MacOS/stepv-occt"
for exe in stepv stepv-occt; do
  relocate "$app/Contents/MacOS/$exe" "$fw"
  install_name_tool -add_rpath "@executable_path/../Frameworks" "$app/Contents/MacOS/$exe" 2>/dev/null || true
done
# The supervisors' kernels, each with the closure in its own service.
for x in "$app"/Contents/PlugIns/*.appex/Contents/XPCServices/*.xpc; do
  mkdir -p "$x/Contents/Frameworks"
  relocate "$x/Contents/Helpers/stepv-occt" "$x/Contents/Frameworks"
  install_name_tool -add_rpath "@executable_path/../Frameworks" "$x/Contents/Helpers/stepv-occt"
done
for lib in "$fw"/*.dylib "$app"/Contents/PlugIns/*.appex/Contents/XPCServices/*.xpc/Contents/Frameworks/*.dylib; do
  install_name_tool -add_rpath "@loader_path" "$lib" 2>/dev/null || true
done

# Nothing in the bundle may still point into /nix/store.
leaks=$(find "$app" -type f \( -name '*.dylib' -o -perm -u+x \) -exec sh -c 'otool -L "$1" 2>/dev/null | tail -n +2 | grep "/nix/store/"' _ {} \; || true)
[ -z "$leaks" ] || { echo "build-macos-app: unrelocated /nix/store references:" >&2; echo "$leaks" >&2; exit 1; }

# ── Sign inside-out. ──
sign_opts=(--force --sign "$identity")
[ "$identity" = "-" ] || sign_opts+=(--options runtime --timestamp)
codesign "${sign_opts[@]}" "$app"/Contents/Frameworks/*.dylib
for x in "$app"/Contents/PlugIns/*.appex/Contents/XPCServices/*.xpc; do
  codesign "${sign_opts[@]}" "$x"/Contents/Frameworks/*.dylib
  codesign "${sign_opts[@]}" --identifier ch.exoma.stepv.kernel \
    --entitlements "$src/kernel.entitlements" "$x/Contents/Helpers/stepv-occt"
  codesign "${sign_opts[@]}" --entitlements "$src/supervisor.entitlements" "$x"
done
for x in "$app"/Contents/PlugIns/*.appex; do
  codesign "${sign_opts[@]}" --entitlements "$src/extension.entitlements" "$x"
done
codesign "${sign_opts[@]}" "$app/Contents/MacOS/stepv" "$app/Contents/MacOS/stepv-occt"
codesign "${sign_opts[@]}" "$app"
codesign --verify --deep --strict "$app"

du -sh "$app" | awk '{print "build-macos-app: " $2 " (" $1 ")"}'
