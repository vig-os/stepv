#!/usr/bin/env bash
# #28: the GPU viewer on Linux, from a Mac or any podman/docker host. Builds
# the kernel and stepv in a Debian container, then on Mesa's lavapipe (a
# Vulkan driver on the CPU) and Xvfb:
#   - the GPU renderer's tests, required to find an adapter (STEPV_REQUIRE_GPU);
#   - scripts/test-window-tests.sh: tests/cache.rs's tests that need a window
#     session (#84), ignored in nix's build sandbox, on Xvfb;
#   - scripts/test-viewer.sh: real windows, light and dark, --software, the
#     fallbacks, a click, and --frames on every fixture; Xvfb presents, so the
#     full window is required (STEPV_VIEW_REQUIRE_WINDOW);
#   - the size budget: 24 MB stripped (scripts/check-size.sh);
#   - then with the GPU drivers removed (Mesa's Vulkan ICDs, EGL vendor and
#     DRI drivers deleted): `stepv view --frames 5` falls back to the
#     software window and says so (#32).
#
#   scripts/test-linux-viewer.sh [screenshot dir]
#
# The local loop, and CI's Linux viewer job (kernel.yml). Builds with
# CARGO_BUILD_JOBS=2 by default: podman's default 4 GiB VM runs out of
# memory compiling `ash` at full parallelism (CI sets more). The cache lives in the
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
  -e CARGO_TARGET_DIR=/build/target -e RUSTUP_HOME=/build/rustup -e CARGO_BUILD_JOBS="${CARGO_BUILD_JOBS:-2}" \
  -e STEPV_VIEW_REQUIRE_WINDOW=1 \
  -e STEPV_OCCT=/build/kernel/stepv-occt -e STEPV_REQUIRE_GPU=1 \
  "$image" sh -euc '
    cmake -S kernel -B /build/kernel -DCMAKE_BUILD_TYPE=Release >/dev/null
    cmake --build /build/kernel --target stepv-occt -j2 >/dev/null
    cargo build -q --release --bin stepv
    cargo test -q --release --lib view::
    cargo test -q --release --test view
    # The measurement server (#33) under Landlock + seccomp.
    cargo test -q --release --test measure
    # Runs a command on its own Xvfb, at the given screen size. xvfb-run
    # starts it only once the server signals it is ready (SIGUSR1). -noreset
    # (#104): an X server resets whenever its last client disconnects, and
    # drops any connection that arrives during the reset. The window tests
    # run in parallel, each stepv its own client, so one could connect just
    # as the others had all closed, and winit failed with "Failed to open
    # connection to X server". Kept up, the server serves every client.
    xvfb() { size=$1; shift; xvfb-run -a -s "-screen 0 ${size}x24 -noreset" "$@"; }
    # The tests that need a window session (#84), on Xvfb.
    xvfb 1600x1000 scripts/test-window-tests.sh -q --release
    xvfb 1600x1000 scripts/test-viewer.sh /build/target/release/stepv \
      "$([ -d /shots ] && echo /shots/linux)"
    scripts/check-size.sh /build/target/release/stepv 24
    # No GPU at all: the drivers gone, the viewer must still open, in
    # software, and say why.
    rm -rf /usr/share/vulkan/icd.d /usr/share/glvnd/egl_vendor.d /usr/lib/*/dri
    out=$(xvfb 1280x800 /build/target/release/stepv view \
      tests/data/assembly.step --frames 5 2>/tmp/err) || true
    echo "$out" | grep -q "\"backend\":\"software\"" \
      || { echo "FAIL: no GPU: $out $(cat /tmp/err)" >&2; exit 1; }
    # The probe found nothing (not: found one the window then refused).
    grep -q "no usable GPU adapter; using the software viewer" /tmp/err \
      || { echo "FAIL: no GPU, and no note: $(cat /tmp/err)" >&2; exit 1; }
    if grep -q "could not start" /tmp/err; then
      echo "FAIL: a GPU adapter was still found: $(cat /tmp/err)" >&2; exit 1
    fi
    echo "ok: without GPU drivers, stepv view falls back to software and says so"
  '
