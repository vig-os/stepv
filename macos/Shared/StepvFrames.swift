// The kernel's event stream (`stepv-occt --stream`, kernel/stepv_occt.h) read
// in Swift: what the supervisor service makes of its child's stdout (#64).
// The same contract and bounds as src/occt.rs's read_events: "SVEV", the kind
// as one byte, the payload's length as a little-endian u32, the payload. The
// last frame is DONE; a stream that ends without it is a crashed kernel.
//
// The payloads stay JSON bytes here: the service forwards them, and the
// extension that shows them decodes them.

import Foundation

enum StepvEvent {
    static let tree: UInt8 = 1
    static let progress: UInt8 = 2
    static let done: UInt8 = 3
    /// STEPV_OCCT_MAX_FRAME: checked before a payload is buffered.
    static let maxFrame: UInt32 = 64 << 20
}

struct StepvFrame: Equatable {
    let kind: UInt8
    let payload: Data
}

/// A stream that breaks the contract. The kernel writes it, so each is a
/// crashed or misbehaving kernel, never bad input.
enum StepvStreamError: Error, Equatable, CustomStringConvertible {
    /// It ended without the summary frame, at a frame boundary.
    case noSummary
    /// It ended partway through a frame.
    case truncated
    case badMagic
    case badKind(UInt8)
    case tooLarge(UInt32)
    /// Bytes after the summary.
    case trailingBytes

    var description: String {
        switch self {
        case .noSummary: return "event stream ended without its summary"
        case .truncated: return "event stream ended partway through a frame"
        case .badMagic: return "not an event frame"
        case .badKind(let k): return "unknown event kind \(k)"
        case .tooLarge(let n): return "event frame of \(n) bytes is over the cap"
        case .trailingBytes: return "bytes after the summary"
        }
    }
}

/// Decodes the stream incrementally, as a pipe delivers it: `feed` takes
/// whatever arrived and returns the frames it completed, DONE last; `finish`
/// is the end of the stream. After an error the reader stays failed: every
/// later call throws it.
struct StepvFrameReader {
    private var buffer = Data()
    /// The error that ended the stream, once there is one: set even when
    /// the call that met it returned frames.
    private(set) var failed: StepvStreamError?
    private(set) var sawDone = false

    mutating func feed(_ bytes: Data) throws -> [StepvFrame] {
        if let failed { throw failed }
        buffer.append(bytes)
        var frames: [StepvFrame] = []
        do {
            while !buffer.isEmpty {
                if sawDone { throw StepvStreamError.trailingBytes }
                // The header is checked as soon as it is whole, before the
                // payload is waited for: a bad stream fails at once.
                guard buffer.count >= 9 else { break }
                let head = [UInt8](buffer.prefix(9))
                guard head[0..<4] == [0x53, 0x56, 0x45, 0x56] else { throw StepvStreamError.badMagic } // "SVEV"
                let kind = head[4]
                guard (StepvEvent.tree...StepvEvent.done).contains(kind) else { throw StepvStreamError.badKind(kind) }
                let len = UInt32(head[5]) | UInt32(head[6]) << 8 | UInt32(head[7]) << 16 | UInt32(head[8]) << 24
                guard len <= StepvEvent.maxFrame else { throw StepvStreamError.tooLarge(len) }
                guard buffer.count >= 9 + Int(len) else { break }
                let start = buffer.startIndex + 9
                frames.append(StepvFrame(kind: kind, payload: Data(buffer[start..<start + Int(len)])))
                buffer = Data(buffer[(start + Int(len))...])
                sawDone = kind == StepvEvent.done
            }
        } catch let e as StepvStreamError {
            // The frames before the error still count, as in read_events:
            // they are returned, and the next call throws.
            failed = e
            if frames.isEmpty { throw e }
        }
        return frames
    }

    /// The stream ended: fine only right after DONE.
    mutating func finish() throws {
        if let failed { throw failed }
        if !buffer.isEmpty { failed = .truncated } else if !sawDone { failed = .noSummary }
        if let failed { throw failed }
    }
}
