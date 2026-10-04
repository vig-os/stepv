#!/usr/bin/env bash
# #18: the kernel's Linux sandbox (Landlock + seccomp), tested on a real Linux
# kernel from a Mac or any podman/docker host: builds the kernel in a Debian
# container against Debian's OCCT and runs tests/sandbox.rs against it.
#
#   scripts/test-linux-sandbox.sh [cargo test args]
#
# CI covers the same on the ubuntu Kernel lane with the nix-built kernel; this
# is the local loop. The build cache lives in the `stepv-linux-build` volume.
set -euo pipefail
root=$(cd "$(dirname "$0")/.." && pwd)
engine=$(command -v podman || command -v docker)
image=stepv-linux-test
"$engine" build -q -t "$image" - >/dev/null <<'CONTAINERFILE'
FROM debian:trixie
RUN apt-get update -qq && apt-get install -y -qq --no-install-recommends \
      cmake g++ make curl ca-certificates libocct-data-exchange-dev \
      libocct-modeling-algorithms-dev libocct-modeling-data-dev \
      libocct-foundation-dev libocct-ocaf-dev libocct-visualization-dev libtbb-dev libfontconfig-dev libfreetype-dev >/dev/null
RUN curl -sSf https://sh.rustup.rs | sh -s -- -y --profile minimal --default-toolchain none
ENV PATH=/root/.cargo/bin:$PATH
CONTAINERFILE
# shellcheck disable=SC2016 # expanded by the container's shell, not this one
"$engine" run --rm -v "$root:/src:ro" -v stepv-linux-build:/build -w /src \
  -e CARGO_TARGET_DIR=/build/target -e RUSTUP_HOME=/build/rustup -e STEPV_OCCT=/build/kernel/stepv-occt \
  "$image" sh -euc '
    cmake -S kernel -B /build/kernel -DCMAKE_BUILD_TYPE=Release >/dev/null
    cmake --build /build/kernel --target stepv-occt -j"$(nproc)" >/dev/null
    cargo test -q --no-default-features --test sandbox "$@"
  ' sh "$@"
