// Space-bar preview for STEP/IGES/BREP: an interactive SceneKit view built
// straight from the in-process kernel's STEPVMSH buffers. Buffers, not
// USDZ: SceneKit cannot read glTF, and the buffers are already planar arrays
// SCNGeometrySource takes as-is (plan.md §4).
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

    func preparePreviewOfFile(at url: URL, completionHandler handler: @escaping (Error?) -> Void) {
        // The kernel can take seconds on a large assembly: off the main thread,
        // then back to it to touch the views.
        DispatchQueue.global(qos: .userInitiated).async {
            var lines = StepvSceneBuilder.describe(StepvKernel.info(url))
            let run = StepvKernel.tessellate(url, linearRel: StepvKernel.preview.linearRel,
                                             angularDeg: StepvKernel.preview.angularDeg)
            var built: SCNScene?
            var camera: SCNNode?
            if let data = run.mesh, let scene = try? decodeStepvMesh(data) {
                let scn = StepvSceneBuilder.build(scene)
                // The thumbnail's view, so a flat part opens face-on.
                let view = StepvSceneBuilder.viewAngles(data)
                camera = StepvSceneBuilder.camera(for: scn, azimuthDeg: view.azimuthDeg,
                                                  elevationDeg: view.elevationDeg)
                camera.map(scn.rootNode.addChildNode)
                built = scn
                lines.append("\(scene.parts.count) part\(scene.parts.count == 1 ? "" : "s")")
                switch scene.worstFace {
                case .approx?: lines.append("⚠ some faces approximated (amber)")
                case .missing?: lines.append("⚠ some faces missing (red outline)")
                default: break
                }
            } else {
                // Degrade honestly: the header still says what the file is.
                lines.append("No preview: \(run.error ?? "no geometry")")
            }
            DispatchQueue.main.async {
                self.sceneView.scene = built
                if let camera, let built {
                    self.sceneView.pointOfView = camera
                    let (lo, hi) = built.rootNode.boundingBox
                    self.sceneView.defaultCameraController.target =
                        SCNVector3((lo.x + hi.x) / 2, (lo.y + hi.y) / 2, (lo.z + hi.z) / 2)
                }
                self.info.stringValue = lines.joined(separator: "\n")
                self.panel.isHidden = lines.isEmpty
                handler(nil)
            }
        }
    }

}
