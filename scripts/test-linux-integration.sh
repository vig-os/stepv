#!/usr/bin/env bash
# S4 (vig-os/stepv#7): check the Linux desktop integration the way a desktop
# uses it, without a desktop.
#
#   1. The .thumbnailer's Exec line, with %i/%o/%s substituted exactly as
#      Nautilus/Tumbler do, produces a PNG of the requested size.
#   2. stepv-mime.xml compiles with update-mime-database, and the compiled
#      database maps our globs and magic to the right types.
#   3. The .desktop file validates (when desktop-file-validate is present).
#
# Needs: stepv on PATH (with its kernel), update-mime-database (shared-mime-info),
# python3. Usage: scripts/test-linux-integration.sh
set -euo pipefail
root=$(cd "$(dirname "$0")/.." && pwd)
pkg="$root/packaging/linux"
tmp=$(mktemp -d)
trap 'rm -rf "$tmp"' EXIT
fail() { echo "FAIL: $*" >&2; exit 1; }

# ── 1. thumbnailer ──
exec_line=$(sed -n 's/^Exec=//p' "$pkg/stepv.thumbnailer")
[ -n "$exec_line" ] || fail "no Exec= in stepv.thumbnailer"
for f in assembly.step box.igs box.brep sketch.step; do
  out="$tmp/${f}.png"
  cmd=${exec_line//%i/$root/tests/data/$f}
  cmd=${cmd//%o/$out}
  cmd=${cmd//%s/128}
  # Word-split like the desktop's Exec parser (no quoting in our line).
  # shellcheck disable=SC2086
  $cmd >/dev/null || fail "thumbnailer exited $? on $f"
  python3 - "$out" <<'PY' || fail "bad PNG for $f"
import struct, sys
b = open(sys.argv[1], "rb").read(32)
assert b[:8] == b"\x89PNG\r\n\x1a\n", "not a PNG"
w, h = struct.unpack(">II", b[16:24])
assert (w, h) == (128, 128), (w, h)
PY
  echo "ok: thumbnailer -> $f"
done
# A broken file must make the thumbnailer FAIL (so the desktop marks it), not
# write a bogus image.
printf 'ISO-10303-21;\nHEADER;\nENDSEC;\n' > "$tmp/broken.step"
cmd=${exec_line//%i/$tmp/broken.step}; cmd=${cmd//%o/$tmp/broken.png}; cmd=${cmd//%s/128}
# shellcheck disable=SC2086
if $cmd >/dev/null; then fail "thumbnailer succeeded on a broken file"; fi
[ ! -e "$tmp/broken.png" ] || fail "thumbnailer wrote an image for a broken file"
echo "ok: thumbnailer fails cleanly on a broken file"

# ── 2. MIME database ──
mkdir -p "$tmp/mime/packages"
cp "$pkg/stepv-mime.xml" "$tmp/mime/packages/stepv.xml"
update-mime-database "$tmp/mime" || fail "update-mime-database rejected stepv-mime.xml"
for pair in "*.step:model/step" "*.stp:model/step" "*.igs:model/iges" "*.brep:model/x-brep"; do
  grep -qxF "50:${pair#*:}:${pair%%:*}" "$tmp/mime/globs2" || fail "glob ${pair%%:*} -> ${pair#*:} missing"
done
grep -q "model/x-brep" "$tmp/mime/magic" || fail "BREP magic missing"
grep -q "model/step" "$tmp/mime/magic" || fail "STEP magic missing"
echo "ok: MIME database"

# ── 3. desktop entry ──
if command -v desktop-file-validate >/dev/null; then
  desktop-file-validate "$pkg/stepv.desktop" || fail "stepv.desktop invalid"
  echo "ok: desktop entry"
fi
echo "linux integration: all checks passed"
