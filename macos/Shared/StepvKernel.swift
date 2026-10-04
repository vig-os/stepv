// stepv inside a Quick Look extension: the kernel and the renderer called
// IN-PROCESS. The extension sandbox forbids exec (posix_spawn fails with
// EPERM), so the CLI the Linux thumbnailer uses cannot run here; these are the
// same code paths through their C ABIs (Bridge.h).
//
// Containment: on macOS the extension process itself is the boundary. The
// system runs it apart from Finder and Quick Look, and kills it on hang or
// memory pressure; a file that crashes OCCT fails that one preview.

import Foundation
import os

/// Everything the extensions report lands in the unified log:
///   /usr/bin/log show --predicate 'subsystem == "ch.exoma.stepv"' --info
let stepvLog = Logger(subsystem: "ch.exoma.stepv", category: "quicklook")

enum StepvKernel {
    /// Deflection settings, as stepv::Deflection.
    static let thumbnail = (linearRel: 0.005, angularDeg: 30.0)
    static let preview = (linearRel: 0.001, angularDeg: 20.0)

    struct Run {
        var mesh: Data?
        var summary: [String: Any]?
        var error: String? { summary?["error"] as? String }
    }

    /// Tessellates `url`; `mesh` is STEPVMSH v3 on success.
    static func tessellate(_ url: URL, linearRel: Double, angularDeg: Double) -> Run {
        let out = FileManager.default.temporaryDirectory
            .appendingPathComponent("stepv-\(UUID().uuidString).msh")
        defer { try? FileManager.default.removeItem(at: out) }
        var code: Int32 = 3
        guard let raw = stepv_occt_run(url.path, out.path, linearRel, angularDeg, &code) else {
            return Run(mesh: nil, summary: nil)
        }
        defer { stepv_occt_free(raw) }
        let summary = (try? JSONSerialization.jsonObject(with: Data(String(cString: raw).utf8))) as? [String: Any]
        let mesh = code == 0 ? try? Data(contentsOf: out) : nil
        if mesh == nil {
            stepvLog.info("no geometry for \(url.lastPathComponent, privacy: .public): \((summary?["error"] as? String) ?? "?", privacy: .public)")
        }
        return Run(mesh: mesh, summary: summary)
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
