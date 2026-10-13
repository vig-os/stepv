// supervisor-probe (#64): the supervisor service exercised outside Quick
// Look, through the same client code the extensions use. scripts/ql-e2e/
// supervisor.sh puts it, sandboxed, into a copy of a built extension, so its
// main bundle is that extension's and it reaches that extension's real
// StepvSupervisor.xpc, its real kernel and its real Frameworks.
//
//   supervisor-probe <service> <small.step> <box.igs> <box.brep> <slow.step>
//
// One line per check, "ok: …" or "FAIL: …"; exit 1 if any failed. The kernel
// pids the service used are in its log, for the caller to see they are gone.

import Foundation

let a = CommandLine.arguments
guard a.count == 6 else {
    print("usage: supervisor-probe <service> <small.step> <box.igs> <box.brep> <slow.step>")
    exit(2)
}
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

    init(_ path: String, _ format: String, deadlineMs: Int = 20_000, memoryMB: Int = 4096) {
        run = StepvSupervisorRun(serviceName: service, onEvent: { [unowned self] kind, _ in
            self.lock.lock(); self.kinds.append(kind)
            if self.result != nil { self.replyAfterEvents = false }
            self.lock.unlock()
        }, completion: { [unowned self] r in
            self.lock.lock(); self.result = r; self.ms = now() - self.t0; self.lock.unlock()
            self.done.signal()
        })
        t0 = now()
        run.start(file: URL(fileURLWithPath: path), format: format, linearRel: 0.001, angularDeg: 20,
                  deadlineMs: deadlineMs, memoryMB: memoryMB)
    }

    /// Waits for the reply; nil past `seconds`.
    func wait(_ seconds: Double = 30) -> StepvRunResult? {
        guard done.wait(timeout: .now() + seconds) == .success else { return nil }
        lock.lock(); defer { lock.unlock() }
        return result
    }
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
check(rb?.outcome == .done && slowStillRunning,
      "concurrent: \(String(format: "%.0f", beside.ms)) ms while the slow run went on — \(meshLine(rb))")
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

// 5. Cancel: the connection goes, the service kills the kernel, and no
//    completion follows.
let cancelled = Probe(a[5], "step")
usleep(500_000)
cancelled.run.cancel()
check(cancelled.wait(2) == nil, "cancel: no completion after cancel()")

// 6. A refused request: a format the kernel does not read.
let bad = Probe(a[2], "obj")
let rr = bad.wait()
check(rr?.outcome == .refused, "refused: \(rr.map { "\($0.outcome.rawValue) \($0.detail)" } ?? "no reply")")

exit(failed ? 1 : 0)
