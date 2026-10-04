// stepv.app: the container the Quick Look extensions ship in. Launching it
// once registers them; the window just says what to do next.

import AppKit

final class AppDelegate: NSObject, NSApplicationDelegate {
    var window: NSWindow!

    func applicationDidFinishLaunching(_ notification: Notification) {
        window = NSWindow(contentRect: NSRect(x: 0, y: 0, width: 460, height: 170),
                          styleMask: [.titled, .closable], backing: .buffered, defer: false)
        window.title = "stepv"
        let text = NSTextField(wrappingLabelWithString: """
            stepv's Quick Look extensions are installed.

            In Finder, select a STEP, IGES or BREP file and press Space for an interactive \
            preview; icon view shows thumbnails. If nothing happens, enable "stepv" under \
            System Settings → General → Login Items & Extensions → Quick Look.
            """)
        text.frame = NSRect(x: 20, y: 20, width: 420, height: 130)
        window.contentView?.addSubview(text)
        window.center()
        window.makeKeyAndOrderFront(nil)
        NSApp.activate(ignoringOtherApps: true)
    }

    func applicationShouldTerminateAfterLastWindowClosed(_ sender: NSApplication) -> Bool { true }
}

let app = NSApplication.shared
let delegate = AppDelegate()
app.delegate = delegate
app.setActivationPolicy(.regular)
app.run()
