#!/bin/sh
# Install stepv's Linux integration: the CLI, its kernel, the thumbnailer,
# the MIME types. Run from a built tree (`cargo build --release` and
# `just kernel`) or point STEPV_BIN / STEPV_OCCT at built binaries.
#
#   install.sh --integration-only
#       only the thumbnailer, MIME types and desktop entry, for a `stepv`
#       that is already on PATH (the AppImage, or a distro/Nix package).
#
#   PREFIX   install prefix              (default /usr/local)
#   DESTDIR  staging root for packagers  (default empty)
#
# Layout: $PREFIX/bin/stepv and $PREFIX/libexec/stepv/stepv-occt, which is
# where stepv looks for its kernel relative to itself (src/occt.rs).
set -eu
here=$(cd "$(dirname "$0")" && pwd)
root=$(cd "$here/../.." && pwd)
PREFIX=${PREFIX:-/usr/local}
DESTDIR=${DESTDIR:-}
d="$DESTDIR$PREFIX"
if [ "${1:-}" = "--integration-only" ]; then
  command -v stepv >/dev/null || echo "install.sh: warning: no stepv on PATH; thumbnails will not work until there is one" >&2
else
  bin=${STEPV_BIN:-$root/target/release/stepv}
  occt=${STEPV_OCCT:-$root/target/kernel/stepv-occt}
  for f in "$bin" "$occt"; do
    [ -x "$f" ] || { echo "install.sh: missing $f — build first, or pass --integration-only" >&2; exit 1; }
  done
  install -Dm755 "$bin" "$d/bin/stepv"
  install -Dm755 "$occt" "$d/libexec/stepv/stepv-occt"
fi
install -Dm644 "$here/stepv.thumbnailer" "$d/share/thumbnailers/stepv.thumbnailer"
install -Dm644 "$here/stepv-mime.xml" "$d/share/mime/packages/stepv.xml"
install -Dm644 "$here/stepv.desktop" "$d/share/applications/stepv.desktop"
# Licence obligations travel with the binary (NOTICE; OCCT's LGPL + exception).
install -Dm644 "$root/NOTICE" "$d/share/doc/stepv/NOTICE"
for f in "$root"/licenses/*.txt; do install -Dm644 "$f" "$d/share/doc/stepv/licenses/$(basename "$f")"; done
# Packagers run these in their post-install hooks; a direct install runs them
# here, best effort (they are absent in minimal containers).
if [ -z "$DESTDIR" ]; then
  command -v update-mime-database >/dev/null && update-mime-database "$PREFIX/share/mime" || true
  command -v update-desktop-database >/dev/null && update-desktop-database "$PREFIX/share/applications" || true
fi
echo "stepv installed under $d"
