// ql-thumb <file> [size] [timeout-s]: ask Quick Look for a thumbnail (this
// runs the registered thumbnail extension) and print the outcome. Exit 0 for
// a real thumbnail, 3 for Quick Look's fallback icon, 124 past the timeout.
// From #79's harness (#64).
import AppKit
import QuickLookThumbnailing
let a = CommandLine.arguments
guard a.count > 1 else { print("usage: ql-thumb <file> [size] [timeout-s]"); exit(2) }
let url = URL(fileURLWithPath: a[1])
let size = a.count > 2 ? Double(a[2])! : 256
let timeout = a.count > 3 ? Double(a[3])! : 30
let req = QLThumbnailGenerator.Request(fileAt: url, size: CGSize(width: size, height: size), scale: 2,
                                       representationTypes: .thumbnail)
let t0 = Date()
let done = DispatchSemaphore(value: 0)
var rc: Int32 = 1
QLThumbnailGenerator.shared.generateBestRepresentation(for: req) { rep, err in
    let ms = Int(Date().timeIntervalSince(t0) * 1000)
    if let rep = rep {
        let types = [0: "icon", 1: "lowQualityThumbnail", 2: "thumbnail"]
        print("ql-thumb: OK type=\(types[rep.type.rawValue] ?? "\(rep.type.rawValue)") cgImage=\(rep.cgImage.width)x\(rep.cgImage.height) in \(ms) ms")
        rc = rep.type == .thumbnail ? 0 : 3
        // QL_THUMB_PNG=<path>: keep the image, to look at what was drawn.
        if let out = ProcessInfo.processInfo.environment["QL_THUMB_PNG"],
           let png = NSBitmapImageRep(cgImage: rep.cgImage).representation(using: .png, properties: [:]) {
            try? png.write(to: URL(fileURLWithPath: out))
        }
    } else {
        print("ql-thumb: ERROR \(err.map { "\($0)" } ?? "nil") in \(ms) ms")
    }
    done.signal()
}
if done.wait(timeout: .now() + timeout) == .timedOut { print("ql-thumb: TIMEOUT after \(Int(timeout)) s"); exit(124) }
exit(rc)
