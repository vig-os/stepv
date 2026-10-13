// stepv inside a Quick Look extension. The kernel runs OUT of process (#64):
// each extension's StepvSupervisor.xpc runs `stepv-occt --stream` as its
// child, on the file's descriptor (StepvSupervisor.swift). What stays
// in-process is the Rust side through its C ABI (Bridge.h): the header
// reader and the rasteriser, neither of which runs OCCT.
//
// Containment: the kernel runs in the service's sandbox ("macos-outer": its
// own Seatbelt profile cannot nest there), bounded by a deadline and a
// memory cap, and killed and reaped in ~30 ms when either passes or the
// extension goes; the extension itself stays small.

import Foundation
import os

/// Everything the extensions report lands in the unified log:
///   /usr/bin/log show --predicate 'subsystem == "ch.exoma.stepv"' --info
let stepvLog = Logger(subsystem: "ch.exoma.stepv", category: "quicklook")

enum StepvKernel {
    /// Deflection settings, as stepv::Deflection.
    static let thumbnail = (linearRel: 0.005, angularDeg: 30.0)
    static let preview = (linearRel: 0.001, angularDeg: 20.0)

    /// The kernel's memory cap in both extensions: the CLI's default
    /// (`--memory-mb 4096`).
    static let memoryMB = 4096

    /// Each extension's own deadlines. Quick Look gives no cancel callback,
    /// and its watchdogs are undocumented: reportedly ~20 s for a thumbnail
    /// and ~30 s for a preview, after which it gives up on the extension.
    ///
    /// Thumbnail: one reply, the image, so the kernel must be done before
    /// the watchdog: 15 s for the run, the rest left for the render and the
    /// reply. A bigger file (Ultimaker 3: ~42 s) keeps its generic icon.
    static let thumbnailDeadlineMs = 15_000
    /// Thumbnail backstop: past this the extension replies without waiting
    /// for the service (a wedged service must not take the watchdog).
    static let thumbnailReplyAfter = 17.0

    /// Preview: handler(nil) is due within the watchdog, but the view lives
    /// on after it: progress keeps arriving and the scene replaces it. So the
    /// handler is called once the header and tree are shown (or the first
    /// mesh, or a failure, when there is no tree), and at the latest after
    /// 20 s, leaving 10 s of margin to the ~30 s watchdog for a loaded
    /// machine; the run itself may take up to 120 s (a 42 s assembly loads),
    /// a bound on what one preview may cost.
    static let previewHandlerAfter = 20.0
    static let previewDeadlineMs = 120_000
    /// Preview backstop: past this the session gives up on the service and
    /// shows why (the thumbnail's margin over its run, 2 s).
    static let previewReplyAfter = 122.0

    /// The kernel's reader for a file Quick Look hands over: by extension,
    /// as `stepv-occt` picks it for a path. A descriptor has none, so it is
    /// named (`--format`).
    static func format(of url: URL) -> String? {
        switch url.pathExtension.lowercased() {
        case "step", "stp": return "step"
        case "iges", "igs": return "iges"
        case "brep": return "brep"
        default: return nil
        }
    }

    /// The `stepv --info` header metadata. Never fails on a real path.
    static func info(_ url: URL) -> [String: Any]? {
        guard let raw = stepv_info_json(url.path) else { return nil }
        defer { stepv_string_free(raw) }
        return (try? JSONSerialization.jsonObject(with: Data(String(cString: raw).utf8))) as? [String: Any]
    }

    /// PNG of `mesh` at `size` px: the same rasteriser and broken-face overlay
    /// as `stepv --png`.
    static func png(_ mesh: Data, size: Int) -> Data? {
        var out: UnsafeMutablePointer<UInt8>?
        var len = 0
        let rc = mesh.withUnsafeBytes { buf in
            stepv_render_png(buf.bindMemory(to: UInt8.self).baseAddress, buf.count,
                             UInt32(min(max(size, 16), 4096)), false, &out, &len)
        }
        guard rc == 0, let out else {
            stepvLog.info("render failed: \(rc)")
            return nil
        }
        defer { stepv_buffer_free(out, len) }
        return Data(bytes: out, count: len)
    }
}
