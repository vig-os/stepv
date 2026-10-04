// Finder icon-view thumbnails for STEP/IGES/BREP: the in-process kernel plus
// the Rust rasteriser, so the thumbnail is exactly the Linux one, overlay
// included: approximated faces striped amber, missing faces outlined, a badge.

import AppKit
import QuickLookThumbnailing

final class ThumbnailProvider: QLThumbnailProvider {
    override func provideThumbnail(for request: QLFileThumbnailRequest,
                                   _ handler: @escaping (QLThumbnailReply?, Error?) -> Void) {
        stepvLog.info("thumbnail request: \(request.fileURL.lastPathComponent, privacy: .public)")
        let side = max(request.maximumSize.width, request.maximumSize.height) * request.scale
        let run = StepvKernel.tessellate(request.fileURL, linearRel: StepvKernel.thumbnail.linearRel,
                                         angularDeg: StepvKernel.thumbnail.angularDeg)
        guard let mesh = run.mesh, let png = StepvKernel.png(mesh, size: Int(side)),
              let image = NSImage(data: png)
        else {
            // No thumbnail: Quick Look falls back to the generic icon, which is
            // better than a wrong picture.
            handler(nil, ThumbnailError.noGeometry(run.error))
            return
        }
        let size = request.maximumSize
        handler(QLThumbnailReply(contextSize: size, currentContextDrawing: {
            // Aspect-fit the square render into the requested box.
            let s = min(size.width, size.height)
            image.draw(in: NSRect(x: (size.width - s) / 2, y: (size.height - s) / 2, width: s, height: s))
            return true
        }), nil)
    }
}

enum ThumbnailError: Error {
    case noGeometry(String?)
}
