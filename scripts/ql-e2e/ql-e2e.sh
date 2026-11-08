#!/usr/bin/env bash
# #64: one Quick Look end-to-end run, with no human: Quick Look itself runs
# the installed stepv extension on a fresh copy of <file> (a new path, so no
# Quick Look cache answers), and the extension's own log line for that path
# is the verdict (verdict.py). Hard timeouts throughout; never qlmanage,
# which hangs when driven from a script.
#
#   scripts/ql-e2e/ql-e2e.sh <thumb|preview> <file>
#
# Run by run-all.sh, which gives it the run directory and the log stream
# (QL_E2E_DIR, QL_E2E_STREAM). Needs a window session, target/ql-e2e
# (build.sh) and the build under test installed at /Applications/stepv.app
# (just macos-install).
set -uo pipefail
root=$(cd "$(dirname "$0")/../.." && pwd)
bin="$root/target/ql-e2e"
kind=${1:?thumb|preview}
file=${2:?file}
work=${QL_E2E_DIR:?the run directory, from run-all.sh}
stream=${QL_E2E_STREAM:?the log stream file, from run-all.sh}
copy="$work/$(date +%H%M%S)-$$-$kind-$(basename "$file")"
# Freshly written bytes: a new file, a new inode, whatever cp would do (GNU
# cp may clone). Quick Look's thumbnail cache is keyed on the inode: a file
# it has seen is answered from the cache, and the extension never runs.
# QL_E2E_REUSE=1 uses <file> as it is: run-all.sh's cache-hit control.
if [ "${QL_E2E_REUSE:-}" = 1 ]; then copy=$file; else cat "$file" >"$copy"; fi
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
# The extension's result line for this path, from the one log stream: by
# the path alone, never by when it was stamped (e2elog.py). At most 30 s;
# past that, say so and judge what is there. For the cache control's
# second request that is the expected end: no line ever comes.
end=$((SECONDS + 30))
until python3 "$root/scripts/ql-e2e/verdict.py" --logged "$kind" "$copy" <"$stream"; do
  [ $SECONDS -lt $end ] || { echo "  no result line for $copy in the log stream within 30 s"; break; }
  sleep 0.2
done
python3 "$root/scripts/ql-e2e/verdict.py" "$kind" "$copy" <"$stream" && exit 0
# The evidence, for a failure on a machine no one can look at afterwards:
# the stream's last Quick Look extension lines, this request's among them if
# any came.
echo "  the extensions' last lines in the log stream:"
python3 -c '
import sys
sys.path.insert(0, sys.argv[1])
import e2elog
ql = [e for e in e2elog.events(sys.stdin) if e["category"] == "quicklook"]
for e in ql[-20:]:
    print("   ", e["process"].rsplit("/", 1)[-1], "pid", e["pid"], e["message"][:200])
' "$root/scripts/ql-e2e" <"$stream"
exit 1
