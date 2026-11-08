// One preview's run, apart from its view (#64): the streamed events, Quick
// Look's handler(nil) rule, the client backstop, and the decode of the
// result off the main thread. The view controller only draws what it is
// handed; macos/Tests/PreviewSessionTests drives this with a fake service.
//
// A session ends live or cancelled (the view went, or another prepare
// superseded it), on the main thread. Once cancelled, nothing of it reaches
// the view: a tree, progress, result or ready already on its way, a decode
// already running, is dropped and logged "stale, dropped". Quick Look's
// handler is called exactly once per session: nil when ready, or
// CocoaError(.userCancelled) if it is cancelled first.

import Foundation

/// First caller wins, across threads: one reply, one ready.
final class StepvOnce {
    private let lock = NSLock()
    private var done = false
    func claim() -> Bool {
        lock.lock(); defer { lock.unlock() }
        if done { return false }
        done = true
        return true
    }
}

final class StepvPreviewSession<Prepared> {
    /// Starts the run: events and the completion as StepvSupervisorRun
    /// delivers them; returns its cancel.
    typealias Start = (_ onEvent: @escaping (UInt8, Data) -> Void,
                       _ completion: @escaping (StepvRunResult) -> Void) -> () -> Void

    /// On the main thread: the tree's lines, a progress line, the result
    /// (prepared off it; its handler calls markReady with mesh or failure),
    /// and ready (once, with why: tree, mesh, failure or deadline).
    var onTree: ([String]) -> Void = { _ in }
    var onProgress: (String) -> Void = { _ in }
    var onResult: (Prepared) -> Void = { _ in }
    var onReady: (String) -> Void = { _ in }

    let path: String
    /// Released once called: Quick Look's handler retains its view
    /// controller, which holds this session, and a controller kept alive
    /// past its close would never cancel its run (ql-e2e's closed preview).
    private var handler: ((Error?) -> Void)?
    private let prepare: (StepvRunResult) -> Prepared
    private let ready = StepvOnce()
    private let finished = StepvOnce()
    private var cancelRun: (() -> Void)?
    /// Main thread only: false once cancelled.
    private var live = true
    private let started = Date()

    /// `handler`: Quick Look's, for `path`. `prepare` runs on a background
    /// queue: decoding a mesh and building its scene can take long enough to
    /// stall the main thread.
    init(path: String, handler: @escaping (Error?) -> Void,
         prepare: @escaping (StepvRunResult) -> Prepared) {
        self.path = path
        self.handler = handler
        self.prepare = prepare
    }

    /// `readyAfter`: Quick Look is told the preview is up at the latest
    /// then. `replyAfter`: past it the session gives up on the service, just
    /// past the run's own deadline (the service kills the kernel then and
    /// replies; this is for a service that cannot).
    func start(readyAfter: TimeInterval, replyAfter: TimeInterval, _ start: Start) {
        cancelRun = start({ [weak self] kind, payload in
            DispatchQueue.main.async { if self?.current("event") == true { self?.event(kind, payload) } }
        }, { [weak self] result in
            self?.finish(result)
        })
        DispatchQueue.main.asyncAfter(deadline: .now() + readyAfter) { [weak self] in
            self?.markReady("deadline")
        }
        DispatchQueue.main.asyncAfter(deadline: .now() + replyAfter) { [weak self] in
            guard let self, self.finished.claim() else { return }
            self.cancelRun?()
            stepvLog.notice("preview: no reply from the supervisor service in \(Int(replyAfter)) s: cancelled")
            self.deliver(StepvRunResult(outcome: .crashed, detail: "no reply from the supervisor service"))
        }
    }

    /// Main thread. The view went, or another prepare superseded this one:
    /// its kernel goes, nothing more of it is shown, and Quick Look's handler
    /// is completed as cancelled if it was not yet called.
    func cancel() {
        guard live else { return }
        live = false
        _ = finished.claim()
        cancelRun?()
        if ready.claim() { complete("cancelled", CocoaError(.userCancelled)) }
    }

    /// Main thread. Quick Look's handler(nil), once, while live.
    func markReady(_ why: String) {
        if current("ready (\(why))"), ready.claim() {
            onReady(why)
            complete(why, nil)
        }
    }

    private func complete(_ why: String, _ error: Error?) {
        let ms = Int(Date().timeIntervalSince(started) * 1000)
        stepvLog.notice("preview \(self.path, privacy: .public): ready (\(why, privacy: .public)) in \(ms) ms")
        let handler = self.handler
        self.handler = nil
        handler?(error)
    }

    /// Main thread: whether this session is still the view's; if not, `what`
    /// is dropped, and said so.
    private func current(_ what: String) -> Bool {
        if !live { stepvLog.notice("stale, dropped: preview \(self.path, privacy: .public) \(what, privacy: .public)") }
        return live
    }

    private func event(_ kind: UInt8, _ payload: Data) {
        let json = (try? JSONSerialization.jsonObject(with: payload)) as? [String: Any]
        if kind == StepvEvent.tree, let nodes = json?["tree"] as? [[String: Any]] {
            onTree(StepvPreviewText.outline(nodes, leaves: json?["leaves"] as? Int))
            markReady("tree")
        } else if kind == StepvEvent.progress, let stage = json?["stage"] as? String,
                  let done = json?["done"] as? Int, let total = json?["total"] as? Int, total > 0 {
            onProgress("\(stage == "mesh" ? "Meshing" : "Transferring") \(done * 100 / total) %")
        }
    }

    /// The run's reply, if it is the first word on the run: a reply after
    /// the backstop, or after a cancel, is dropped.
    private func finish(_ result: StepvRunResult) {
        if finished.claim() { deliver(result) }
    }

    private func deliver(_ result: StepvRunResult) {
        DispatchQueue.global(qos: .userInitiated).async { [weak self] in
            guard let self else { return }
            let prepared = self.prepare(result)
            DispatchQueue.main.async { if self.current("result") { self.onResult(prepared) } }
        }
    }
}

/// A view controller's preview, one at a time (#64). Quick Look makes a
/// controller per previewed item, so a second prepare on one is not
/// expected; if one comes, the session it replaces is cancelled first.
final class StepvPreviewSlot<Prepared> {
    private(set) var session: StepvPreviewSession<Prepared>?

    /// Main thread: `next` (nil: none) becomes the view's preview.
    func replace(with next: StepvPreviewSession<Prepared>?, path: String) {
        if let old = session {
            stepvLog.notice("preview controller reused: \(old.path, privacy: .public) replaced by \(path, privacy: .public)")
            old.cancel()
        }
        session = next
    }
}

/// The preview's text, apart from the view.
enum StepvPreviewText {
    /// The assembly tree as indented lines, at most `limit` of them.
    static func outline(_ roots: [[String: Any]], leaves: Int?, limit: Int = 14) -> [String] {
        var out: [String] = []
        var more = 0
        func walk(_ nodes: [[String: Any]], _ depth: Int) {
            for n in nodes {
                if out.count < limit {
                    out.append(String(repeating: "   ", count: depth) + ((n["name"] as? String) ?? "?"))
                } else {
                    more += 1
                }
                walk((n["children"] as? [[String: Any]]) ?? [], depth + 1)
            }
        }
        walk(roots, 0)
        if more > 0 { out.append("… \(more) more") }
        if let leaves { out.insert("\(leaves) part\(leaves == 1 ? "" : "s"), loading…", at: 0) }
        return out
    }

    /// Why a run gave no geometry, in a person's words.
    static func why(_ r: StepvRunResult) -> String {
        switch r.outcome {
        case .done: return (r.summary?["error"] as? String) ?? "no geometry"
        case .deadline: return "it took longer than Quick Look allows"
        case .memory: return "it needs more memory than Quick Look allows"
        case .crashed, .refused: return r.detail
        }
    }
}
