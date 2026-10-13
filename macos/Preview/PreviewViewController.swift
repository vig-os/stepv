// Space-bar preview for STEP/IGES/BREP: an interactive SceneKit view built
// straight from the kernel's STEPVMSH buffers. Buffers, not USDZ: SceneKit
// cannot read glTF, and the buffers are already planar arrays
// SCNGeometrySource takes as-is (plan.md §4).
//
// The kernel runs in this extension's supervisor service (#64) and streams:
// the header (read here) shows at once, then the assembly tree, then
// progress, then the scene. Quick Look is told the preview is ready
// (handler(nil)) as soon as the header and tree are on screen; a file with
// no tree (IGES, BREP, an outline that failed or was too big) is ready with
// its first mesh or its failure, and every file is at the latest after
// StepvKernel.previewHandlerAfter.
//
// Degrades honestly: if there is no geometry, the header metadata (the
// `stepv --info` reader) is still shown, with the reason — never a blank panel.

import AppKit
import QuickLookUI
import SceneKit

/// Scroll zooms, as in every CAD viewer. SceneKit's camera control would
/// dolly instead, which does nothing for an orthographic camera, and the
/// event would fall through as a pan. A pinch still zooms through SceneKit.
final class ZoomingSceneView: SCNView {
    override func scrollWheel(with event: NSEvent) {
        guard let cam = pointOfView?.camera, cam.usesOrthographicProjection else {
            return super.scrollWheel(with: event)
        }
        let notches = event.hasPreciseScrollingDeltas ? event.scrollingDeltaY / 40 : event.scrollingDeltaY
        cam.orthographicScale *= pow(1.12, -Double(notches))
    }
}

final class PreviewViewController: NSViewController, QLPreviewingController {
    private let sceneView = ZoomingSceneView()
    private let info = NSTextField(labelWithString: "")
    /// The info text's backing: a light panel with dark text reads against
    /// the scene's fixed light grey in light and dark mode alike, where
    /// secondaryLabelColor (light grey in dark mode) did not.
    private let panel = NSView()

    override func loadView() {
        let root = NSView()
        sceneView.translatesAutoresizingMaskIntoConstraints = false
        sceneView.allowsCameraControl = true
        sceneView.autoenablesDefaultLighting = true
        sceneView.backgroundColor = NSColor(calibratedRed: 0.91, green: 0.92, blue: 0.93, alpha: 1)
        info.translatesAutoresizingMaskIntoConstraints = false
        info.font = .systemFont(ofSize: 12)
        info.textColor = NSColor(calibratedWhite: 0.08, alpha: 1)
        info.maximumNumberOfLines = 0
        panel.translatesAutoresizingMaskIntoConstraints = false
        panel.wantsLayer = true
        panel.layer?.backgroundColor = NSColor(calibratedWhite: 1, alpha: 0.88).cgColor
        panel.layer?.cornerRadius = 6
        panel.isHidden = true
        root.addSubview(sceneView)
        root.addSubview(panel)
        panel.addSubview(info)
        NSLayoutConstraint.activate([
            sceneView.leadingAnchor.constraint(equalTo: root.leadingAnchor),
            sceneView.trailingAnchor.constraint(equalTo: root.trailingAnchor),
            sceneView.topAnchor.constraint(equalTo: root.topAnchor),
            sceneView.bottomAnchor.constraint(equalTo: root.bottomAnchor),
            panel.leadingAnchor.constraint(equalTo: root.leadingAnchor, constant: 10),
            panel.trailingAnchor.constraint(lessThanOrEqualTo: root.trailingAnchor, constant: -10),
            panel.topAnchor.constraint(equalTo: root.topAnchor, constant: 10),
            info.leadingAnchor.constraint(equalTo: panel.leadingAnchor, constant: 8),
            info.trailingAnchor.constraint(equalTo: panel.trailingAnchor, constant: -8),
            info.topAnchor.constraint(equalTo: panel.topAnchor, constant: 6),
            info.bottomAnchor.constraint(equalTo: panel.bottomAnchor, constant: -6),
        ])
        preferredContentSize = NSSize(width: 800, height: 600)
        view = root
    }

    private var run: StepvSupervisorRun?
    // The panel's parts, on the main thread: the header, the tree, and the
    // run's status (progress, then the result).
    private var header: [String] = []
    private var tree: [String] = []
    private var status: [String] = []
    private var handler: ((Error?) -> Void)?
    private var started = Date()
    private var path = ""

    deinit { run?.cancel() }  // the preview closed: its kernel goes

    func preparePreviewOfFile(at url: URL, completionHandler handler: @escaping (Error?) -> Void) {
        self.handler = handler
        started = Date()
        path = url.path
        header = StepvSceneBuilder.describe(StepvKernel.info(url))
        guard let format = StepvKernel.format(of: url) else {
            status = ["No preview: not a STEP, IGES or BREP file"]
            showPanel()
            return ready("failure")
        }
        status = ["Reading…"]
        showPanel()
        let run = StepvSupervisorRun(onEvent: { [weak self] kind, payload in
            DispatchQueue.main.async { self?.event(kind, payload) }
        }, completion: { [weak self] result in
            DispatchQueue.main.async { self?.finished(result) }
        })
        self.run = run
        run.start(file: url, format: format, linearRel: StepvKernel.preview.linearRel,
                  angularDeg: StepvKernel.preview.angularDeg,
                  deadlineMs: StepvKernel.previewDeadlineMs, memoryMB: StepvKernel.memoryMB)
        DispatchQueue.main.asyncAfter(deadline: .now() + StepvKernel.previewHandlerAfter) { [weak self] in
            self?.ready("deadline")
        }
    }

    /// Tells Quick Look the preview is up, once; `why` goes to the log.
    private func ready(_ why: String) {
        guard let handler else { return }
        self.handler = nil
        let ms = Int(Date().timeIntervalSince(started) * 1000)
        stepvLog.notice("preview \(self.path, privacy: .public): ready (\(why, privacy: .public)) in \(ms) ms")
        handler(nil)
    }

    private func event(_ kind: UInt8, _ payload: Data) {
        let json = (try? JSONSerialization.jsonObject(with: payload)) as? [String: Any]
        if kind == StepvEvent.tree, let nodes = json?["tree"] as? [[String: Any]] {
            tree = StepvPreviewText.outline(nodes, leaves: json?["leaves"] as? Int)
            showPanel()
            ready("tree")
        } else if kind == StepvEvent.progress, let stage = json?["stage"] as? String,
                  let done = json?["done"] as? Int, let total = json?["total"] as? Int, total > 0 {
            status = ["\(stage == "mesh" ? "Meshing" : "Transferring") \(done * 100 / total) %"]
            showPanel()
        }
    }

    private func finished(_ result: StepvRunResult) {
        run = nil
        var lines: [String] = []
        var built: SCNScene?
        if let data = result.mesh, let scene = try? decodeStepvMesh(data) {
            let scn = StepvSceneBuilder.build(scene)
            // The thumbnail's view, so a flat part opens face-on.
            let view = StepvSceneBuilder.viewAngles(data)
            let camera = StepvSceneBuilder.camera(for: scn, azimuthDeg: view.azimuthDeg,
                                                  elevationDeg: view.elevationDeg)
            scn.rootNode.addChildNode(camera)
            sceneView.scene = scn
            sceneView.pointOfView = camera
            let (lo, hi) = scn.rootNode.boundingBox
            sceneView.defaultCameraController.target =
                SCNVector3((lo.x + hi.x) / 2, (lo.y + hi.y) / 2, (lo.z + hi.z) / 2)
            built = scn
            lines.append("\(scene.parts.count) part\(scene.parts.count == 1 ? "" : "s")")
            switch scene.worstFace {
            case .approx?: lines.append("⚠ some faces approximated (amber)")
            case .missing?: lines.append("⚠ some faces missing (red outline)")
            default: break
            }
        } else {
            // Degrade honestly: the header still says what the file is.
            lines.append("No preview: \(StepvPreviewText.why(result))")
        }
        if let note = StepvSceneBuilder.externalNote(result.summary) { lines.append(note) }
        // The scene speaks for the tree once it is there.
        if built != nil { tree = [] }
        status = lines
        showPanel()
        // What scripts/ql-e2e reads back: this file, and what was shown.
        stepvLog.notice("preview \(self.path, privacy: .public): \(built != nil ? "shown" : "none", privacy: .public): \((self.header + lines).joined(separator: "; "), privacy: .public)")
        ready(built != nil ? "mesh" : "failure")
    }

    private func showPanel() {
        let lines = header + tree + status
        info.stringValue = lines.joined(separator: "\n")
        panel.isHidden = lines.isEmpty
    }
}

/// The preview's text, apart from the view.
enum StepvPreviewText {
    /// The assembly tree as indented lines, at most `limit` of them.
    static func outline(_ roots: [[String: Any]], leaves: Int?, limit: Int = 14) -> [String] {
        var out: [String] = []
        var more = 0
        func walk(_ nodes: [[String: Any]], _ depth: Int) {
            for n in nodes {
                if out.count < limit {
                    out.append(String(repeating: "   ", count: depth) + ((n["name"] as? String) ?? "?"))
                } else {
                    more += 1
                }
                walk((n["children"] as? [[String: Any]]) ?? [], depth + 1)
            }
        }
        walk(roots, 0)
        if more > 0 { out.append("… \(more) more") }
        if let leaves { out.insert("\(leaves) part\(leaves == 1 ? "" : "s"), loading…", at: 0) }
        return out
    }

    /// Why a run gave no geometry, in a person's words.
    static func why(_ r: StepvRunResult) -> String {
        switch r.outcome {
        case .done: return (r.summary?["error"] as? String) ?? "no geometry"
        case .deadline: return "it took longer than Quick Look allows"
        case .memory: return "it needs more memory than Quick Look allows"
        case .crashed, .refused: return r.detail
        }
    }
}
