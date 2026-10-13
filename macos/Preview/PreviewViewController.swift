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

    /// What the background decode hands the main thread: the scene and its
    /// camera, or nil, with the panel's lines.
    private typealias Prepared = (scene: SCNScene?, camera: SCNNode?, lines: [String])
    private var session: StepvPreviewSession<Prepared>?
    // The panel's parts, on the main thread: the header, the tree, and the
    // run's status (progress, then the result).
    private var header: [String] = []
    private var tree: [String] = []
    private var status: [String] = []
    private var handler: ((Error?) -> Void)?
    private var started = Date()
    private var path = ""

    // Closing the preview releases this controller: Quick Look has no
    // cancel callback, and the remote view gets no disappear calls, but
    // QLPreviewView.close() and a closed panel release it (ql-e2e proves the
    // kernel is reaped then). Its kernel goes with it.
    deinit { session?.cancel() }

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
        let session = StepvPreviewSession<Prepared>(prepare: PreviewViewController.prepare)
        session.onTree = { [weak self] lines in self?.tree = lines; self?.showPanel() }
        session.onProgress = { [weak self] line in self?.status = [line]; self?.showPanel() }
        session.onResult = { [weak self] p in self?.show(p) }
        session.onReady = { [weak self] why in self?.ready(why) }
        self.session = session
        session.start(readyAfter: StepvKernel.previewHandlerAfter,
                      replyAfter: StepvKernel.previewReplyAfter) { onEvent, completion in
            let run = StepvSupervisorRun(onEvent: onEvent, completion: completion)
            run.start(file: url, format: format, linearRel: StepvKernel.preview.linearRel,
                      angularDeg: StepvKernel.preview.angularDeg,
                      deadlineMs: StepvKernel.previewDeadlineMs, memoryMB: StepvKernel.memoryMB)
            return run.cancel
        }
    }

    /// Tells Quick Look the preview is up; the session calls it once.
    private func ready(_ why: String) {
        guard let handler else { return }
        self.handler = nil
        let ms = Int(Date().timeIntervalSince(started) * 1000)
        stepvLog.notice("preview \(self.path, privacy: .public): ready (\(why, privacy: .public)) in \(ms) ms")
        handler(nil)
    }

    /// Off the main thread: the mesh decoded and its scene built.
    private static func prepare(_ result: StepvRunResult) -> Prepared {
        var lines: [String] = []
        var scene: SCNScene?
        var camera: SCNNode?
        if let data = result.mesh, let decoded = try? decodeStepvMesh(data) {
            let scn = StepvSceneBuilder.build(decoded)
            // The thumbnail's view, so a flat part opens face-on.
            let view = StepvSceneBuilder.viewAngles(data)
            let cam = StepvSceneBuilder.camera(for: scn, azimuthDeg: view.azimuthDeg,
                                               elevationDeg: view.elevationDeg)
            scn.rootNode.addChildNode(cam)
            (scene, camera) = (scn, cam)
            lines.append("\(decoded.parts.count) part\(decoded.parts.count == 1 ? "" : "s")")
            switch decoded.worstFace {
            case .approx?: lines.append("⚠ some faces approximated (amber)")
            case .missing?: lines.append("⚠ some faces missing (red outline)")
            default: break
            }
        } else {
            // Degrade honestly: the header still says what the file is.
            lines.append("No preview: \(StepvPreviewText.why(result))")
        }
        if let note = StepvSceneBuilder.externalNote(result.summary) { lines.append(note) }
        return (scene, camera, lines)
    }

    /// On the main thread: the prepared result applied.
    private func show(_ p: Prepared) {
        if let scene = p.scene, let camera = p.camera {
            sceneView.scene = scene
            sceneView.pointOfView = camera
            let (lo, hi) = scene.rootNode.boundingBox
            sceneView.defaultCameraController.target =
                SCNVector3((lo.x + hi.x) / 2, (lo.y + hi.y) / 2, (lo.z + hi.z) / 2)
            tree = []  // the scene speaks for the tree once it is there
        }
        status = p.lines
        showPanel()
        // What scripts/ql-e2e reads back: this file, and what was shown.
        stepvLog.notice("preview \(self.path, privacy: .public): \(p.scene != nil ? "shown" : "none", privacy: .public): \((self.header + p.lines).joined(separator: "; "), privacy: .public)")
        session?.markReady(p.scene != nil ? "mesh" : "failure")
    }

    private func showPanel() {
        let lines = header + tree + status
        info.stringValue = lines.joined(separator: "\n")
        panel.isHidden = lines.isEmpty
    }
}
