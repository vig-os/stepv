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
// Known gaps, each a mismatch rather than a silent error:
//   - a multi-file assembly's externally referenced products are leaves:
//     the transfer reads those files, the outline only the one it is given;
//   - a product with sub-assemblies AND its own shape loses the shape (the
//     transfer adds it as an unnamed child; no corpus file has one);
//   - products() could merge a real sub-assembly whose products and
//     instances all have no name, id or description at all.
// Bounded: at most 60 levels deep and a million nodes (shared
// sub-assemblies expand per instance), else an error.
#pragma once

#include <string>

class STEPControl_Reader;

namespace stepv {

// The outline of the STEP file at `path` (read as `format` when given, as
// stepv-occt --format; input.h) as one JSON object:
//
//   {"format": "stepv-outline", "version": 1,
//    "tree": [node], "leaves": n}      node as in topology.cpp's "tree", a
//                                      leaf's "part" numbering the leaves
//
// Throws std::runtime_error with what went wrong: not a STEP file, or one
// that does not read.
std::string outline_json(const std::string& path, const std::string& format = "");

// The same, of a file `reader` has already read (ReadFile done, nothing
// transferred): how a streamed run (--stream) sends the outline before its
// transfer without parsing the file twice. Throws std::runtime_error past
// the bounds above.
std::string outline_json(STEPControl_Reader& reader);

}  // namespace stepv
