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
}

func session(_ seen: Seen) -> StepvPreviewSession<String> {
    let s = StepvPreviewSession<String>(prepare: { r in
        if Thread.isMainThread { seen.preparedOffMain = false }
        return r.outcome == .done ? "done" : r.detail
    })
    s.onTree = { _ in seen.trees += 1 }
    s.onResult = { p in seen.results.append(p); s.markReady(p == "done" ? "mesh" : "failure") }
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
    check(seen.trees == 1 && seen.ready == ["tree"] && seen.results == ["done"] && fake.cancels == 0,
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
    check(seen.results.isEmpty && fake.cancels == 1,
          "cancelled: no result, the run cancelled once, no backstop: \(seen.results) \(fake.cancels)")
}

print(failures == 0 ? "preview session: all checks passed" : "preview session: \(failures) FAILED")
exit(failures == 0 ? 0 : 1)
