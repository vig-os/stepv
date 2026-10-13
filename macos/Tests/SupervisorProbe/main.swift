// supervisor-probe (#64): the supervisor service exercised outside Quick
// Look, through the same client code the extensions use. scripts/ql-e2e/
// supervisor.sh puts it, sandboxed, into a copy of a built extension, so its
// main bundle is that extension's and it reaches that extension's real
// StepvSupervisor.xpc, its real kernel and its real Frameworks.
//
//   supervisor-probe <service> <small.step> <box.igs> <box.brep> <slow.step> <L>
//                    L: the service's admission limit, from its log
//   supervisor-probe <service> hold <slow.step>    one run, then wait, for
//                    the caller to kill the service under it
//   supervisor-probe <service> admission <slow.step> <small.step>   against
//                    a service whose admission limit is 1 (a test build):
//                    the second run waits for the first, and a third whose
//                    deadline passes in the queue never spawns
//   supervisor-probe <service> live <slow.step>    one run that must finish
//                    with its mesh, while the caller starts a second
//                    instance of the service, whose sweep must spare it
//   supervisor-probe <service> standin <frames> <late> <silent> <truncated> <noexit>
//                    against KernelStandIn in place of the kernel
//
// One line per check, "ok: …" or "FAIL: …"; exit 1 if any failed. The kernel
// pids the service used are in its log, for the caller to see they are gone.

import Foundation

let a = CommandLine.arguments
let usage = "usage: supervisor-probe <service> <small.step> <box.igs> <box.brep> <slow.step> <L> | hold <slow.step> | live <slow.step> | admission <slow.step> <small.step> | standin <frames> <late> <silent> <truncated> <noexit>"
guard a.count >= 3 else { print(usage); exit(2) }
let service = a[1]
var failed = false
func check(_ ok: Bool, _ what: String) {
    print(ok ? "ok: \(what)" : "FAIL: \(what)")
    if !ok { failed = true }
}
func now() -> Double { Double(DispatchTime.now().uptimeNanoseconds) / 1e6 }

final class Probe {
    let lock = NSLock()
    var kinds: [UInt8] = []
    var result: StepvRunResult?
    var replyAfterEvents = true
    let done = DispatchSemaphore(value: 0)
    var run: StepvSupervisorRun!
    var t0 = 0.0, ms = 0.0

    init(_ path: String, _ format: String, deadlineMs: Int = 20_000, memoryMB: Int = 4096,
         linearRel: Double = 0.001, angularDeg: Double = 20) {
        run = StepvSupervisorRun(serviceName: service, onEvent: { [unowned self] kind, _ in
            self.lock.lock(); self.kinds.append(kind)
            if self.result != nil { self.replyAfterEvents = false }
            self.lock.unlock()
        }, completion: { [unowned self] r in
            self.lock.lock(); self.result = r; self.ms = now() - self.t0; self.lock.unlock()
            self.done.signal()
        })
        t0 = now()
        run.start(file: URL(fileURLWithPath: path), format: format, linearRel: linearRel, angularDeg: angularDeg,
                  deadlineMs: deadlineMs, memoryMB: memoryMB)
    }

    /// Waits for the reply; nil past `seconds`.
    func wait(_ seconds: Double = 30) -> StepvRunResult? {
        guard done.wait(timeout: .now() + seconds) == .success else { return nil }
        lock.lock(); defer { lock.unlock() }
        return result
    }
}

func outcomeLine(_ r: StepvRunResult?) -> String {
    r.map { "\($0.outcome.rawValue) \($0.detail)" } ?? "no reply"
}

switch (a[2], a.count) {
case ("hold", 4):
    // The caller kills the service while this run's kernel works.
    let held = Probe(a[3], "step", deadlineMs: 60_000)
    print("hold: \(outcomeLine(held.wait(60)))")
    exit(0)
case ("admission", 5):
    let first = Probe(a[3], "step", deadlineMs: 30_000)
    usleep(300_000)
    let second = Probe(a[4], "step", deadlineMs: 30_000)
    let third = Probe(a[4], "step", deadlineMs: 1000)
    let r3 = third.wait(10)
    check(r3?.outcome == .deadline && r3?.detail.contains("never spawned") == true,
          "admission: a request past its deadline in the queue never spawns: \(outcomeLine(r3))")
    let r1 = first.wait(40), r2 = second.wait(40)
    // When each ended: the second, queued, ends after the first.
    let end1 = first.t0 + first.ms, end2 = second.t0 + second.ms
    check(r1?.outcome == .done && r2?.outcome == .done && end2 > end1,
          "admission: the second run ended \(Int(end2 - end1)) ms after the first: \(outcomeLine(r2))")
    exit(failed ? 1 : 0)
case ("live", 4):
    let p = Probe(a[3], "step", deadlineMs: 60_000)
    let r = p.wait(60)
    check(r?.outcome == .done && r?.mesh != nil,
          "a live run beside a second instance's sweep: \(meshLine(r))")
    exit(failed ? 1 : 0)
case ("standin", 8):
    for (path, what) in [(a[3], "frames then exit"), (a[4], "exits before its frames arrive")] {
        let p = Probe(path, "step")
        let r = p.wait()
        check(r?.outcome == .done && r?.summary?["sandbox"] as? String == "stand-in" && p.kinds == [1, 2],
              "stand-in \(what): the stream drained (events \(p.kinds)) — \(outcomeLine(r))")
    }
    let rs = Probe(a[5], "step").wait()
    check(rs?.outcome == .crashed && rs?.detail.contains("without its summary") == true,
          "stand-in exits silent: no summary is a crash — \(outcomeLine(rs))")
    let rt = Probe(a[6], "step").wait()
    check(rt?.outcome == .crashed && rt?.detail.contains("partway") == true,
          "stand-in half a frame: a crash — \(outcomeLine(rt))")
    let rn = Probe(a[7], "step", deadlineMs: 1000).wait()
    check(rn?.outcome == .deadline, "stand-in closes stdout, never exits: the deadline ends it — \(outcomeLine(rn))")
    exit(failed ? 1 : 0)
case (_, 7):
    break
default:
    print(usage); exit(2)
}

func meshLine(_ r: StepvRunResult?) -> String {
    guard let r else { return "no reply in time" }
    guard let mesh = r.mesh, let scene = try? decodeStepvMesh(mesh) else {
        return "\(r.outcome.rawValue) \(r.detail) — no mesh: \(r.summary?["error"] ?? "-")"
    }
    return "\(scene.parts.count) part(s), \(scene.parts.reduce(0) { $0 + $1.faceIds.count }) triangles — \(r.detail)"
}

// 1. A small STEP file: the tree, then progress, then the reply with a mesh;
//    the kernel ran under the service's sandbox, not its own.
let small = Probe(a[2], "step")
let r1 = small.wait()
check(r1?.outcome == .done && r1?.mesh != nil, "step: \(meshLine(r1))")
check(small.kinds.first == StepvEvent.tree && small.kinds.contains(StepvEvent.progress) && small.replyAfterEvents,
      "step: events \(small.kinds) (TREE first, PROGRESS, all before the reply)")
check(r1?.summary?["sandbox"] as? String == "macos-outer",
      "step: kernel sandbox \(r1?.summary?["sandbox"] ?? "-") (macos-outer: the service's own)")

// 2. IGES and BREP: no tree, still a mesh.
for (path, format) in [(a[3], "iges"), (a[4], "brep")] {
    let p = Probe(path, format)
    let r = p.wait()
    check(r?.outcome == .done && r?.mesh != nil && !p.kinds.contains(StepvEvent.tree), "\(format): \(meshLine(r))")
}

// 3. A deadline kill while a second run goes on beside it, then a third
//    right after the kill: no throttle, and the runs are independent.
let slow = Probe(a[5], "step", deadlineMs: 1500)
usleep(200_000)
let beside = Probe(a[2], "step")
let rb = beside.wait()
slow.lock.lock(); let slowStillRunning = slow.result == nil; slow.lock.unlock()
if Int(a[6]) ?? 1 >= 2 {
    check(rb?.outcome == .done && slowStillRunning,
          "concurrent: \(String(format: "%.0f", beside.ms)) ms while the slow run went on — \(meshLine(rb))")
} else {
    // One kernel at a time (Admission): the second waits for the first.
    check(rb?.outcome == .done && !slowStillRunning,
          "admission limit 1: the second run was served after the first ended, in \(String(format: "%.0f", beside.ms)) ms — \(meshLine(rb))")
}
let rs = slow.wait()
check(rs?.outcome == .deadline, "deadline: \(rs.map { "\($0.outcome.rawValue) \($0.detail)" } ?? "no reply") in \(String(format: "%.0f", slow.ms)) ms")
let after = Probe(a[2], "step")
let ra = after.wait()
check(ra?.outcome == .done && after.ms < 2000,
      "next run right after the kill: \(String(format: "%.0f", after.ms)) ms — \(meshLine(ra))")

// 4. The memory cap: 1 MB is past the moment the kernel starts.
let capped = Probe(a[2], "step", memoryMB: 1)
let rc = capped.wait()
check(rc?.outcome == .memory, "memory cap: \(rc.map { "\($0.outcome.rawValue) \($0.detail)" } ?? "no reply")")

// A realistic cap: 256 MB against the slow file's ~500 MB peak.
let capped256 = Probe(a[5], "step", memoryMB: 256)
let rc256 = capped256.wait()
check(rc256?.outcome == .memory, "memory cap 256 MB on a ~500 MB run: \(outcomeLine(rc256))")

// 5. Cancel: the connection goes, the service kills the kernel, and no
//    completion follows.
let cancelled = Probe(a[5], "step")
usleep(500_000)
cancelled.run.cancel()
check(cancelled.wait(2) == nil, "cancel: no completion after cancel()")

// 6. Refused before anything runs: every run is bounded and well formed.
for (what, p) in [
    ("format obj", { Probe(a[2], "obj") }),
    ("deadlineMs 0", { Probe(a[2], "step", deadlineMs: 0) }),
    ("deadlineMs -1", { Probe(a[2], "step", deadlineMs: -1) }),
    ("deadlineMs Int.max", { Probe(a[2], "step", deadlineMs: Int.max) }),
    ("memoryMB 0", { Probe(a[2], "step", memoryMB: 0) }),
    ("memoryMB Int.max", { Probe(a[2], "step", memoryMB: Int.max) }),
    ("linearRel NaN", { Probe(a[2], "step", linearRel: .nan) }),
    ("linearRel -1", { Probe(a[2], "step", linearRel: -1) }),
    ("linearRel 2", { Probe(a[2], "step", linearRel: 2) }),
    ("angularDeg 0", { Probe(a[2], "step", angularDeg: 0) }),
    ("angularDeg infinity", { Probe(a[2], "step", angularDeg: .infinity) }),
] as [(String, () -> Probe)] {
    let r = p().wait()
    check(r?.outcome == .refused, "refused \(what): \(outcomeLine(r))")
}

exit(failed ? 1 : 0)
