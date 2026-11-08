// Unit tests of the preview's session (macos/Shared/StepvPreviewSession.swift,
// #64) against a fake supervisor service, for what a real one will not do on
// cue: never reply. Then the session's backstop must cancel the run, show
// the error once, and drop the reply that comes after it.

import Foundation

var failures = 0
func check(_ ok: Bool, _ what: String) {
    print(ok ? "ok: \(what)" : "FAIL: \(what)")
    if !ok { failures += 1 }
}
func spin(_ seconds: TimeInterval) { RunLoop.main.run(until: Date().addingTimeInterval(seconds)) }

/// A fake service: hands its callbacks to the test, counts cancels.
final class FakeService {
    var onEvent: ((UInt8, Data) -> Void)?
    var completion: ((StepvRunResult) -> Void)?
    var cancels = 0
    func start(_ e: @escaping (UInt8, Data) -> Void, _ c: @escaping (StepvRunResult) -> Void) -> () -> Void {
        onEvent = e
        completion = c
        return { self.cancels += 1 }
    }
}

final class Seen {
    var ready: [String] = []
    var results: [String] = []
    var trees = 0
    var preparedOffMain = true
    /// Quick Look's handler: nil, or "cancelled" for CocoaError(.userCancelled).
    var completions: [String] = []
}

/// `decode`: how long the result's prepare takes (a mesh's decode).
func session(_ seen: Seen, path: String = "/a.step", decode: TimeInterval = 0) -> StepvPreviewSession<String> {
    let s = StepvPreviewSession<String>(path: path, handler: { e in
        seen.completions.append(e == nil ? "nil" : (e as? CocoaError)?.code == .userCancelled ? "cancelled" : "\(e!)")
    }, prepare: { r in
        if Thread.isMainThread { seen.preparedOffMain = false }
        Thread.sleep(forTimeInterval: decode)
        return r.outcome == .done ? "\(path): done" : r.detail
    })
    s.onTree = { _ in seen.trees += 1 }
    s.onResult = { p in seen.results.append(p); s.markReady(p.hasSuffix(": done") ? "mesh" : "failure") }
    s.onReady = { seen.ready.append($0) }
    return s
}

// 1. The service never replies: ready at its timer, then the backstop.
do {
    let seen = Seen(), fake = FakeService(), s = session(seen)
    s.start(readyAfter: 0.2, replyAfter: 0.5) { fake.start($0, $1) }
    spin(0.35)
    check(seen.ready == ["deadline"] && seen.results.isEmpty,
          "silent service: ready (deadline) at its timer, no result yet: \(seen.ready) \(seen.results)")
    spin(0.4)
    check(seen.results == ["no reply from the supervisor service"] && fake.cancels == 1,
          "silent service: the backstop shows the error once and cancels the run once: \(seen.results), \(fake.cancels) cancel(s)")
    // The reply that comes after the backstop is dropped.
    fake.completion?(StepvRunResult(outcome: .done, detail: "late"))
    spin(0.3)
    check(seen.results.count == 1 && seen.ready == ["deadline"],
          "a reply after the backstop is dropped: \(seen.results), ready \(seen.ready)")
}

// 2. A normal run: the tree, then the result, prepared off the main thread;
//    ready once, on the tree; the backstop never fires.
do {
    let seen = Seen(), fake = FakeService(), s = session(seen)
    s.start(readyAfter: 5, replyAfter: 0.6) { fake.start($0, $1) }
    let tree = try! JSONSerialization.data(withJSONObject: ["tree": [["name": "a", "children": [["name": "b", "part": 0]]]], "leaves": 1])
    fake.onEvent?(StepvEvent.tree, tree)
    spin(0.1)
    fake.completion?(StepvRunResult(outcome: .done, detail: "run"))
    spin(0.9)
    check(seen.trees == 1 && seen.ready == ["tree"] && seen.results == ["/a.step: done"] && fake.cancels == 0
          && seen.completions == ["nil"],
          "a run: tree, ready (tree) once, one result, no backstop: \(seen.ready) \(seen.results) \(fake.cancels)")
    check(seen.preparedOffMain, "the result is prepared off the main thread")
}

// 3. Cancelled (the view went): no result, even if the reply comes.
do {
    let seen = Seen(), fake = FakeService(), s = session(seen)
    s.start(readyAfter: 5, replyAfter: 0.3) { fake.start($0, $1) }
    s.cancel()
    fake.completion?(StepvRunResult(outcome: .done, detail: "late"))
    spin(0.5)
    check(seen.results.isEmpty && fake.cancels == 1 && seen.completions == ["cancelled"],
          "cancelled: no result, the run cancelled once, no backstop, the handler completed cancelled once: \(seen.results) \(fake.cancels) \(seen.completions)")
}

// 4. The controller reused (#107's review): A's run has finished, and its
//    decode is still running when a prepare for B supersedes it. Nothing of
//    A reaches the view: not its scene, not its ready, which must not count
//    as B's. A's handler completes once, cancelled; B's once, on B's own
//    result.
do {
    let a = Seen(), b = Seen(), fa = FakeService(), fb = FakeService()
    let slot = StepvPreviewSlot<String>()
    // As the controller's show(): a result marks ready the slot's session.
    func shows(_ seen: Seen, _ s: StepvPreviewSession<String>) {
        s.onResult = { p in seen.results.append(p); slot.session?.markReady("mesh") }
    }
    let sa = session(a, path: "/a.step", decode: 0.3)
    shows(a, sa)
    slot.replace(with: sa, path: "/a.step")
    sa.start(readyAfter: 5, replyAfter: 5) { fa.start($0, $1) }
    fa.completion?(StepvRunResult(outcome: .done, detail: "a"))
    spin(0.05)  // A's decode runs now
    let sb = session(b, path: "/b.brep", decode: 0)
    shows(b, sb)
    slot.replace(with: sb, path: "/b.brep")
    sb.start(readyAfter: 5, replyAfter: 5) { fb.start($0, $1) }
    spin(0.5)  // A's decode is done
    check(a.results.isEmpty && a.ready.isEmpty && fa.cancels == 1 && a.completions == ["cancelled"],
          "reused: A's late result is dropped, not shown, its run cancelled, its handler completed cancelled once: \(a.results) \(a.ready) \(fa.cancels) \(a.completions)")
    check(b.results.isEmpty && b.ready.isEmpty && b.completions.isEmpty,
          "reused: B shows nothing of A's, and A's result does not make B ready: \(b.results) \(b.ready) \(b.completions)")
    fb.completion?(StepvRunResult(outcome: .done, detail: "b"))
    spin(0.2)
    check(b.results == ["/b.brep: done"] && b.ready == ["mesh"] && b.completions == ["nil"] && slot.session === sb,
          "reused: B's own result shown once, ready on it, its handler once: \(b.results) \(b.ready) \(b.completions)")
}

// 5. Quick Look's handler retains the view controller, which holds the
//    session: once called, the session lets it go, or a closed preview's
//    controller would live on and never cancel its run.
do {
    final class Owner {}
    weak var owner: Owner?
    var s: StepvPreviewSession<String>?
    do {
        let o = Owner()
        owner = o
        s = StepvPreviewSession<String>(path: "/o.step", handler: { _ in _ = o }, prepare: { _ in "" })
    }
    check(owner != nil, "the handler holds what it captured until called")
    s?.markReady("tree")
    check(owner == nil, "the handler is released once called")
}

print(failures == 0 ? "preview session: all checks passed" : "preview session: \(failures) FAILED")
exit(failures == 0 ? 0 : 1)
