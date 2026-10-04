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

final class PreviewViewController: NSViewController, QLPreviewingController {
    private let sceneView = SCNView()
    private let info = NSTextField(labelWithString: "")

    override func loadView() {
        let root = NSView()
        sceneView.translatesAutoresizingMaskIntoConstraints = false
        sceneView.allowsCameraControl = true
        sceneView.autoenablesDefaultLighting = true
        sceneView.backgroundColor = NSColor(calibratedRed: 0.91, green: 0.92, blue: 0.93, alpha: 1)
        info.translatesAutoresizingMaskIntoConstraints = false
        info.font = .systemFont(ofSize: 11)
        info.textColor = .secondaryLabelColor
        info.maximumNumberOfLines = 0
        root.addSubview(sceneView)
        root.addSubview(info)
        NSLayoutConstraint.activate([
            sceneView.leadingAnchor.constraint(equalTo: root.leadingAnchor),
            sceneView.trailingAnchor.constraint(equalTo: root.trailingAnchor),
            sceneView.topAnchor.constraint(equalTo: root.topAnchor),
            sceneView.bottomAnchor.constraint(equalTo: root.bottomAnchor),
            info.leadingAnchor.constraint(equalTo: root.leadingAnchor, constant: 10),
            info.trailingAnchor.constraint(lessThanOrEqualTo: root.trailingAnchor, constant: -10),
            info.topAnchor.constraint(equalTo: root.topAnchor, constant: 8),
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
            if let data = run.mesh, let scene = try? decodeStepvMesh(data) {
                built = StepvSceneBuilder.build(scene)
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
                self.info.stringValue = lines.joined(separator: "\n")
                handler(nil)
            }
        }
    }

}
