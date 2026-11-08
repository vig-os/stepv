// ql-preview-host: show a QLPreviewView for argv[1] (this runs the
// registered preview extension), and quit after argv[2] seconds at the
// latest. With argv[3], close the preview (QLPreviewView.close(), the view
// out of its window), or with argv[4] too, switch the view to the file
// argv[4] instead: at argv[3] seconds, or with argv[3] = "signal" on
// SIGUSR1, which run-all.sh sends once the extension has logged an event,
// and then quit 3 s later ("signal-stay": stay up to argv[2] seconds). Started with `open -n`, so it is a real app in the
// window session. From #79's harness (#64).
import AppKit
import Quartz
import os
// Its pid, the close, the switch and the quit go to the unified log, beside
// the extension's and the service's lines: run-all.sh signals it by that
// pid, and orders the kernel's cancel between the close and the quit.
let hostLog = Logger(subsystem: "ch.exoma.stepv", category: "ql-e2e")
let a = CommandLine.arguments
let file = a.count > 1 ? a[1] : ""
let secs = a.count > 2 ? Double(a[2]) ?? 8 : 8
let act = a.count > 3 ? a[3] : nil
let switchTo = a.count > 4 ? a[4] : nil
func quit() {
    hostLog.notice("preview host quits \(file, privacy: .public)")
    NSApp.terminate(nil)
}
final class D: NSObject, NSApplicationDelegate {
    var w: NSWindow!
    var v: QLPreviewView!
    var signal: DispatchSourceSignal?
    func applicationDidFinishLaunching(_ n: Notification) {
        let r = NSRect(x: 0, y: 0, width: 800, height: 600)
        w = NSWindow(contentRect: r, styleMask: [.titled], backing: .buffered, defer: false)
        w.title = "ql-e2e preview: \((file as NSString).lastPathComponent)"
        v = QLPreviewView(frame: r, style: .normal)!
        v.autostarts = true
        v.previewItem = URL(fileURLWithPath: file) as NSURL
        w.contentView = v
        w.center()
        w.orderFrontRegardless()
        print("ql-preview-host: showing \(file) for \(secs) s at most")
        if act == "signal" || act == "signal-stay" {
            Darwin.signal(SIGUSR1, SIG_IGN)
            let s = DispatchSource.makeSignalSource(signal: SIGUSR1, queue: .main)
            s.setEventHandler { [unowned self] in self.go() }
            s.resume()
            signal = s
        } else if let after = act.flatMap(Double.init) {
            DispatchQueue.main.asyncAfter(deadline: .now() + after) { self.go() }
        }
        hostLog.notice("preview host shows \(file, privacy: .public) as pid \(getpid())")
        DispatchQueue.main.asyncAfter(deadline: .now() + secs) { quit() }
    }
    /// Closes or switches the preview, once; on a signal, quits 3 s later.
    func go() {
        signal?.cancel()
        signal = nil
        if let switchTo {
            hostLog.notice("preview host switched \(file, privacy: .public) to \(switchTo, privacy: .public)")
            v.previewItem = URL(fileURLWithPath: switchTo) as NSURL
        } else {
            hostLog.notice("preview host closed \(file, privacy: .public)")
            v.close()
            w.contentView = NSView(frame: w.contentView?.frame ?? .zero)
        }
        if act == "signal" { DispatchQueue.main.asyncAfter(deadline: .now() + 3) { quit() } }
    }
}
let d = D()
NSApplication.shared.setActivationPolicy(.accessory)
NSApplication.shared.delegate = d
NSApplication.shared.run()
