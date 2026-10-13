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

/// One connection: at most one run.
final class Supervisor: NSObject, StepvSupervisorProtocol {
    weak var connection: NSXPCConnection?
    private let lock = NSLock()
    private var child: pid_t = 0
    private var used = false
    private var cancelled: UInt64?

    /// The connection went: kill the kernel, if one runs.
    func cancel() {
        lock.lock(); defer { lock.unlock() }
        if cancelled == nil { cancelled = DispatchTime.now().uptimeNanoseconds }
        if child > 0 { kill(child, SIGKILL) }
    }

    func run(_ file: FileHandle, format: String, linearRel: Double, angularDeg: Double,
             deadlineMs: Int, memoryMB: Int,
             reply: @escaping (StepvRunOutcome, String, Data?, FileHandle?) -> Void) {
        lock.lock(); let first = !used; used = true; lock.unlock()
        guard first else { return reply(.refused, "one run per connection", nil, nil) }
        if let why = Supervisor.invalid(format: format, linearRel: linearRel, angularDeg: angularDeg,
                                        deadlineMs: deadlineMs, memoryMB: memoryMB) {
            return reply(.refused, why, nil, nil)
        }
        servedLock.lock(); served += 1; let n = served; servedLock.unlock()
        DispatchQueue.global(qos: .userInitiated).async {
            let (outcome, detail, summary, mesh) = self.supervise(
                file, n: n, format: format, linearRel: linearRel, angularDeg: angularDeg,
                deadlineMs: deadlineMs, memoryMB: memoryMB)
            withExtendedLifetime(file) {}
            reply(outcome, detail, summary, mesh)
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
        if !(1...maxMemoryMB).contains(memoryMB) { return "memoryMB \(memoryMB) is not in 1...\(maxMemoryMB)" }
        return nil
    }
    static let maxDeadlineMs = 600_000  // 10 min: far past Quick Look's own watchdogs
    static let maxMemoryMB = 1 << 20  // 1 TB: any real machine's memory, below overflow

    private func supervise(_ file: FileHandle, n: Int, format: String, linearRel: Double,
                           angularDeg: Double, deadlineMs: Int, memoryMB: Int)
        -> (StepvRunOutcome, String, Data?, FileHandle?) {
        let tag = "run #\(n) (service pid \(getpid()), up \(Int(elapsedMs(since: started))) ms)"
        let kernel = Bundle.main.bundleURL.appendingPathComponent("Contents/Helpers/stepv-occt").path
        // The mesh lands in this service's container; it goes back as an
        // open, unlinked file, so no large XPC message carries it.
        let meshPath = FileManager.default.temporaryDirectory
            .appendingPathComponent("stepv-\(UUID().uuidString).msh").path
        defer { unlink(meshPath) }
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
        let env = ["PATH=/usr/bin:/bin", "TMPDIR=\(NSTemporaryDirectory())"]
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
        let deadline = t0 + UInt64(deadlineMs) * 1_000_000  // validated: no overflow
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

// What runs left behind: a service that died mid-run never unlinked its
// kernel's mesh, and a teardown that kills the kernel's whole job (launchd)
// also kills the child that would have removed its IGES copy (#102,
// stepv-input-*). None is in use yet: this service's runs start after this.
if let tmp = try? FileManager.default.contentsOfDirectory(atPath: NSTemporaryDirectory()) {
    for f in tmp where (f.hasPrefix("stepv-") && f.hasSuffix(".msh")) || f.hasPrefix("stepv-input-") {
        unlink(NSTemporaryDirectory() + "/" + f)
    }
}

let delegate = Delegate()
let listener = NSXPCListener.service()
listener.delegate = delegate
listener.resume()
