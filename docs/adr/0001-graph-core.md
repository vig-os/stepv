# ADR-0001: The graph core

- **Status:** Proposed (2026-11-08)
- **Milestone:** MVP: graph core
- **Issues:** #115 (the spike, its 8 design comments), #124 (stepv as an agent tool), #66 (per-part boxes),
  #54 (progressive loading), #62 (outline artifact)
- **Owner rulings this records:** 2026-11-08, "graph core first; hardening is filed as issues and done
  later"; the same day's addendum, "Rust meshing of the common primitives is in the MVP"; and the
  owner's answers to the first round's open questions (§7).
- **Round 2 (2026-11-08):** revised after a fresh design review. It fixes three structural errors: the
  occurrence tree is now a DAG with caps (§2.2), complex entities get an API (§2.4), and units are
  scaled per placement before composing (§2.2). The review's nits are folded in too.

## 1. Context

Today stepv has no Rust model of a STEP file. Everything that knows the file's structure is OCCT, run
as the `stepv-occt` subprocess (`kernel/`, driven by `src/occt.rs`). The Rust side knows the header
(`src/header.rs`), the outline's tree (`occt::outline`, from OCCT's parsed model), and whatever the
kernel sends back: the meshes (`Scene`), the topology JSON (`src/topology.rs`) and the summary.

Three things follow from that, measured in plan.md §5:

- **Every view waits for OCCT.** Ultimaker 3 (143 MB, 258 products, 523 occurrences) shows its
  outline at 4.6 s, which is OCCT's parse, and its geometry at 42 s, after the per-face healing
  budget. Across the S1 files that take over 1 s, 72% of wall time is the STEP transfer (entity
  translation plus OCCT's default healing) and 22% is meshing (plan.md §4, "Progressive display").
  That share is aggregated over those files; it isn't Ultimaker's own split.
- **Nothing is drawn before the transfer.** `OutlineArtifact::boxes` has been empty since #62, because
  per-part boxes need either the transfer or a cheaper pass that doesn't exist (#54, #66).
- **An agent can't ask the file anything cheaply** (#124). Every question goes through a full kernel
  run, and there is no index to run a query (`jaq`) over.

The owner's MVP ruling sets the order: stepv gets its own graph of the STEP file, and the viewer and
the agent tool (#124) sit on it. The addendum adds a Rust mesher for analytic faces, so that common
mechanical parts are drawn without OCCT at all.

The corpus that decides the numbers below is the fixture set (`tests/fixtures/manifest.toml`): 391
files in the S1 sets, the exporter matrix, the 221 MB stress assembly and Ultimaker 3.

**What a grep says about the local corpus** (397 non-malformed files: abc-dataset, nist-pmi,
occt-import-js and stress-assembly; the exporter matrix and Ultimaker aren't fetched on this host).
This is an entity count, not a face count. The graph's census (G2) replaces it: see
[`0001-census.md`](0001-census.md).

| Entity | Count | Note |
| --- | ---: | --- |
| `ADVANCED_FACE` | 143,884 | |
| `PLANE`, `CYLINDRICAL_SURFACE`, `CONICAL_SURFACE`, `SPHERICAL_SURFACE`, `TOROIDAL_SURFACE` | 125,286 | 42,526 / 57,229 / 18,229 / 3,043 / 4,259 |
| B-spline surfaces | 26,361 | 7,327 simple `…_WITH_KNOTS`, plus 19,034 complex instances (9,517 rational) |
| Swept and offset surfaces | 3,954 | revolution 2,647, linear extrusion 1,264, offset 43 |
| `EDGE_CURVE` | 334,706 | |
| `LINE` / `CIRCLE` / `ELLIPSE` | 270,861 / 135,113 / 4,129 | includes 2-D p-curve geometry |
| B-spline curves | 123,274 | includes 2-D p-curve geometry (180,342 `PCURVE`s) |
| `CARTESIAN_POINT` | 3,856,409 | |

So about **80% of surface entities are analytic** (125,286 of 155,601). That is an upper bound on the
fast path's face share. It is not a forecast of the product share, which is what v1 routes on (§2.6).
The edge-curve shares can't be separated from p-curves by grep. A 3-D B-spline edge between two
analytic faces, such as a cylinder meeting a cylinder, is common in mechanical parts. So, by the
owner's decision (§7), M1 covers B-spline edge curves as well as the conics.

Density, for the memory targets: the stress assembly has 3,283,040 entities in 221 MB (67 bytes per
entity) and 3.68 M references, 1.12 per entity (counted by grep: every `#n` minus the definitions). By
the same density, Ultimaker has roughly 2.1 M entities.

## 2. Decision

stepv gets a `graph` module: a memory-mapped Part 21 file turned into an entity table with references
in CSR form, with arguments parsed only when something reads them. On top of it are:

- the product structure: products, instances, placements and units;
- conservative per-node bounding boxes;
- a Rust mesher for products whose every face is analytic;
- a JSON view that `jaq` queries.

OCCT stays the reference and handles everything the Rust side doesn't certify. Any routing to OCCT
is recorded with its reason.

### 2.1 The parser: hand-written, over `memmap2`

**Decision:** a hand-written lexer and entity scanner in `src/graph/`, about 400 lines, with `memmap2`
for the mapping. No Part 21 crate.

| Candidate | Licence | Maintenance (2026-11-08) | Shape | Verdict |
| --- | --- | --- | --- | --- |
| `ruststep` 0.4.0 | Apache-2.0 | last release 2024-09, last commit 2025-03, 26 open issues | nom parser into a full `Exchange` syntax tree, then serde into structs generated from EXPRESS | **Rejected:** an eager tree of every parameter of every entity is the allocation pattern the graph exists to avoid, and there's no lazy access |
| Foxtrot's `step` (diodeinc fork) | Apache-2.0 / MIT | active (2026-10), 2 stars, not on crates.io | `strip_flatten` copies the whole file, then a typed eager parse into a 1.6 MB generated `ap214.rs` enum, into a dense `Vec` sized by the largest id | **Rejected:** a full copy of the file, eager typed decoding, an AP214-only schema, and a git dependency. Its chunked rayon split is the reference for parallel scanning later |
| `iso-10303` 0.5.0 | MIT | last release 2021-05 | EXPRESS code generator plus a parser | **Rejected:** unmaintained |
| `truck-stepio` 0.3.0 | Apache-2.0 | release 2024-09 | built on `ruststep`; truck's own topology | **Rejected:** export-leaning (plan.md §2), and it brings truck's whole B-rep model |
| crates.io `step` 0.0.0 | MIT/Apache-2.0 | 2017, a placeholder | none | **Rejected** |

**Why hand-written is right, not just cheaper:** Part 21's grammar is small: records, strings with `''`,
enums, typed and untyped lists, `$`, `*`, complex instances, comments. The work is in the *data
layout*, which no crate gives: an offset table, CSR references and lazy arguments. The lexer reads
untrusted input and has to be fuzzed either way, and owning ~400 lines is a smaller surface to fuzz
than a nom tree plus serde. The string decoding (`\X\`, `\X2\`, `\S\` and so on) already exists in
`header::decode_part21` and is reused.

**Correction to #115's comment:** it said "the original Foxtrot is CC BY-NC". GitHub reports
`Formlabs/foxtrot` and its `diodeinc` fork as Apache-2.0/MIT. The CC BY-NC project is
`amzoo/quicklook-step` (plan.md §1). Nothing here takes code from either.

**The layout.** One streaming pass over the mapped bytes records each entity:

| Column | Type | Bytes per entity |
| --- | --- | ---: |
| the file's `#id` | `u32` (an id over `u32::MAX` refuses the file) | 4 |
| type | `u16` into an interned type table; a complex instance interns its sorted list of part types as one entry (§2.4, "complex entities") | 2 |
| argument span | `u64` start, `u32` length, into the map | 12 |
| reference start (CSR) | `u32` into `refs` | 4 |
| references | `u32` dense node indices, in argument order across all parts | 4 per reference |
| id → node lookup | a dense `Vec<u32>` when the largest id ≤ 4× the count; otherwise a sorted `(id, node)` table, searched by bisection | 4 to 8 |
| reverse CSR (`referrers`, built on first use) | `u32` start plus `u32` per reference | 4 + 4 per reference |

**The per-entity cost, from the stress assembly's measured counts** (3.28 M entities, 1.12 references
each):
- without `referrers`: 22 B of columns, 4.5 B of references and 4–8 B of lookup, which is **31–35 B per
  entity**, or 101–114 MB;
- with `referrers`: another 8.5 B, so **39–43 B per entity**, or 129–142 MB.

§2.7 states the targets from these numbers. Dangling references (a `#n` that never appears) don't fail
the scan: they are left out of `refs`, listed (`Graph::dangling`) and counted, and anything that
walks into one gets an error (`ArgError::Dangling` from `args`).

**Lazy arguments.** `Graph::args(node)` parses only that entity's span into a borrowed
`Value<'a>`:

- `Ref(Node)`, `Int(i64)`, `Real(f64)`, `Str(Untrusted<'a>)`, `Enum(&'a str)`, `Binary(&'a [u8])`,
  `List(Vec<Value>)`, `Typed(&'a str, Box<Value>)`, `Null` (`$`), `Derived` (`*`).
- The nesting depth is capped (64), so a hostile file can't overflow the stack. Over the cap is an error,
  never a panic. That cap is structure, not hardening, and it is in the MVP.
- Reverse references (`referrers(node)`, needed for "which faces use this edge" and "which
  `SHAPE_DEFINITION_REPRESENTATION` names this product") are a second CSR. It's built on first use by
  a counting sort over `refs`, O(E), and kept in a `OnceLock`.

**Scope of the scanner:**
- **The header** goes through the existing `header::read`.
- **`DATA` sections:** every section is read, including edition 3's parameterised
  `DATA('name', ('schema'));`, whose parameters are recorded and otherwise ignored. Ids are global
  across sections, as the standard says.
- **Edition 3's other sections:**
  - `ANCHOR` and `SIGNATURE` are skipped as sections: neither changes an entity's meaning.
  - A `REFERENCE` section, an external reference (`<…>`) or a value instance (`@n`) refuses the file,
    so it falls back to OCCT. Supporting them is filed for later (§5). No file in the local corpus has
    an edition 3 section, a parameterised `DATA` or an `@` instance (by grep).
- **Strings:** line breaks (CR, LF) inside a string aren't part of its value in Part 21. The lexer
  strips them before `decode_part21`, which today keeps them literally. G1 makes the header path strip
  them too, so the two can't disagree.
- **A duplicate `#id`** doesn't refuse the file: OCCT reads such files, and a graph that refused them
  would show less than today's viewer.
  - Every record stays in the table, and each reference is resolved **per reference**, as OCCT's
    reader does (`StepData_StepReaderData::SetEntityNumbers`): to the nearest definition before the
    referring record, else the nearest after it (#149). "Keep first" and "keep last" each fail one
    ordering. `Graph::resolve(id, from)` implements the rule, and the forward CSR resolves every
    reference through it. `Graph::node(id)`, which has no referrer, gives the first definition.
    G1's test checks the rule against `occt::outline` with `#7` defined twice and its referrer
    before, between and after the two; G2's test checks the CSR on the same three orderings.
  - The count goes into the census as `duplicate_ids`, with a warning.
- **Refusal:** an unterminated string or record, a missing `ENDSEC`, or a file that isn't Part 21
  refuses the file with a `GraphError` naming the byte offset.
- **A refused graph never blocks display.** The viewer and the CLI fall back to today's OCCT-only path
  and say why (§3).

**Single-threaded in the MVP.** At the target (§2.7) one thread is fast enough. A parallel scan splits
at record boundaries, which a quoted `;` or a comment makes non-local, and it is filed for later
along with its test (it must produce the same table as the serial scan).

### 2.2 The product structure, resolved from the graph

**Decision:** `graph::structure(&Graph) -> Structure`, an owned value with no borrow of the `Graph`
(§2.4). It mirrors OCCT's outline rules exactly, so that
the existing outline is the test oracle and the leaf numbering stays the join key between the graph and
the kernel's parts. That numbering is the contract #54 set: "leaf numbers count products, so the
geometry's tree replaces the outline".

The chain it resolves:

```text
PRODUCT ← PRODUCT_DEFINITION_FORMATION ← PRODUCT_DEFINITION ← PRODUCT_DEFINITION_SHAPE
  ← SHAPE_DEFINITION_REPRESENTATION → SHAPE_REPRESENTATION
       ← SHAPE_REPRESENTATION_RELATIONSHIP → ADVANCED_BREP_SHAPE_REPRESENTATION (and kin)
            → MANIFOLD_SOLID_BREP → CLOSED_SHELL → ADVANCED_FACE → …
NEXT_ASSEMBLY_USAGE_OCCURRENCE (parent PD, child PD)
  ← PRODUCT_DEFINITION_SHAPE ← CONTEXT_DEPENDENT_SHAPE_REPRESENTATION
       → (REPRESENTATION_RELATIONSHIP_WITH_TRANSFORMATION + SHAPE_REPRESENTATION_RELATIONSHIP)
            → ITEM_DEFINED_TRANSFORMATION (placement in parent, placement in child)
```

**A DAG, not a tree.** Products are the nodes and their child instances (NAUOs) are the edges.
A shared sub-assembly is stored once, however many times it's instanced. Expanding instances into an
occurrence tree can be exponential in the file's size: `kernel/outline.cpp` guards it, with a depth
of 60 and a million nodes. So:
- **`structure()` builds the DAG only.** Each product holds its child instances, and nothing is
  expanded. Its size is O(products + NAUOs).
- **Cycles:** a three-colour DFS over the DAG finds any cycle (a product that is its own ancestor).
  A cycle refuses the structure with `StructureError::Cycle(node)`, and the view falls back to OCCT,
  whose outline fails the same file at its depth cap.
- **Leaf numbering without expanding:**
  - The outline numbers leaves in depth-first order over each product's *shown* children. A product
    is shown when it has a shape or a shown child, memoised as in `Walker::shown`.
  - From the DAG, `leaf_count(p)` is 1 for a product with no shown children, and otherwise the sum of
    its shown children's counts. It's computed bottom-up once and saturates at `u64::MAX`.
  - A leaf's number is the sum, along its path, of the leaf counts of the siblings before it. So
    `leaf → path` and `path → leaf` are O(depth × fan-out), and the numbering is identical to
    `occt::outline`'s, which stays the join key.
- **Expansion is lazy and capped like the outline:** `Structure::occurrences()` walks depth-first on
  demand, with depth ≤ 60 and ≤ 1,000,000 nodes. Over a cap it returns the same error the outline
  throws, so the two fail the same files. `tree()` is that expansion, made into `topology::Node`s
  for the viewer.
- This is structure, not hardening: it's in G3.

- **Roots** follow `STEPControl_Reader::NbRootsForTransfer` (its product-definition branch, in OCCT's
  default product mode): a product definition is a root unless
  - it is some NAUO's related (child) product definition, or
  - it is a `PRODUCT_DEFINITION_WITH_ASSOCIATED_DOCUMENTS` whose formation is shared by a
    `DOCUMENT_PRODUCT_EQUIVALENCE` (a document, not a part), or
  - OCCT's product-context filter is set to a specific life-cycle stage and the definition's
    `PRODUCT_DEFINITION_CONTEXT` doesn't match it. The kernel doesn't set that option, and OCCT's
    default accepts every context (the filter applies only above its default value), so the graph
    ignores this rule.

  G3's oracle (`occt::outline` on the corpus) settles any remaining difference.
- **Children** come in the same order as `kernel/outline.cpp`: the NAUO order
  `STEPControl_ActorRead::TransferEntity` builds.
- **Names** follow both of the outline's rules (`kernel/outline.cpp`, its header comment):
  - **STEPCAFControl's naming:**
    - a product: its name, else its id;
    - an instance (NAUO): its description, else its name, else its id.
  - **The kernel's own rule on top:** a tree node (an assembly or a part) shows its product's name,
    else its instance's name.
- **Products without shape** are left out (dm1's materials). An AP203 hybrid (a solid plus a wireframe)
  is one leaf.
- **The placement** of an instance comes from the relationship
  `REPRESENTATION_RELATIONSHIP(rep_1, rep_2)` with an `ITEM_DEFINED_TRANSFORMATION(…, item1, item2)`.
  The relationship is usually a complex entity (§2.4).
  - **Pairing:** `item_k` belongs to `rep_k`, so item1 is a placement in rep_1's context and item2 one
    in rep_2's.
    - If each item is instead found among the *other* representation's items, the pair is swapped
      back.
    - If an item is in neither representation, the pairing is kept and a warning recorded.
  - **Parent and child** are decided by matching, never by position. The child representation is
    the one used by the NAUO's *related* (child) product definition's shape, and the parent the one
    used by its *relating* (parent) product definition's.
    - In the common CAx-IF layout rep_1 is the child and rep_2 the parent.
    - When the relationship's order reverses the NAUO's, the same transform is applied inverted.
  - **The formula,** with each item scaled to mm by its own context's unit before composing:
    `T = S_mm(item_child)⁻¹ · S_mm(item_parent)`, applied to the child's shape.
    - `S_mm` turns the placement into a rigid frame (local → its representation's coordinates), its
      location scaled by its context's mm-per-unit; directions are unitless.
    - This is **OCCT's change-of-coordinates convention**, and stepv follows it on purpose: G3's oracle
      is `--topology` within 1e-9, and M4b draws Rust and OCCT products in one scene, where two
      conventions would misplace parts.
    - Reading ISO 10303 as "carry the child's frame onto the parent's" gives
      `S(item_parent) · S(item_child)⁻¹`. That differs whenever the two frames don't commute, and stepv
      deliberately follows OCCT there too.
    - T is stored row-major 3×4 like `topology::PlacedPart::transform`.
  - **Checked against OCCT's behaviour.** A clean-room description, read for behaviour only (OCCT is
    LGPL; no code is copied), from OCCT 7.9 `src/STEPControl/STEPControl_ActorRead.cxx`:
    - **`STEPControl_ActorRead::ComputeSRRWT`** takes item1 and item2 with `Rep1` and `Rep2` as
      origin and target.
    - **`STEPControl_ActorRead::ComputeTransformation`** checks that each item belongs to its
      representation, and swaps them back if each is in the other's. It converts each placement in its
      own representation's units (it prepares the units per context before building each axis).
      It then builds a **change of coordinates** between the two axis systems:
      `gp_Trsf::SetTransformation(target, origin)`.
      - A probe compiled in the round-3 review against nixpkgs' TKMath 7.9.3, with item1 = Rz(90°) and
        item2 = a translation by (10, 0, 0), maps the origin point to (0, −10, 0). That is `item1⁻¹ · item2`, not
        `item2 · item1⁻¹`, which would give (10, 0, 0).
    - **`STEPControl_ActorRead::TransferEntity(CONTEXT_DEPENDENT_SHAPE_REPRESENTATION)`** transfers
      the shape of `Rep1`, the origin and so the child, and moves it by that transform
      (`ApplyTransformation`, a `Move` of the shape). Hence `S(item_child)⁻¹ · S(item_parent)`.
    - **When `STEPConstruct_Assembly::CheckSRRReversesNAUO`** finds the relationship reversed against
      the NAUO, it transfers `Rep2`'s shape instead and moves it by the inverse of the same transform.
      The graph does the same: the child is rep_2, and its placement is `T⁻¹`. That is again
      `S(item_child)⁻¹ · S(item_parent)`, with the items assigned by the match.
  - **Units:** composing in either side's raw units would be wrong whenever the two contexts differ,
    such as an inch sub-assembly inside a mm assembly. G3 has hand-written fixtures, all checked
    against `--topology`:
    - an inch sub-assembly inside a mm assembly;
    - a reversed relationship;
    - a **non-commuting** pair: the child's item is a non-identity rotation that doesn't commute with
      the parent's item. It is the one that tells the two conventions apart.
- **`MAPPED_ITEM`** (`REPRESENTATION_MAP` plus a target placement; 173 in the local corpus) is the
  second instancing mechanism, and is resolved with the same convention. OCCT calls the same
  `ComputeTransformation` there, with the map's `mapping_origin` as the origin and the item's
  `mapping_target` as the target (`STEPControl_ActorRead`'s mapped-item transfer). So the mapped shape
  is moved by `S_mm(mapping_origin)⁻¹ · S_mm(mapping_target)`, each scaled to mm in its own context.
- Any other transformation operator, such as `CARTESIAN_TRANSFORMATION_OPERATOR_3D`, leaves the
  instance **unplaced**, with a reason. It is never placed silently at the identity.
- **Units:**
  - each representation context's length unit gives a scale to millimetres: `SI_UNIT` with its
    prefix, or `CONVERSION_BASED_UNIT` through its `LENGTH_MEASURE_WITH_UNIT`. Both the context and the
    unit are complex entities (§2.4);
  - every length leaving the graph is in mm, as OCCT's reader produces by default;
  - each representation carries its scale, so a product's geometry is converted once, at the boundary;
    placements are converted per item, as above.
- **Colours and styles** (`STYLED_ITEM` → `PRESENTATION_STYLE_ASSIGNMENT` → `SURFACE_STYLE_USAGE` → …
  → `COLOUR_RGB` or a `DRAUGHTING_PRE_DEFINED_COLOUR`), per solid and per face. Products meshed in
  Rust are never drawn by OCCT, so their colours must come from the graph, or the two halves of the
  view look different (G7).
- **Multi-file assemblies (#92)** are out of the MVP's graph: a product whose geometry lives in another
  file is a leaf with no shape and the reason `external`. The outline leaves those files out today for
  the same reason (plan.md, "Outline before geometry").

### 2.3 Conservative bounding boxes without OCCT

**Decision:** a box is computed **per geometric entity type**, not from "every `CARTESIAN_POINT` reachable
from the product". The reachability closure #115 sketched is **neither tight nor conservative**:

- It's loose, because it picks up axis-placement origins, which needn't lie on the shape.
- It's *wrong*, because a full-circle edge has one vertex, so a cylinder's box from its points is a
  sliver. A sphere has a single centre point.

A box that may be too small breaks the culling and containment queries (#115 P2, #124 `relations`) that
rely on it. So every node's box is **conservative by construction**, it contains the exact geometry, or
the node's box is `Unknown(reason)`. It is never a guess.

| Entity | Box | Why it is conservative |
| --- | --- | --- |
| `VERTEX_POINT` | the point | exact |
| `EDGE_CURVE` on `LINE` | its two vertices | the segment between them |
| on `CIRCLE` | the full circle's box: per world axis i, c_i ± r·√(1 − n_i²), where n is the unit normal of the circle's plane; tighter (later): endpoints plus the axis extrema that lie within the arc's range | contains the arc |
| on `ELLIPSE` | per world axis i, c_i ± √((a·x_i)² + (b·y_i)²), where x and y are the unit major and minor axes and a and b the semi-axes | contains the arc |
| on a B-spline curve | its control points' box. Rational: every weight must be > 0, else `Unknown(non-positive weight)` | convex-hull property, which holds only for positive weights |
| `SURFACE_CURVE` / `SEAM_CURVE` | its 3-D curve's box | the 3-D curve is the edge |
| face on `PLANE`, `CYLINDRICAL_SURFACE`, `CONICAL_SURFACE`, `SURFACE_OF_LINEAR_EXTRUSION` | the union of its bounding edges' boxes | these surfaces are linear in v, so for fixed u, every coordinate is extreme at the region's v-ends, which lie on the boundary |
| face on `SPHERICAL_SURFACE` | the sphere's box ∪ edges (centre ± r) | contains the surface |
| face on `TOROIDAL_SURFACE` | the torus' local box, (R + r) radially and ±r along the axis, re-boxed into world axes through its 8 corners | contains the surface |
| face on a B-spline surface | its control points' box. Rational: every weight must be > 0, else `Unknown(non-positive weight)` | convex-hull property, which holds only for positive weights |
| face on `SURFACE_OF_REVOLUTION` | a local box in the axis frame (the basis curve's largest distance from the axis as the radius, its extent along the axis), re-boxed into world axes through its 8 corners | contains the swept curve |
| face on `OFFSET_SURFACE` | the basis surface's box grown by \|d\| | contains the offset |
| anything else | `Unknown(type)` | — |

- **One rule for every change of frame:** a box held in some local frame (a torus' or a revolution's
  axis frame, a product's coordinates under a placement) is carried into the target axes by
  transforming its **8 corners** and boxing them. That's conservative under any rotation, and loose by
  at most the rotation's growth.
- Boxes compose **bottom-up** in one post-order pass over the DAG: face → shell → solid → product (in
  its own coordinates, mm) → instance (its placement applied through the 8 corners) → assembly → model.
  Each product's box is computed once, however often it's instanced. It's O(entities + NAUOs).
- A node with any `Unknown` child is `Unknown` for that reason. It isn't a box of the rest, which
  would be too small.
- **What isn't covered:**
  - **Tightness:** a sphere or torus face gets its whole surface's box; a B-spline's control polygon
    can bulge well past the surface; a rotated instance's box is grown by the rotation.
  - **Faces with no edges** (a closed sphere bounded by a vertex loop, or a full torus) work: the
    table doesn't need edges for those surfaces. A bounded plane without edges can't exist in valid
    AP203/214.
  - **Geometry outside the B-rep:** these aren't in a product's box: wireframe (an
    `EDGE_BASED_WIREFRAME_MODEL`, a `GEOMETRIC_CURVE_SET`, the curve and point elements of a
    `GEOMETRIC_SET`), bare points and axis placements among a representation's items, and PMI.
    - A `GEOMETRIC_SET`'s *surface* elements are in the box. OCCT's transfer makes them faces
      (bounded, curve-bounded and rectangular composite surfaces), so each is boxed by the table,
      or is `Unknown` when its type has no row (an unbounded plane, a trimmed or composite
      surface).
- **Checked against the exact geometry, as OCCT triangulates it** (G4): on every corpus file, every
  node of the kernel's mesh of a product's prototype, every point of its face edges' polylines
  (`--edges`) and every B-rep vertex of those edges lies inside the product's graph box, and, placed,
  inside its leaf's box. These points lie on the exact faces and edges. The slack is only the mesh's
  f32 rounding (1e-6 of the box's size), tighter than any shape tolerance. Free edges and their
  vertices (wireframe) are left out, as the box leaves them out.
  - **Why not OCCT's own box:** it is itself over-conservative. OCCT's `BndLib` boxes a conic arc
    by points at `r / cos(π/8)` (a circumscribed octagon), so its box can reach up to about 8.2% of a
    radius past the exact shape, beyond its tolerance gap. On the local corpus, "the graph box contains
    `topology::Prototype::bbox`" failed on 234 products by up to 62.8 mm (G4, round 1), while no kernel
    vertex lay outside a graph box. That gate tested OCCT's over-reach, not the graph.
  - OCCT's box stays as reported data, not a gate: how often and how far it reaches past the graph's,
    and the volume ratio (graph box ÷ OCCT box) per product as tightness data.

### 2.4 The public Rust API (sketch)

The surface the viewer, the CLI and #124 call. The names are binding for the slices. Signatures may
gain lifetimes and error variants.

```rust
// src/graph/mod.rs: the entity layer.
pub struct Graph { /* Mmap, header::Info, tables, OnceLock<Referrers> */ }
#[derive(Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct Node(u32);                       // dense index, file order
pub struct StepId(pub u32);                 // the file's #id

impl Graph {
    pub fn open(path: &Path) -> Result<Graph, GraphError>;
    pub fn from_bytes(bytes: Vec<u8>) -> Result<Graph, GraphError>;   // fd input, tests
    pub fn header(&self) -> &header::Info;
    pub fn len(&self) -> usize;
    pub fn node(&self, id: StepId) -> Option<Node>;
    pub fn id(&self, n: Node) -> StepId;
    pub fn type_name(&self, n: Node) -> TypeName<'_>;                 // Simple(&str) | Complex(&[&str])
    pub fn is_a(&self, n: Node, part: &str) -> bool;                  // simple type, or one part of a complex
    pub fn parts(&self, n: Node) -> impl Iterator<Item = &str> + '_;  // one item for a simple entity
    pub fn refs(&self, n: Node) -> &[Node];                           // all parts, in record order
    pub fn referrers(&self, n: Node) -> &[Node];                      // built once, lazily
    pub fn args(&self, n: Node) -> Result<Args<'_>, ArgError>;        // simple entity only; complex: ArgError::Complex
    pub fn args_of(&self, n: Node, part: &str)                        // one part's own arguments; a simple
        -> Option<Result<Args<'_>, ArgError>>;                        // entity's when `part` is its type
    pub fn of_type(&self, name: &str) -> impl Iterator<Item = Node> + '_;   // matches parts too
    pub fn census(&self) -> Census;                                   // counts by type and by part
    pub fn text(&self, t: TextRef) -> Untrusted<'_>;                  // resolve an owned text handle
}

/// File text: attacker-controlled (#124). No Display, no Deref<str>: callers
/// decide what leaves, through #124's text policy.
pub struct Untrusted<'a>(&'a [u8]);
impl Untrusted<'_> { pub fn decode(&self) -> String /* CR/LF stripped, then header::decode_part21 */; }
/// An owned pointer to a string argument: no borrow, so a Structure outlives any one borrow of the Graph.
#[derive(Clone, Copy)] pub struct TextRef { pub node: Node, pub span: (u32, u32) /* within the args */ }

// src/graph/structure.rs: owned data only (Node ids, TextRefs, numbers). No lifetime, so a
// long-lived holder (#124's daemon) keeps `Arc<Graph>` and `Structure` side by side.
pub fn structure(g: &Graph) -> Result<Structure, StructureError>;   // Cycle, Unsupported, …
pub struct Structure {
    pub products: Vec<Product>,          // the DAG's nodes: one per PRODUCT_DEFINITION
    pub roots: Vec<usize>,               // product indices, in the outline's order
    pub leaves: u64,                     // == occt::Outline::leaves (saturating)
}
pub struct Product { pub def: Node, pub name: TextRef, pub id: TextRef, pub bodies: Vec<Node>,
                     pub shown: bool, pub leaf_count: u64, pub children: Vec<Instance>,
                     pub mm_per_unit: f64, pub color: Option<Color> }
pub struct Instance { pub nauo: Option<Node>, pub child: usize, pub name: TextRef,
                      pub placement: Placement }                     // the DAG's edges
pub enum Placement { Placed([f64; 12]), Unplaced(Reason) }           // child mm → parent mm
pub struct Path(Vec<(usize, usize)>);                                // (product, child index) per level
pub struct Expand { pub max_depth: u32 /* 60 */, pub max_nodes: u64 /* 1_000_000 */ }
impl Structure {
    pub fn leaf_path(&self, leaf: u64) -> Option<Path>;              // O(depth × fan-out), no expansion
    pub fn leaf_of(&self, path: &Path) -> Option<u64>;
    pub fn occurrences(&self, cap: Expand)                           // lazy DFS; Err at a cap, like the outline
        -> impl Iterator<Item = Result<Occurrence, ExpandError>> + '_;
    pub fn tree(&self, g: &Graph, cap: Expand) -> Result<Vec<topology::Node>, ExpandError>;
}
pub struct Occurrence { pub path: Path, pub leaf: Option<u64>, pub placement: [f64; 12] /* to model */ }

// src/graph/bounds.rs
pub enum Bound { Box(BBox), Unknown(Reason) }                      // BBox in mm, crate::BBox
pub fn bounds(g: &Graph, s: &Structure) -> Bounds;
impl Bounds {
    pub fn model(&self) -> Bound;
    pub fn product(&self, p: usize) -> Bound;                       // its own coordinates
    pub fn leaf(&self, leaf: u64) -> Bound;                         // placed via leaf_path, model coordinates
    pub fn node(&self, n: Node) -> Bound;                           // any face, shell, solid
}

/// What an answer is worth (#124's honesty contract).
pub enum Tier { Proxy, Mesh { deflection: Deflection }, Exact }

// src/mesh/: the Rust fast path (§2.6)
pub fn classify(g: &Graph, s: &Structure, p: usize) -> Route;
pub enum Route { Rust, Occt(Vec<(Node, Reason)>) }                 // every face's reason
pub fn mesh_product(g: &Graph, s: &Structure, p: usize, d: Deflection, diag: f64)
    -> Result<crate::Part, Vec<(Node, Reason)>>;
pub struct Ledger { /* per product: route; per face: reason */ }
pub struct Reason(&'static str, Option<String>);                    // stable signature, §2.6

// src/graph/json.rs: the query view (G5); behind the `query` feature (jaq)
pub fn query(g: &Graph, s: &Structure, b: &Bounds, filter: &str, limit: Limit)
    -> Result<Vec<serde_json::Value>, QueryError>;
```

- **G3's realised structure API** (#135) differs from the sketch where the sketch couldn't hold:
  - `Product::name` is what STEPCAF names it by (the product's name, else its id) and `id` the
    product's id, both `Option<TextRef>`, since either may be absent. `Instance::name` is
    likewise the chosen label (description, else name, else id). `Product::color` comes with G7.
  - `Instance::nauo` is a `Node`: every DAG edge is a NAUO. A `MAPPED_ITEM` places geometry
    inside a representation, not a product, so it is `mapped_placement(g, item, rep)`, which G4
    calls as it walks a body.
  - `Occurrence::placement` is a `Placement`: an unplaced instance leaves everything under it
    unplaced. `Placement::Unplaced` carries `Reason(&'static str)`; the mesh's richer `Reason`
    is M4a's.
  - `Path`'s pairs are `(product, slot)`: the slot indexes `roots` at the first level and the
    parent's `children` below, so a root leaf has a path too.
  - `Product::bodies` are the representations the transfer starts from (its shape definitions',
    and its shape aspects' unless GD&T): an assembly's own placement representation is one.
- **G4's realised bounds API** (#136) keeps the sketch's names and signatures:
  - `Bounds` owns a clone of the `Structure`, so `leaf(leaf)` takes no structure argument and
    `Bounds` holds no borrow, like `Structure`.
  - `Bound::Unknown` carries G3's `Reason(&'static str)`: `"unsupported surface"`,
    `"non-positive weight"`, `"non-finite placement"`, and so on. A product with no B-rep geometry
    at all (only placements, wireframe or external parts) is `Unknown("no B-rep geometry")`, and
    adds nothing to its parents' boxes.
  - `node(n)` answers for every face, shell, solid, edge, vertex and representation the walk reached,
    in that representation's mm, else `Unknown("not reached")`.
- **G6a's realised viewer stage** (#138):
  - `view::Load::Graph(Result<Box<GraphView>, String>)` comes from `view::placeholder::load`, run
    in-process on a thread of its own beside the kernel's run, for STEP input only. A refusal
    (`GraphError`, `StructureError`, or the expansion's cap) is the status line's
    `graph refused: <reason>`, and the window is otherwise as before.
  - The graph's tree shows until the kernel's arrives. `Placeholders` holds `(leaf, BBox)` per
    leaf whose `Bounds::leaf` is a box, and counts the rest (`graph: N parts, M without a box`).
  - On `Done`, `Placeholders::replace` drops each leaf the kernel meshed, joined by
    `topology::parts_per_leaf` over `--topology`'s tree. Without a topology there is no join, and
    the scene replaces every box.
  - `gpu::Renderer::plan` lists a frame's draws as `(Pipeline, Geometry)` pairs, and `render_with`
    encodes exactly those. Placeholder boxes have their own pipelines (unlit, alpha-blended, no
    depth write): a translucent fill and a line-list wireframe.
- **G5's realised CLI and query view** (#137):
  - **Native filters, not `ValT`.** jaq 3's `ValT::values` and `key_values` return `'static`
    iterators, so an `Entity(Node)` variant would have to own the graph (an `Arc<Graph>` in every
    value), and the whole value surface (arithmetic, ordering, `jaq_std::ValT`) would be
    re-implemented beside `jaq_json::Val`'s, several hundred lines on their own. The fallback is the
    view: native filters over plain `jaq_json` values, each materialising only when called.
  - **The heap test passes with it** (`tests/graph_cli.rs`, stress assembly, debug build): the
    graph's own heap (table + `referrers`) is 127.9 MB; the peak through a filter walking one
    part's faces (663) is 179.3 MB, **1.40×**, against the 2× bar. The peak includes the
    structure and the boxes, which `products` computes.
  - `src/graph/json.rs` is always built: `View` (the graph, with `structure()` and `bounds()`
    computed on first use and timed) and the JSON it writes, `summary`, `census`, `products`,
    `tree(TreeOptions)` (a page of occurrences: `--depth`, `--offset`, `--limit`, the outline's
    caps) and `entity`/`refs`/`referrers` by step id. `src/graph/query.rs`, behind `query`, is
    `query(&View, filter, limit) -> Result<Answer, QueryError>`: the sketch's `(g, s, b)` became
    the `View`, so a filter that only reads entities never builds the structure or the boxes.
  - The filters are `summary`, `census`, `products`, `tree`, `entity($id)`, `refs($id)` and
    `referrers($id)`, `$id` a step id; `.` is null. `env`, `halt` and the logging natives raise an
    error, so a filter can't read the host or end the process.
  - **Untrusted text:** every string from the file (header fields, names, type names, enumerations,
    string arguments) is `{"untrusted": "…"}`, never a bare string or an object key, so the census
    is a list of `{"type", "count"}`.
  - `--timings` prints the phases (`scan`, then `structure`, `bounds`, `expand` as they ran, then
    `answer`) on stderr. Ultimaker 3, release build: `summary` scan 0.56 s (cold), structure
    0.035 s, bounds 0.69 s, total 1.29 s; `census` 0.22 s; `tree` 0.26 s.
- **Complex entities** (`#1=(A(…) B(…) C(…));`, one record holding several entity types) are first-class:
  - Their type is the interned sorted list of part names. `TypeName::Complex` holds them as
    `&[Box<str>]`, the interned table's own slice, rather than the sketch's `&[&str]` (#152). `is_a`/`of_type` match any part, so
    `of_type("SI_UNIT")` finds a complex unit too.
  - `args_of(n, part)` parses just that part's own arguments. In Part 21's internal mapping each part
    carries only the attributes its own type declares. `args(n)` on a complex entity is an error, never
    a silent first part.
  - `refs(n)` lists the references of all parts in record order, so the CSR and the box and structure
    walks don't care which form an entity has.
  - **The cases the MVP relies on:**
    - **Units:** `#5=(LENGTH_UNIT() NAMED_UNIT(*) SI_UNIT(.MILLI.,.METRE.));` →
      `args_of(#5, "SI_UNIT")` = `[.MILLI., .METRE.]`. An inch is
      `(CONVERSION_BASED_UNIT('INCH',#7) LENGTH_UNIT() NAMED_UNIT(#6))` →
      `args_of(_, "CONVERSION_BASED_UNIT")[1]` → `LENGTH_MEASURE_WITH_UNIT(25.4, #mm)`.
    - **The context that holds the units** is itself complex:
      `(GEOMETRIC_REPRESENTATION_CONTEXT(3) GLOBAL_UNCERTAINTY_ASSIGNED_CONTEXT((#u)) GLOBAL_UNIT_ASSIGNED_CONTEXT((#l,#a,#s)) REPRESENTATION_CONTEXT('',''))`
      → `args_of(_, "GLOBAL_UNIT_ASSIGNED_CONTEXT")`.
    - **The placement relationship in §2.2's chain:**
      `(REPRESENTATION_RELATIONSHIP('','',#r1,#r2) REPRESENTATION_RELATIONSHIP_WITH_TRANSFORMATION(#idt) SHAPE_REPRESENTATION_RELATIONSHIP())`
      → `rep_1`/`rep_2` from the `REPRESENTATION_RELATIONSHIP` part, the transform from the `…_WITH_TRANSFORMATION`
      part.
    - **Rational B-splines** (9,517 surfaces in the local corpus):
      `(BOUNDED_SURFACE() B_SPLINE_SURFACE(du,dv,((#p…)),…) B_SPLINE_SURFACE_WITH_KNOTS(…) … RATIONAL_B_SPLINE_SURFACE(((w…))) …)`
      → control points from the `B_SPLINE_SURFACE` part, knots from `…_WITH_KNOTS`, weights from
      `RATIONAL_B_SPLINE_SURFACE`. The weights decide whether the box is conservative (§2.3). Rational
      curves follow the same pattern.
- **`Untrusted`** is the type-level half of #124's layer 1. File text can't reach a `format!`
  without an explicit `decode()`. #124's NFKC normalisation, length caps and screening act on the
  decoded string at the tool boundary, not in the graph.
- **The JSON view for `jaq`:**
  - **Top level** is materialised on access: `summary` (header, units, census, model box), `tree` and
    `products` (names as `{"untrusted": …}`, boxes with `"tier": "proxy"`). It's bounded by products
    and instances, not entities.
  - **Entity level, lazily:** `entity($id)` returns `{"id", "type", "args"}` for one node.
    `refs`/`referrers` return ids. Arguments are only parsed for the entities a filter reaches.
  - **The primary design** implements `jaq_core::ValT` over `enum V { Json(jaq_json::Val),
    Entity(Node) }`, so `.args[2]` on an entity materialises just that entity.
  - **The fallback,** if `ValT` can't carry it in jaq 3: the same functions as native filters over
    plain `jaq_json` values. That is still lazy, since nothing is materialised before it's called.
  - The G5 acceptance test decides between them: a filter that walks one product's faces on the stress
    assembly must keep the heap within 2× the graph's.
  - `jaq-core` and `jaq-json` are MIT, actively released (2026-10), and allowed by `deny.toml`.
- **Feature gating:**
  - `graph` and `mesh` are always built: the viewer and the CLI need them, and they pull only
    `memmap2`, `rayon` and `spade`, none of them viewer crates, so the capi tripwire stays green.
  - `jaq` sits behind a default-on `query` feature, so the Quick Look library doesn't carry a query
    engine it has no use for.

### 2.5 Where OCCT stays

OCCT remains the kernel for:
- every face the fast path doesn't certify;
- healing (plan.md §2: 99.5% of the corpus is faithful because of it);
- exact measurement (`measure`, #124's `Tier::Exact`);
- section caps;
- IGES and BREP, which the graph doesn't read;
- multi-file assemblies.

- **The sandbox, the memory cap and the per-face budget are unchanged.**
- **The graph runs in-process.** It's safe Rust over a read-only map, and its failure mode is a refused
  file, not code execution, so it needs no subprocess.
  - The one unsafe call is the map. A file truncated while mapped raises `SIGBUS`, which is filed as
    hardening, together with a heap cap (§5).
- **#115's further routing is later work:** per-face routing, carving mini-STEP files per product, the
  priority queue, the SVEV `Upsert`/`Cull` patch stream and winding-number culling. This ADR fixes
  only what v1 needs.

### 2.6 The Rust fast path for analytic products (owner addendum)

**Edge-first.** Each `EDGE_CURVE` gets **one canonical polyline**, built once and shared by every face
that uses it (through its `ORIENTED_EDGE`s). It starts and ends at the vertex points, with interior
points from the curve.
- **MVP curves:** `LINE`, `CIRCLE`, `ELLIPSE`, and B-spline curves (`B_SPLINE_CURVE_WITH_KNOTS`,
  rational or not, including the complex form), directly or through a `SURFACE_CURVE`/`SEAM_CURVE`'s
  3-D curve. The B-spline curves are in by the owner's decision (§7).
  - They're needed for the edges where analytic faces meet off-axis, such as a cylinder meeting a
    cylinder, which exporters write as B-spline curves.
  - On an analytic surface the (u, v) inversion of the edge's points stays closed-form, whatever the
    curve is, so the face stays simple.
- A face's boundary refers to its points as `(edge node, index)`, walked in the oriented edge's
  direction. **Stitching is an index merge**, so neighbouring faces share vertices by construction.
  That's the contract #115's stitching comment set out.

**Faces in scope:** `ADVANCED_FACE` on `PLANE`, `CYLINDRICAL_SURFACE`, `CONICAL_SURFACE`,
`SPHERICAL_SURFACE` and `TOROIDAL_SURFACE`, bounded only by MVP edges. Every boundary point is inverted into (u, v) only after checking that it
lies on the surface within the representation context's uncertainty (`GLOBAL_UNCERTAINTY_ASSIGNED_CONTEXT`).
Otherwise the face is routed (`curve:off-surface`).
- **Each boundary point is mapped to (u, v) by closed-form inversion** of the analytic surface. No
  p-curves are needed, and no projection, which is #51's hot spot.
- **Periodic surfaces:** u is unwrapped continuously along each loop. A seam edge (the same
  `EDGE_CURVE` twice in one loop, with opposite senses) becomes the two sides of the cut.
- **Sphere poles** are degenerate points, handled explicitly.
- **Triangulation:** a constrained Delaunay triangulation in (u, v), plus interior Steiner points on
  the curved directions at the deflection's step. Triangles outside the outer loop or inside a hole are
  removed by a flood from the constraints.
- **Normals are exact** from the surface, flipped by the face's `same_sense` and the shell's
  orientation.

**Triangulator: `spade`** (2.15, MIT OR Apache-2.0, 23 M downloads, released 2026-03):
- a CDT with exact predicates;
- constraint edges that cross are reported by its non-panicking API (`try_add_constraint`) rather than
  being split silently. That's the signal to route the face to OCCT.

Rejected alternatives:
- **`earcut`/`earcutr`:** ear clipping makes slivers on the long thin faces CAD is full of, and it
  can't take Steiner points, which curved faces need;
- **Formlabs' `cdt`:** 0.1.0 from 2021, unmaintained;
- **writing our own:** robust predicates are the hard part, and `spade` already has them.

**Granularity in v1: whole products.** A product is meshed in Rust only if *every* face classifies as
supported. Otherwise the whole product goes to OCCT. Per face would be finer, but it is the wrong v1:
1. **The win is in the transfer, not the meshing.**
   - Across the S1 files that take over 1 s, 72% of wall time is the STEP transfer: entity translation
     plus default healing (`plan.md` §4, aggregated over those files, not per file).
   - OCCT translates and heals a whole shape, so routing one face of a product to OCCT still costs the
     product's whole transfer, and saves only part of the 22% that is meshing. Only skipping whole
     products removes OCCT's work.
   - **Nothing is saved until M5,** when the kernel transfers only the routed products. Before that,
     OCCT still runs on the whole file, and the Rust path only makes products appear earlier.
2. **No mixed-mesher seams.** Separate products share no edges. So v1 needs neither prescribed edge
   polylines inside OCCT's BRepMesh, which is unproven (#115), nor a re-sync after healing changes an
   edge.
3. **One interface to the kernel:** "these products". It is the unit the kernel already reports
   (`topology::PlacedPart`), and the unit the leaf numbering joins on.

- **The cost:** one B-spline fillet sends a whole bracket to OCCT.
- **The ledger records every face's reason anyway,** so the census reports how many products were lost
  to one or two hard faces. That number is the trigger for per-face routing (v2, filed in §5).
- **This doesn't block per-face mixing later,** provided M5 reports the STEP entity id of every edge
  next to every face. Then a later per-face mode can join OCCT's edges to the graph's `EDGE_CURVE`s
  and stitch by index. So M5 reports both.

**The mesh quality contract.** The Rust path uses the same numbers the kernel uses, so the two halves of
a view are indistinguishable:
- **The chordal deviation** is at most `Deflection::linear_abs(diag)`, using the model's diagonal as the
  kernel does. The viewer uses `Deflection::PREVIEW`: 0.1% of the diagonal and 20°.
- **The angle** between consecutive segments of an edge polyline, and between the normals across any
  interior triangle edge, is at most `angular_deg`.
- **For a circle arc of radius r and sweep θ**, the segment count is
  `n = max(n_min, ⌈max(θ / a, θ / (2·acos(1 − min(d/r, 1))))⌉)`, where a is the angular deflection and
  d the linear one.
  - The `min(d/r, 1)` clamp keeps `acos` defined when d > r: a tiny arc against a coarse deflection,
    which M1's 1e-3 mm radii hit. Without it the formula gives NaN.
  - `n_min` is 1 for an open arc and 3 for a full circle, so a closed edge never collapses.
- **Ellipses (semi-axes A ≥ B):** a uniform parameter step under-samples near the major vertices of an
  eccentric ellipse. So the step is bounded by the worst case over the curve:
  - **angle:** the tangent turns at most A/B times faster than the parameter, so `Δt ≤ a · B / A`;
  - **chord:** the step is in the parameter t, and the sag of a chord over a parameter interval Δt is at
    most `Δt² / 8 · max|C''(t)|`. On `C(t) = (A cos t, B sin t)`, `|C''(t)| ≤ A`, so `Δt ≤ √(8d / A)`.
    - **The bound used is exactly `Δt ≤ √(8d / A)`.** The circle formula `2·acos(1 − d/A)` is close but
      not used. It is slightly larger, and not a proven bound for a curve that isn't a circle.
    - The smallest radius of curvature, B²/A, bounds the sag per unit of *arc length*, not per unit of t.
      So it can't be used here.
    - A counter-example to using B²/A: with A = 1, B = 0.5 and d = 1e-3, the circle formula at
      r = B²/A gives Δt ≈ 0.179 and a sag of ≈ 4.0e-3 (4·d) at the major vertex. `√(8d / A)` gives
      Δt ≈ 0.089.
  - n is the larger of the two counts, which is conservative and costs extra points only on eccentric
    ellipses. A curvature-adaptive step is a later refinement.
- **B-spline curves:** recursive subdivision (de Boor / knot insertion) until each span's control
  polygon lies within d of its chord and its legs turn by at most a. The convex-hull property makes
  both tests conservative. Rational curves need weights > 0, else the edge is routed. Lines get no
  interior points.
- **The diagonal:** the graph's model box is conservative, so it may be larger than OCCT's. G4 reports
  the ratio. If it is over 1.05 on corpus files, the fast path uses the OCCT-equivalent diagonal of the
  B-rep's edges instead, so the Rust mesh is never coarser than OCCT's.
- **Two gates, both in M4a's corpus job, with 0 failures required:**
  - **Against the exact surface, without OCCT:**
    - every interior Rust vertex lies on its face's analytic surface within 1e-9 relative;
    - every boundary vertex lies within the context's uncertainty, which is checked before inversion;
    - **every triangle's deviation from its surface is bounded analytically** from its vertices, where
      a closed-form bound exists. Each bound holds for every point of the triangle, not a sample:
      - plane: 0;
      - cylinder and cone, with the triangle's vertices spanning Δu ≤ π around the axis: every point
        lies inside the surface, short of it by at most `r_max · (1 − cos(Δu / 2))`, where r_max is
        the largest of the vertices' radii. A convex combination of radial vectors within an angular
        window Δu has a norm of at least cos(Δu/2) times the combined radius;
      - sphere: if the vertices lie within an angle θ ≤ π/2 of some direction from the centre, every
        point of the triangle is at least R·cos θ from the centre, so the deviation is at most
        `R · (1 − cos θ)`. Over π/2 the bound says nothing, and the triangle fails the gate;
    - **tori are checked by sampling**: the centroid, the edge midpoints and a 4 × 4 barycentric grid
      per triangle, each against the closed-form distance. A sample can miss the worst point, so this
      is labelled sampled in the report. An analytic torus bound is later work (§5).
  - **Against OCCT:** per product, the symmetric Hausdorff distance between the Rust mesh and OCCT's
    mesh of the same leaf is at most 2·d, because each is within d of the true surface.
    - It is **sampled**, and a sampled Hausdorff distance only **under-estimates** the true one: it can
      miss the worst point. The samples are every vertex of both meshes, plus 1,000 area-weighted random
      points per mesh per product (fixed seed), measured against the other mesh's triangles in both
      directions.
    - The measured distances are recorded per product (max, p99) in the job's report.
    - **Tightening this gate** (to d against an OCCT run at d/2, by the owner's answer) is an issue to
      file when this ADR is accepted (§5).

**Routing and the coverage ledger (MVP).** `classify` gives every face of a product either *supported*
or a **reason signature**: stable and coarse, as #115's ledger comment set out. Examples:
- `surface:B_SPLINE_SURFACE_WITH_KNOTS`, `surface:SURFACE_OF_REVOLUTION`;
- `curve:B_SPLINE_CURVE_WITH_KNOTS`;
- `loop:seam-unresolved`, `loop:pole`;
- `rs-fail:cdt-crossing`, `rs-fail:orientation`;
- `structure:unplaced`, `structure:external`.

- A product with any reason is routed to OCCT. A Rust failure at run time is routed too, with its
  reason. **Nothing is dropped silently.**
- **The ledger is part of the result:**
  - in `stepv graph census`;
  - in the CLI's JSON (`--info`);
  - in the viewer's status line ("212 of 258 products meshed in Rust; 46 routed to OCCT: surface:B_SPLINE…
    ×31, …").
- It stays local. There's no telemetry.
- Filing one issue per signature from a corpus job is later (§5).

**Expected coverage, stated before measuring:**
- **Faces:** at most 80% (the analytic share of surface entities above).
- **Products:** lower, by the products that contain any non-analytic face. The guess before
  measuring was 50–70% of products in mechanical assemblies and far less in the ABC subset.
- **Measured by G2's census, [`0001-census.md`](0001-census.md),** which replaces the guess: 87.0%
  of faces are analytic, and 100.0% of edges are MVP edges. Per shape representation (until G3
  gives products), 70.9% are in scope: Ultimaker 55.3%, the stress assembly 50.0%, `nist-pmi` 69.0%,
  ABC 75.3%. The mechanical assemblies land at the guess's low end, not far below it. The census is
  re-run per product after G3 (#153). If the product share comes out far below the guess, the M slices are
  re-cut before M1a starts (§6).

**Parallelism:** rayon over products, and over the edges and then the faces within a product. Edges
first, because faces read the edge polylines. Within a product the dependency is edges → faces →
stitch. Across products there is none.

### 2.7 Performance and memory targets

Reference host: sage (Mac Studio, Apple Silicon), release build. Measured by `stepv graph summary
--timings` and the census example, once per corpus file, **one heavy job at a time** under the memory
cap (the 2026-10-12 incident).

| Measure | Ultimaker 3 (143 MB) | Stress assembly (221 MB, 3.28 M entities) | Corpus |
| --- | --- | --- | --- |
| scan: entity table + CSR | ≤ 1.0 s | ≤ 1.5 s | the whole corpus ≤ 20 s, serial |
| structure + boxes | ≤ 0.5 s | ≤ 0.75 s | |
| heap beyond the map, without `referrers` | ≤ 36 B per entity (≈ 76 MB at 2.1 M) | ≤ 36 B per entity, ≤ 120 MB | |
| heap beyond the map, with `referrers` | ≤ 45 B per entity (≈ 95 MB) | ≤ 45 B per entity, ≤ 150 MB | |
| outline today, for comparison | 4.6 s (OCCT parse) | 5.1 s | |
| first placeholder frame in the viewer | ≤ 1.5 s from open | | |
| failures | 0 panics; every refusal names a byte offset | | 0 panics on all 401 + malformed |

- **The heap targets** are the §2.1 estimate (31–35 B, and 39–43 B with `referrers`, from the stress
  file's measured 3.28 M entities and 1.12 references each), plus about 5% headroom for the type table
  and the dangling list. G1 and G2 measure them. A miss is a design finding to report, not a target to
  move quietly.
- **Structure + boxes, measured (G4, #136):**
  - Ultimaker takes **0.76 s** (`structure()` 0.04 s + `bounds()` 0.72 s), a miss against 0.5 s.
    The stress assembly takes 0.25 s.
  - Cause: the post-order walk reaches about 1.5 M entities in DAG order, which is random order in
    the 143 MB map, so it is bound by memory latency. Parsing Ultimaker's 519k points takes 0.11 s
    in file order and 0.31 s shuffled. Argument parsing is about 61% of the profile (B-spline poles
    about 26%).
  - The target stands. The fix (rayon over products in the Rust path, or a file-order pass) is
    #177.
- **First placeholder frame, measured (G6a, #138):**
  - Ultimaker: **0.85–0.89 s** from `stepv view`'s start to the first frame of placeholders, over
    3 runs, release build, warm cache. All 523 leaves have a box. Target: ≤ 1.5 s.
  - The stress assembly: 0.63 s (120 boxes).
- **The fast path's targets are measured, not promised.** M4b and M5 report, on Ultimaker, against the
  OCCT-only path (42 s today):
  - time to the first Rust-meshed product;
  - time to the first full view;
  - the share of products and of triangles that came from Rust.
- The MVP's bar: the first full view is no slower than today's, and the Rust-path products appear
  before OCCT's transfer would have finished.

### 2.8 Placeholders: boxes, and primitive fits (owner's answer, §7)

Before a leaf has a mesh it is drawn as a **placeholder**. In the MVP that is either its box or a fitted
cylinder or sphere.

**The guarantee is the same for both: the placeholder contains the leaf's exact geometry.** It is
conservative, like the box. A fit is drawn instead of the box only when it is also smaller than the
box, so it's a tighter claim of the same kind, never a guess at the shape.

**When a fit is used (per product, then placed like its box):**
- **Cylinder:** every face of the product is on a surface of one axis: coaxial cylinders, cones and
  tori, and planes perpendicular to that axis. The axis is taken from the surfaces' own axis
  placements, collinear within the context's uncertainty.
  - The radius is the largest conservative radial bound of any face about that axis:
    - a cylinder's r;
    - a cone face's largest radius at its edges' axial extent;
    - a torus' R + r;
    - a plane's largest distance from its edges to the axis. The distance is convex along a line and
      is the circle's radius on a coaxial circle, so the edges bound it.
  - The axial extent is the union of the faces' conservative axial intervals, from their edges, and
    a torus' ±r.
- **Sphere:** every face is on a sphere with one centre, or on a plane. The radius is the largest
  sphere radius and the largest distance from the centre to any plane face's edges.
- **Tolerance:** axes are collinear, and centres coincide, only within a tolerance. So a fit is
  conservative only up to that tolerance, and it is **inflated by it**:
  - ε is the largest distance, over the fit's axial extent, between any face's own axis (or centre) and
    the fitted one, plus the context's uncertainty;
  - the radius grows by ε, and a cylinder's axial extent by ε at each end;
  - a fit whose ε exceeds the context's uncertainty by more than 10× isn't used: the axes don't really
    agree.
- **Otherwise,** or when any bound is `Unknown`, the leaf keeps its box.
- **The size test compares what is drawn.** A fit replaces the box only when its *drawn* volume is
  smaller than the box's. That's the circumscribed shape below, after inflation: a prism of
  `n · R² · tan(π/n) · h` for an inradius R, or the circumscribed polyhedron's volume. It isn't the
  volume at radius r, which would favour a fit that draws larger than the box.

**Drawn so that the drawing is conservative too:**
- the cylinder's polygon is *circumscribed*: its vertices lie at R / cos(π/n), where R is the inflated
  radius, so every side of the drawn prism lies outside the true radius;
- the sphere is a circumscribed polyhedron: its vertices lie at r / cos α, where α is the angular radius
  of its largest facet's circumcircle, so every facet's plane is at least r from the centre.

**Styled like the box,** never like geometry: translucent with a wireframe, never in the shaded
pipeline. The status line counts boxes and fits together: "N parts shown as placeholders".

## 3. Consequences

**The viewer:**
- **It opens on the graph:**
  - the tree comes from `Structure::tree()` instead of a cached outline artifact or the kernel's
    `Event::Tree`;
  - every leaf whose box is known is drawn as a **placeholder** (§2.8): its box, or a fitted cylinder
    or sphere, translucent with a wireframe, never shaded like geometry, following #115's honesty rule;
  - a status line says "N parts shown as placeholders".
- `Load` gains `Graph(GraphView)`, carrying the tree, the leaf boxes, the census and the ledger. It's
  sent before the kernel starts.
- **Meshes replace proxies per leaf** as they arrive: the Rust path's products first, then OCCT's
  `Done` scene. The leaf index is the join key.
- **A tree mismatch is detected, not hidden.** If the kernel's tree disagrees with the graph's, which
  happens on multi-file assemblies, the kernel's tree wins. Proxies that don't map are dropped, and the
  status line says so.
- **A refused graph** leaves the viewer exactly as it is today: the outline, progress, then the scene.
  The status line names the reason.

**The kernel:** in the MVP it gains one option (M5): `--products <step ids>`, which transfers and
meshes only the listed product definitions.
- `STEPControl_Reader::TransferEntity` runs per routed product's shape representation.
- Its faces **and edges** are reported with their STEP entity ids: faces for colours, edges so that
  per-face mixing stays possible later (§2.6).
- **The graph is the source of truth for names, the tree and colours** (owner's decision, §7).
  - XCAF stays only as the test oracle, and as the fallback when the graph is refused.
  - G7 proves colour parity with XCAF before M4b relies on it.
- **M5's parity check is a CI gate:** a per-product run against a full XCAF run must give the same
  geometry and colours.
  - It runs in `kernel.yml` on `tests/data` plus the sets CI already fetches (`nist-pmi`, `malformed`),
    plus new small fixtures for each style case G7 covers.
  - The full-corpus run is a `just` recipe whose result is recorded in M5's PR.
  - If parity fails, M5 is reshaped (carving, #115 P3) before anything ships.
- **Until M5 lands,** M4b runs OCCT on the whole file as today. It uses OCCT's meshes for the routed
  products, and M4a uses them as the Hausdorff oracle for the Rust ones.

**The cache:**
- The graph is **not** cached on disk in the MVP: rebuilding it costs ≤ 1 s.
- `OutlineArtifact::boxes` is finally filled, one box per leaf from the graph, closing #66's gap for the
  front ends that read the artifact (Swift included). `OUTLINE_VERSION` stays 1: the field already
  exists.
  - A leaf with an `Unknown` box can't be represented in the artifact's schema (`Vec<[f64; 6]>`), so
    `boxes` is written only when every leaf has a box. Otherwise it stays absent.
- **The scene entry's key** gains the fast path's version (`mesh::VERSION`), so a change to the Rust
  mesher invalidates composed scenes. An entry is still written only for a complete scene: Rust
  products and OCCT's routed products both present.

**The CLI:**
- `stepv graph summary|tree|census|query` is new (G5).
- Existing commands keep their contract and exit codes.
- `--info` gains the census and the ledger once the graph exists.

**#124** builds its `summary`, `tree`, `find` and `query` tools on the graph, with `Tier::Proxy` boxes
for `relations` candidates, and keeps its text policy at the tool boundary on top of `Untrusted`.

**Maintenance:**
- stepv owns a Part 21 scanner and an analytic mesher, two things plan.md §2 deliberately bought from
  OCCT. The scanner is small and fuzzable.
- The mesher is limited to surfaces with closed-form inversion and is gated per product by OCCT's
  output. Growing it is the ledger's uplift process (#115), never an unmeasured expansion.

## 4. Alternatives rejected

- **Only OCCT's XDE tree** (extend `--outline` and the kernel's streaming, keep no Rust model):
  - every answer still waits for OCCT's parse, which is 4.6 s on Ultimaker against ~1 s for a scan;
  - boxes still need a transfer, because OCCT has none before it;
  - an agent query (#124) needs an index in the process that answers it;
  - the tree would stay a product of OCCT's run, not of the file, so Rust meshing would have nothing to
    walk.
- **A full eager parse** (`ruststep`, Foxtrot's `step`, or a typed decode of every entity): memory
  grows with every parameter of every entity. For the stress assembly that's millions of
  heap-allocated parameter trees, against ~100 MB of flat tables. And most entities are never read by
  any query: a box needs the topology, not the presentation layer.
- **Boxes from the closure of reachable points** (#115's first sketch): not conservative (§2.3).
- **Per-face routing in v1:** saves almost none of OCCT's time, and needs seams between meshers
  (§2.6).
- **A Rust mesher for B-spline surfaces in the MVP:** needs p-curves or projection, which is #51's
  problem, and healing, which plan.md §2 chose not to rebuild. It goes through the ledger's uplift
  process later, one signature at a time.
- **Running the graph as a sandboxed subprocess like the kernel:** the graph is safe Rust with no
  native parser behind it, and its answers are needed in the viewer's process, at memory-copy cost.
  What remains of the risk (`SIGBUS`, heap growth) is hardening (§5).
- **Porting or translating OCCT code into Rust:** forbidden by licence (#115's clean-room rule).
  Nothing in this ADR depends on OCCT's source.

## 5. MVP vs later

**MVP (this milestone):** every slice in §6, with:
- refusal without panics;
- the nesting cap;
- the DAG with its cycle check and expansion caps;
- `Untrusted` text;
- complex entities;
- B-spline edge curves;
- primitive fits;
- the coverage ledger;
- placeholder styling and the status line;
- both M4a mesh gates;
- the M5 parity gate in CI and the timing report.

**Later. Each is filed as an issue when this ADR is accepted:**
- **Hardening:**
  - a fuzz target for the scanner and the argument parser (cargo-fuzz), seeded from the malformed set;
  - a heap budget for the graph, admission-controlled like #108, with refusal over the budget;
  - `SIGBUS` on a file truncated while mapped (read small files into memory; handle or pre-check large
    ones);
  - adversarial id ranges and reference fan-out;
  - edition 3 `REFERENCE` sections, external references (`<…>`) and value instances (`@n`), which
    refuse the graph today (§2.1);
  - recovering from malformed records rather than refusing the file;
  - the graph inside the Quick Look extensions (sandbox, memory cap, the capi tripwire).
- **Speed:**
  - the parallel scan, split at record boundaries, equal to the serial scan by test;
  - the graph cached on disk, a mmappable table keyed like the outline artifact;
  - tighter arc, sphere and torus boxes;
  - a curvature-adaptive step on ellipses (§2.6);
  - an analytic per-triangle deviation bound on tori, replacing the sampled check (§2.6).
- **Coverage:**
  - per-face routing (v2) with prescribed edge polylines in OCCT and a re-sync after healing;
  - B-spline and swept surfaces in Rust, one ledger signature at a time;
  - multi-file assemblies in the graph (#92);
  - one issue per ledger signature from a corpus job.
- **#115's remaining architecture:**
  - structural hashing and deduplication;
  - the cost model (P1);
  - the priority queue and the detail budget;
  - the SVEV patch stream (`Upsert`/`Cull`);
  - carving mini-STEP files and a process pool;
  - winding-number culling (P2);
  - an ADR for the slot and patch protocol.
- **Quality:** tightening the OCCT mesh gate from 2·d to d against an OCCT run at d/2, using M4a's
  recorded distances (owner's answer, §7).
- **#124:** text screening, attestation and `reveal`, on top of `Untrusted`.

## 6. Slice plan

Each slice is at most ~400 product lines, plus its tests, and lands as its own PR behind review. Tests
run memory-bounded, one heavy job at a time.

**Order and why:**
- **G1 → G2 → G3 → G4 → G6a → G6b → G6c:** placeholders in the viewer as early as possible, which is the
  first visible value.
- **G5 (the CLI)** runs beside G6a–c once G4 lands, since #124 waits on it.
- **G2's census gates the M slices:** if the product share the fast path could cover comes out far
  below the guess in §2.6, the M plan is re-cut before M1a, not after.
- **G7 (colours) comes before M4b,** so that the first view combining both meshers already looks
  consistent, and its parity is proven before anything relies on it.
- **M5 comes last,** since it changes the kernel and its acceptance is the timing that justifies the
  fast path.

| Slice | Scope | Acceptance | Test command |
| --- | --- | --- | --- |
| **G1** Scanner + entity table | `src/graph/{mod,lex}.rs`: map, header via `header::read`, records → id, interned type (complex as a sorted part list), arg span; the id → node lookup (dense or sorted); edition 3 `DATA(…)` parameters, `ANCHOR`/`SIGNATURE` skipped; CR/LF stripped in strings (the header path too); duplicate ids kept as OCCT keeps them; refusal with a byte offset | A table equal to an independent oracle (`ids` and types from a regex pass) on `tests/data`; the duplicate-id fixture agrees with `occt::outline`; all malformed fixtures refuse without panicking; Ultimaker/stress within §2.7's scan time and the "without `referrers`" heap | `nix develop -c cargo test graph::` and `nix develop -c cargo run --release --example graph_scan -- <file>` (time and heap) |
| **G2** CSR + lazy args + complex parts + census | Forward CSR in the scan; `args()`/`args_of()` → `Value`, depth cap 64; `is_a`/`parts`/`of_type` over complex parts; `referrers()` built lazily; `census()`; `examples/graph_census.rs` writes per-file surface and edge-curve shares (3-D only, p-curves excluded) and the share of products all of whose faces are analytic | Round trip: every `#n` in a span is in `refs`, in order; args parse for every entity and every complex part of every corpus file; the unit, context, relationship and rational-B-spline walkthroughs of §2.4 as unit tests; the census table is committed to `docs/adr/0001-census.md`, replacing §2.6's guess; the "with `referrers`" heap | `nix develop -c cargo test graph::` and the census example over the corpus |
| **G3** Structure as a DAG + placements + units | `structure()`: the product DAG, the cycle check, shown/leaf counts, `leaf_path`/`leaf_of`, lazy capped `occurrences()`, both naming rules, shapeless products dropped, `ITEM_DEFINED_TRANSFORMATION` (`T = S_mm(item_child)⁻¹ · S_mm(item_parent)` applied to the child's shape, OCCT's change-of-coordinates convention; item_k with rep_k, parent and child by matching against the NAUO, swapped items corrected, `T⁻¹` when reversed) and `MAPPED_ITEM` (the same convention), each item scaled to mm, `Unplaced(reason)` | `Structure::tree()` equals `occt::outline`'s tree (names, shape, leaf numbers) on every corpus file, the two multi-file assemblies included (the outline, like the graph, reads only the given file); placements equal `--topology`'s `PlacedPart::transform` within 1e-9 relative on every corpus file with instances, except the two multi-file assemblies' top files, whose parts the transfer reads from other files; both run locally on the whole corpus (`STEPV_CORPUS`), and CI runs both on the fetched `tests/fixtures/nist-pmi` and `occt-import-js` (trees on every file, placements on occt-import-js's CAx-IF assemblies), failing when `STEPV_CORPUS` names no file or compares no placement; **the inch sub-assembly inside a mm assembly** fixture and a reversed-relationship (rep_1 = parent) fixture and a non-commuting fixture (the child's item a rotation that doesn't commute with the parent's) place exactly as `--topology` does; the root rule matches the outline's roots; a cyclic fixture refuses; a shared-sub-assembly fixture whose expansion exceeds the cap gives the outline's error while `structure()` stays O(products) | `nix develop -c cargo test --test graph_structure` (runs the kernel for the oracle) |
| **G4** Conservative boxes | `bounds()`: the per-type table of §2.3, the 8-corner rule, post-order composition over the DAG, `Unknown` propagation (non-positive weights included), leaf boxes via `leaf_path` | On every corpus file, every node of the kernel's mesh of each prototype, its face edges' polyline points and its B-rep vertices lie inside the product's graph box and, placed, the leaf's (§2.3: OCCT's own box over-reaches by up to r·(1/cos(π/8) − 1), so it is data, not the gate); 0 violations, locally (`STEPV_CORPUS`) and in CI on nist-pmi and occt-import-js, failing when no product is compared; the tightness ratio and OCCT's over-reach per product are reported; §2.7's structure + boxes time | `nix develop -c cargo test --test graph_bounds` |
| **G5** `stepv graph` CLI | `summary` (header, units, census, model box), `tree` (paginated, depth-limited, capped), `census`, `query <jq>` via `jaq` over the lazy view (§2.4); text as `{"untrusted": …}`; `--timings` | Golden JSON on `tests/data`; an injection-style product name comes back only as a data field; the lazy-view test: a one-product face walk on the stress assembly keeps the heap ≤ 2× the graph's (this decides `ValT` vs native functions) | `nix develop -c cargo test --test graph_cli` |
| **G6a** The viewer opens on the graph | `Load::Graph`, the graph's tree, box placeholders (translucent, wireframe), the status line, the fallback when the graph is refused | Headless viewer test (`tests/view.rs`): first frame with placeholders before `Done`; no placeholder drawn with the shaded pipeline; a refused graph gives today's behaviour, with the reason in the status line | `nix develop -c cargo test --test view` |
| **G6b** Meshes replace placeholders | Per-leaf replacement as meshes arrive, the tree-mismatch rule (the kernel's tree wins, unmapped placeholders dropped and counted), `OutlineArtifact::boxes` filled when every leaf has a box | Headless test: after `Done` no placeholder remains on a matching tree; a multi-file fixture shows the mismatch rule and its status line; the artifact's `boxes` length equals `leaves`, and it's absent when any box is `Unknown` | `nix develop -c cargo test --test view` and `nix develop -c cargo test --test cache` |
| **G6c** Primitive fits | §2.8: coaxial-cylinder and concentric-sphere fits from the graph's analytic surfaces, conservative radial and axial bounds, circumscribed drawing, fallback to the box | On the corpus, every fit contains OCCT's mesh vertices of that leaf (0 outside); a fit is used only when smaller than the box; fixtures of a shaft (cylinder), a ball (sphere) and a bracket (box fallback); fits are styled as placeholders | `nix develop -c cargo test --test graph_fits` |
| **G7** Colours from the graph | The style chain → product, solid and face colours. **Cases covered:** `STYLED_ITEM` on a `MANIFOLD_SOLID_BREP`/`BREP_WITH_VOIDS`/shell (solid colour), on an `ADVANCED_FACE` (face override), on a shape representation (part colour); `OVER_RIDING_STYLED_ITEM`; `COLOUR_RGB` and `DRAUGHTING_PRE_DEFINED_COLOUR`; `SURFACE_STYLE_USAGE` with `.BOTH.`/`.POSITIVE.` sides through `SURFACE_SIDE_STYLE` → `SURFACE_STYLE_FILL_AREA` → `FILL_AREA_STYLE_COLOUR`; styles held in a `MECHANICAL_DESIGN_GEOMETRIC_PRESENTATION_REPRESENTATION` | On the corpus, the graph's per-product and per-face colours equal the kernel's XCAF colours for every leaf; one committed fixture per covered case; a case XCAF colours that the graph doesn't is a test failure, never a silent default | `nix develop -c cargo test --test graph_colors` |
| **M1a** Analytic edge polylines | `mesh::edges`: LINE/CIRCLE/ELLIPSE (direct or via `SURFACE_CURVE`/`SEAM_CURVE`), the clamped circle count and the ellipse bound of §2.6, one polyline per `EDGE_CURVE`, shared and oriented | Unit tests: chordal error ≤ d and turning angle ≤ a on arcs with radii from 1e-3 to 1e3 mm (no NaN, d > r included) and on ellipses with B/A down to 0.01; two faces sharing an edge get identical points in opposite order | `nix develop -c cargo test mesh::edges` |
| **M1b** B-spline edge polylines (MVP, required) | De Boor evaluation and knot-insertion subdivision, rational weights > 0, complex-form curves, flattened to the same contract | As M1a, plus corpus curves against OCCT's edge polylines from `--edges`: within d | `nix develop -c cargo test mesh::edges` |
| **M2** Planar faces with holes | Plane inversion, `spade` CDT with the outer loop and holes, flood removal, exact normals, routing on `try_add_constraint` failure | Area equals the topology's face area within 1e-6 relative on planar corpus faces; the 1,250-hole plate (ABC `00000046`) meshes, with its time and memory reported against OCCT's 7.7 s and 4.2 GB | `nix develop -c cargo test mesh::plane` |
| **M3** Cylinder, cone, sphere, torus | Closed-form (u, v) inversion after the on-surface check, u unwrapped on periodic surfaces, seams, poles, Steiner grid at the deflection step | Fixture solids (full and partial cylinders, cones to the apex, spheres with poles, tori, a cylinder–cylinder intersection with B-spline edges) mesh watertight (every interior edge used by exactly two triangles); area within the deflection's bound of the topology's area | `nix develop -c cargo test mesh::analytic` |
| **M4a** Classify + ledger + rayon meshing | `classify`, `mesh_product`, the ledger with reason signatures, rayon over products, the ledger in `--info`; `just fastpath-corpus` runs both gates | **The surface gate** (every triangle within d of the exact surface) and **the OCCT gate** (sampled symmetric Hausdorff ≤ 2·d per product, distances recorded): 0 failures over the corpus; `Part::is_well_formed` on every Rust part | `nix develop -c cargo test --test fastpath` and `nix develop -c just fastpath-corpus` |
| **M4b** The combined view | The viewer and the CLI combining Rust products with OCCT's routed ones (OCCT still runs on the whole file), the viewer's ledger status line, the cache key's `mesh::VERSION`, an entry only for a complete composed scene | Headless viewer test: Rust products replace their placeholders before `Done`, OCCT's routed ones at `Done`, and nothing is drawn twice; changing `mesh::VERSION` misses the cache; a killed OCCT run caches nothing | `nix develop -c cargo test --test view` and `nix develop -c cargo test --test cache` |
| **M5** The kernel transfers only routed products | `stepv-occt --products <ids>` via `TransferEntity`, face and edge STEP entity ids in the output, the Rust side passing only routed products | **CI gate:** geometry and colours of routed products equal a full XCAF run on `tests/data`, `nist-pmi` and the G7 fixtures; the full corpus is recorded in the PR; **the timing report on Ultimaker:** first Rust product, first full view, the Rust share of products and triangles, against 42 s OCCT-only | `nix develop -c cargo test --test kernel_products` and `nix develop -c just fastpath-bench real-assembly` |

`just fastpath-corpus` and `just fastpath-bench` are added by M4a and M5. The `--test` targets are
created by their slices.

## 7. Owner decisions (2026-11-08)

The first round's open questions, answered by the owner. They are binding for the slices.

1. **B-spline edges are always in the MVP.** M1 covers LINE, CIRCLE, ELLIPSE and B-spline curves
   (with knots, rational or not). It is split into M1a (analytic edges) and M1b (B-spline edges) to
   stay within ~400 lines. Both are required.
2. **The graph is the source of truth for names and colours.** XCAF remains only as the test oracle
   and the fallback. G7 proves colour parity with XCAF before M4b relies on it, and M5's parity check
   (a per-product run against a full XCAF run) is a CI gate.
3. **Placeholders are boxes plus primitive fits in the MVP** (G6c, §2.8). A fit is conservative like
   the box, contains the exact geometry, and is styled like the box, never like geometry.
4. **The mesh gate is 2·d against OCCT, tightened later.** The distances are recorded and the
   tightening is filed (§5). The extra gate, every triangle within d of the exact analytic surface,
   needs no OCCT and is adopted too. It is an analytic per-triangle bound on planes, cylinders, cones
   and spheres, and sampled on tori (§2.6). A sampled Hausdorff distance under-estimates the true one. The
   samples are stated in §2.6.
