// Unit tests of the Swift SVEV reader (macos/Shared/StepvFrames.swift, #64)
// against src/occt.rs's read_events contract: a real kernel stream decodes
// the same however a pipe splits it, and every broken stream fails as
// read_events fails it, with the frames before the break still handed over.
//
//   frames-tests <stream>   <stream>: `stepv-occt <file.step> --stream` output

import Foundation

var failures = 0
func check(_ ok: Bool, _ what: String) {
    print(ok ? "ok: \(what)" : "FAIL: \(what)")
    if !ok { failures += 1 }
}

func frame(_ kind: UInt8, _ payload: Data, magic: String = "SVEV", length: UInt32? = nil) -> Data {
    var d = Data(magic.utf8)
    d.append(kind)
    let n = length ?? UInt32(payload.count)
    d.append(contentsOf: [UInt8(n & 0xff), UInt8(n >> 8 & 0xff), UInt8(n >> 16 & 0xff), UInt8(n >> 24)])
    return d + payload
}

/// Feeds `bytes` in chunks of `step`, then ends the stream. The frames it
/// gave, and the error it ended with.
func decode(_ bytes: Data, step: Int) -> ([StepvFrame], StepvStreamError?) {
    var r = StepvFrameReader()
    var frames: [StepvFrame] = []
    var at = 0
    do {
        while at < bytes.count {
            let end = min(at + step, bytes.count)
            frames += try r.feed(bytes.subdata(in: at..<end))
            at = end
        }
        try r.finish()
        return (frames, nil)
    } catch {
        return (frames, error as? StepvStreamError)
    }
}

let args = CommandLine.arguments
guard args.count == 2, let stream = FileManager.default.contents(atPath: args[1]) else {
    print("usage: frames-tests <stream>")
    exit(2)
}

// A real stream: TREE first, PROGRESS frames, DONE last with the summary,
// identical whether it arrives whole, byte by byte or in odd chunks.
let (whole, wholeErr) = decode(stream, step: stream.count)
check(wholeErr == nil, "real stream decodes (\(whole.count) frames)")
check(whole.first?.kind == StepvEvent.tree, "real stream: TREE first")
check(whole.contains { $0.kind == StepvEvent.progress }, "real stream: PROGRESS frames")
let summary = whole.last.flatMap { try? JSONSerialization.jsonObject(with: $0.payload) } as? [String: Any]
check(whole.last?.kind == StepvEvent.done && summary?["ok"] as? Bool == true, "real stream: DONE last, the summary")
for step in [1, 7, 9, 4096] {
    let (frames, err) = decode(stream, step: step)
    check(err == nil && frames == whole, "real stream in \(step)-byte chunks: the same frames")
}

let tree = frame(1, Data("{}".utf8))
let done = frame(3, Data("{\"ok\":true}".utf8))
let cases: [(String, Data, StepvStreamError, Int)] = [
    ("empty stream", Data(), .noSummary, 0),
    ("no DONE", tree, .noSummary, 1),
    ("ends in a header", tree + done.prefix(5), .truncated, 1),
    ("ends in a payload", tree + done.prefix(done.count - 1), .truncated, 1),
    ("bad magic", tree + frame(3, Data(), magic: "SVEW"), .badMagic, 1),
    ("kind 0", frame(0, Data()), .badKind(0), 0),
    ("kind 4, reserved (#54 P1)", tree + frame(4, Data()), .badKind(4), 1),
    // Refused from the header alone: nothing of the 64 MB + 1 is awaited.
    ("frame over MAX_FRAME", tree + frame(1, Data(), length: StepvEvent.maxFrame + 1), .tooLarge(StepvEvent.maxFrame + 1), 1),
    ("bytes after DONE", tree + done + Data([0]), .trailingBytes, 2),
]
for (name, bytes, want, before) in cases {
    for step in [1, bytes.count == 0 ? 1 : bytes.count] {
        let (frames, err) = decode(bytes, step: step)
        check(err == want && frames.count == before,
              "\(name) (\(step)-byte chunks): \(err.map { "\($0)" } ?? "no error"), \(frames.count) frame(s) before it")
    }
}
// A frame of exactly MAX_FRAME is within bounds: its header is accepted.
var atCap = StepvFrameReader()
check((try? atCap.feed(frame(1, Data(), length: StepvEvent.maxFrame))) != nil, "a frame of exactly MAX_FRAME is accepted")
// After an error the reader stays failed.
var broken = StepvFrameReader()
_ = try? broken.feed(frame(9, Data()))
check((try? broken.feed(done)) == nil && (try? broken.finish()) == nil, "a failed reader stays failed")

print(failures == 0 ? "frames: all checks passed" : "frames: \(failures) FAILED")
exit(failures == 0 ? 0 : 1)
