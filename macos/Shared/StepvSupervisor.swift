// The kernel out of process (#64): each Quick Look extension embeds an XPC
// service, StepvSupervisor.xpc, that runs `stepv-occt --stream` as its own
// child and kills it on cancel, deadline or memory cap. A Quick Look
// extension may not spawn a process itself (EPERM, #79), and an XPC service
// that ran the kernel in-process could neither be killed while busy nor
// restarted without launchd's ~10 s throttle (#64's pre-mortem); a child of a
// long-lived service is killed and reaped in ~30 ms, and the next run starts
// at once.
//
// The file goes over as a FileHandle: a URL carries no sandbox permission
// across XPC, and the service cannot open the file by path. The kernel reads
// it as /dev/fd/3 (#91), which is why the format is named: a descriptor has
// no extension. A multi-file assembly's part files are out of reach (#92).
//
// One run per connection. Invalidating the connection is the cancel: the
// service kills that run's kernel. Concurrent runs are concurrent
// connections, each with its own kernel.

import Foundation

/// The extension's side of a run: the service calls it with each TREE and
/// PROGRESS frame's payload as the kernel writes it, in order, and before
/// the run's reply.
@objc(StepvRunObserver) protocol StepvRunObserver {
    func event(_ kind: Int, payload: Data)
}

/// How a run ended.
@objc enum StepvRunOutcome: Int {
    /// The kernel finished: the summary says whether there is geometry.
    case done
    /// Killed at the deadline.
    case deadline
    /// Killed past the memory cap.
    case memory
    /// The kernel died, or broke its stream; the detail says how.
    case crashed
    /// Not run: a bad request, or the kernel could not start.
    case refused
}

@objc(StepvSupervisorProtocol) protocol StepvSupervisorProtocol {
    /// Runs the kernel on `file` read as `format` ("step", "iges" or
    /// "brep"), at `linearRel` and `angularDeg`. `deadlineMs` and `memoryMB`
    /// bound it; 0 is none. The reply carries the summary (the DONE frame's
    /// JSON) and, on success, the STEPVMSH v3 mesh as an unlinked file.
    func run(_ file: FileHandle, format: String, linearRel: Double, angularDeg: Double,
             deadlineMs: Int, memoryMB: Int,
             reply: @escaping (StepvRunOutcome, String, Data?, FileHandle?) -> Void)
}

/// The XPC interfaces, both ways, shared by the two ends.
enum StepvSupervisorInterface {
    static var service: NSXPCInterface { NSXPCInterface(with: StepvSupervisorProtocol.self) }
    static var observer: NSXPCInterface { NSXPCInterface(with: StepvRunObserver.self) }
}

/// One run, from the extension's side.
struct StepvRunResult {
    var outcome: StepvRunOutcome
    var detail: String
    var summary: [String: Any]? = nil
    var mesh: Data? = nil
}

/// A run through the calling bundle's supervisor service, on a connection
/// of its own. `onEvent` gets each TREE and PROGRESS payload, `completion`
/// the result, each on an XPC queue; `completion` is called once, unless
/// the run is cancelled first.
final class StepvSupervisorRun: NSObject, StepvRunObserver {
    private let connection: NSXPCConnection
    private let onEvent: (UInt8, Data) -> Void
    private let lock = NSLock()
    private var completion: ((StepvRunResult) -> Void)?

    /// The service embedded in the calling bundle: "<bundle id>.supervisor".
    static var serviceName: String { (Bundle.main.bundleIdentifier ?? "") + ".supervisor" }

    init(serviceName: String = StepvSupervisorRun.serviceName,
         onEvent: @escaping (UInt8, Data) -> Void,
         completion: @escaping (StepvRunResult) -> Void) {
        connection = NSXPCConnection(serviceName: serviceName)
        self.onEvent = onEvent
        self.completion = completion
        super.init()
        connection.remoteObjectInterface = StepvSupervisorInterface.service
        connection.exportedInterface = StepvSupervisorInterface.observer
        connection.exportedObject = self
        // The connection holds its exported object strongly until it is
        // invalidated, which every way out of a run does.
        connection.interruptionHandler = { [weak self] in
            self?.finish(StepvRunResult(outcome: .crashed, detail: "the supervisor service exited"))
        }
        connection.invalidationHandler = { [weak self] in
            self?.finish(StepvRunResult(outcome: .crashed, detail: "the supervisor connection closed"))
        }
    }

    func start(file: URL, format: String, linearRel: Double, angularDeg: Double,
               deadlineMs: Int, memoryMB: Int) {
        let handle: FileHandle
        do { handle = try FileHandle(forReadingFrom: file) } catch {
            connection.resume()  // invalidated in finish: never left suspended
            return finish(StepvRunResult(outcome: .refused, detail: "cannot open the file: \(error.localizedDescription)"))
        }
        connection.resume()
        let proxy = connection.remoteObjectProxyWithErrorHandler { [weak self] error in
            self?.finish(StepvRunResult(outcome: .crashed, detail: "XPC: \(error.localizedDescription)"))
        } as? StepvSupervisorProtocol
        proxy?.run(handle, format: format, linearRel: linearRel, angularDeg: angularDeg,
                   deadlineMs: deadlineMs, memoryMB: memoryMB) { [weak self] outcome, detail, summary, mesh in
            let json = summary.flatMap { try? JSONSerialization.jsonObject(with: $0) } as? [String: Any]
            let bytes = mesh.flatMap { try? $0.readToEnd() }
            try? mesh?.close()
            self?.finish(StepvRunResult(outcome: outcome, detail: detail, summary: json, mesh: bytes))
        }
    }

    /// Kills the run's kernel (the service sees the connection go); no
    /// completion follows.
    func cancel() {
        lock.lock(); completion = nil; lock.unlock()
        connection.invalidate()
    }

    func event(_ kind: Int, payload: Data) {
        lock.lock(); let live = completion != nil; lock.unlock()
        if live { onEvent(UInt8(truncatingIfNeeded: kind), payload) }
    }

    private func finish(_ result: StepvRunResult) {
        lock.lock(); let done = completion; completion = nil; lock.unlock()
        guard let done else { return }
        connection.invalidate()
        done(result)
    }
}
