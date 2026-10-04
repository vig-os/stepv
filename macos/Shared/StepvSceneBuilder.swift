// Building the SceneKit scene the preview shows, from STEPVMSH buffers.
// Shared by the preview extension and macos/Tests (an offscreen snapshot of
// the very same scene, so the preview's output is checked without a window).

import AppKit
import SceneKit

enum StepvSceneBuilder {
    /// The header facts a person recognises, in a fixed order.
    static func describe(_ h: [String: Any]?) -> [String] {
        guard let h else { return [] }
        var out: [String] = []
        let format = (h["format"] as? String)?.uppercased() ?? "CAD"
        let proto = h["protocol"] as? String
        out.append([format, proto].compactMap { $0 }.joined(separator: " · "))
        if let s = h["originating_system"] as? String, !s.isEmpty { out.append(s) }
        if let n = h["name"] as? String, !n.isEmpty { out.append(n) }
        return out
    }

    /// The view to open on, from the renderer (stepv_view_angles): the same
    /// one `stepv --png` and the thumbnail use, so a flat part is face-on.
    static func viewAngles(_ mesh: Data) -> (azimuthDeg: Float, elevationDeg: Float) {
        var az: Float = -35, el: Float = 30  // render::Camera::default()
        _ = mesh.withUnsafeBytes { buf in
            stepv_view_angles(buf.bindMemory(to: UInt8.self).baseAddress, buf.count, &az, &el)
        }
        return (az, el)
    }

    /// An orthographic camera on `scene` from stepv's orbit angles, framing
    /// its bounding sphere. The direction toward the eye and the screen's up
    /// are render.rs's view() inverted: (-sin a cos e, sin e, cos a cos e)
    /// and (sin a sin e, cos e, -cos a sin e). That up is well-defined
    /// straight overhead, where the world's Y would make look(at:) degenerate.
    static func camera(for scene: SCNScene, azimuthDeg: Float, elevationDeg: Float) -> SCNNode {
        let (lo, hi) = scene.rootNode.boundingBox
        let centre = SCNVector3((lo.x + hi.x) / 2, (lo.y + hi.y) / 2, (lo.z + hi.z) / 2)
        let (dx, dy, dz) = (hi.x - lo.x, hi.y - lo.y, hi.z - lo.z)
        let radius = max((dx * dx + dy * dy + dz * dz).squareRoot() / 2, 1e-6)
        let a = CGFloat(azimuthDeg) * .pi / 180, e = CGFloat(elevationDeg) * .pi / 180
        let toward = SCNVector3(-sin(a) * cos(e), sin(e), cos(a) * cos(e))
        let up = SCNVector3(sin(a) * sin(e), cos(e), -cos(a) * sin(e))
        let node = SCNNode()
        node.name = "stepv-camera"
        let cam = SCNCamera()
        cam.usesOrthographicProjection = true
        cam.orthographicScale = Double(radius) * 1.1
        cam.zNear = Double(radius) * 0.01
        cam.zFar = Double(radius) * 10
        node.camera = cam
        node.position = SCNVector3(centre.x + toward.x * radius * 4, centre.y + toward.y * radius * 4,
                                   centre.z + toward.z * radius * 4)
        node.look(at: centre, up: up, localFront: SCNVector3(0, 0, -1))
        return node
    }

    /// SceneKit scene from the buffers: one node per part, triangles grouped
    /// by (colour, faithful?) into elements, approximated faces amber,
    /// missing-face outlines and sketch curves as lines; construction hidden.
    static func build(_ s: StepvScene) -> SCNScene {
        let scene = SCNScene()
        let root = SCNNode()
        root.eulerAngles.x = -.pi / 2  // CAD Z-up -> SceneKit Y-up
        scene.rootNode.addChildNode(root)
        let fallback = StepvColor(r: 0.38, g: 0.43, b: 0.50)
        for part in s.parts {
            let node = SCNNode()
            node.name = part.name
            if !part.indices.isEmpty {
                let vertexCount = part.positions.count / 3
                let pos = SCNGeometrySource(data: Data(bytes: part.positions, count: part.positions.count * 4),
                                            semantic: .vertex, vectorCount: vertexCount, usesFloatComponents: true,
                                            componentsPerVector: 3, bytesPerComponent: 4, dataOffset: 0, dataStride: 12)
                let nrm = SCNGeometrySource(data: Data(bytes: part.normals, count: part.normals.count * 4),
                                            semantic: .normal, vectorCount: vertexCount, usesFloatComponents: true,
                                            componentsPerVector: 3, bytesPerComponent: 4, dataOffset: 0, dataStride: 12)
                struct Key: Hashable { var r: Float, g: Float, b: Float, approx: Bool }
                var groups: [Key: [UInt32]] = [:]
                for t in 0..<part.faceIds.count {
                    let fid = part.faceIds[t]
                    let approx = Int(fid) < part.faces.count && part.faces[Int(fid)].status == .approx
                    let c = part.faceColor(fid) ?? fallback
                    groups[Key(r: c.r, g: c.g, b: c.b, approx: approx), default: []]
                        .append(contentsOf: part.indices[(t * 3)..<(t * 3 + 3)])
                }
                var elements: [SCNGeometryElement] = []
                var materials: [SCNMaterial] = []
                for (k, idx) in groups {
                    elements.append(SCNGeometryElement(data: Data(bytes: idx, count: idx.count * 4),
                                                       primitiveType: .triangles, primitiveCount: idx.count / 3,
                                                       bytesPerIndex: 4))
                    let m = SCNMaterial()
                    m.isDoubleSided = true  // CAD faces are not reliably oriented
                    m.diffuse.contents = k.approx
                        ? NSColor(calibratedRed: 0.96, green: 0.62, blue: 0.12, alpha: 1)
                        : NSColor(calibratedRed: CGFloat(k.r), green: CGFloat(k.g), blue: CGFloat(k.b), alpha: 1)
                    materials.append(m)
                }
                let g = SCNGeometry(sources: [pos, nrm], elements: elements)
                g.materials = materials
                node.addChildNode(SCNNode(geometry: g))
            }
            for kind in [StepvLineKind.sketch, .missingOutline] {
                var pts: [Float] = []
                for (i, k) in part.segmentKinds.enumerated() where k == kind {
                    pts.append(contentsOf: part.segments[(i * 6)..<(i * 6 + 6)])
                }
                guard !pts.isEmpty else { continue }
                let n = pts.count / 3
                let src = SCNGeometrySource(data: Data(bytes: pts, count: pts.count * 4), semantic: .vertex,
                                            vectorCount: n, usesFloatComponents: true, componentsPerVector: 3,
                                            bytesPerComponent: 4, dataOffset: 0, dataStride: 12)
                let idx = (0..<UInt32(n)).map { $0 }
                let el = SCNGeometryElement(data: Data(bytes: idx, count: idx.count * 4), primitiveType: .line,
                                            primitiveCount: n / 2, bytesPerIndex: 4)
                let g = SCNGeometry(sources: [src], elements: [el])
                let m = SCNMaterial()
                m.lightingModel = .constant
                m.diffuse.contents = kind == .missingOutline
                    ? NSColor(calibratedRed: 0.85, green: 0.05, blue: 0.10, alpha: 1)
                    : NSColor(calibratedWhite: 0.12, alpha: 1)
                g.materials = [m]
                node.addChildNode(SCNNode(geometry: g))
            }
            root.addChildNode(node)
        }
        return scene
    }
}
