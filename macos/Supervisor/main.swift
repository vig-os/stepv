// StepvSupervisor.xpc (#64): the XPC service each Quick Look extension
// embeds. It lives as long as launchd keeps it and serves every run of its
// extension, each on its own connection: it spawns the bundled kernel,
// Contents/Helpers/stepv-occt, with the extension's file as fd 3, forwards
// the kernel's events, and kills it on cancel (the connection goes), at the
// deadline, or past the memory cap. Protocol: Shared/StepvSupervisor.swift.
//
// Sandbox: the service is App-Sandboxed; the kernel is signed app-sandbox +
// inherit, so it runs in this service's sandbox. The kernel's own Seatbelt
// profile cannot nest in it and reports "macos-outer": this sandbox, with
// the one file's descriptor and this service's container, is the boundary.

import Darwin
import Foundation
import os

let log = Logger(subsystem: "ch.exoma.stepv", category: "supervisor")
let started = DispatchTime.now().uptimeNanoseconds
func elapsedMs(since t: UInt64) -> Double { Double(DispatchTime.now().uptimeNanoseconds - t) / 1e6 }
var served = 0
let servedLock = NSLock()

/// The kernel's resident size, nil once it is gone. The CLI caps the
/// footprint (src/occt.rs's `footprint`, proc_pid_rusage), but the sandbox
/// denies proc_pid_rusage even on a child (EPERM) and allows PROC_PIDTASKINFO.
/// Resident size counts the shared dylibs and drops as the compressor takes
/// pages, so under memory pressure this cap acts later than the CLI's.
func residentSize(_ pid: pid_t) -> UInt64? {
    var info = proc_taskinfo()
    let size = Int32(MemoryLayout<proc_taskinfo>.size)
    return proc_pidinfo(pid, PROC_PIDTASKINFO, 0, &info, size) == size ? info.pti_resident_size : nil
}

/// How many kernels this service runs at once (#64): the in-process kernel
/// ran one file at a time, and N concurrent runs at a 4096 MB cap each
/// could take the machine. L = clamp(physical memory / (4 × 4096 MB), 1,
/// cores): at most ~¼ of RAM per service, ~½ across the thumbnail and
/// preview services. Over L, requests wait in a FIFO queue; their deadline
/// counts from submission, and one that expires or is cancelled there is
/// withdrawn without ever spawning a kernel.
final class Admission {
    /// One slot's memory budget: a run's cap may not be more (Supervisor
    /// refuses it), so L slots stay within the share of RAM L is made for.
    static let slotMemoryMB = 4096
    let limit: Int
    private let lock = NSLock()
    private var running = 0
    private var waiting: [(id: Int, start: () -> Void)] = []

    init(limit: Int) { self.limit = limit }

    static var forThisMachine: Int {
        let cores = ProcessInfo.processInfo.activeProcessorCount
        #if STEPV_TEST_HOOKS
        // Test builds only (scripts/ql-e2e/supervisor.sh): the limit from
        // Contents/Resources/admission-limit, to force queueing. Out of
        // 1...cores it is a mistake in the test, not something to clamp.
        if let n = testResource("admission-limit") {
            guard (1...cores).contains(n) else {
                log.fault("admission-limit \(n) is not in 1...\(cores): the service will not start")
                exit(78)  // EX_CONFIG
            }
            return n
        }
        #endif
        let byMemory = Int(ProcessInfo.processInfo.physicalMemory / (4 * (UInt64(slotMemoryMB) << 20)))
        return min(max(byMemory, 1), cores)
    }

    #if STEPV_TEST_HOOKS
    /// A whole number from the test build's Contents/Resources, if present.
    static func testResource(_ name: String) -> Int? {
        guard let url = Bundle.main.url(forResource: name, withExtension: nil),
              let text = try? String(contentsOf: url) else { return nil }
        return Int(text.trimmingCharacters(in: .whitespacesAndNewlines))
    }
    /// Test builds only (Contents/Resources/admission-hold present): each
    /// admitted run is held before it starts until the test releases it,
    /// so its deadline can pass between admission and start on cue.
    static let holdsAdmitted = Bundle.main.url(forResource: "admission-hold", withExtension: nil) != nil
    #endif

    /// Starts `start` now if a slot is free and none waits, else queues it.
    /// True if it started.
    func submit(id: Int, _ start: @escaping () -> Void) -> Bool {
        lock.lock()
        guard running < limit && waiting.isEmpty else {
            waiting.append((id, start))
            lock.unlock()
            return false
        }
        running += 1
        lock.unlock()
        start()
        return true
    }

    /// Takes a queued request out. True if it was still waiting: it never
    /// starts.
    func withdraw(id: Int) -> Bool {
        lock.lock(); defer { lock.unlock() }
        guard let i = waiting.firstIndex(where: { $0.id == id }) else { return false }
        waiting.remove(at: i)
        return true
    }

    /// A run ended: its slot goes to the first in the queue.
    func leave() {
        lock.lock()
        guard !waiting.isEmpty else {
            running -= 1
            lock.unlock()
            return
        }
        let next = waiting.removeFirst()
        lock.unlock()
        next.start()
    }
}

let admission = Admission(limit: Admission.forThisMachine)

/// One connection: at most one run.
final class Supervisor: NSObject, StepvSupervisorProtocol {
    weak var connection: NSXPCConnection?
    private let lock = NSLock()
    private var child: pid_t = 0
    private var used = false
    /// The events this run forwarded: its reply says how many, so the
    /// client can wait for any still in flight (XPC does not order them
    /// against the reply). Written on the run's thread alone.
    private var eventsSent = 0
    private var cancelled: UInt64?
    /// While the run waits for admission: takes it out of the queue.
    private var withdrawQueued: (() -> Void)?

    /// The connection went: kill the kernel, if one runs, or withdraw the
    /// run from the queue, if it waits.
    func cancel() {
        lock.lock()
        if cancelled == nil { cancelled = DispatchTime.now().uptimeNanoseconds }
        if child > 0 { kill(child, SIGKILL) }
        let withdraw = withdrawQueued
        lock.unlock()
        withdraw?()
    }

    func run(_ file: FileHandle, format: String, linearRel: Double, angularDeg: Double,
             deadlineMs: Int, memoryMB: Int,
             reply: @escaping (StepvRunOutcome, String, Data?, FileHandle?, Int) -> Void) {
        lock.lock(); let first = !used; used = true; lock.unlock()
        guard first else { return reply(.refused, "one run per connection", nil, nil, 0) }
        if let why = Supervisor.invalid(format: format, linearRel: linearRel, angularDeg: angularDeg,
                                        deadlineMs: deadlineMs, memoryMB: memoryMB) {
            return reply(.refused, why, nil, nil, 0)
        }
        servedLock.lock(); served += 1; let n = served; servedLock.unlock()
        // Who the run is in this log, for its client (StepvRunObserver).
        (connection?.remoteObjectProxy as? StepvRunObserver)?.accepted(n)
        let submitted = DispatchTime.now().uptimeNanoseconds
        // A queued run leaves the queue at its deadline or on a cancel, never
        // spawned, with one reply: whichever takes it out (Admission's lock).
        let withdraw = { (outcome: StepvRunOutcome, why: String) in
            guard admission.withdraw(id: n) else { return }
            log.notice("run #\(n) (service pid \(getpid())): \(why, privacy: .public) in the queue, never spawned")
            reply(outcome, "run #\(n): \(why) in the queue, never spawned", nil, nil, 0)
        }
        // Installed before the run can be queued, so a cancel at any moment
        // finds it.
        lock.lock(); withdrawQueued = { withdraw(.crashed, "cancelled") }; lock.unlock()
        let admitted = admission.submit(id: n) {
            DispatchQueue.global(qos: .userInitiated).async {
                self.lock.lock(); self.withdrawQueued = nil; self.lock.unlock()
                let waited = elapsedMs(since: submitted)
                if waited >= 1 {
                    log.notice("run #\(n) (service pid \(getpid())): admitted after \(waited, format: .fixed(precision: 0)) ms in the queue")
                }
                let (outcome, detail, summary, mesh) = self.supervise(
                    file, n: n, format: format, linearRel: linearRel, angularDeg: angularDeg,
                    deadlineMs: deadlineMs, memoryMB: memoryMB, submitted: submitted)
                admission.leave()
                withExtendedLifetime(file) {}
                reply(outcome, detail, summary, mesh, self.eventsSent)
            }
        }
        guard !admitted else { return }
        log.notice("run #\(n) (service pid \(getpid())): queued, \(admission.limit) kernel(s) running")
        // A cancel that came before the run was in the queue found nothing to
        // take out: take it out now.
        lock.lock(); let wasCancelled = cancelled != nil; lock.unlock()
        if wasCancelled { withdraw(.crashed, "cancelled") }
        DispatchQueue.global().asyncAfter(deadline: .now() + .milliseconds(deadlineMs)) {
            withdraw(.deadline, "past its deadline")
        }
    }

    /// Why a request is refused before anything runs, or nil. Every run is
    /// bounded: a deadline and a memory cap are required, and in range.
    static func invalid(format: String, linearRel: Double, angularDeg: Double,
                        deadlineMs: Int, memoryMB: Int) -> String? {
        if !["step", "iges", "brep"].contains(format) { return "unknown format \(format)" }
        if !(linearRel > 0 && linearRel <= 1) { return "linearRel \(linearRel) is not in (0, 1]" }
        if !(angularDeg > 0 && angularDeg <= 180) { return "angularDeg \(angularDeg) is not in (0, 180]" }
        if !(1...maxDeadlineMs).contains(deadlineMs) { return "deadlineMs \(deadlineMs) is not in 1...\(maxDeadlineMs)" }
        if !(1...Admission.slotMemoryMB).contains(memoryMB) {
            return "memoryMB \(memoryMB) is not in 1...\(Admission.slotMemoryMB) (one admission slot's budget)"
        }
        return nil
    }
    static let maxDeadlineMs = 600_000  // 10 min: far past Quick Look's own watchdogs

    private func supervise(_ file: FileHandle, n: Int, format: String, linearRel: Double,
                           angularDeg: Double, deadlineMs: Int, memoryMB: Int, submitted: UInt64)
        -> (StepvRunOutcome, String, Data?, FileHandle?) {
        let tag = "run #\(n) (service pid \(getpid()), up \(Int(elapsedMs(since: started))) ms)"
        // The deadline counts from submission: time in the queue is spent.
        let deadline = submitted + UInt64(deadlineMs) * 1_000_000  // validated: no overflow
        #if STEPV_TEST_HOOKS
        if Admission.holdsAdmitted {
            // Held until the test creates release-<n> in this service's tmp;
            // the deadline passing meanwhile is logged, for the test to see
            // before it releases (scripts/ql-e2e/supervisor.sh). Never
            // forever: a cancel ends the hold, and so does the deadline plus
            // 10 s with no release, loudly (a test that lost its way).
            log.notice("\(tag, privacy: .public): held for release")
            let release = NSTemporaryDirectory() + "/release-\(n)"
            let bound = deadline + 10_000_000_000
            var told = false
            while !FileManager.default.fileExists(atPath: release) {
                lock.lock(); let gone = cancelled != nil; lock.unlock()
                if gone {
                    log.notice("\(tag, privacy: .public): cancelled while held, never spawned")
                    return (.crashed, "\(tag): cancelled while held, never spawned", nil, nil)
                }
                let now = DispatchTime.now().uptimeNanoseconds
                if !told && now >= deadline {
                    log.notice("\(tag, privacy: .public): deadline passed while held")
                    told = true
                }
                if now >= bound {
                    log.fault("\(tag, privacy: .public): held 10 s past its deadline with no release: let go")
                    break
                }
                usleep(10_000)
            }
            unlink(release)
        }
        #endif
        guard DispatchTime.now().uptimeNanoseconds < deadline else {
            log.notice("\(tag, privacy: .public): past its deadline before admission, never spawned")
            return (.deadline, "\(tag): past its deadline before admission, never spawned", nil, nil)
        }
        let kernel = Bundle.main.bundleURL.appendingPathComponent("Contents/Helpers/stepv-occt").path
        // The run's files live in a directory of its own in this service's
        // container: the mesh, which goes back as an open, unlinked file (no
        // large XPC message carries it), and the kernel's $TMPDIR (#102's
        // IGES copy). Its lock is held for the whole run, so another
        // instance's start-up sweep (sweepStaleRuns) leaves it alone.
        guard let run = RunDirectory() else {
            return (.refused, "\(tag): cannot create the run's directory: \(String(cString: strerror(errno)))", nil, nil)
        }
        defer { run.remove() }
        let meshPath = run.path + "/mesh.msh"
        let argv = [kernel, "/dev/fd/3", "--format", format, "--stream", "--mesh", meshPath,
                    "--linear-rel", "\(linearRel)", "--angular-deg", "\(angularDeg)", "--lifeline", "4"]
        // The file's descriptor, moved clear of 0-3: XPC hands it over as
        // the lowest free number, often 3 itself, and dup2(3, 3) is no
        // file action, so under POSIX_SPAWN_CLOEXEC_DEFAULT it would close.
        let input = fcntl(file.fileDescriptor, F_DUPFD_CLOEXEC, 10)
        guard input >= 0 else { return (.refused, "\(tag): dup: \(String(cString: strerror(errno)))", nil, nil) }
        defer { close(input) }
        var out: [Int32] = [0, 0]
        guard pipe(&out) == 0 else { return (.refused, "\(tag): pipe: \(String(cString: strerror(errno)))", nil, nil) }
        // The kernel's lifeline (--lifeline 4): it exits when this write end
        // closes, so a service that dies, even by SIGKILL, takes its kernel
        // with it. The write end is this service's alone (close-on-exec,
        // and POSIX_SPAWN_CLOEXEC_DEFAULT keeps it from other kernels).
        var life: [Int32] = [0, 0]
        guard pipe(&life) == 0 else {
            close(out[0]); close(out[1])
            return (.refused, "\(tag): pipe: \(String(cString: strerror(errno)))", nil, nil)
        }
        _ = fcntl(life[1], F_SETFD, FD_CLOEXEC)
        defer { close(life[1]) }
        var fa: posix_spawn_file_actions_t?
        posix_spawn_file_actions_init(&fa)
        posix_spawn_file_actions_addopen(&fa, 0, "/dev/null", O_RDONLY, 0)
        posix_spawn_file_actions_adddup2(&fa, out[1], 1)
        // OCCT's chatter: the kernel's stdout carries the frames alone.
        posix_spawn_file_actions_addopen(&fa, 2, "/dev/null", O_WRONLY, 0)
        posix_spawn_file_actions_adddup2(&fa, input, 3)
        posix_spawn_file_actions_adddup2(&fa, life[0], 4)
        // Nothing else of this service reaches the kernel.
        var attr: posix_spawnattr_t?
        posix_spawnattr_init(&attr)
        posix_spawnattr_setflags(&attr, Int16(POSIX_SPAWN_CLOEXEC_DEFAULT))
        let env = ["PATH=/usr/bin:/bin", "TMPDIR=\(run.path)"]
        let cargv: [UnsafeMutablePointer<CChar>?] = argv.map { strdup($0) } + [nil]
        let cenv: [UnsafeMutablePointer<CChar>?] = env.map { strdup($0) } + [nil]
        defer { (cargv + cenv).forEach { free($0) } }
        let t0 = DispatchTime.now().uptimeNanoseconds
        var pid: pid_t = 0
        lock.lock()
        let rc = cancelled != nil ? ECANCELED : posix_spawn(&pid, kernel, &fa, &attr, cargv, cenv)
        if rc == 0 { child = pid }
        lock.unlock()
        posix_spawn_file_actions_destroy(&fa)
        posix_spawnattr_destroy(&attr)
        close(out[1])
        close(life[0])
        guard rc == 0 else {
            close(out[0])
            if rc == ECANCELED {  // cancelled after admission, before the spawn
                log.notice("\(tag, privacy: .public): cancelled, never spawned")
                return (.crashed, "\(tag): cancelled, never spawned", nil, nil)
            }
            log.error("\(tag, privacy: .public): spawn failed: \(String(cString: strerror(rc)), privacy: .public)")
            return (.refused, "\(tag): spawn failed: \(String(cString: strerror(rc)))", nil, nil)
        }
        log.notice("\(tag, privacy: .public): kernel pid \(pid) spawned in \(elapsedMs(since: t0), format: .fixed(precision: 1)) ms")

        // Read the frames until the stream ends, the deadline passes or the
        // resident size passes the cap; the kernel is reaped on every way
        // out. Its exit does not end the reading: what it wrote before it
        // exited is drained to the end, and only a stream that ends with
        // DONE is a finished run.
        var reader = StepvFrameReader()
        var summary: Data?
        var streamError: StepvStreamError?
        var killedFor: StepvRunOutcome?
        let cap = UInt64(memoryMB) << 20
        var lastProbe: UInt64 = 0
        var chunk = [UInt8](repeating: 0, count: 64 << 10)
        var eof = false
        var status: Int32 = 0
        var reaped = false
        while !(reaped && eof) {
            if !eof {
                var p = pollfd(fd: out[0], events: Int16(POLLIN), revents: 0)
                if poll(&p, 1, 5) > 0 {
                    let k = read(out[0], &chunk, chunk.count)
                    if k > 0 {
                        do {
                            for frame in try reader.feed(Data(chunk[0..<k])) {
                                if frame.kind == StepvEvent.done { summary = frame.payload; continue }
                                (connection?.remoteObjectProxy as? StepvRunObserver)?
                                    .event(Int(frame.kind), payload: frame.payload)
                                eventsSent += 1
                            }
                            // A break after good frames in one chunk: now,
                            // not at the next read.
                            if let failed = reader.failed { throw failed }
                        } catch {
                            streamError = error as? StepvStreamError
                            if !reaped { kill(pid, SIGKILL) }
                            eof = true
                        }
                    } else if k == 0 || errno != EINTR {
                        eof = true
                        if streamError == nil {
                            do { try reader.finish() } catch { streamError = error as? StepvStreamError }
                        }
                    }
                }
            } else {
                usleep(2000)
            }
            let now = DispatchTime.now().uptimeNanoseconds
            if reaped {
                // Draining: an exited kernel's pipe ends at once, unless a
                // process it left behind holds it; the deadline bounds that.
                if now >= deadline && !eof { streamError = .truncated; eof = true }
                continue
            }
            // Exited? Seen without reaping it: the pid is no longer ours to
            // kill once it is reaped, so cancel() must stop aiming at it first.
            var info = siginfo_t()
            if waitid(P_PID, id_t(pid), &info, WEXITED | WNOHANG | WNOWAIT) == 0 && info.si_pid == pid {
                lock.lock(); child = 0; lock.unlock()
                waitpid(pid, &status, 0)
                reaped = true
                continue
            }
            if streamError != nil {
                continue  // killed already: wait for the reap
            } else if now >= deadline {
                killedFor = .deadline
            } else if now - lastProbe >= 5_000_000 {
                lastProbe = now
                if residentSize(pid).map({ $0 > cap }) == true { killedFor = .memory }
            }
            if let killedFor {
                let k0 = DispatchTime.now().uptimeNanoseconds
                lock.lock(); kill(pid, SIGKILL); child = 0; lock.unlock()
                waitpid(pid, &status, 0)
                reaped = true
                eof = true  // a killed kernel's stream is not waited for
                log.notice("\(tag, privacy: .public): kernel pid \(pid) killed (\(killedFor == .deadline ? "deadline" : "memory", privacy: .public)) at \(elapsedMs(since: t0), format: .fixed(precision: 0)) ms, reaped in \(elapsedMs(since: k0), format: .fixed(precision: 1)) ms")
            }
        }
        close(out[0])
        lock.lock(); let wasCancelled = cancelled; lock.unlock()
        let ms = Int(elapsedMs(since: t0))
        let how = status & 0x7f == 0 ? "exit \((status >> 8) & 0xff)" : "signal \(status & 0x7f)"

        let result: (StepvRunOutcome, String, Data?, FileHandle?)
        if let wasCancelled {
            result = (.crashed, "\(tag): cancelled, kernel pid \(pid) \(how), reaped \(String(format: "%.1f", elapsedMs(since: wasCancelled))) ms after the cancel", nil, nil)
        } else if let killedFor {
            result = (killedFor, "\(tag): kernel pid \(pid) killed at \(ms) ms", nil, nil)
        } else if let streamError {
            result = (.crashed, "\(tag): kernel pid \(pid) \(how): \(streamError)", nil, nil)
        } else if status & 0x7f != 0 || ![0, 3].contains((status >> 8) & 0xff) {
            result = (.crashed, "\(tag): kernel pid \(pid) \(how)", summary, nil)
        } else {
            // Exit 0: geometry; 3: a clean failure the summary explains.
            let mesh = (status >> 8) & 0xff == 0 ? FileHandle(forReadingAtPath: meshPath) : nil
            result = (.done, "\(tag): kernel pid \(pid) \(how) in \(ms) ms", summary, mesh)
        }
        log.notice("\(result.1, privacy: .public)")
        return result
    }
}

final class Delegate: NSObject, NSXPCListenerDelegate {
    func listener(_ listener: NSXPCListener, shouldAcceptNewConnection c: NSXPCConnection) -> Bool {
        let s = Supervisor()
        s.connection = c
        c.exportedInterface = StepvSupervisorInterface.service
        c.exportedObject = s
        c.remoteObjectInterface = StepvSupervisorInterface.observer
        // Both mean the extension is gone, or gave up: so does the kernel.
        c.invalidationHandler = { s.cancel() }
        c.interruptionHandler = { s.cancel() }
        c.resume()
        return true
    }
}

/// One run's directory, stepv-run-<uuid> in this service's tmp, and its
/// lock (flock, LOCK_EX), held from creation until remove(). Another
/// instance of this service can share the container, as two client
/// processes each start one: the lock is what tells it the run is live.
/// A process that dies releases its locks, so a dead run's is free.
final class RunDirectory {
    let path: String
    private let lock: Int32

    init?() {
        path = NSTemporaryDirectory() + "/stepv-run-" + UUID().uuidString
        guard mkdir(path, 0o700) == 0 else { return nil }
        // Created already locked (O_EXLOCK): no moment where a sweep could
        // take a new run's lock.
        lock = open(path + "/lock", O_RDWR | O_CREAT | O_EXCL | O_EXLOCK | O_CLOEXEC, 0o600)
        guard lock >= 0 else {
            rmdir(path)
            return nil
        }
    }

    /// Everything in it, then it; the lock goes last.
    func remove() {
        RunDirectory.empty(path)
        close(lock)
    }

    static func empty(_ path: String) {
        for f in (try? FileManager.default.contentsOfDirectory(atPath: path)) ?? [] where f != "lock" {
            unlink(path + "/" + f)
        }
        unlink(path + "/lock")
        rmdir(path)
    }
}

/// Removes what dead runs left behind: a service killed mid-run never
/// removed its run's directory, and a teardown that kills the kernel's
/// whole job (launchd) also kills the child that would have removed its
/// IGES copy (#102). Only a directory whose lock is free is a dead run's;
/// one whose lock is held belongs to a live run of another instance. A
/// directory without its lock yet is being created: left alone. Returns
/// (swept, kept).
func sweepStaleRuns() -> (Int, Int) {
    var swept = 0, kept = 0
    let tmp = NSTemporaryDirectory()
    for f in (try? FileManager.default.contentsOfDirectory(atPath: tmp)) ?? [] where f.hasPrefix("stepv-run-") {
        let dir = tmp + "/" + f
        let fd = open(dir + "/lock", O_RDWR | O_CLOEXEC)
        guard fd >= 0 else { kept += 1; continue }
        defer { close(fd) }
        guard flock(fd, LOCK_EX | LOCK_NB) == 0 else { kept += 1; continue }
        RunDirectory.empty(dir)
        swept += 1
    }
    return (swept, kept)
}

let (swept, kept) = sweepStaleRuns()
// The bundle id names the service: the thumbnail's and the preview's are the
// same binary, so the log cannot tell them apart by image path.
log.notice("service pid \(getpid()) started: swept \(swept) dead run(s), kept \(kept) live, admission limit \(admission.limit), \(Bundle.main.bundleIdentifier ?? "?", privacy: .public)")

let delegate = Delegate()
let listener = NSXPCListener.service()
listener.delegate = delegate
listener.resume()
