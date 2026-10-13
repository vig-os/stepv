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

# 2. The probe proper, on a fresh service (which sweeps step 1's mesh).
start=$(date '+%Y-%m-%d %H:%M:%S')
timeout 300 "$probe" "$service" "$tmp"/files/{assembly.step,box.igs,box.brep,slow.step} || rc=1

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
timeout 120 "$tmp/standin.app/Contents/PlugIns/StepvPreview.appex/Contents/MacOS/supervisor-probe" \
  "$service" standin "$tmp"/files/{frames,late,silent,truncated,noexit}.step || rc=1

# 5. No mesh left in the container: unlinked on every way out, and swept
#    after the killed service.
left=$(find "$container_tmp" -name 'stepv-*.msh' 2>/dev/null | wc -l | tr -d ' ')
if [ "$left" = 0 ]; then echo "ok: no mesh left in $container_tmp"; else echo "FAIL: $left mesh(es) left in $container_tmp"; rc=1; fi

[ $rc -eq 0 ] && echo "supervisor: all checks passed" || echo "supervisor: FAILURES"
exit $rc
