#!/usr/bin/env bash
# #32: the binary-size budget. The GPU viewer (egui, wgpu, winit) roughly
# doubled the CLI; this keeps the next dependency from doing it again
# unnoticed. Measures a stripped copy, as the packages ship it.
#
#   scripts/check-size.sh <binary> <budget MB>
set -euo pipefail
bin=$1
budget_mb=$2
tmp=$(mktemp)
trap 'rm -f "$tmp"' EXIT
cp "$bin" "$tmp"
strip "$tmp" 2>/dev/null || strip -x "$tmp"
bytes=$(wc -c <"$tmp" | tr -d ' ')
mb=$(awk -v b="$bytes" 'BEGIN { printf "%.1f", b / 1048576 }')  # MiB, as the budget
if [ "$bytes" -gt $((budget_mb * 1048576)) ]; then
  echo "FAIL: $bin is $mb MiB stripped, over its $budget_mb MiB budget" >&2
  exit 1
fi
echo "ok: $bin is $mb MiB stripped (budget $budget_mb MiB)"
