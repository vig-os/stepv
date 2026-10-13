#!/usr/bin/env bash
# #64: build ql-e2e's two Quick Look clients into target/ql-e2e: ql-thumb
# (QLThumbnailGenerator) and ql-preview-host.app (a QLPreviewView).
set -euo pipefail
root=$(cd "$(dirname "$0")/../.." && pwd)
src="$root/scripts/ql-e2e"
out="$root/target/ql-e2e"
sdk=$(env -i PATH=/usr/bin:/bin /usr/bin/xcrun --sdk macosx --show-sdk-path)
swiftc() {
  env -i PATH=/usr/bin:/bin HOME="$HOME" TMPDIR="${TMPDIR:-/tmp}" /usr/bin/xcrun swiftc -O -swift-version 5 \
    -target "$(uname -m)-apple-macos14.0" -sdk "$sdk" "$@"
}
mkdir -p "$out"
swiftc "$src/thumb.swift" -framework QuickLookThumbnailing -framework AppKit -o "$out/ql-thumb"
h="$out/ql-preview-host.app/Contents"
rm -rf "$out/ql-preview-host.app"
mkdir -p "$h/MacOS"
cp "$src/PreviewHost/Info.plist" "$h/Info.plist"
swiftc "$src/PreviewHost/main.swift" -framework Quartz -framework AppKit -o "$h/MacOS/ql-preview-host"
codesign --force --sign - "$out/ql-thumb" 2>/dev/null
codesign --force --sign - "$out/ql-preview-host.app" 2>/dev/null
echo "ql-e2e: built $out"
