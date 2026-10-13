#!/usr/bin/env bash
# #64: ql-e2e's whole matrix. Exit 0 if every check passes.
#   1. supervisor.sh: the supervisor service, probed outside Quick Look.
#   2. Quick Look itself runs the installed extensions: a thumbnail and a
#      preview of a small STEP file, an IGES and a BREP file.
# Needs a window session and the build installed (just ql-e2e does both).
set -uo pipefail
root=$(cd "$(dirname "$0")/../.." && pwd)
e2e="$root/scripts/ql-e2e"
app=/Applications/stepv.app
built="$root/target/macos/stepv.app"
# The installed app must be the build under test, byte for byte.
for x in StepvThumbnail StepvPreview; do
  cmp -s "$built/Contents/PlugIns/$x.appex/Contents/MacOS/$x" "$app/Contents/PlugIns/$x.appex/Contents/MacOS/$x" \
    || { echo "ql-e2e: $app is not the build in $built — run: just macos-install" >&2; exit 2; }
done
# An extension process started before the install still runs the old
# build, and Quick Look keeps sending it requests until it exits. macOS's
# own stat and date: the nix shell puts GNU coreutils first.
installed=$(/usr/bin/stat -f %m "$app/Contents/PlugIns/StepvThumbnail.appex/Contents/MacOS/StepvThumbnail")
for p in $(pgrep -f "$app/Contents/PlugIns/" || true); do
  started=$(LC_ALL=C ps -o lstart= -p "$p") || continue  # exited meanwhile
  since=$(LC_ALL=C /bin/date -j -f '%a %b %d %T %Y' "$started" +%s 2>/dev/null) \
    || { echo "ql-e2e: cannot read when pid $p started ('$started'), so cannot tell its build" >&2; exit 2; }
  [ "$since" -ge "$installed" ] || {
    echo "ql-e2e: pid $p runs the build installed before this one: $(ps -o comm= -p "$p")" >&2
    echo "        quit it (just macos-install does), then rerun" >&2
    exit 2
  }
done
# Canonical (/private/var, not /var): the extensions log the real path.
QL_E2E_DIR=$(cd -P "$(mktemp -d)" && pwd); export QL_E2E_DIR
trap 'rm -rf "$QL_E2E_DIR"' EXIT
rc=0
"$e2e/supervisor.sh" "$built" || rc=1
for f in assembly.step box.igs box.brep; do
  for k in thumb preview; do
    "$e2e/ql-e2e.sh" "$k" "$root/tests/data/$f" || rc=1
  done
done
# The control: the same file thumbnailed twice. Quick Look answers the
# second from its cache, and the verdict must say so and fail: proof that a
# cached thumbnail can never pass for the extension's.
cat "$root/tests/data/box.brep" >"$QL_E2E_DIR/cache-control.brep"
QL_E2E_REUSE=1 "$e2e/ql-e2e.sh" thumb "$QL_E2E_DIR/cache-control.brep" >/dev/null 2>&1 \
  || { echo "FAIL: cache control: the first request did not pass"; rc=1; }
out=$(QL_E2E_REUSE=1 "$e2e/ql-e2e.sh" thumb "$QL_E2E_DIR/cache-control.brep" 2>&1) \
  && { echo "FAIL: cache control: a cached thumbnail passed"; rc=1; }
if grep -q "QL served a cached thumbnail" <<<"$out"; then
  echo "ok: cache control: a repeated request is caught as a cached thumbnail"
else
  echo "FAIL: cache control: the repeated request was not caught as cached:"; echo "$out"; rc=1
fi
echo "== ql-e2e: $([ $rc = 0 ] && echo ALL PASS || echo FAILURES)"
exit $rc
