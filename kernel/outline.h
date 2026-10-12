// The assembly outline (#54): the tree and names of a STEP file read straight
// from its product structure, with no shape transfer. On a large assembly it
// arrives in the time it takes to parse the file, minutes before the geometry
// can (#51), so a front-end can show the tree first.
//
// Its contract, held by tests/outline.rs and checked over the whole corpus:
// it is the tree --topology writes after a full run, down to the products.
// Its leaves are products (shapeless ones, like materials, left out, as the
// transfer leaves them out); where --topology splits a product into unnamed
// bodies, the outline has that product as one leaf. So --topology's tree,
// with every node whose children are all unnamed parts collapsed into a
// leaf, IS the outline. Leaf numbers count the outline's leaves only: the
// geometry's tree replaces the outline when it arrives.
//
// One deliberate gap: a multi-file assembly's externally referenced products
// are leaves. The transfer reads those files and expands them; the outline
// reads only the file it is given, to stay a parse of one file.
#pragma once

#include <string>

namespace stepv {

// The outline of the STEP file at `path` as one JSON object:
//
//   {"format": "stepv-outline", "version": 1,
//    "tree": [node], "leaves": n}      node as in topology.cpp's "tree", a
//                                      leaf's "part" numbering the leaves
//
// Throws std::runtime_error with what went wrong: not a STEP file, or one
// that does not read.
std::string outline_json(const std::string& path);

}  // namespace stepv
