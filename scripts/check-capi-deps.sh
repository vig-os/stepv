#!/usr/bin/env bash
# The capi tripwire (#28): the Quick Look extensions link libstepv_capi.a, and
# it must never carry the viewer's window and GPU stack (eframe, egui, winit,
# wgpu, naga, minifb). A Quick Look extension is a sandboxed, memory-capped
# helper; a GPU stack inside it is megabytes of code and a second renderer
# nobody tests there. Cargo's feature unification makes this easy to break
# silently: one `--workspace` build, or a dependency that turns `viewer` on,
# and the extension grows a window toolkit.
#
#   scripts/check-capi-deps.sh [path/to/libstepv_capi.a]
#
# Checks both the dependency graph (cargo tree, what capi would build) and the
# built archive's symbols (what it did build). The archive defaults to
# target/release/libstepv_capi.a, built here if missing.
set -euo pipefail
root=$(cd "$(dirname "$0")/.." && pwd)
cd "$root"
banned='eframe|egui[a-z_-]*|epaint[a-z_-]*|winit|wgpu[a-z_-]*|naga|minifb|ash|glow'
fail=0

# 1. The graph: every normal and build dependency of stepv-capi, as built on
#    its own (as justfile.project's macos-app does).
if tree=$(cargo tree -p stepv-capi -e normal,build --prefix none --format '{p}' 2>&1); then
  hits=$(awk '{print $1}' <<<"$tree" | sort -u | grep -xE "$banned" || true)
  if [ -n "$hits" ]; then
    echo "FAIL: stepv-capi depends on the viewer stack:" >&2
    while read -r h; do echo "  $h"; done <<<"$hits" >&2
    fail=1
  else
    echo "ok: stepv-capi's dependency graph has no viewer crates ($(wc -l <<<"$tree" | tr -d ' ') entries)"
  fi
else
  echo "FAIL: cargo tree -p stepv-capi: $tree" >&2
  exit 1
fi

# 2. The archive: no symbol from those crates. Rust's legacy mangling writes
#    a crate name length-prefixed (`6eframe`), v0 the same inside `_R`.
lib=${1:-target/release/libstepv_capi.a}
if [ ! -f "$lib" ]; then
  cargo build --release -p stepv-capi
fi
syms=$(nm -g "$lib" 2>/dev/null || true)
[ -n "$syms" ] || { echo "FAIL: no symbols read from $lib" >&2; exit 1; }
hits=$(grep -oE '[0-9]+(eframe|egui|epaint|winit|wgpu(_core|_hal|_types)?|naga|minifb)[0-9_]' <<<"$syms" \
  | sed -E 's/^[0-9]+//; s/[0-9_]$//' | sort -u || true)
if [ -n "$hits" ]; then
  echo "FAIL: $lib carries symbols of:" >&2
  while read -r h; do echo "  $h"; done <<<"$hits" >&2
  fail=1
else
  echo "ok: $lib has no viewer symbols ($(wc -l <<<"$syms" | tr -d ' ') symbols)"
fi
exit $fail
