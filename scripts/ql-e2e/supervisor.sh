#!/usr/bin/env bash
# #64: the supervisor service, probed outside Quick Look. A sandboxed probe
# (macos/Tests/SupervisorProbe/main.swift) is placed in a COPY of the built
# preview extension, so it reaches that extension's real StepvSupervisor.xpc,
# the real bundled kernel and Frameworks, signed as built. Checks: a STEP, an
# IGES and a BREP run; events before the reply; a deadline kill beside a
# concurrent run; the next run with no throttle; the memory cap; cancel; a
# refused request. Then, from the service's own log, every kernel it spawned
# is gone. Needs no window session: it runs in CI.
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

start=$(date '+%Y-%m-%d %H:%M:%S')
echo "== supervisor probe: $service"
timeout 180 "$probe" "$service" "$tmp"/files/{assembly.step,box.igs,box.brep,slow.step}
rc=$?
[ $rc -eq 124 ] && echo "FAIL: the probe timed out"

# From the service's own log: it killed the deadline run and the capped
# one, and reaped them; it killed the cancelled one; and every kernel it
# spawned in this run is gone, killed or finished.
sleep 1
timeout 60 /usr/bin/log show --start "$start" --style ndjson --info \
  --predicate 'subsystem == "ch.exoma.stepv" AND category == "supervisor"' >"$tmp/log.ndjson" 2>/dev/null
python3 - "$tmp/log.ndjson" >"$tmp/log.txt" <<'PY'
import json, sys
for line in open(sys.argv[1]):
    try: print(json.loads(line).get("eventMessage", ""))
    except ValueError: pass
PY
for want in 'killed \(deadline\) at [0-9]+ ms, reaped in [0-9.]+ ms, gone=true' \
            'killed \(memory\) at [0-9]+ ms, reaped in [0-9.]+ ms, gone=true' \
            'cancelled, kernel pid [0-9]+ killed'; do
  if line=$(grep -E -m1 "$want" "$tmp/log.txt"); then
    echo "ok: logged: ${line#*: }"
  else
    echo "FAIL: the service never logged /$want/"
    rc=1
  fi
done
pids=$(grep -oE 'kernel pid [0-9]+ spawned' "$tmp/log.txt" | awk '{print $3}')
n=$(wc -w <<<"$pids" | tr -d ' ')
[ "$n" -eq 8 ] || { echo "FAIL: the service logged $n kernel spawns, expected 8"; rc=1; }
alive=0
for p in $pids; do ps -p "$p" >/dev/null 2>&1 && { echo "FAIL: kernel pid $p survived"; alive=1; }; done
if [ $alive -eq 0 ]; then echo "ok: all $n kernels the service spawned are gone"; else rc=1; fi
[ $rc -eq 0 ] && echo "supervisor: all checks passed" || echo "supervisor: FAILURES"
exit $rc
