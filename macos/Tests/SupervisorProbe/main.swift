// supervisor-probe (#64): the supervisor service exercised outside Quick
// Look, through the same client code the extensions use. scripts/ql-e2e/
// supervisor.sh puts it, sandboxed, into a copy of a built extension, so its
// main bundle is that extension's and it reaches that extension's real
// StepvSupervisor.xpc, its real kernel and its real Frameworks.
//
//   supervisor-probe <service> <small.step> <box.igs> <box.brep> <slow.step> <huge.step> <L>
//                    L: the service's admission limit, from its log
//   supervisor-probe <service> hold <slow.step>    one run, then wait, for
//                    the caller to kill the service under it
//   supervisor-probe <service> admission <slow.step> <small.step>   against
//                    a service whose admission limit is 1 (a test build):
//                    a queued run cancelled, the next admitted after the
//                    first, and one withdrawn at its deadline from the queue
//   supervisor-probe <service> handover <slow.step> <small.step>   against a
//                    test build with L = 1 and a held handover
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
let usage = "usage: supervisor-probe <service> <small.step> <box.igs> <box.brep> <slow.step> <huge.step> <L> | hold <slow.step> | live <slow.step> | admission|handover <slow.step> <small.step> | standin <frames> <late> <silent> <truncated> <noexit>"
guard a.count >= 3 else { print(usage); exit(2) }
let service = a[1]
var failed = false
func check(_ ok: Bool, _ what: String) {
    print(ok ? "ok: \(what)" : "FAIL: \(what)")
    if !ok { failed = true }
}
func now() -> Double { Double(DispatchTime.now().uptimeNanoseconds) / 1e6 }

/// Asks supervisor.sh, on stdout, to wait until the service has logged
/// `what` for run #`run` of service pid `pid` ("spawned": its kernel
/// spawned; "queued": it was queued; "held-deadline": its deadline passed
/// while a test build held it), and to answer on stdin. Ordering on the
/// logged event itself, never on time, nor on a first event a short run may
/// never send (#111); scoped by identity, never by a time window.
func awaitLog(_ what: String, run: Int, pid: Int32) -> Bool {
    print("await \(what) \(run) \(pid)"); fflush(stdout)
    return readLine() == "seen"
}

/// Asks supervisor.sh to release run #`run` held by a test build
/// (release-N in the service's tmp).
func release(_ run: Int) {
    print("release \(run)"); fflush(stdout)
    _ = readLine()
}

final class Probe {
    let lock = NSLock()
    var kinds: [UInt8] = []
    var result: StepvRunResult?
    var replyAfterEvents = true
    let done = DispatchSemaphore(value: 0)
    let taken = DispatchSemaphore(value: 0)
    var run: StepvSupervisorRun!
    /// Who the run is in the service's log, from the service itself.
    var number = 0, servicePid: Int32 = 0
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
            self.taken.signal()  // a refused run is never accepted: do not wait for it
        })
        run.onAccepted = { [unowned self] n, pid in
            self.lock.lock(); (self.number, self.servicePid) = (n, pid); self.lock.unlock()
            self.taken.signal()
        }
        t0 = now()
        run.start(file: URL(fileURLWithPath: path), format: format, linearRel: linearRel, angularDeg: angularDeg,
                  deadlineMs: deadlineMs, memoryMB: memoryMB)
    }

    /// Waits until the service has taken the run: its number and pid.
    /// False if it replied without taking it, or not within `seconds`.
    func awaitTaken(_ seconds: Double = 20) -> Bool {
        guard taken.wait(timeout: .now() + seconds) == .success else { return false }
        lock.lock(); defer { lock.unlock() }
        return number > 0 && servicePid > 0
    }

    /// Waits until the service has logged `what` for this run.
    func awaitLogged(_ what: String) -> Bool {
        awaitTaken() && awaitLog(what, run: number, pid: servicePid)
    }

    /// Waits until the service has logged this run's kernel as spawned. Not
    /// seen: says what became of the run, for the log of a failure no one
    /// can look at afterwards.
    func awaitSpawned() -> Bool {
        if awaitLogged("spawned") { return true }
        lock.lock(); let r = result; lock.unlock()
        print("  run #\(number) of service pid \(servicePid) not seen spawned; its reply so far: \(outcomeLine(r))")
        return false
    }

    var isDone: Bool { lock.lock(); defer { lock.unlock() }; return result != nil }

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
    // The caller kills the service while this run's kernel works: it is
    // told who the run is ("held run N service pid P") once its kernel is
    // logged spawned.
    let held = Probe(a[3], "step", deadlineMs: 60_000)
    if held.awaitSpawned() {
        print("held run \(held.number) service pid \(held.servicePid)"); fflush(stdout)
    }
    print("hold: \(outcomeLine(held.wait(60)))")
    exit(0)
case ("admission", 5):
    // L = 1 (a test build). A holds the slot; B queues and is cancelled
    // there once the service has logged it queued; C queues behind it and
    // must be admitted next; D queues with a 1 s deadline and must be
    // withdrawn by its deadline while A still runs.
    let a1 = Probe(a[3], "step", deadlineMs: 30_000)
    check(a1.awaitSpawned(), "admission: the first run's kernel spawned")
    print("service pid \(a1.servicePid)")  // supervisor.sh scopes its log checks by it
    let b = Probe(a[4], "step", deadlineMs: 30_000)
    check(b.awaitLogged("queued"), "admission: the second run is queued")
    b.run.cancel()
    let c = Probe(a[4], "step", deadlineMs: 30_000)
    let d = Probe(a[4], "step", deadlineMs: 1000)
    let rd = d.wait(10)
    check(rd?.outcome == .deadline && rd?.detail.contains("in the queue, never spawned") == true && !a1.isDone,
          "admission: withdrawn at its deadline from the queue while the first still runs: \(outcomeLine(rd))")
    // A (the huge file) holds the slot until it is cancelled here: no run
    // ends on its own while the order is being checked.
    let freed = now()
    a1.run.cancel()
    let rc = c.wait(40)
    check(rc?.outcome == .done && c.t0 + c.ms > freed,
          "admission: the run behind the cancelled one was served once the first ended: \(outcomeLine(rc))")
    check(b.wait(1) == nil, "admission: no completion for the cancelled run")
    exit(failed ? 1 : 0)
case ("handover", 5):
    // A test build holds each admitted run before it starts until the test
    // releases it. B (deadline 2 s) is held; once the service logs that its
    // deadline passed while held, the test releases it, and the
    // admission-time check refuses it, never spawned: ordered on the
    // service's own events, not on a hold of some length.
    let b = Probe(a[4], "step", deadlineMs: 2000)
    check(b.awaitLogged("held-deadline"), "handover: the run's deadline passed while it was held after admission")
    release(b.number)
    let rb = b.wait(15)
    check(rb?.outcome == .deadline && rb?.detail.contains("past its deadline before admission, never spawned") == true,
          "handover: a deadline that passes between admission and start: \(outcomeLine(rb))")
    exit(failed ? 1 : 0)
case ("live", 4):
    let p = Probe(a[3], "step", deadlineMs: 60_000)
    if p.awaitTaken() { print("service pid \(p.servicePid)"); fflush(stdout) }
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
case (_, 8):
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
// The service this probe talks to: supervisor.sh scopes its log checks by it.
print("service pid \(small.servicePid)"); fflush(stdout)
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

// 3. A deadline kill, then the next run right after it: no throttle. The
//    log shows the kill; no "running" precondition (#111: a run killed at
//    1.5 s may never send an event first on a slow machine).
let slow = Probe(a[5], "step", deadlineMs: 1500)
let rs = slow.wait()
check(rs?.outcome == .deadline, "deadline: \(rs.map { "\($0.outcome.rawValue) \($0.detail)" } ?? "no reply") in \(String(format: "%.0f", slow.ms)) ms")
let after = Probe(a[2], "step")
let ra = after.wait()
check(ra?.outcome == .done && after.ms < 2000,
      "next run right after the kill: \(String(format: "%.0f", after.ms)) ms — \(meshLine(ra))")

// 3b. A run beside a held one, once the held one's kernel is logged spawned:
//     independent at L >= 2, served after it at L = 1.
// The held run: the huge file, which the probe cancels itself, so it can't
// end before the run beside it is checked.
let held = Probe(a[6], "step", deadlineMs: 120_000)
check(held.awaitSpawned(), "the held run's kernel spawned")
let beside = Probe(a[2], "step")
if Int(a[7]) ?? 1 >= 2 {
    let rb = beside.wait()
    check(rb?.outcome == .done && !held.isDone,
          "concurrent: \(String(format: "%.0f", beside.ms)) ms while the held run went on — \(meshLine(rb))")
    // The deadline kill under concurrency: a 1.5 s run beside the held one
    // is killed at its deadline while the held one goes on. (At L = 1 no
    // two kernels run at once: the kill alone is section 3.)
    let slowBeside = Probe(a[5], "step", deadlineMs: 1500)
    let rsb = slowBeside.wait()
    check(rsb?.outcome == .deadline && !held.isDone,
          "deadline beside a running run: \(outcomeLine(rsb))")
    held.run.cancel()
} else {
    // One kernel at a time (Admission): the second waits for the first,
    // which ends here, on a cancel.
    check(beside.awaitLogged("queued"), "admission limit 1: the second run queued behind the held one")
    let freed = now()
    held.run.cancel()
    let rb = beside.wait()
    check(rb?.outcome == .done && beside.t0 + beside.ms > freed,
          "admission limit 1: the second run was served once the held one was cancelled — \(meshLine(rb))")
}

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
let cancelled = Probe(a[6], "step", deadlineMs: 120_000)
check(cancelled.awaitSpawned(), "the run to cancel spawned")
cancelled.run.cancel()
check(cancelled.wait(2) == nil, "cancel: no completion after cancel()")

// 6. Refused before anything runs: every run is bounded and well formed.
for (what, p) in [
    ("format obj", { Probe(a[2], "obj") }),
    ("deadlineMs 0", { Probe(a[2], "step", deadlineMs: 0) }),
    ("deadlineMs -1", { Probe(a[2], "step", deadlineMs: -1) }),
    ("deadlineMs Int.max", { Probe(a[2], "step", deadlineMs: Int.max) }),
    ("memoryMB 0", { Probe(a[2], "step", memoryMB: 0) }),
    ("memoryMB 4097, over a slot's 4096", { Probe(a[2], "step", memoryMB: 4097) }),
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
