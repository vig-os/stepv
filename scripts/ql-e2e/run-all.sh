#!/usr/bin/env bash
# #64: ql-e2e's whole matrix. Exit 0 if every check passes.
#   1. supervisor.sh: the supervisor service, probed outside Quick Look.
#   2. Quick Look itself runs the installed extensions: a thumbnail and a
#      preview of a small STEP file, an IGES and a BREP file.
#   3. Through them, the supervisor's limits: a thumbnail past its deadline
#      with another beside it, the next one unthrottled, and a preview
#      closed, and one switched to another file, while its kernel runs.
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
# stepv's log, the extensions' and the supervisor's, as logd pushes it: one
# `log stream` for the whole run, started before the first request, read
# from its file by every check (QL_E2E_STREAM). Not `log show`, polled per
# request: each check finds its lines by who they are (a request's path, a
# service pid), never by a window opened at a wall-clock time, which the
# log's own timestamps do not keep to (e2elog.py, #111).
QL_E2E_STREAM="$QL_E2E_DIR/stream.ndjson"; export QL_E2E_STREAM
/usr/bin/log stream --style ndjson --level info \
  --predicate 'subsystem == "ch.exoma.stepv"' >"$QL_E2E_STREAM" 2>/dev/null &
streamer=$!
trap 'kill "$streamer" 2>/dev/null; rm -rf "$QL_E2E_DIR"' EXIT
# The stream prints a header line once it is filtering: from then on no line
# is missed.
end=$((SECONDS + 30))
until [ -s "$QL_E2E_STREAM" ]; do
  [ $SECONDS -lt $end ] || { echo "ql-e2e: log stream did not start within 30 s" >&2; exit 2; }
  sleep 0.1
done
rc=0
# The identity checks below, on made-up streams first.
python3 "$e2e/runs_test.py" >"$QL_E2E_DIR/runs_test.txt" \
  || { echo "FAIL: runs.py's own check:"; grep FAIL "$QL_E2E_DIR/runs_test.txt"; rc=1; }
"$e2e/supervisor.sh" "$built" || rc=1
for f in assembly.step box.igs box.brep; do
  for k in thumb preview; do
    "$e2e/ql-e2e.sh" "$k" "$root/tests/data/$f" || rc=1
  done
done
# The checks below find a request's run in the supervisor service by
# identity (runs.py): the run and service pid its extension logged as
# accepted for that file, then that run's own lines, in stream order.
runs() { python3 "$e2e/runs.py" "$@" <"$QL_E2E_STREAM"; }
# runs.py <args> until it passes, at most $1 s; its last output either way.
await_runs() {
  local end=$((SECONDS + $1)) out; shift
  until out=$(runs "$@"); do
    [ $SECONDS -lt $end ] || { echo "$out"; return 1; }
    sleep 0.2
  done
  echo "$out"
}
bin="$root/target/ql-e2e"

# Through the real thumbnail extension (#64): a file past its 15 s deadline
# (4000 distinct parts: ~30 s of kernel and 1.6 GB on an M-series Mac), with
# box.brep beside it, which must not wait for it where the service admits
# two kernels; then the deadline kill with no kernel left, then the next
# request with no throttle.
huge="$QL_E2E_DIR/huge.step"
"$root/target/kernel/stress-gen" "$root/tests/data/assembly.step" 4000 "$huge" >/dev/null 2>&1 \
  || { echo "FAIL: stress-gen"; rc=1; }
echo "== thumb huge.step (past its deadline), and box.brep beside it"
timeout 60 "$bin/ql-thumb" "$huge" 256 40 >"$QL_E2E_DIR/huge.txt" 2>&1 &
held=$!
# box.brep goes once huge.step's own kernel is logged spawned.
huge_kernel=$(await_runs 30 spawned thumbnail "$huge") \
  || { echo "FAIL: huge.step's kernel was not seen spawned: $huge_kernel"; rc=1; }
# The admission limit (#108) of the service that took huge.step.
limit=$(runs limit thumbnail "$huge") || { echo "FAIL: $limit"; rc=1; limit=0; }
echo "  thumbnail service admission limit: $limit"
beside="$QL_E2E_DIR/beside-box.brep"
cat "$root/tests/data/box.brep" >"$beside"
if [ "$limit" -ge 2 ]; then
  QL_E2E_REUSE=1 "$e2e/ql-e2e.sh" thumb "$beside" || rc=1
elif [ "$limit" = 1 ]; then
  # One kernel at a time (CI's 7 GB runner): box.brep waits in the queue
  # until huge.step's kill, then runs or expires there; it need not draw.
  timeout 60 "$bin/ql-thumb" "$beside" 256 40 >/dev/null 2>&1
fi
wait "$held"
sed 's/^/  /' "$QL_E2E_DIR/huge.txt"
if dl=$(await_runs 30 deadline "$huge") && ! grep -q "type=thumbnail" "$QL_E2E_DIR/huge.txt"; then
  echo "ok: deadline: no thumbnail; kernel pid $dl"
  if ps -p "${dl%% *}" >/dev/null 2>&1; then echo "FAIL: the killed kernel ${dl%% *} survives"; rc=1; else
    echo "ok: no survivor: kernel pid ${dl%% *} is gone"; fi
else
  echo "FAIL: deadline: $dl, or a thumbnail came"; rc=1
fi
if [ "$limit" -ge 2 ]; then
  if out=$(runs concurrent "$huge" "$beside"); then echo "ok: concurrent: $out"; else
    echo "FAIL: concurrent: $out"; rc=1; fi
elif [ "$limit" = 1 ]; then
  if out=$(runs queued "$huge" "$beside"); then echo "ok: admission limit 1: $out"; else
    echo "FAIL: admission limit 1: $out"; rc=1; fi
fi
echo "== the next request, right after the kill"
"$e2e/ql-e2e.sh" thumb "$root/tests/data/assembly.step" >"$QL_E2E_DIR/next.txt" 2>&1 || rc=1
cat "$QL_E2E_DIR/next.txt"
ms=$(grep -oE 'in [0-9]+ ms' "$QL_E2E_DIR/next.txt" | head -1 | tr -dc 0-9)
if [ -n "$ms" ] && [ "$ms" -lt 3000 ]; then echo "ok: no throttle: ${ms} ms"; else
  echo "FAIL: throttle: the next thumbnail took ${ms:-?} ms"; rc=1; fi

# A preview that goes while its kernel runs (#64): Quick Look releases the
# extension's view controller, which cancels its run, and the preview
# service kills that kernel. Quick Look holds the controller until the
# preview's handler is called, so a ready preview is closed: the host acts
# on a signal sent once the file's ready line is in the stream, never at a
# set time (#113's CI: a tree that came after a fixed 8 s close). The host
# logs its act and its quit, 3 s later: the cancel must fall between them,
# so the kill at the quit cannot pass; reaped within 1 s; and gone. No
# result is shown for the file, and its handler completed exactly once.
#   $1: closed|switched; $2: the file; $3: the file switched to.
preview_goes() {
  local how=$1 file=$2 to=${3:-} from out host
  open -n -a "$bin/ql-preview-host.app" --args "$file" 90 signal ${to:+"$to"}
  host=$(await_runs 30 first "preview host shows $file as pid ") \
    || { echo "FAIL: $how preview: the host did not log its pid"; rc=1; return; }
  host=${host##* }
  if ready=$(await_runs 60 first "preview $file: ready ("); then
    kill -USR1 "$host"
  else
    echo "FAIL: $how preview: never ready"; rc=1
  fi
  await_runs 100 logged "preview host quits $file" >/dev/null \
    || { echo "FAIL: $how preview: the host never logged its quit"; rc=1; return; }
  from="preview host closed $file"
  [ -z "$to" ] || from="preview host switched $file to $to"
  if out=$(runs cancelled "$file" "$from" "preview host quits $file") && ! ps -p "${out%% *}" >/dev/null 2>&1; then
    echo "ok: $how preview: ${ready##*: }, then kernel pid $out, and gone"
  else
    echo "FAIL: $how preview: $out"; rc=1
  fi
  # Nothing of it shown after, and Quick Look's handler completed once.
  if out=$(runs gone "$file"); then echo "ok: $how preview: $out"; else
    echo "FAIL: $how preview: $out"; rc=1; fi
}
echo "== preview huge.step, closed once ready, while its kernel runs"
cat "$huge" >"$QL_E2E_DIR/huge-closed.step"
preview_goes closed "$QL_E2E_DIR/huge-closed.step"
# Closed before it is ready: Quick Look keeps the controller until its
# handler is called, so the run is cancelled at the preview's ready (its
# tree, mesh or failure, or StepvKernel.previewHandlerAfter, 20 s, at the
# latest), not at the close (#122). The host closes once the file's kernel
# is spawned, and stays up 40 s: the close must come before the file's
# ready line, and the cancel after it, before the quit.
echo "== preview huge.step, closed before it is ready"
early="$QL_E2E_DIR/huge-early.step"
cat "$huge" >"$early"
open -n -a "$bin/ql-preview-host.app" --args "$early" 40 signal-stay
ready='' out=''
if host=$(await_runs 30 first "preview host shows $early as pid ") \
   && await_runs 30 spawned preview "$early" >/dev/null && kill -USR1 "${host##* }" \
   && await_runs 50 logged "preview host quits $early" >/dev/null \
   && ready=$(runs first "preview $early: ready (") \
   && runs order "preview host closed $early" "$ready" \
   && out=$(runs cancelled "$early" "$ready" "preview host quits $early") \
   && ! ps -p "${out%% *}" >/dev/null 2>&1 && gone=$(runs gone "$early"); then
  echo "ok: closed before ready: ${ready##*: }, then kernel pid $out, and gone; $gone"
else
  echo "FAIL: closed before ready: ${ready:-never ready}; ${out:-}; $(runs gone "$early")"; rc=1
fi
# The previewed item changed under a live run: Quick Look prepares the next
# file on a new controller (or, should it ever, on the same one, which then
# cancels the run it replaces): either way the old kernel goes at the switch.
echo "== preview huge.step, switched to box.brep once ready, while its kernel runs"
cat "$huge" >"$QL_E2E_DIR/huge-switched.step"
cat "$root/tests/data/box.brep" >"$QL_E2E_DIR/switched-to-box.brep"
preview_goes switched "$QL_E2E_DIR/huge-switched.step" "$QL_E2E_DIR/switched-to-box.brep"
echo "  controller: $(runs reused "$QL_E2E_DIR/huge-switched.step")"
python3 "$e2e/verdict.py" preview "$QL_E2E_DIR/switched-to-box.brep" <"$QL_E2E_STREAM" \
  || { echo "FAIL: switched preview: box.brep was not shown"; rc=1; }

# The control: one file thumbnailed twice, the second time through a hard
# link: a new path, the same inode, which Quick Look's cache is keyed on. It
# answers from the cache, the extension never sees the link's path, and the
# verdict, which goes by the path, must say so and fail: proof that a cached
# thumbnail can never pass for the extension's. By identity, not by time
# (#111).
cat "$root/tests/data/box.brep" >"$QL_E2E_DIR/cache-control.brep"
QL_E2E_REUSE=1 "$e2e/ql-e2e.sh" thumb "$QL_E2E_DIR/cache-control.brep" >/dev/null 2>&1 \
  || { echo "FAIL: cache control: the first request did not pass"; rc=1; }
ln "$QL_E2E_DIR/cache-control.brep" "$QL_E2E_DIR/cache-control-link.brep"
out=$(QL_E2E_REUSE=1 "$e2e/ql-e2e.sh" thumb "$QL_E2E_DIR/cache-control-link.brep" 2>&1) \
  && { echo "FAIL: cache control: a cached thumbnail passed"; rc=1; }
if grep -q "QL served a cached thumbnail" <<<"$out"; then
  echo "ok: cache control: a repeated request is caught as a cached thumbnail"
else
  echo "FAIL: cache control: the repeated request was not caught as cached:"; echo "$out"; rc=1
fi
echo "== ql-e2e: $([ $rc = 0 ] && echo ALL PASS || echo FAILURES)"
exit $rc
