#!/usr/bin/env bash
# #64: one Quick Look end-to-end run, with no human: Quick Look itself runs
# the installed stepv extension on a fresh copy of <file> (a new path, so no
# Quick Look cache answers), and the extension's own log line for that path
# is the verdict (verdict.py). Hard timeouts throughout; never qlmanage,
# which hangs when driven from a script.
#
#   scripts/ql-e2e/ql-e2e.sh <thumb|preview> <file>
#
# Needs a window session, target/ql-e2e (build.sh) and the build under test
# installed at /Applications/stepv.app (just macos-install).
set -uo pipefail
root=$(cd "$(dirname "$0")/../.." && pwd)
bin="$root/target/ql-e2e"
kind=${1:?thumb|preview}
file=${2:?file}
work=${QL_E2E_DIR:?the run directory, from run-all.sh}
copy="$work/$(date +%H%M%S)-$$-$kind-$(basename "$file")"
cp "$file" "$copy"
start=$(date '+%Y-%m-%d %H:%M:%S')
echo "== $kind $(basename "$file")"
case $kind in
  thumb)
    timeout 45 "$bin/ql-thumb" "$copy" 256 40 | sed 's/^/  /'
    echo "  ql-thumb exit=${PIPESTATUS[0]}" ;;
  preview)
    # Only our own host is ever stopped, and only if it outlives its timeout.
    timeout 45 open -n -W -a "$bin/ql-preview-host.app" --args "$copy" 6
    rc=$?
    echo "  ql-preview-host exit=$rc"
    [ $rc -eq 124 ] && pkill -f "$bin/ql-preview-host.app/Contents/MacOS/ql-preview-host" ;;
  *) echo "kind: thumb|preview" >&2; exit 2 ;;
esac
sleep 1  # the log catches up
timeout 60 /usr/bin/log show --start "$start" --style ndjson --info \
  --predicate 'subsystem == "ch.exoma.stepv"' 2>/dev/null \
  | python3 "$root/scripts/ql-e2e/verdict.py" "$kind" "$copy"
