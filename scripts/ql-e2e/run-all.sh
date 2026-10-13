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
e2e_start=$(date '+%Y-%m-%d %H:%M:%S')
e2e_start_iso=$(python3 -c 'import sys; sys.path.insert(0, sys.argv[1]); import e2elog; print(e2elog.now())' "$e2e")
now() { python3 -c 'import sys; sys.path.insert(0, sys.argv[1]); import e2elog; print(e2elog.now())' "$e2e"; }
# The run's log from this request's start on (e2elog.py), one line per
# event: time, the service that logged it (thumbnail|preview), the message.
log_from() {
  timeout 60 /usr/bin/log show --start "$e2e_start" --style ndjson --info \
    --predicate 'subsystem == "ch.exoma.stepv"' 2>/dev/null | python3 "$e2e/e2elog.py" "$1"
}
trap 'rm -rf "$QL_E2E_DIR"' EXIT
rc=0
"$e2e/supervisor.sh" "$built" || rc=1
for f in assembly.step box.igs box.brep; do
  for k in thumb preview; do
    "$e2e/ql-e2e.sh" "$k" "$root/tests/data/$f" || rc=1
  done
done
# Through the real thumbnail extension (#64): a file past its 15 s deadline
# (4000 distinct parts: ~30 s of kernel and 1.6 GB on an M-series Mac), with
# a second request beside it that must not wait for it, then the deadline
# kill with no kernel left, then the next request with no throttle.
bin="$root/target/ql-e2e"
huge="$QL_E2E_DIR/huge.step"
"$root/target/kernel/stress-gen" "$root/tests/data/assembly.step" 4000 "$huge" >/dev/null 2>&1 \
  || { echo "FAIL: stress-gen"; rc=1; }
echo "== thumb huge.step (past the deadline), and box.brep beside it"
since=$(now)
t0=$SECONDS
timeout 60 "$bin/ql-thumb" "$huge" 256 40 >"$QL_E2E_DIR/huge.txt" 2>&1 &
held=$!
logged() { log_from "$since" >"$QL_E2E_DIR/held.log"; grep -qE "$1" "$QL_E2E_DIR/held.log"; }
until logged $'\tthumbnail\t.*kernel pid [0-9]+ spawned'; do
  [ $((SECONDS - t0)) -le 20 ] || { echo "FAIL: huge.step's kernel did not start within 20 s"; rc=1; break; }
  sleep 0.5
done
# huge.step's own kernel: the first the thumbnail service spawned from its
# request on.
huge_pid=$(grep -E -m1 $'\tthumbnail\t.*kernel pid [0-9]+ spawned' "$QL_E2E_DIR/held.log" \
  | sed -E 's/.*kernel pid ([0-9]+) spawned.*/\1/')
# The thumbnail service's admission limit (#108), from its own start line.
limit=$(log_from "$e2e_start_iso" | awk -F'\t' '$2 == "thumbnail" && /admission limit/' \
  | grep -oE -m1 'admission limit [0-9]+' | awk '{print $3}')
echo "  thumbnail service admission limit: ${limit:-?}"
if [ "${limit:-0}" -ge 2 ]; then
  "$e2e/ql-e2e.sh" thumb "$root/tests/data/box.brep" || rc=1
  if kill -0 "$held" 2>/dev/null; then echo "ok: concurrent: box.brep drew while huge.step still ran"; else
    echo "FAIL: concurrent: huge.step was done before box.brep was"; rc=1; fi
  wait "$held"
elif [ "${limit:-0}" = 1 ]; then
  # One kernel at a time (CI's 7 GB runner): box.brep waits in the queue
  # until huge.step is killed at its deadline, then runs, or expires there,
  # never spawned. Either way no second kernel spawns before the kill.
  beside="$QL_E2E_DIR/beside-box.brep"
  cat "$root/tests/data/box.brep" >"$beside"
  timeout 60 "$bin/ql-thumb" "$beside" 256 40 >/dev/null 2>&1
  wait "$held"
  logged x || true
  if awk -F'\t' '$2 == "thumbnail"' "$QL_E2E_DIR/held.log" | python3 -c '
import re, sys
huge, queued, kill, spawns = sys.argv[1], False, None, []
for i, line in enumerate(sys.stdin):
    if re.search(r"queued, 1 kernel", line): queued = True
    if f"kernel pid {huge} killed" in line: kill = i
    if re.search(r"kernel pid \d+ spawned", line): spawns.append(i)
sys.exit(0 if queued and kill is not None and spawns[:1] and spawns[0] < kill
         and all(i > kill for i in spawns[1:]) else 1)' "${huge_pid:-none}"
  then echo "ok: admission limit 1: box.brep queued behind huge.step, spawned only after its kill"
  else echo "FAIL: admission limit 1: box.brep was not queued behind huge.step"; rc=1; fi
else
  echo "FAIL: the thumbnail service logged no admission limit"; rc=1
  wait "$held"
fi
held_s=$((SECONDS - t0))
sed 's/^/  /' "$QL_E2E_DIR/huge.txt"
until logged "thumbnail $huge: none:" || [ $((SECONDS - t0)) -gt 90 ]; do sleep 0.5; done
kill_line=$(grep -E -m1 $'\tthumbnail\t'".*kernel pid ${huge_pid:-none} killed \\(deadline\\)" "$QL_E2E_DIR/held.log" | cut -f3)
if [ -n "$kill_line" ] && ! grep -q "type=thumbnail" "$QL_E2E_DIR/huge.txt" && [ "$held_s" -lt 25 ]; then
  echo "ok: deadline: no thumbnail in ${held_s} s; ${kill_line#*: }"
else
  echo "FAIL: deadline: no kill logged, or a thumbnail, or ${held_s} s"; rc=1
fi
kpid=$(sed -E 's/.*kernel pid ([0-9]+) killed.*/\1/' <<<"$kill_line")
if [ -n "$kpid" ] && ! ps -p "$kpid" >/dev/null 2>&1; then echo "ok: no survivor: kernel pid $kpid is gone"; else
  echo "FAIL: the killed kernel ${kpid:-?} survives"; rc=1; fi
echo "== the next request, right after the kill"
"$e2e/ql-e2e.sh" thumb "$root/tests/data/assembly.step" >"$QL_E2E_DIR/next.txt" 2>&1 || rc=1
cat "$QL_E2E_DIR/next.txt"
ms=$(grep -oE 'in [0-9]+ ms' "$QL_E2E_DIR/next.txt" | head -1 | tr -dc 0-9)
if [ -n "$ms" ] && [ "$ms" -lt 3000 ]; then echo "ok: no throttle: ${ms} ms"; else
  echo "FAIL: throttle: the next thumbnail took ${ms:-?} ms"; rc=1; fi

# A preview closed while its kernel runs (#64): Quick Look releases the
# extension's view controller, which cancels the run, and the preview
# service kills that kernel. From the log, scoped to this request: the
# preview service's kernel for it is cancelled with SIGKILL within 3 s of
# the host's own logged close (at 8 s; the host quits at 14 s, so a kill at
# the quit cannot pass), reaped within 1 s of the cancel, and gone.
echo "== preview huge.step, closed at 8 s while its kernel runs"
closed="$QL_E2E_DIR/huge-closed.step"
cat "$huge" >"$closed"
since=$(now)
# open -W does not always block for this host: poll the log for the verdict.
timeout 60 open -n -W -a "$bin/ql-preview-host.app" --args "$closed" 14 8
t0=$SECONDS
while :; do
  verdict=$(log_from "$since" | python3 -c '
import re, sys
from datetime import datetime
closed_file = sys.argv[1]
spawn = closed = cancel = None
for line in sys.stdin:
    t, svc, m = line.rstrip("\n").split("\t", 2)
    t = datetime.fromisoformat(t)
    if m == f"preview host closed {closed_file}": closed = t
    s = re.search(r"kernel pid (\d+) spawned", m)
    if svc == "preview" and s and not spawn: spawn = s.group(1)
    c = re.search(r"cancelled, kernel pid (\d+) signal 9, reaped ([\d.]+) ms after the cancel", m)
    if svc == "preview" and c and c.group(1) == spawn: cancel = (t, float(c.group(2)))
if not spawn: print("FAIL no preview kernel spawned"); sys.exit()
if not closed: print("FAIL the host never logged its close"); sys.exit()
if not cancel: print(f"FAIL kernel pid {spawn} was never cancelled"); sys.exit()
after = (cancel[0] - closed).total_seconds()
ok = 0 <= after <= 3 and cancel[1] <= 1000
print(("ok " if ok else "FAIL ") + f"{spawn} cancelled {after:.2f} s after the close, reaped {cancel[1]:.1f} ms after the cancel")' "$closed")
  [[ "$verdict" == "FAIL kernel pid"*"never cancelled" || "$verdict" == "FAIL no preview"* || "$verdict" == "FAIL the host never"* ]] || break
  [ $((SECONDS - t0)) -lt 30 ] || break
  sleep 1
done
pid=$(awk '{print $2}' <<<"$verdict")
if [[ "$verdict" == ok* ]] && ! ps -p "$pid" >/dev/null 2>&1; then
  echo "ok: closed preview: kernel pid ${verdict#ok }, and gone"
else
  echo "FAIL: closed preview: $verdict"; rc=1
fi

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
