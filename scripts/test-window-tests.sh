#!/bin/sh
# The integration tests that need a window session (#84): `stepv view` fails
# before its kernel without one, so they are ignored in nix's build sandbox,
# which has none, and run here instead: by `just view-test` on the macOS
# runner, and under Xvfb in the Linux viewer job.
#
# Fails unless every one of them ran and passed: an ignored test that nothing
# runs is a skip. The count is taken from the ignore markers themselves.
#
#   scripts/test-window-tests.sh [cargo test options, e.g. --release]
set -eu
root=$(cd "$(dirname "$0")/.." && pwd)
cd "$root"
want=$(grep -c '^#\[ignore = "needs a window session' tests/cache.rs)
out=$(cargo test "$@" --test cache -- --ignored 2>&1) || {
  printf '%s\n' "$out"
  exit 1
}
printf '%s\n' "$out"
printf '%s\n' "$out" | grep -q "test result: ok. $want passed; 0 failed" || {
  echo "FAIL: expected all $want window tests in tests/cache.rs to run and pass" >&2
  exit 1
}
