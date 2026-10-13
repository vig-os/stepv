// Finder icon-view thumbnails for STEP/IGES/BREP: the kernel, run by this
// extension's supervisor service (#64), plus the Rust rasteriser, so the
// thumbnail is exactly the Linux one, overlay included: approximated faces
// striped amber, missing faces outlined, a badge.

import AppKit
import QuickLookThumbnailing

final class ThumbnailProvider: QLThumbnailProvider {
    override func provideThumbnail(for request: QLFileThumbnailRequest,
                                   _ handler: @escaping (QLThumbnailReply?, Error?) -> Void) {
        let path = request.fileURL.path
        // Notice, not info: persisted, so scripts/ql-e2e can tell a request
        // the extension got from one Quick Look answered from its cache.
        stepvLog.notice("thumbnail request: \(path, privacy: .public)")
        // One reply, whichever comes first: the run's, or the backstop's.
        let lock = NSLock()
        var replied = false
        func reply(_ r: QLThumbnailReply?, _ e: Error?) {
            lock.lock(); let first = !replied; replied = true; lock.unlock()
            if first { handler(r, e) }
        }
        guard let format = StepvKernel.format(of: request.fileURL) else {
            return reply(nil, ThumbnailError.noGeometry("not a STEP, IGES or BREP file"))
        }
        let side = max(request.maximumSize.width, request.maximumSize.height) * request.scale
        let size = request.maximumSize
        let run = StepvSupervisorRun(onEvent: { _, _ in }, completion: { r in
            guard r.outcome == .done, let mesh = r.mesh, let png = StepvKernel.png(mesh, size: Int(side)),
                  let image = NSImage(data: png)
            else {
                // No thumbnail: Quick Look falls back to the generic icon,
                // which is better than a wrong picture.
                let why = (r.summary?["error"] as? String) ?? r.detail
                stepvLog.notice("thumbnail \(path, privacy: .public): none: \(why, privacy: .public)")
                return reply(nil, ThumbnailError.noGeometry(why))
            }
            // What scripts/ql-e2e reads back, as the preview's line.
            stepvLog.notice("thumbnail \(path, privacy: .public): drawn, \(Int(side)) px")
            reply(QLThumbnailReply(contextSize: size, currentContextDrawing: {
                // Aspect-fit the square render into the requested box.
                let s = min(size.width, size.height)
                image.draw(in: NSRect(x: (size.width - s) / 2, y: (size.height - s) / 2, width: s, height: s))
                return true
            }), nil)
        })
        run.start(file: request.fileURL, format: format, linearRel: StepvKernel.thumbnail.linearRel,
                  angularDeg: StepvKernel.thumbnail.angularDeg,
                  deadlineMs: StepvKernel.thumbnailDeadlineMs, memoryMB: StepvKernel.memoryMB)
        // The service kills the kernel at its deadline and replies; this is
        // for a service that cannot: the watchdog must not be what ends it.
        DispatchQueue.global().asyncAfter(deadline: .now() + StepvKernel.thumbnailReplyAfter) {
            run.cancel()
            reply(nil, ThumbnailError.noGeometry("no reply from the supervisor service"))
        }
    }
}

enum ThumbnailError: Error {
    case noGeometry(String?)
}
