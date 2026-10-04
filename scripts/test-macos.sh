#!/usr/bin/env bash
# S5 (vig-os/stepv#9): headless checks of the macOS front-end.
#
#   1. The Swift STEPVMSH reader agrees with the Rust one: for every file in
#      tests/data, the part and triangle counts it decodes equal what the
#      CLI reported for the same buffers.
#   2. The preview's SceneKit scene renders offscreen (SCNRenderer) and is
#      not blank, and the assembly shows its per-face colours.
#   3. With --quicklook and stepv.app installed in /Applications: Quick Look
#      itself produces a thumbnail for every format, through our extension.
#
# Needs: target/release/stepv + target/kernel (just macos-app builds both).
set -euo pipefail
root=$(cd "$(dirname "$0")/.." && pwd)
tmp=$(mktemp -d)
trap 'rm -rf "$tmp"' EXIT
fail() { echo "FAIL: $*" >&2; exit 1; }
stepv="$root/target/release/stepv"

env -i PATH=/usr/bin:/bin HOME="$HOME" TMPDIR="$tmp" /usr/bin/xcrun swiftc -O -swift-version 5 \
  -target "$(uname -m)-apple-macos14.0" \
  "$root"/macos/Shared/StepvMesh.swift "$root"/macos/Shared/StepvSceneBuilder.swift \
  "$root"/macos/Tests/main.swift -framework SceneKit -framework AppKit -framework Metal \
  -o "$tmp/snapshot"

for f in assembly.step box.igs box.brep; do
  report=$("$stepv" "$root/tests/data/$f" --mesh "$tmp/$f.msh" --quality preview --no-cache)
  want=$(python3 -c 'import json,sys; k=json.loads(sys.argv[1])["kernel"]; print("parts=%d triangles=%d" % (k["parts"], k["triangles"]))' "$report")
  got=$("$tmp/snapshot" "$tmp/$f.msh" "$tmp/$f.png")
  [[ "$got" == "$want"* ]] || fail "$f: Swift decoded '$got', CLI reported '$want'"
  python3 - "$tmp/$f.png" "$f" <<'PY' || fail "$f: snapshot is blank or colourless"
import struct, sys, zlib
data = open(sys.argv[1], "rb").read()
assert data[:8] == b"\x89PNG\r\n\x1a\n"
# Decode just enough PNG to count non-background pixels.
pos, idat, w, h, ct = 8, b"", 0, 0, 0
while pos < len(data):
    n, kind = struct.unpack(">I4s", data[pos:pos + 8])
    body = data[pos + 8:pos + 8 + n]
    if kind == b"IHDR":
        w, h, _, ct = struct.unpack(">IIBB", body[:10])
    elif kind == b"IDAT":
        idat += body
    pos += 12 + n
raw = zlib.decompress(idat)
bpp = 4 if ct == 6 else 3
stride = w * bpp
px, prev = [], bytearray(stride)
for y in range(h):
    f, line = raw[y * (stride + 1)], bytearray(raw[y * (stride + 1) + 1:(y + 1) * (stride + 1)])
    for i in range(stride):
        a = line[i - bpp] if i >= bpp else 0
        b = prev[i]
        c = prev[i - bpp] if i >= bpp else 0
        if f == 1: line[i] = (line[i] + a) & 255
        elif f == 2: line[i] = (line[i] + b) & 255
        elif f == 3: line[i] = (line[i] + (a + b) // 2) & 255
        elif f == 4:
            p = a + b - c; pa, pb, pc = abs(p - a), abs(p - b), abs(p - c)
            line[i] = (line[i] + (a if pa <= pb and pa <= pc else b if pb <= pc else c)) & 255
    px += [tuple(line[i:i + 3]) for i in range(0, stride, bpp)]
    prev = line
drawn = [p for p in px if p != px[0]]
assert len(drawn) > len(px) * 0.05, f"only {len(drawn)} drawn pixels"
if sys.argv[2] == "assembly.step":
    red = sum(1 for r, g, b in drawn if r > g + 60 and r > b + 60)
    blue = sum(1 for r, g, b in drawn if b > r + 40)
    assert red > 100 and blue > 100, f"per-face colours missing: red={red} blue={blue}"
PY
  echo "ok: $f — $got"
done

if [ "${1:-}" = "--quicklook" ]; then
  [ -d /Applications/stepv.app ] || fail "--quicklook needs /Applications/stepv.app"
  mkdir -p "$tmp/ql"
  cp "$root"/tests/data/{assembly.step,box.igs,box.brep,sketch.step} "$tmp/ql/"
  qlmanage -r cache >/dev/null 2>&1 || true
  out=$(qlmanage -t -x -s 256 -o "$tmp/ql" "$tmp"/ql/*.step "$tmp"/ql/*.igs "$tmp"/ql/*.brep 2>&1)
  n=$(grep -c "produced one thumbnail" <<<"$out" || true)
  [ "$n" -eq 4 ] || fail "Quick Look produced $n/4 thumbnails:\n$out"
  echo "ok: Quick Look thumbnails for all 4 formats"
fi
echo "macos: all checks passed"
