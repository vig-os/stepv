// Reader for the kernel's STEPVMSH v3 buffers (spec: kernel/stepv-occt.cpp).
// The Swift twin of src/occt.rs `read_mesh`; keep the two in step.

import Foundation

struct StepvColor: Equatable {
    var r: Float, g: Float, b: Float
}

/// FaceStatus from src/lib.rs. Raw values are the wire bytes.
enum StepvFaceStatus: UInt8 {
    case ok = 0, remeshed, healed, refined, coarse, degenerate, approx, missing

    /// Drawn without a warning treatment (lib.rs `is_faithful`).
    var isFaithful: Bool { rawValue <= StepvFaceStatus.degenerate.rawValue }
}

/// LineKind from src/lib.rs.
enum StepvLineKind: UInt8 {
    case sketch = 0, missingOutline = 1, construction = 2
}

struct StepvFace {
    var status: StepvFaceStatus
    var color: StepvColor?
}

struct StepvPart {
    var name: String?
    var color: StepvColor?
    var faces: [StepvFace]
    var positions: [Float]
    var normals: [Float]
    var indices: [UInt32]
    var faceIds: [UInt32]
    var segments: [Float]
    var segmentKinds: [StepvLineKind]

    func faceColor(_ id: UInt32) -> StepvColor? {
        Int(id) < faces.count ? (faces[Int(id)].color ?? color) : color
    }
}

struct StepvScene {
    var bboxMin: (Double, Double, Double)
    var bboxMax: (Double, Double, Double)
    var parts: [StepvPart]

    var worstFace: StepvFaceStatus? {
        parts.flatMap { $0.faces.map(\.status) }.max { $0.rawValue < $1.rawValue }
    }
}

enum StepvMeshError: Error, Equatable {
    case badMagic, unsupportedVersion(UInt32), truncated, badEnum(UInt8), trailingBytes(Int)
}

/// Decodes STEPVMSH v3. Bounds every count against the remaining bytes
/// before allocating, like the Rust reader.
func decodeStepvMesh(_ data: Data) throws -> StepvScene {
    var c = Cursor(bytes: [UInt8](data))
    guard c.remaining >= 8, Array(c.take(8)) == Array("STEPVMSH".utf8) else { throw StepvMeshError.badMagic }
    let version = try c.u32()
    guard version == 3 else { throw StepvMeshError.unsupportedVersion(version) }
    var b = [Double]()
    for _ in 0..<6 { b.append(try c.f64()) }
    let partCount = Int(try c.u32())
    guard partCount <= c.remaining / 33 else { throw StepvMeshError.truncated }
    var parts: [StepvPart] = []
    parts.reserveCapacity(partCount)
    for _ in 0..<partCount {
        let nameLen = Int(try c.u32())
        guard nameLen <= c.remaining else { throw StepvMeshError.truncated }
        let name = String(decoding: c.take(nameLen), as: UTF8.self)
        let hasColor = try c.u8() != 0
        let rgb = StepvColor(r: try c.f32(), g: try c.f32(), b: try c.f32())
        let faceCount = Int(try c.u32())
        guard faceCount <= c.remaining / 14 else { throw StepvMeshError.truncated }
        var faces: [StepvFace] = []
        faces.reserveCapacity(faceCount)
        for _ in 0..<faceCount {
            let s = try c.u8()
            guard let status = StepvFaceStatus(rawValue: s) else { throw StepvMeshError.badEnum(s) }
            let has = try c.u8() != 0
            let fc = StepvColor(r: try c.f32(), g: try c.f32(), b: try c.f32())
            faces.append(StepvFace(status: status, color: has ? fc : nil))
        }
        let vertices = Int(try c.u32())
        let triangles = Int(try c.u32())
        let positions = try c.floats(vertices * 3)
        let normals = try c.floats(vertices * 3)
        let indices = try c.u32s(triangles * 3)
        let faceIds = try c.u32s(triangles)
        let segments = Int(try c.u32())
        let segPoints = try c.floats(segments * 6)
        guard segments <= c.remaining else { throw StepvMeshError.truncated }
        var kinds: [StepvLineKind] = []
        for byte in c.take(segments) {
            guard let k = StepvLineKind(rawValue: byte) else { throw StepvMeshError.badEnum(byte) }
            kinds.append(k)
        }
        parts.append(StepvPart(name: nameLen > 0 ? name : nil, color: hasColor ? rgb : nil, faces: faces,
                               positions: positions, normals: normals, indices: indices, faceIds: faceIds,
                               segments: segPoints, segmentKinds: kinds))
    }
    guard c.remaining == 0 else { throw StepvMeshError.trailingBytes(c.remaining) }
    return StepvScene(bboxMin: (b[0], b[1], b[2]), bboxMax: (b[3], b[4], b[5]), parts: parts)
}

private struct Cursor {
    let bytes: [UInt8]
    var i = 0
    var remaining: Int { bytes.count - i }

    mutating func take(_ n: Int) -> ArraySlice<UInt8> {
        defer { i += n }
        return bytes[i..<(i + n)]
    }

    mutating func need(_ n: Int) throws {
        guard n >= 0, n <= remaining else { throw StepvMeshError.truncated }
    }

    mutating func u8() throws -> UInt8 { try need(1); return take(1).first! }

    mutating func u32() throws -> UInt32 {
        try need(4)
        return take(4).reversed().reduce(0) { ($0 << 8) | UInt32($1) }
    }

    mutating func f32() throws -> Float { Float(bitPattern: try u32()) }

    mutating func f64() throws -> Double {
        try need(8)
        return Double(bitPattern: take(8).reversed().reduce(UInt64(0)) { ($0 << 8) | UInt64($1) })
    }

    mutating func floats(_ n: Int) throws -> [Float] {
        try need(n * 4)
        return (0..<n).map { _ in Float(bitPattern: take(4).reversed().reduce(UInt32(0)) { ($0 << 8) | UInt32($1) }) }
    }

    mutating func u32s(_ n: Int) throws -> [UInt32] {
        try need(n * 4)
        return (0..<n).map { _ in take(4).reversed().reduce(UInt32(0)) { ($0 << 8) | UInt32($1) } }
    }
}
