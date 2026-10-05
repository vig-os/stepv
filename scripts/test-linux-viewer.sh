#!/usr/bin/env bash
# #28: the GPU viewer on Linux, from a Mac or any podman/docker host. Builds
# the kernel and stepv in a Debian container, then on Mesa's lavapipe (a
# Vulkan driver on the CPU) and Xvfb:
#   - the GPU renderer's tests, required to find an adapter (STEPV_REQUIRE_GPU);
#   - scripts/test-viewer.sh: real windows, light and dark, --software, and the
#     fallback with no adapter.
#
#   scripts/test-linux-viewer.sh [screenshot dir]
#
# CI's Linux smoke test on lavapipe is #32; this is the local loop. Builds
# with CARGO_BUILD_JOBS=2: podman's default 4 GiB VM runs out of memory
# compiling `ash` at full parallelism. The cache lives in the
# `stepv-linux-viewer` volume.
set -euo pipefail
root=$(cd "$(dirname "$0")/.." && pwd)
engine=$(command -v podman || command -v docker)
image=stepv-linux-viewer
shots=${1:-}
"$engine" build -q -t "$image" - >/dev/null <<'CONTAINERFILE'
FROM debian:trixie
RUN apt-get update -qq && apt-get install -y -qq --no-install-recommends \
      cmake g++ make curl ca-certificates python3 libocct-data-exchange-dev \
      libocct-modeling-algorithms-dev libocct-modeling-data-dev \
      libocct-foundation-dev libocct-ocaf-dev libocct-visualization-dev libtbb-dev libfontconfig-dev libfreetype-dev \
      xvfb xauth mesa-vulkan-drivers libvulkan1 libegl1 libgl1-mesa-dri \
      libx11-6 libx11-xcb1 libxcursor1 libxrandr2 libxi6 libxkbcommon0 libxkbcommon-x11-0 >/dev/null
RUN curl -sSf https://sh.rustup.rs | sh -s -- -y --profile minimal --default-toolchain none
ENV PATH=/root/.cargo/bin:$PATH
CONTAINERFILE
mounts=(-v "$root:/src:ro" -v stepv-linux-viewer:/build)
[ -n "$shots" ] && mkdir -p "$shots" && mounts+=(-v "$shots:/shots")
# shellcheck disable=SC2016 # expanded by the container's shell, not this one
"$engine" run --rm "${mounts[@]}" -w /src \
  -e CARGO_TARGET_DIR=/build/target -e RUSTUP_HOME=/build/rustup -e CARGO_BUILD_JOBS=2 \
  -e STEPV_OCCT=/build/kernel/stepv-occt -e STEPV_REQUIRE_GPU=1 \
  "$image" sh -euc '
    cmake -S kernel -B /build/kernel -DCMAKE_BUILD_TYPE=Release >/dev/null
    cmake --build /build/kernel --target stepv-occt -j2 >/dev/null
    cargo build -q --release --bin stepv
    cargo test -q --release --lib view::
    cargo test -q --release --test view
    xvfb-run -a -s "-screen 0 1600x1000x24" scripts/test-viewer.sh /build/target/release/stepv \
      "$([ -d /shots ] && echo /shots/linux)"
  '
