// ql-preview-host: show a QLPreviewView for argv[1] (this runs the
// registered preview extension), wait argv[2] seconds, quit. With argv[3],
// close the preview after that many seconds (QLPreviewView.close(), the
// view out of its window), and stay up for the rest. Started with
// `open -n -W`, so it is a real app in the window session. From #79's
// harness (#64).
import AppKit
import Quartz
let a = CommandLine.arguments
let file = a.count > 1 ? a[1] : ""
let secs = a.count > 2 ? Double(a[2]) ?? 8 : 8
let closeAfter = a.count > 3 ? Double(a[3]) : nil
final class D: NSObject, NSApplicationDelegate {
    var w: NSWindow!
    func applicationDidFinishLaunching(_ n: Notification) {
        let r = NSRect(x: 0, y: 0, width: 800, height: 600)
        w = NSWindow(contentRect: r, styleMask: [.titled], backing: .buffered, defer: false)
        w.title = "ql-e2e preview: \((file as NSString).lastPathComponent)"
        let v = QLPreviewView(frame: r, style: .normal)!
        v.autostarts = true
        v.previewItem = URL(fileURLWithPath: file) as NSURL
        w.contentView = v
        w.center()
        w.orderFrontRegardless()
        print("ql-preview-host: showing \(file) for \(secs) s")
        if let closeAfter {
            DispatchQueue.main.asyncAfter(deadline: .now() + closeAfter) {
                v.close()
                self.w.contentView = NSView(frame: r)
                print("ql-preview-host: closed the preview at \(closeAfter) s")
            }
        }
        DispatchQueue.main.asyncAfter(deadline: .now() + secs) { NSApp.terminate(nil) }
    }
}
let d = D()
NSApplication.shared.setActivationPolicy(.accessory)
NSApplication.shared.delegate = d
NSApplication.shared.run()
