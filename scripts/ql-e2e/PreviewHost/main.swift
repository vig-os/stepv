// ql-preview-host: show a QLPreviewView for argv[1] (this runs the
// registered preview extension), wait argv[2] seconds, quit. With argv[3],
// at that many seconds close the preview (QLPreviewView.close(), the view
// out of its window), or with argv[4] too, switch the view to the file
// argv[4] instead; either way stay up for the rest. Started with
// `open -n -W`, so it is a real app in the window session. From #79's
// harness (#64).
import AppKit
import Quartz
import os
// The close, the switch and the quit go to the unified log, beside the
// extension's and the service's lines: run-all.sh orders the kernel's
// cancel between them.
let hostLog = Logger(subsystem: "ch.exoma.stepv", category: "ql-e2e")
let a = CommandLine.arguments
let file = a.count > 1 ? a[1] : ""
let secs = a.count > 2 ? Double(a[2]) ?? 8 : 8
let actAfter = a.count > 3 ? Double(a[3]) : nil
let switchTo = a.count > 4 ? a[4] : nil
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
        if let actAfter {
            DispatchQueue.main.asyncAfter(deadline: .now() + actAfter) {
                if let switchTo {
                    hostLog.notice("preview host switched \(file, privacy: .public) to \(switchTo, privacy: .public)")
                    v.previewItem = URL(fileURLWithPath: switchTo) as NSURL
                    print("ql-preview-host: switched to \(switchTo) at \(actAfter) s")
                } else {
                    hostLog.notice("preview host closed \(file, privacy: .public)")
                    v.close()
                    self.w.contentView = NSView(frame: r)
                    print("ql-preview-host: closed the preview at \(actAfter) s")
                }
            }
        }
        DispatchQueue.main.asyncAfter(deadline: .now() + secs) {
            hostLog.notice("preview host quits \(file, privacy: .public)")
            NSApp.terminate(nil)
        }
    }
}
let d = D()
NSApplication.shared.setActivationPolicy(.accessory)
NSApplication.shared.delegate = d
NSApplication.shared.run()
