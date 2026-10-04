#!/bin/sh
# Install stepv's Linux integration: the CLI, its kernel, the thumbnailer,
# the MIME types. Run from a built tree (`cargo build --release` and
# `just kernel`) or point STEPV_BIN / STEPV_OCCT at built binaries.
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
bin=${STEPV_BIN:-$root/target/release/stepv}
occt=${STEPV_OCCT:-$root/target/kernel/stepv-occt}
for f in "$bin" "$occt"; do
  [ -x "$f" ] || { echo "install.sh: missing $f — build first" >&2; exit 1; }
done
d="$DESTDIR$PREFIX"
install -Dm755 "$bin" "$d/bin/stepv"
install -Dm755 "$occt" "$d/libexec/stepv/stepv-occt"
install -Dm644 "$here/stepv.thumbnailer" "$d/share/thumbnailers/stepv.thumbnailer"
install -Dm644 "$here/stepv-mime.xml" "$d/share/mime/packages/stepv.xml"
install -Dm644 "$here/stepv.desktop" "$d/share/applications/stepv.desktop"
# Packagers run these in their post-install hooks; a direct install runs them
# here, best effort (they are absent in minimal containers).
if [ -z "$DESTDIR" ]; then
  command -v update-mime-database >/dev/null && update-mime-database "$PREFIX/share/mime" || true
  command -v update-desktop-database >/dev/null && update-desktop-database "$PREFIX/share/applications" || true
fi
echo "stepv installed under $d"
