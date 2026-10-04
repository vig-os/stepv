// Offscreen check of the macOS preview without a window: decode STEPVMSH with
// the extension's Swift reader, build the extension's SceneKit scene, render
// it with SCNRenderer, and report what was decoded, for the caller to compare
// with the Rust side.
//
//   snapshot <mesh.msh> <out.png>   prints: parts=<n> triangles=<n> worst=<s>

import AppKit
import Metal
import SceneKit

let args = CommandLine.arguments
guard args.count == 3 else {
    FileHandle.standardError.write(Data("usage: snapshot <mesh.msh> <out.png>\n".utf8))
    exit(2)
}
do {
    let scene = try decodeStepvMesh(try Data(contentsOf: URL(fileURLWithPath: args[1])))
    let triangles = scene.parts.reduce(0) { $0 + $1.faceIds.count }
    let worst = scene.worstFace.map { "\($0)" } ?? "none"
    print("parts=\(scene.parts.count) triangles=\(triangles) worst=\(worst)")

    let scn = StepvSceneBuilder.build(scene)
    // A camera framing the bounding sphere, as the preview's default view does.
    let (lo, hi) = scn.rootNode.boundingBox
    let centre = SCNVector3((lo.x + hi.x) / 2, (lo.y + hi.y) / 2, (lo.z + hi.z) / 2)
    let radius = max(hi.x - lo.x, hi.y - lo.y, hi.z - lo.z)
    let cam = SCNNode()
    cam.camera = SCNCamera()
    cam.camera?.usesOrthographicProjection = true
    cam.camera?.orthographicScale = Double(radius) * 0.9
    cam.camera?.zFar = Double(radius) * 20
    cam.position = SCNVector3(centre.x + radius * 1.5, centre.y + radius * 1.2, centre.z + radius * 1.5)
    cam.look(at: centre)
    scn.rootNode.addChildNode(cam)
    let light = SCNNode()
    light.light = SCNLight()
    light.light?.type = .directional
    light.eulerAngles = SCNVector3(-0.9, 0.6, 0)
    scn.rootNode.addChildNode(light)
    let ambient = SCNNode()
    ambient.light = SCNLight()
    ambient.light?.type = .ambient
    ambient.light?.intensity = 400
    scn.rootNode.addChildNode(ambient)

    guard let device = MTLCreateSystemDefaultDevice() else { throw NSError(domain: "no Metal device", code: 1) }
    let r = SCNRenderer(device: device, options: nil)
    r.scene = scn
    r.pointOfView = cam
    let img = r.snapshot(atTime: 0, with: CGSize(width: 400, height: 300), antialiasingMode: .multisampling4X)
    guard let tiff = img.tiffRepresentation, let rep = NSBitmapImageRep(data: tiff),
          let png = rep.representation(using: .png, properties: [:])
    else { throw NSError(domain: "encode failed", code: 1) }
    try png.write(to: URL(fileURLWithPath: args[2]))
} catch {
    FileHandle.standardError.write(Data("snapshot: \(error)\n".utf8))
    exit(1)
}
