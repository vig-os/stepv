#!/usr/bin/env bash
# S6 (vig-os/stepv#10): a relocatable Linux tarball from the nix build.
#
#   scripts/package-linux.sh <nix-result> <version>   -> dist/stepv-<version>-<arch>-linux.tar.gz
#
# Why not an AppImage: nix-appimage's runtime bind-mounts /nix through an
# unprivileged user namespace, and Ubuntu 24.04+ (AppArmor) forbids those by
# default ("cannot write uid_map: Operation not permitted"). So instead the
# tarball carries the binaries, every shared library they load, and nix's own
# dynamic loader, and starts them through that loader:
#
#   stepv/bin/stepv                     wrapper: exec lib/ld.so --library-path lib libexec/stepv.bin
#   stepv/libexec/stepv-occt            wrapper for the kernel (the CLI finds it via STEPV_OCCT)
#   stepv/libexec/{stepv,stepv-occt}.bin
#   stepv/lib/                          glibc, libstdc++, OCCT, … from the nix closure
#   stepv/share/                        thumbnailer, MIME, desktop entry, NOTICE + licences
#
# Using the bundled loader makes the host's glibc irrelevant: it runs on any
# x86_64/aarch64 Linux, old or new. No FUSE, no namespaces, no root.
set -euo pipefail
result=$(readlink -f "$1")
version=$2
root=$(cd "$(dirname "$0")/.." && pwd)
arch=$(uname -m)
name="stepv-$version-$arch-linux"
stage=$(mktemp -d)
trap 'rm -rf "$stage"' EXIT
pkg="$stage/stepv"
mkdir -p "$pkg/bin" "$pkg/libexec" "$pkg/lib"

install -m755 "$result/bin/stepv" "$pkg/libexec/stepv.bin"
install -m755 "$result/libexec/stepv/stepv-occt" "$pkg/libexec/stepv-occt.bin"
cp -r "$result/share" "$pkg/share"
chmod -R u+w "$pkg/share"

# Every library the two binaries load, resolved by the nix loader itself.
interp=""
for exe in "$pkg/libexec/stepv.bin" "$pkg/libexec/stepv-occt.bin"; do
  while read -r lib; do
    [ -n "$lib" ] && cp -nL "$lib" "$pkg/lib/" 2>/dev/null || true
  done < <(ldd "$exe" | awk '/=> \//{print $3} /^\s*\/nix\/store/{print $1}')
  i=$(patchelf --print-interpreter "$exe")
  [ -z "$interp" ] || [ "$interp" = "$i" ] || { echo "package-linux: two interpreters" >&2; exit 1; }
  interp=$i
done
cp -L "$interp" "$pkg/lib/ld.so"
chmod u+w "$pkg"/lib/*

wrapper() { # $1 target .bin under libexec, $2 output path
  cat > "$2" <<EOF
#!/bin/sh
# stepv launcher: runs the bundled binary through the bundled loader, so the
# host's glibc and libraries never matter. Symlink this anywhere on PATH.
self=\$(readlink -f "\$0")
here=\$(dirname "\$(dirname "\$self")")
export STEPV_OCCT="\$here/libexec/stepv-occt"
# Ours first; the host's after, for what only the host has: what
# \`stepv view\` dlopens at run time (X11/Wayland/xkbcommon for the window,
# libvulkan.so.1 / libGL / libEGL and the GPU drivers for wgpu). They load
# against the bundled glibc; a host library that needs a newer glibc fails
# to load, and the viewer then falls back to its software window, saying
# so (#32; test-linux-tarball.sh checks it exits cleanly without a display).
libs="\$here/lib:/usr/lib/$arch-linux-gnu:/usr/lib64:/usr/lib:/lib/$arch-linux-gnu:/lib64:/lib"
exec "\$here/lib/ld.so" --library-path "\$libs" "\$here/libexec/$1" "\$@"
EOF
  chmod 755 "$2"
}
wrapper stepv.bin "$pkg/bin/stepv"
wrapper stepv-occt.bin "$pkg/libexec/stepv-occt"

# The thumbnailer and desktop entry say `stepv`, which is right once bin/ is on
# PATH; install-integration.sh puts the share/ files where desktops look.
cat > "$pkg/install-integration.sh" <<'EOF'
#!/bin/sh
# Link stepv onto PATH and install the thumbnailer, MIME types and desktop
# entry for the current user (or PREFIX=/usr/local, run as root, for all).
set -eu
here=$(dirname "$(readlink -f "$0")")
PREFIX=${PREFIX:-$HOME/.local}
mkdir -p "$PREFIX/bin" "$PREFIX/share/thumbnailers" "$PREFIX/share/mime/packages" "$PREFIX/share/applications"
ln -sf "$here/bin/stepv" "$PREFIX/bin/stepv"
cp "$here/share/thumbnailers/stepv.thumbnailer" "$PREFIX/share/thumbnailers/"
cp "$here/share/mime/packages/stepv.xml" "$PREFIX/share/mime/packages/"
cp "$here/share/applications/stepv.desktop" "$PREFIX/share/applications/"
command -v update-mime-database >/dev/null && update-mime-database "$PREFIX/share/mime" || true
command -v update-desktop-database >/dev/null && update-desktop-database "$PREFIX/share/applications" || true
echo "stepv: linked $PREFIX/bin/stepv; thumbnails appear after the file manager restarts"
EOF
chmod 755 "$pkg/install-integration.sh"

# Nothing may still need /nix/store at run time.
if grep -rl --binary-files=text '/nix/store/[a-z0-9]\{32\}-[^/]*/lib/ld-linux' "$pkg/bin" "$pkg/libexec"/*-occt "$pkg/install-integration.sh" 2>/dev/null; then
  echo "package-linux: a launcher references /nix/store" >&2; exit 1
fi

mkdir -p "$root/dist"
tar -C "$stage" -czf "$root/dist/$name.tar.gz" stepv
du -sh "$root/dist/$name.tar.gz" | awk '{print "package-linux: " $2 " (" $1 ")"}'
