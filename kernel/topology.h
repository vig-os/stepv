// The model's exact topology (#21): what a viewer needs for a model tree and
// for measuring on the B-rep rather than the mesh. Written beside the mesh as
// JSON ("stepv-topology", version 1); the format is at the top of
// topology.cpp.
#pragma once

#include <TopoDS_Shape.hxx>
#include <gp_Trsf.hxx>

#include <cstddef>
#include <string>
#include <vector>

namespace stepv {

// One node of the assembly tree, flattened: `parent` is an index into the
// same vector (-1 for a root), `part` the placed part a leaf is (-1 for an
// assembly).
struct TopoNode {
    std::string name;
    int parent = -1;
    long part = -1;
};

// One placed part, in the mesh's part order: the mesh's part i is this i.
struct TopoPart {
    std::string name;
    std::size_t prototype = 0;  // index into the prototypes
    gp_Trsf placement;
};

// Writes the topology of `prototypes` (each in its own coordinates; a part
// places one) to `path`. Returns "" on success, else what went wrong.
std::string write_topology(const std::string& path, const std::vector<TopoNode>& tree,
                           const std::vector<TopoPart>& parts,
                           const std::vector<TopoDS_Shape>& prototypes);

}  // namespace stepv
