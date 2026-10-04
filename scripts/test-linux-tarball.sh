#!/usr/bin/env bash
# Prove the Linux tarball runs on systems WITHOUT nix, old and new: unpack it
# in clean distro containers and render every test format through the
# launcher. A leftover /nix/store dependency cannot hide here.
#
#   scripts/test-linux-tarball.sh dist/stepv-*-linux.tar.gz
set -euo pipefail
tarball=$(readlink -f "$1")
root=$(cd "$(dirname "$0")/.." && pwd)
for image in debian:11 ubuntu:22.04 ubuntu:24.04 fedora:41; do
  echo "== $image"
  docker run --rm -v "$tarball:/stepv.tar.gz:ro" -v "$root/tests/data:/data:ro" "$image" sh -ec '
    test ! -e /nix
    mkdir /opt && cd /opt && tar -xzf /stepv.tar.gz
    s=/opt/stepv/bin/stepv
    $s --version
    for f in assembly.step box.igs box.brep sketch.step; do
      $s /data/$f --png /tmp/$f.png --size 128 --no-cache >/dev/null
      test -s /tmp/$f.png
    done
    $s /data/assembly.step --info >/dev/null
    # A broken file must fail cleanly with exit 3.
    printf "ISO-10303-21;\nHEADER;\nENDSEC;\n" > /tmp/bad.step
    set +e; $s /tmp/bad.step --png /tmp/bad.png --no-cache >/dev/null; rc=$?; set -e
    test "$rc" = 3
    echo ok
  '
done
echo "linux tarball: runs on every image"
