#!/usr/bin/env bash
# `stepv view` end to end (#28): opens real windows, so it needs a display
# (macOS: a logged-in session; Linux: X11 or Wayland, e.g. Xvfb).
#
#   1. The GPU viewer opens the assembly in the light and the dark theme,
#      reports a GPU "backend", and its window (STEPV_VIEW_SCREENSHOT) shows
#      the model in the theme asked for. (A window that is never presented,
#      as on a display-less CI runner, saves its viewport render instead:
#      the viewport's background is themed too, so the check holds.)
#   2. --software opens the minifb window and reports "software".
#   3. With no usable adapter (WGPU_BACKEND naming one this OS lacks), the
#      viewer falls back to the software window and says so on stderr.
#   4. The same when the probe found one but the window's adapter selection
#      fails (STEPV_VIEW_REJECT_ADAPTERS, a test hook).
#   5. A click (STEPV_VIEW_PICK) in the window picks the plate's top face,
#      and the inspector's topology names it a plane.
#
#   scripts/test-viewer.sh [path/to/stepv] [screenshot dir]
#
# With a screenshot dir, the images are kept there (the PR's screenshots).
set -euo pipefail
root=$(cd "$(dirname "$0")/.." && pwd)
stepv=${1:-$root/target/release/stepv}
keep=${2:-}
tmp=$(mktemp -d)
trap 'rm -rf "$tmp"' EXIT
fail() { echo "FAIL: $*" >&2; exit 1; }
input="$root/tests/data/assembly.step"

# Summarises a PNG: "<w> <h> <toolbar brightness> <colours>".
png_stats() {
  python3 - "$1" <<'PY'
import struct, sys, zlib
data = open(sys.argv[1], "rb").read()
assert data[:8] == b"\x89PNG\r\n\x1a\n", "not a PNG"
pos, idat = 8, b""
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
rows, prev = [], bytearray(stride)
for y in range(h):
    f = raw[y * (stride + 1)]
    line = bytearray(raw[y * (stride + 1) + 1:(y + 1) * (stride + 1)])
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
    rows.append(line)
    prev = line
px = lambda x, y: tuple(rows[y][x * bpp:x * bpp + 3])
# The toolbar's background, a little right of centre (no icons there).
tb = px(w * 3 // 5, 4)
colours = {px(x, y) for y in range(0, h, 4) for x in range(0, w, 4)}
print(w, h, sum(tb) // 3, len(colours))
PY
}

report() {
  python3 -c 'import json,sys
try: r = json.loads(sys.stdin.read())
except ValueError: print("no-json none"); sys.exit()
print(r.get("status"), r.get("backend"), r.get("error") or "")'
}

# 1. GPU, both themes.
for theme in light dark; do
  shot="$tmp/gpu-$theme.png"
  # `|| true`: under set -e a failing run would end the script before
  # `fail` could say why (and the trap would delete the evidence).
  out=$(STEPV_VIEW_SCREENSHOT="$shot" "$stepv" view "$input" --theme "$theme" 2>"$tmp/err") || true
  read -r status backend error <<<"$(report <<<"$out")"
  [ "$status" = ok ] || fail "gpu $theme: status $status ($error): $(cat "$tmp/err")"
  case "$backend" in
    metal|vulkan|gl|dx12) ;;
    software) fail "gpu $theme: fell back to software: $(cat "$tmp/err")" ;;
    *) fail "gpu $theme: backend '$backend'" ;;
  esac
  [ -s "$shot" ] || fail "gpu $theme: no screenshot"
  read -r w h bright colours <<<"$(png_stats "$shot")"
  [ "$w" -ge 480 ] && [ "$h" -ge 320 ] || fail "gpu $theme: window ${w}x$h"
  [ "$colours" -ge 50 ] || fail "gpu $theme: only $colours colours: a blank window?"
  if [ "$theme" = light ]; then
    [ "$bright" -ge 200 ] || fail "light theme toolbar is dark ($bright)"
  else
    [ "$bright" -le 60 ] || fail "dark theme toolbar is light ($bright)"
  fi
  how=window
  grep -q "never presented" "$tmp/err" && how="viewport render: the window was never presented"
  echo "ok: gpu $theme on $backend (${w}x$h, $colours colours; $how)"
done

# 2. --software.
shot="$tmp/software.png"
out=$(STEPV_VIEW_SCREENSHOT="$shot" "$stepv" view "$input" --software 2>"$tmp/err") || true
read -r status backend error <<<"$(report <<<"$out")"
[ "$status $backend" = "ok software" ] || fail "--software: $status $backend ($error): $(cat "$tmp/err")"
[ -s "$shot" ] || fail "--software: no screenshot"
read -r _ _ _ colours <<<"$(png_stats "$shot")"
[ "$colours" -ge 20 ] || fail "--software: only $colours colours"
echo "ok: --software"

# 3. No usable adapter: fall back, and say so.
missing=gl
[ "$(uname)" = Linux ] && missing=dx12
shot="$tmp/fallback.png"
out=$(WGPU_BACKEND=$missing STEPV_VIEW_SCREENSHOT="$shot" "$stepv" view "$input" 2>"$tmp/err") || true
read -r status backend error <<<"$(report <<<"$out")"
[ "$status $backend" = "ok software" ] || fail "fallback: $status $backend ($error): $(cat "$tmp/err")"
grep -q "no usable GPU adapter" "$tmp/err" || fail "fallback said nothing: $(cat "$tmp/err")"
[ -s "$shot" ] || fail "fallback: no screenshot"
echo "ok: no adapter (WGPU_BACKEND=$missing) falls back to software, and says so"

# 4. The probe finds an adapter but the window cannot use it (the selector
#    rejects every adapter): the window fails before it opens, and the
#    scene still reaches the software viewer.
shot="$tmp/rejected.png"
out=$(STEPV_VIEW_REJECT_ADAPTERS=1 STEPV_VIEW_SCREENSHOT="$shot" "$stepv" view "$input" 2>"$tmp/err") || true
read -r status backend error <<<"$(report <<<"$out")"
[ "$status $backend" = "ok software" ] || fail "rejected adapter: $status $backend ($error): $(cat "$tmp/err")"
grep -q "GPU viewer could not start" "$tmp/err" || fail "rejected adapter said nothing: $(cat "$tmp/err")"
[ -s "$shot" ] || fail "rejected adapter: no screenshot"
echo "ok: an adapter the window cannot use falls back to software, and says so"

# 5. A click through the real window's id pass (#29).
shot="$tmp/pick.png"
out=$(STEPV_VIEW_PICK=0.55,0.57 STEPV_VIEW_SCREENSHOT="$shot" "$stepv" view "$input" --theme light 2>"$tmp/err") || true
read -r status backend error <<<"$(report <<<"$out")"
[ "$status" = ok ] || fail "pick: $status $backend ($error): $(cat "$tmp/err")"
grep -q "STEPV_VIEW_PICK hit part 0 face [0-9]* (Plane)" "$tmp/err" \
  || fail "the click did not pick the plate's top: $(cat "$tmp/err")"
echo "ok: a click picks the plate's top face ($(grep -o 'face [0-9]* (Plane)' "$tmp/err"))"

if [ -n "$keep" ]; then
  mkdir -p "$keep"
  cp "$tmp"/*.png "$keep"/
  echo "screenshots in $keep"
fi
