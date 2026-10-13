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

# The service's log since $start, one message per line.
log_since() {
  timeout 60 /usr/bin/log show --start "$start" --style ndjson --info \
    --predicate 'subsystem == "ch.exoma.stepv" AND category == "supervisor"' 2>/dev/null \
    | python3 -c '
import json, sys
for line in sys.stdin:
    try: print(json.loads(line).get("eventMessage", ""))
    except ValueError: pass'
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
container_tmp="$HOME/Library/Containers/$service/Data/tmp"
rc=0

# 1. The service dies under a running kernel (SIGKILL: no cleanup of its
#    own runs): the kernel's lifeline closes and it exits within 1 s. Its
#    mesh stays behind until the next service sweeps it (step 2).
start=$(date '+%Y-%m-%d %H:%M:%S')
echo "== supervisor probe: $service"
"$probe" "$service" hold "$tmp/files/slow.step" >"$tmp/hold.txt" 2>&1 &
holder=$!
if await_log 'kernel pid [0-9]+ spawned' 20; then
  line=$(grep -E -m1 'kernel pid [0-9]+ spawned' "$tmp/log.txt")
  spid=$(sed -E 's/.*service pid ([0-9]+).*/\1/' <<<"$line")
  kpid=$(sed -E 's/.*kernel pid ([0-9]+) spawned.*/\1/' <<<"$line")
  sleep 1  # into the run: the kernel is reading
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
  echo "FAIL: the held run's kernel never started"
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
start=$(date '+%Y-%m-%d %H:%M:%S')
# The admission limit this machine gives the service (Admission), from the
# held run's service: the concurrency check depends on it.
limit=$(grep -oE -m1 'admission limit [0-9]+' "$tmp/log.txt" | awk '{print $3}')
[ -n "$limit" ] || { echo "FAIL: the service logged no admission limit"; rc=1; limit=1; }
echo "service admission limit: $limit"
timeout 300 "$probe" "$service" "$tmp"/files/{assembly.step,box.igs,box.brep,slow.step} "$limit" || rc=1
if await_log 'started: swept ([2-9]|[1-9][0-9]+) dead run' 20; then
  echo "ok: logged: $(grep -E -m1 'started: swept' "$tmp/log.txt")"
else
  echo "FAIL: the fresh service did not sweep step 1's and the planted dead runs"
  rc=1
fi

# 3. From the service's own log: it killed the deadline run and the capped
#    ones, and reaped them; the cancelled one died of its SIGKILL (signal 9,
#    not its own exit: slow.step takes seconds); and every kernel it spawned
#    in this run is gone, killed or finished.
await_log 'run #[0-9]+ .*cancelled' 20
for want in 'killed \(deadline\) at [0-9]+ ms, reaped in [0-9.]+ ms' \
            'killed \(memory\) at [0-9]+ ms, reaped in [0-9.]+ ms' \
            'cancelled, kernel pid [0-9]+ signal 9, reaped [0-9.]+ ms after the cancel'; do
  if line=$(grep -E -m1 "$want" "$tmp/log.txt"); then
    echo "ok: logged: ${line#*: }"
  else
    echo "FAIL: the service never logged /$want/"
    rc=1
  fi
done
pids=$(grep -oE 'kernel pid [0-9]+ spawned' "$tmp/log.txt" | awk '{print $3}')
n=$(wc -w <<<"$pids" | tr -d ' ')
[ "$n" -eq 9 ] || { echo "FAIL: the service logged $n kernel spawns, expected 9"; rc=1; }
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
start=$(date '+%Y-%m-%d %H:%M:%S')
# The probe waits on stdin until the cancelled run is seen queued: an
# ordering signal, not a sleep.
mkfifo "$tmp/go"
exec 9<>"$tmp/go"
timeout 120 "$tmp/limit1.app/Contents/PlugIns/StepvPreview.appex/Contents/MacOS/supervisor-probe" \
  "$service" admission "$tmp/files/slow.step" "$tmp/files/assembly.step" <"$tmp/go" &
probe_pid=$!
await_log 'run #2 \(service pid [0-9]+\): queued' 30 || { echo "FAIL: admission: run #2 was never queued"; rc=1; }
echo go >&9
wait "$probe_pid" || rc=1
exec 9>&-
log_since >"$tmp/log.txt"
order=$(python3 - "$tmp/log.txt" <<'PY'
import re, sys
lines = open(sys.argv[1]).read().splitlines()
limit = next((l for l in lines if "admission limit 1" in l), None)
pid = re.search(r"service pid (\d+)", limit).group(1) if limit else "?"
lines = [l for l in lines if f"service pid {pid}" in l]
runs = {}
for i, l in enumerate(lines):
    m = re.match(r"run #(\d+)[ :(]", l)
    if not m: continue
    r = runs.setdefault(int(m.group(1)), {})
    if re.search(r"kernel pid \d+ spawned", l): r["spawned"] = i
    if "queued" in l: r["queued"] = i
    if re.search(r"exit \d+ in \d+ ms|killed \(", l): r["ended"] = i
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

# The admission-time deadline check: a fourth copy, L = 1 and a 3 s hold on
# each handover (STEPV_TEST_HOOKS), so a queued run's deadline passes after
# its slot is handed over and before it starts.
ditto "$tmp/limit1.app" "$tmp/handover.app"
hxpc="$tmp/handover.app/Contents/PlugIns/StepvPreview.appex/Contents/XPCServices/StepvSupervisor.xpc"
echo 3000 >"$hxpc/Contents/Resources/admission-handover-delay-ms"
codesign --force --sign - --entitlements "$root/macos/supervisor.entitlements" "$hxpc" 2>/dev/null \
  || fail "signing the handover test supervisor"
timeout 60 "$tmp/handover.app/Contents/PlugIns/StepvPreview.appex/Contents/MacOS/supervisor-probe" \
  "$service" handover "$tmp/files/slow.step" "$tmp/files/assembly.step" || rc=1

# 4b. A live run survives another instance's sweep. Two client processes
#     each start their own instance of the service, in one container: B's
#     start-up sweep must keep A's running run, whose mesh then arrives.
start=$(date '+%Y-%m-%d %H:%M:%S')
"$probe" "$service" live "$tmp/files/slow.step" >"$tmp/live.txt" 2>&1 &
live=$!
if await_log 'kernel pid [0-9]+ spawned' 20; then
  apid=$(grep -E -m1 'kernel pid [0-9]+ spawned' "$tmp/log.txt" | sed -E 's/.*service pid ([0-9]+).*/\1/')
  timeout 60 "$tmp/standin.app/Contents/PlugIns/StepvPreview.appex/Contents/MacOS/supervisor-probe" \
    "$service" live "$tmp/files/frames.step" >/dev/null 2>&1
  log_since >"$tmp/log.txt"
  bline=$(grep -E 'started: swept' "$tmp/log.txt" | grep -v "service pid $apid " | head -1)
  if [ -z "$bline" ]; then
    echo "FAIL: no second instance of the service started beside pid $apid"
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

[ $rc -eq 0 ] && echo "supervisor: all checks passed" || echo "supervisor: FAILURES"
exit $rc
