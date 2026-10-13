#!/usr/bin/env bash
# #64: the supervisor service, probed outside Quick Look. A sandboxed probe
# (macos/Tests/SupervisorProbe/main.swift) is placed in a COPY of the built
# preview extension, so it reaches that extension's real StepvSupervisor.xpc,
# the real bundled kernel and Frameworks, signed as built. Checks:
#   - the service killed under a running kernel: the kernel follows it (its
#     lifeline) within 1 s;
#   - a STEP, an IGES and a BREP run; events before the reply; a deadline
#     kill beside a concurrent run; the next run with no throttle; the
#     memory cap, at 1 MB and at 256 MB; cancel; refused requests;
#   - from the service's own log, the kills, and every kernel it spawned gone;
#   - with KernelStandIn in place of the kernel, in a second copy: streams
#     the real kernel never writes on cue (macos/Tests/KernelStandIn);
#   - no mesh left in the service's container.
# Needs no window session.
#
#   scripts/ql-e2e/supervisor.sh [stepv.app]   (default target/macos/stepv.app)
set -uo pipefail
root=$(cd "$(dirname "$0")/../.." && pwd)
app=${1:-$root/target/macos/stepv.app}
kernel_dir=${STEPV_KERNEL_DIR:-$root/target/kernel}
[ -d "$app" ] || { echo "supervisor: no $app — run: just macos-app" >&2; exit 2; }
tmp=$(mktemp -d); tmp=$(cd -P "$tmp" && pwd)
trap 'rm -rf "$tmp"' EXIT
fail() { echo "FAIL: $*" >&2; exit 1; }

ditto "$app" "$tmp/stepv.app"
appex="$tmp/stepv.app/Contents/PlugIns/StepvPreview.appex"
service=$(/usr/libexec/PlistBuddy -c 'Print :CFBundleIdentifier' \
  "$appex/Contents/XPCServices/StepvSupervisor.xpc/Contents/Info.plist")

# The inputs, and a slow one: 1000 distinct copies of a part, ~5 s of kernel
# on an M-series Mac, against a 1.5 s deadline.
mkdir -p "$tmp/files"
cp "$root"/tests/data/{assembly.step,box.igs,box.brep} "$tmp/files/"
"$kernel_dir/stress-gen" "$root/tests/data/assembly.step" 1000 "$tmp/files/slow.step" >/dev/null 2>&1 \
  || fail "stress-gen"
# A run that holds its slot until the probe cancels it: 4000 parts, ~30 s of
# kernel, so no held run ends on its own while the order is checked (#111).
"$kernel_dir/stress-gen" "$root/tests/data/assembly.step" 4000 "$tmp/files/huge.step" >/dev/null 2>&1 \
  || fail "stress-gen"

# The probe: sandboxed, reading nothing but the inputs. A sandboxed command
# line tool traps (SIGTRAP) without an embedded Info.plist (#79).
cat > "$tmp/probe.plist" <<PLIST
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0"><dict>
	<key>CFBundleIdentifier</key><string>ch.exoma.stepv.supervisor-probe</string>
	<key>CFBundleName</key><string>supervisor-probe</string>
</dict></plist>
PLIST
cat > "$tmp/probe.entitlements" <<PLIST
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0"><dict>
	<key>com.apple.security.app-sandbox</key><true/>
	<key>com.apple.security.temporary-exception.files.absolute-path.read-only</key>
	<array><string>$tmp/files/</string></array>
</dict></plist>
PLIST
probe="$appex/Contents/MacOS/supervisor-probe"
env -i PATH=/usr/bin:/bin HOME="$HOME" TMPDIR="$tmp" /usr/bin/xcrun swiftc -O -swift-version 5 \
  -target "$(uname -m)-apple-macos14.0" \
  "$root"/macos/Shared/{StepvFrames,StepvSupervisor,StepvMesh}.swift "$root/macos/Tests/SupervisorProbe/main.swift" \
  -Xlinker -sectcreate -Xlinker __TEXT -Xlinker __info_plist -Xlinker "$tmp/probe.plist" \
  -o "$probe" || fail "building the probe"
codesign --force --sign - --entitlements "$tmp/probe.entitlements" "$probe" 2>/dev/null || fail "signing the probe"

# Bounds how much log is read: from this instant on ($start for log show,
# $start_iso for e2elog.py). Only a bound: runs are told apart by who they
# are (service pid, run number), never by when they logged (#111).
set_start() {
  start=$(date '+%Y-%m-%d %H:%M:%S')
  start_iso=$(python3 -c 'import sys; sys.path.insert(0, sys.argv[1]); import e2elog; print(e2elog.now())' "$root/scripts/ql-e2e")
}
log_since() {
  timeout 60 /usr/bin/log show --start "$start" --style ndjson --info \
    --predicate 'subsystem == "ch.exoma.stepv" AND category == "supervisor"' 2>/dev/null \
    | python3 -c '
import sys
sys.path.insert(0, sys.argv[1])
import e2elog
for e in e2elog.events(sys.stdin, sys.argv[2]):
    print(e["message"])' "$root/scripts/ql-e2e" "$start_iso"
}
# Polls the log until it holds a line matching $1 (ERE), at most $2 s.
await_log() {
  local end=$((SECONDS + $2))
  while [ $SECONDS -lt $end ]; do
    log_since >"$tmp/log.txt"
    grep -E -q "$1" "$tmp/log.txt" && return 0
    sleep 0.5
  done
  return 1
}
# Runs a probe that asks, on stdout, to wait for one of its runs in the log
# ("await <what> <run> <pid>": run #<run> of service pid <pid>; <what>:
# spawned, queued, held-deadline) and answers "seen" on its stdin once that
# exact line is there ("missing" after 30 s), or to release a run a test
# build holds ("release <run>"). Ordered on the logged event, scoped by
# identity: the service pid and the run number come from the service itself
# (StepvSupervisorRun.onAccepted), never from a time window (#111). The
# probe's output is also kept in $tmp/drive.out, for the checks after it.
drive() {
  rm -f "$tmp/sync" "$tmp/drive.out"; mkfifo "$tmp/sync"
  exec 8<>"$tmp/sync"
  "$@" <"$tmp/sync" | while IFS= read -r line; do
    case $line in
      "await "*)
        read -r _ what n pid <<<"$line"
        if await_log "$(log_pattern "$what" "$n" "$pid")" 30; then echo seen >&8; else echo missing >&8; fi ;;
      "release "*)
        : >"$container_tmp/release-${line#release }"
        echo released >&8 ;;
      *) echo "$line"; echo "$line" >>"$tmp/drive.out" ;;
    esac
  done
  local status=${PIPESTATUS[0]}
  exec 8>&-
  return "$status"
}
# The log line (ERE) for <what> of run #<n> of service pid <pid>: that run
# of that service alone. Every pid is anchored on both sides ("pid " before,
# ", " or ")" or " " after), so 4521 is never 45210's prefix.
log_pattern() {
  case $1 in
    spawned) echo "run #$2 \\(service pid $3, up [0-9]+ ms\\): kernel pid [0-9]+ spawned" ;;
    queued) echo "run #$2 \\(service pid $3\\): queued" ;;
    held-deadline) echo "run #$2 \\(service pid $3, up [0-9]+ ms\\): deadline passed while held" ;;
    started) echo "service pid $3 started: " ;;
    cancelled) echo "run #[0-9]+ \\(service pid $3, up [0-9]+ ms\\): cancelled, kernel pid" ;;
  esac
}
# The lines of service pid <pid> in file <f>, its alone (anchored as above).
owned_by() { grep -E "service pid $1[,):[:space:]]" "$2"; }
# Their own check, on a log whose only lines are a decoy's: another service,
# whose pid has this one's as a prefix (45210, 4521), with a line of every
# kind (#111's failure: another service's line taken for this one's).
printf '%s\n' 'service pid 45210 started: swept 0 dead run(s), kept 1 live, admission limit 1, x' \
  'run #6 (service pid 45210, up 5 ms): kernel pid 900 spawned in 0.4 ms' \
  'run #6 (service pid 45210): queued, 1 kernel(s) running' \
  'run #6 (service pid 45210, up 5 ms): deadline passed while held' \
  'run #6 (service pid 45210, up 5 ms): cancelled, kernel pid 900 signal 9, reaped 1.0 ms after the cancel' \
  'run #6 (service pid 45210): past its deadline in the queue, never spawned' >"$tmp/decoy.log"
for what in spawned queued held-deadline started cancelled; do
  grep -qE "$(log_pattern "$what" 6 4521)" "$tmp/decoy.log" && fail "log_pattern $what takes service pid 45210's line for 4521's"
  grep -qE "$(log_pattern "$what" 6 45210)" "$tmp/decoy.log" || fail "log_pattern $what misses service pid 45210's own line"
done
[ -z "$(owned_by 4521 "$tmp/decoy.log")" ] || fail "owned_by takes service pid 45210's lines for 4521's"
[ "$(owned_by 45210 "$tmp/decoy.log" | wc -l | tr -d ' ')" = 6 ] || fail "owned_by misses service pid 45210's own lines"
# The service pid the last driven probe reported ("service pid P").
driven_pid() { grep -oE -m1 'service pid [0-9]+' "$tmp/drive.out" 2>/dev/null | awk '{print $3}'; }
# Waits for a line matching $2 (ERE) in file $1, at most $3 s: what a probe
# in the background reports.
await_file() {
  local end=$((SECONDS + $3))
  until grep -qE "$2" "$1" 2>/dev/null; do [ $SECONDS -lt $end ] || return 1; sleep 0.2; done
}
container_tmp="$HOME/Library/Containers/$service/Data/tmp"
rc=0
run_start=$(date '+%Y-%m-%d %H:%M:%S')

# 1. The service dies under a running kernel (SIGKILL: no cleanup of its
#    own runs): the kernel's lifeline closes and it exits within 1 s. Its
#    mesh stays behind until the next service sweeps it (step 2).
set_start
echo "== supervisor probe: $service"
drive "$probe" "$service" hold "$tmp/files/huge.step" >"$tmp/hold.txt" 2>&1 &
holder=$!
if await_file "$tmp/hold.txt" '^held run [0-9]+ service pid [0-9]+' 40; then
  read -r _ _ hrun _ _ spid < <(grep -E -m1 '^held run' "$tmp/hold.txt")
  log_since >"$tmp/log.txt"
  kpid=$(grep -oE -m1 "$(log_pattern spawned "$hrun" "$spid")" "$tmp/log.txt" \
    | sed -E 's/.*kernel pid ([0-9]+) spawned/\1/')
  limit=$(grep -E "$(log_pattern started 0 "$spid")" "$tmp/log.txt" | grep -oE -m1 'admission limit [0-9]+' | grep -oE '[0-9]+$')
  [ -n "$kpid" ] || { echo "FAIL: no spawn logged for run #$hrun of service pid $spid"; kpid=0; rc=1; }
  # The huge file runs ~30 s: its kernel is surely still running now.
  kill -9 "$spid"
  t0=$(python3 -c 'import time; print(time.time())')
  while ps -p "$kpid" >/dev/null 2>&1; do
    [ "$(python3 -c "import time; print(int((time.time() - $t0) * 1000))")" -lt 3000 ] || break
    sleep 0.05
  done
  ms=$(python3 -c "import time; print(int((time.time() - $t0) * 1000))")
  if ps -p "$kpid" >/dev/null 2>&1; then
    echo "FAIL: kernel pid $kpid outlived its killed service by over 3 s"
    kill -9 "$kpid"
    rc=1
  elif [ "$ms" -le 1000 ]; then
    echo "ok: the service killed, its kernel pid $kpid was gone ${ms} ms later"
  else
    echo "FAIL: kernel pid $kpid outlived its killed service by ${ms} ms"
    rc=1
  fi
else
  echo "FAIL: the held run's kernel never started"; sed 's/^/  /' "$tmp/hold.txt"
  rc=1
fi
wait "$holder" 2>/dev/null

# 2. The probe proper, on a fresh service, which sweeps what dead runs left
#    behind: step 1's run directory, and one planted here, with its lock
#    free, a mesh and an IGES copy whose remover a teardown killed too (#102).
mkdir -p "$container_tmp/stepv-run-planted"
: >"$container_tmp/stepv-run-planted/lock"
: >"$container_tmp/stepv-run-planted/mesh.msh"
: >"$container_tmp/stepv-run-planted/stepv-input-planted.igs"
set_start
# The admission limit this machine gives the service (Admission), from the
# held run's service (step 1): the concurrency check depends on it.
[ -n "${limit:-}" ] || { echo "FAIL: the service logged no admission limit"; rc=1; limit=1; }
echo "service admission limit: $limit"
drive timeout 300 "$probe" "$service" "$tmp"/files/{assembly.step,box.igs,box.brep,slow.step,huge.step} "$limit" || rc=1
mpid=$(driven_pid)
if await_log "$(log_pattern started 0 "${mpid:-none}")swept ([2-9]|[1-9][0-9]+) dead run" 20; then
  echo "ok: logged: $(grep -E -m1 "$(log_pattern started 0 "${mpid:-none}")" "$tmp/log.txt")"
else
  echo "FAIL: the fresh service did not sweep step 1's and the planted dead runs"
  rc=1
fi

# 3. From the service's own log, its lines alone (service pid $mpid): it
#    killed the deadline run and the capped ones, and reaped them; the
#    cancelled one died of its SIGKILL (signal 9, not its own exit: the huge
#    file takes ~30 s); and every kernel it spawned is gone, killed or
#    finished. 10 spawns, 11 at L >= 2 (the deadline kill beside a run).
await_log "$(log_pattern cancelled 0 "${mpid:-none}")" 20
owned_by "${mpid:-none}" "$tmp/log.txt" >"$tmp/mine.txt"
for want in 'killed \(deadline\) at [0-9]+ ms, reaped in [0-9.]+ ms' \
            'killed \(memory\) at [0-9]+ ms, reaped in [0-9.]+ ms' \
            'cancelled, kernel pid [0-9]+ signal 9, reaped [0-9.]+ ms after the cancel'; do
  if line=$(grep -E -m1 "$want" "$tmp/mine.txt"); then
    echo "ok: logged: ${line#*: }"
  else
    echo "FAIL: the service never logged /$want/"
    rc=1
  fi
done
pids=$(grep -oE 'kernel pid [0-9]+ spawned' "$tmp/mine.txt" | awk '{print $3}')
n=$(wc -w <<<"$pids" | tr -d ' ')
want_n=$([ "$limit" -ge 2 ] && echo 11 || echo 10)
[ "$n" -eq "$want_n" ] || { echo "FAIL: the service logged $n kernel spawns, expected $want_n"; rc=1; }
alive=0
for p in $pids; do ps -p "$p" >/dev/null 2>&1 && { echo "FAIL: kernel pid $p survived"; alive=1; }; done
if [ $alive -eq 0 ]; then echo "ok: all $n kernels the service spawned are gone"; else rc=1; fi

# 4. The stand-in kernel, in a second copy of the app.
ditto "$tmp/stepv.app" "$tmp/standin.app"
sxpc="$tmp/standin.app/Contents/PlugIns/StepvPreview.appex/Contents/XPCServices/StepvSupervisor.xpc"
env -i PATH=/usr/bin:/bin /usr/bin/xcrun clang -O2 -target "$(uname -m)-apple-macos14.0" \
  "$root/macos/Tests/KernelStandIn/standin.c" -o "$sxpc/Contents/Helpers/stepv-occt" || fail "building the stand-in"
codesign --force --sign - --identifier ch.exoma.stepv.kernel \
  --entitlements "$root/macos/kernel.entitlements" "$sxpc/Contents/Helpers/stepv-occt" 2>/dev/null \
  || fail "signing the stand-in"
codesign --force --sign - --entitlements "$root/macos/supervisor.entitlements" "$sxpc" 2>/dev/null \
  || fail "signing the stand-in's service"
for m in frames late silent truncated noexit; do echo "$m" >"$tmp/files/$m.step"; done
# STANDIN_ROUNDS=N repeats it N times (ordering races show up as rare
# failures: events must all reach the client before its completion).
rounds=${STANDIN_ROUNDS:-1}
failed_rounds=0
for _ in $(seq "$rounds"); do
  out=$(timeout 120 "$tmp/standin.app/Contents/PlugIns/StepvPreview.appex/Contents/MacOS/supervisor-probe" \
    "$service" standin "$tmp"/files/{frames,late,silent,truncated,noexit}.step) \
    || { failed_rounds=$((failed_rounds + 1)); grep FAIL <<<"$out"; }
done
if [ "$rounds" -eq 1 ]; then
  echo "$out"
else
  echo "stand-in rounds: $rounds, failed: $failed_rounds"
fi
[ "$failed_rounds" -eq 0 ] || rc=1

# 4a. Admission (#64): a test build of the supervisor (STEPV_TEST_HOOKS,
#     never shipped) with its limit forced to 1, in a third copy: the second
#     of two runs spawns only once the first is reaped, and a run whose
#     deadline passes in the queue never spawns.
ditto "$tmp/stepv.app" "$tmp/limit1.app"
lxpc="$tmp/limit1.app/Contents/PlugIns/StepvPreview.appex/Contents/XPCServices/StepvSupervisor.xpc"
env -i PATH=/usr/bin:/bin HOME="$HOME" TMPDIR="$tmp" /usr/bin/xcrun swiftc -O -swift-version 5 \
  -target "$(uname -m)-apple-macos14.0" -module-name StepvSupervisor -application-extension \
  -D STEPV_TEST_HOOKS "$root"/macos/Shared/{StepvFrames,StepvSupervisor}.swift "$root/macos/Supervisor/main.swift" \
  -o "$lxpc/Contents/MacOS/StepvSupervisor" || fail "building the test supervisor"
mkdir -p "$lxpc/Contents/Resources"
echo 1 >"$lxpc/Contents/Resources/admission-limit"
codesign --force --sign - --entitlements "$root/macos/supervisor.entitlements" "$lxpc" 2>/dev/null \
  || fail "signing the test supervisor"
set_start
drive timeout 120 "$tmp/limit1.app/Contents/PlugIns/StepvPreview.appex/Contents/MacOS/supervisor-probe" \
  "$service" admission "$tmp/files/huge.step" "$tmp/files/assembly.step" || rc=1
log_since >"$tmp/log.txt"
# This probe's service's lines alone (owned_by: anchored), in log order.
owned_by "$(driven_pid)" "$tmp/log.txt" >"$tmp/mine.txt"
order=$(python3 - "$tmp/mine.txt" <<'PY'
import re, sys
lines = open(sys.argv[1]).read().splitlines()
limit = next((l for l in lines if " started: " in l and "admission limit 1" in l), None)
runs = {}
for i, l in enumerate(lines):
    m = re.match(r"run #(\d+)[ :(]", l)
    if not m: continue
    r = runs.setdefault(int(m.group(1)), {})
    if re.search(r"kernel pid \d+ spawned", l): r["spawned"] = i
    if "queued" in l: r["queued"] = i
    if re.search(r"exit \d+ in \d+ ms|killed \(|cancelled, kernel pid", l): r["ended"] = i
    if "cancelled in the queue, never spawned" in l: r["cancelled"] = r.get("cancelled", 0) + 1
    if "past its deadline in the queue, never spawned" in l: r["expired"] = i
first = min((n for n in runs if "spawned" in runs[n]), default=None)
a = runs.get(first, {})
cancelled = [n for n in runs if "cancelled" in runs[n]]
expired = [n for n in runs if "expired" in runs[n]]
behind = [n for n in runs if n not in (first, *cancelled, *expired)]
fails = []
if not limit or len(runs) != 4: fails.append(f"want 4 runs at L=1, got {sorted(runs)}")
if len(cancelled) != 1 or runs[cancelled[0]]["cancelled"] != 1 or "spawned" in runs[cancelled[0]]:
    fails.append(f"the cancelled run: {[runs[n] for n in cancelled]}")
if len(expired) != 1 or "spawned" in runs[expired[0]] or runs[expired[0]]["expired"] > a.get("ended", -1):
    fails.append(f"the expired run: {[runs[n] for n in expired]}, first ended at {a.get('ended')}")
if len(behind) != 1 or not runs[behind[0]].get("spawned", -1) > a.get("ended", 10**9):
    fails.append(f"the run behind: {[runs[n] for n in behind]}, first ended at {a.get('ended')}")
print("ok" if not fails else "FAIL " + "; ".join(fails))
PY
)
if [ "$order" = ok ]; then
  echo "ok: admission limit 1: a queued run cancelled once, never spawned; the run behind it admitted after the first; one withdrawn at its deadline while the first ran"
else
  echo "FAIL: admission order: $order"; rc=1
fi

# The admission-time deadline check: a fourth copy, L = 1, whose test build
# holds each admitted run until the test releases it (admission-hold): the
# run's deadline (2 s) passes while it is held, the service logs that, and
# only then is it released (drive), so it reaches the check past its
# deadline whatever the log's latency.
ditto "$tmp/limit1.app" "$tmp/handover.app"
hxpc="$tmp/handover.app/Contents/PlugIns/StepvPreview.appex/Contents/XPCServices/StepvSupervisor.xpc"
: >"$hxpc/Contents/Resources/admission-hold"
rm -f "$container_tmp"/release-*  # none left over from an earlier run: only this test's release counts
codesign --force --sign - --entitlements "$root/macos/supervisor.entitlements" "$hxpc" 2>/dev/null \
  || fail "signing the handover test supervisor"
set_start
drive timeout 60 "$tmp/handover.app/Contents/PlugIns/StepvPreview.appex/Contents/MacOS/supervisor-probe" \
  "$service" handover "$tmp/files/huge.step" "$tmp/files/assembly.step" || rc=1

# 4b. A live run survives another instance's sweep. Two client processes
#     each start their own instance of the service, in one container: B's
#     start-up sweep must keep A's running run, whose mesh then arrives.
set_start
"$probe" "$service" live "$tmp/files/slow.step" >"$tmp/live.txt" 2>&1 &
live=$!
# A's service, from A itself; its run is spawned once logged so.
if await_file "$tmp/live.txt" '^service pid [0-9]+' 20 \
   && apid=$(grep -oE -m1 'service pid [0-9]+' "$tmp/live.txt" | awk '{print $3}') \
   && await_log "$(log_pattern spawned 1 "$apid")" 20; then
  timeout 60 "$tmp/standin.app/Contents/PlugIns/StepvPreview.appex/Contents/MacOS/supervisor-probe" \
    "$service" live "$tmp/files/frames.step" >"$tmp/live-b.txt" 2>&1
  bpid=$(grep -oE -m1 'service pid [0-9]+' "$tmp/live-b.txt" | awk '{print $3}')
  log_since >"$tmp/log.txt"
  bline=$(grep -E -m1 "$(log_pattern started 0 "${bpid:-none}")" "$tmp/log.txt")
  if [ -z "$bpid" ] || [ "$bpid" = "$apid" ] || [ -z "$bline" ]; then
    echo "FAIL: no second instance of the service started beside pid $apid (got ${bpid:-none})"
    rc=1
  elif grep -qE 'kept [1-9]' <<<"$bline"; then
    echo "ok: a second instance beside pid $apid: ${bline#*: }"
  else
    echo "FAIL: the second instance kept no live run: $bline"
    rc=1
  fi
else
  echo "FAIL: the live run's kernel never started"
  rc=1
fi
wait "$live" || rc=1
cat "$tmp/live.txt"

# 5. Nothing left in the container: each run's directory removed when it
#    ends, and the killed service's and the planted one swept.
left=$(find "$container_tmp" -maxdepth 1 -name 'stepv-*' 2>/dev/null)
if [ -z "$left" ]; then echo "ok: no run directory left in $container_tmp"; else echo "FAIL: left in $container_tmp: $left"; rc=1; fi

if [ $rc -ne 0 ]; then
  # The evidence, for a failure on a machine no one can look at afterwards:
  # every supervisor line of the whole run.
  echo "  the supervisor log of this run:"
  timeout 60 /usr/bin/log show --start "$run_start" --style compact --info \
    --predicate 'subsystem == "ch.exoma.stepv" AND category == "supervisor"' 2>/dev/null \
    | tail -n 300 | sed 's/^/    /'
fi
[ $rc -eq 0 ] && echo "supervisor: all checks passed" || echo "supervisor: FAILURES"
exit $rc
