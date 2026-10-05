// Measurements between B-rep entities (#33), for `stepv-occt --serve`.
//
// The server reads one JSON query per line on stdin and writes one JSON
// answer per line on stdout:
//
//   {"id": 1, "op": "distance", "a": {"part": 1, "face": 0}, "b": {"part": 2, "edge": 3}}
//     -> {"id": 1, "ok": true, "distance": f, "points": [[x,y,z], [x,y,z]],
//         "axis_distance": f}            (when both entities have an axis line)
//   {"id": 2, "op": "angle", "a": {...}, "b": {...}}
//     -> {"id": 2, "ok": true, "angle_deg": f}
//   {"id": 3, "op": "point", "a": {"part": 0, "face": 2}, "near": [x,y,z]}
//     -> {"id": 3, "ok": true, "point": [x,y,z], "normal": [x,y,z]}   (normal: faces)
//   anything wrong -> {"id": n, "ok": false, "error": "..."}
//
// Entities are (part, face) or (part, edge), numbered exactly as --topology
// numbers them: a part is the mesh's part i, a face its prototype's face in
// TopExp_Explorer order, an edge its index in topology_edges. Everything is
// in model coordinates (millimetres), the prototypes placed.
//
// The axis of an entity, for axis_distance and angle: a plane's outward
// normal (as topology.cpp computes it), a cylinder's or cone's axis, a
// line's direction, a circle's or ellipse's normal through its centre.
#pragma once

#include <TopoDS_Shape.hxx>

#include <functional>
#include <optional>
#include <string>

namespace stepv {

// Resolves (part, is_edge, index) to the placed entity; nullopt when there
// is none.
using Resolve = std::function<std::optional<TopoDS_Shape>(long part, bool edge, long index)>;

// Answers one query line. Never throws: OCCT failures become ok:false.
std::string answer_query(const std::string& line, const Resolve& resolve);

}  // namespace stepv
