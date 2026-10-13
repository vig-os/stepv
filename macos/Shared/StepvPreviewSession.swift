// One preview's run, apart from its view (#64): the streamed events, Quick
// Look's handler(nil) rule, the client backstop, and the decode of the
// result off the main thread. The view controller only draws what it is
// handed; macos/Tests/PreviewSessionTests drives this with a fake service.

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

    private let prepare: (StepvRunResult) -> Prepared
    private let ready = StepvOnce()
    private let finished = StepvOnce()
    private var cancelRun: (() -> Void)?

    /// `prepare` runs on a background queue: decoding a mesh and building
    /// its scene can take long enough to stall the main thread.
    init(prepare: @escaping (StepvRunResult) -> Prepared) {
        self.prepare = prepare
    }

    /// `readyAfter`: Quick Look is told the preview is up at the latest
    /// then. `replyAfter`: past it the session gives up on the service, just
    /// past the run's own deadline (the service kills the kernel then and
    /// replies; this is for a service that cannot).
    func start(readyAfter: TimeInterval, replyAfter: TimeInterval, _ start: Start) {
        cancelRun = start({ [weak self] kind, payload in
            DispatchQueue.main.async { self?.event(kind, payload) }
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

    /// The view went: its kernel goes.
    func cancel() {
        _ = finished.claim()
        cancelRun?()
    }

    /// Quick Look's handler(nil), once.
    func markReady(_ why: String) {
        if ready.claim() { onReady(why) }
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
            DispatchQueue.main.async { self.onResult(prepared) }
        }
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
