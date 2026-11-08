# ADR-0001: The graph core

- **Status:** Proposed (2026-11-08)
- **Milestone:** MVP: graph core
- **Issues:** #115 (the spike, its 8 design comments), #124 (stepv as an agent tool), #66 (per-part boxes),
  #54 (progressive loading), #62 (outline artifact)
- **Owner rulings this records:** 2026-11-08, "graph core first; hardening is filed as issues and done
  later"; the same day's addendum, "Rust meshing of the common primitives is in the MVP".

## 1. Context

Today stepv has no Rust model of a STEP file. Everything that knows the file's structure is OCCT, run
as the `stepv-occt` subprocess (`kernel/`, driven by `src/occt.rs`). The Rust side knows the header
(`src/header.rs`), the outline's tree (`occt::outline`, from OCCT's parsed model), and whatever the
kernel sends back: the meshes (`Scene`), the topology JSON (`src/topology.rs`) and the summary.

Three things follow from that, measured in plan.md §5:

- **Every view waits for OCCT.** Ultimaker 3 (143 MB, 258 products, 523 occurrences) shows its
  outline at 4.6 s, which is OCCT's parse, and its geometry at 42 s, after the per-face healing
  budget. Of that, 72% is transfer and healing, and 22% is meshing (S1, files over 1 s).
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
This is an entity count, not a face count. The graph's census (G2) replaces it.

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
analytic faces, such as a cylinder meeting a cylinder, is common in mechanical parts, and it decides
whether M1 needs B-spline curves (§2.6, §6).

Density, for the memory targets: the stress assembly has 3,283,040 entities in 221 MB (67 bytes per
entity) and about 1.1 references per entity. By the same density, Ultimaker has roughly 2.1 M.

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
| type | `u16` into an interned type table; a complex instance interns its sorted list of part types as one entry | 2 |
| argument span | `u64` start, `u32` length, into the map | 12 |
| reference start (CSR) | `u32` into `refs` | 4 |
| references | `u32` dense node indices, in argument order | 4 per reference |
| id → node lookup | a dense `Vec<u32>` when the largest id ≤ 4× the count; otherwise a sorted `(id, node)` table, searched by bisection | 4 to 8 |

That is about 30 bytes per entity, plus 4 per reference. For the stress assembly, about 100 MB
beside the mapped 221 MB. Dangling references (a `#n` that never appears) don't fail the scan: they
are listed and counted, and anything that walks into one gets an error.

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
- The header goes through the existing `header::read`.
- Every `DATA` section is accepted, with ids global as the standard says.
- A duplicate id, an unterminated string or record, a missing `ENDSEC`, an external reference
  (edition 3 `<…>`), or a file that isn't Part 21 refuses the file with a `GraphError` naming the byte
  offset.
- **A refused graph never blocks display.** The viewer and the CLI fall back to today's OCCT-only path
  and say why (§3).

**Single-threaded in the MVP.** At the target (§2.7) one thread is fast enough. A parallel scan splits
at record boundaries, which a quoted `;` or a comment makes non-local, and it is filed for later
along with its test (it must produce the same table as the serial scan).

### 2.2 The product structure, resolved from the graph

**Decision:** `graph::structure(&Graph) -> Structure`. It mirrors OCCT's outline rules exactly, so that
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

- **Roots** are product definitions that are no NAUO's child. Children come in the same order as
  `kernel/outline.cpp` (the NAUO order `STEPControl_ActorRead::TransferEntity` builds).
- **Names** follow the outline:
  - a product: its name, else its id;
  - an instance: its description, else its name, else its id.
- **Products without shape** are left out (dm1's materials). An AP203 hybrid (a solid plus a wireframe)
  is one leaf.
- **The placement** of an instance is `T(item1) · T(item2)⁻¹` from its `ITEM_DEFINED_TRANSFORMATION`,
  in the parent's units, stored row-major 3×4 like `topology::PlacedPart::transform`.
  - **Which representation is the parent's** is decided by matching the relationship's `rep_1`/`rep_2`
    against the parent's and child's shape representations, not by argument position. Exporters
    disagree on the order, and OCCT matches the same way.
- **`MAPPED_ITEM`** (`REPRESENTATION_MAP` plus a target placement; 173 in the local corpus) is the
  second instancing mechanism, and is resolved too.
- Any other transformation operator, such as `CARTESIAN_TRANSFORMATION_OPERATOR_3D`, leaves the
  instance **unplaced**, with a reason. It is never placed silently at the identity.
- **Units:**
  - each representation context's length unit (`SI_UNIT` with its prefix, or `CONVERSION_BASED_UNIT`
    through its `LENGTH_MEASURE_WITH_UNIT`) gives a scale to millimetres;
  - every length leaving the graph is in mm, as OCCT's reader produces by default;
  - each node carries its scale, so the conversion happens once, at the boundary.
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
| on `CIRCLE` / `ELLIPSE` | the full conic's box (centre ± per-axis radius); tighter: endpoints plus the axis extrema that lie within the arc's range | contains the arc |
| on a B-spline curve | its control points' box; rational: weights all positive, else `Unknown` | convex-hull property |
| `SURFACE_CURVE` / `SEAM_CURVE` | its 3-D curve's box | the 3-D curve is the edge |
| face on `PLANE`, `CYLINDRICAL_SURFACE`, `CONICAL_SURFACE`, `SURFACE_OF_LINEAR_EXTRUSION` | the union of its bounding edges' boxes | these surfaces are linear in v, so for fixed u, every coordinate is extreme at the region's v-ends, which lie on the boundary |
| face on `SPHERICAL_SURFACE` | the sphere's box ∪ edges (centre ± r) | contains the surface |
| face on `TOROIDAL_SURFACE` | the torus' box: (R + r) radially, ±r along the axis | contains the surface |
| face on a B-spline surface | its control points' box (positive weights) | convex-hull property |
| face on `SURFACE_OF_REVOLUTION` | the basis curve's box revolved: its radial extent around the axis × its axial extent | contains the swept curve |
| face on `OFFSET_SURFACE` | the basis surface's box grown by \|d\| | contains the offset |
| anything else | `Unknown(type)` | — |

- Boxes compose **bottom-up** in one post-order pass: face → shell → solid → product (in its own
  coordinates) → instance (the 8 corners transformed, then boxed; loose under rotation, still
  conservative) → assembly → model. It's O(entities), done once after the structure.
- A node with any `Unknown` child is `Unknown` for that reason. It isn't a box of the rest, which
  would be too small.
- **What isn't covered:**
  - **Tightness:** a sphere or torus face gets its whole surface's box; a B-spline's control polygon
    can bulge well past the surface; a rotated instance's box is grown by the rotation.
  - **Faces with no edges** (a closed sphere bounded by a vertex loop, or a full torus) work: the
    table doesn't need edges for those surfaces. A bounded plane without edges can't exist in valid
    AP203/214.
  - **Geometry outside the B-rep:** wireframe, `GEOMETRIC_CURVE_SET` and PMI aren't in a product's
    box.
- **Checked against OCCT** (G4): on every corpus file, each product's graph box contains the kernel's
  `topology::Prototype::bbox`, within the kernel's own enlargement (an OCCT box is grown by its
  tolerance). The report gives the volume ratio (graph box ÷ OCCT box) per product, as tightness data,
  not as a gate.

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
    pub fn type_name(&self, n: Node) -> TypeName<'_>;                 // simple or complex
    pub fn refs(&self, n: Node) -> &[Node];
    pub fn referrers(&self, n: Node) -> &[Node];                      // built once, lazily
    pub fn args(&self, n: Node) -> Result<Args<'_>, ArgError>;        // parsed on access
    pub fn of_type(&self, name: &str) -> impl Iterator<Item = Node> + '_;
    pub fn census(&self) -> Census;                                   // counts by type
}

/// File text: attacker-controlled (#124). No Display, no Deref<str>: callers
/// decide what leaves, through #124's text policy.
pub struct Untrusted<'a>(&'a [u8]);
impl Untrusted<'_> { pub fn decode(&self) -> String /* header::decode_part21 */; }

// src/graph/structure.rs
pub fn structure(g: &Graph) -> Result<Structure<'_>, StructureError>;
pub struct Structure<'g> {
    pub roots: Vec<Occurrence<'g>>,          // the tree, in the outline's order
    pub products: Vec<Product<'g>>,         // one per PRODUCT_DEFINITION with shape
    pub leaves: usize,                   // == occt::Outline::leaves
}
pub struct Product<'g> { pub def: Node, pub name: Untrusted<'g>, pub id: Untrusted<'g>,
                     pub bodies: Vec<Node>, pub mm_per_unit: f64, pub color: Option<Color> }
pub struct Occurrence<'g> { pub nauo: Option<Node>, pub product: usize, pub name: Untrusted<'g>,
                        pub placement: Placement, pub children: Vec<Occurrence<'g>> }
pub enum Placement { Placed([f64; 12]), Unplaced(Reason) }
impl Structure<'_> { pub fn tree(&self) -> Vec<topology::Node>; }       // the viewer's tree type

// src/graph/bounds.rs
pub enum Bound { Box(BBox), Unknown(Reason) }                      // BBox in mm, crate::BBox
pub fn bounds(g: &Graph, s: &Structure) -> Bounds;
impl Bounds {
    pub fn model(&self) -> Bound;
    pub fn product(&self, p: usize) -> Bound;                       // its own coordinates
    pub fn leaf(&self, leaf: usize) -> Bound;                       // placed, model coordinates
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
- **MVP curves:** `LINE`, `CIRCLE`, `ELLIPSE`, directly or through a `SURFACE_CURVE`/`SEAM_CURVE`'s 3-D
  curve.
- A face's boundary refers to its points as `(edge node, index)`, walked in the oriented edge's
  direction. **Stitching is an index merge**, so neighbouring faces share vertices by construction.
  That's the contract #115's stitching comment set out.

**Faces in scope:** `ADVANCED_FACE` on `PLANE`, `CYLINDRICAL_SURFACE`, `CONICAL_SURFACE`,
`SPHERICAL_SURFACE` and `TOROIDAL_SURFACE`, bounded only by MVP edges.
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
1. **The win is in the transfer, not the meshing.** 72% of a slow file's time is OCCT's transfer and
   healing, and OCCT transfers and heals a whole shape. Routing one face of a product to OCCT still
   costs the product's whole transfer, and saves only part of the 22% that is meshing. Only routing
   whole products removes OCCT's work.
2. **No mixed-mesher seams.** Separate products share no edges. So v1 needs neither prescribed edge
   polylines inside OCCT's BRepMesh, which is unproven (#115), nor a re-sync after healing changes an
   edge.
3. **One interface to the kernel:** "these products". It is the unit the kernel already reports
   (`topology::PlacedPart`), and the unit the leaf numbering joins on.

- **The cost:** one B-spline fillet sends a whole bracket to OCCT.
- **The ledger records every face's reason anyway,** so the census reports how many products were lost
  to one or two hard faces. That number is the trigger for per-face routing (v2, filed in §5).

**The mesh quality contract.** The Rust path uses the same numbers the kernel uses, so the two halves of
a view are indistinguishable:
- **The chordal deviation** is at most `Deflection::linear_abs(diag)`, using the model's diagonal as the
  kernel does. The viewer uses `Deflection::PREVIEW`: 0.1% of the diagonal and 20°.
- **The angle** between consecutive segments of an edge polyline, and between the normals across any
  interior triangle edge, is at most `angular_deg`.
- **For a circle arc of radius r and sweep θ**, the segment count is
  `n = ⌈max(θ / a, θ / (2·acos(1 − d/r)))⌉`, where a is the angular deflection and d the linear one.
  Ellipses use their larger radius. Lines get no interior points.
- **The diagonal:** the graph's model box is conservative, so it may be larger than OCCT's. G4 reports
  the ratio. If it is over 1.05 on corpus files, the fast path uses the OCCT-equivalent diagonal of the
  B-rep's edges instead, so the Rust mesh is never coarser than OCCT's.
- **Checked in M4:** per product, the symmetric Hausdorff distance between the Rust mesh and the OCCT
  mesh of the same leaf, both run on the corpus. It must be ≤ 2·d, since each mesh is within d of the
  true surface. The check samples 1,000 points per product, or all vertices when there are fewer.
  Zero products over is the gate.

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
- **Products:** lower, by the products that contain any B-spline face or B-spline edge. A guess, to be
  replaced by G2's census: 50–70% of products in mechanical assemblies (fasteners, plates, shafts),
  and far less in the ABC subset, where each file is one freeform part.
- **The decision:** **if the census shows that more than 10% of otherwise analytic faces are excluded
  only by a 3-D B-spline edge** (in a cylinder–cylinder intersection, say), slice M1b adds B-spline
  curve evaluation (de Boor's algorithm, rational weights), about 150 lines. On an analytic surface the
  (u, v) inversion stays closed-form, so the face stays simple.

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
| heap beyond the map | ≤ 0.5× file size, and ≤ 40 B per entity | ≤ 110 MB | |
| outline today, for comparison | 4.6 s (OCCT parse) | 5.1 s | |
| first proxy frame in the viewer | ≤ 1.5 s from open | | |
| failures | 0 panics; every refusal names a byte offset | | 0 panics on all 401 + malformed |

- **The fast path's targets are measured, not promised.** M4 and M5 report, on Ultimaker, against the
  OCCT-only path (42 s today):
  - time to the first Rust-meshed product;
  - time to the first full view;
  - the share of products and of triangles that came from Rust.
- The MVP's bar: the first full view is no slower than today's, and the Rust-path products appear
  before OCCT's transfer would have finished.

## 3. Consequences

**The viewer:**
- **It opens on the graph:**
  - the tree comes from `Structure::tree()` instead of a cached outline artifact or the kernel's
    `Event::Tree`;
  - every leaf whose box is known is drawn as a **proxy**: a translucent box with a wireframe, never
    shaded like geometry, following #115's honesty rule;
  - a status line says "N parts shown as boxes".
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
- `STEPControl_Reader::TransferEntity` runs per routed product's shape representation. Its faces are
  reported with their STEP entity ids, for face colours.
- Names, the tree and colours now come from the graph, so this path doesn't need XCAF.
- **M5 starts by proving that** the colours and the geometry match a full XCAF run on the corpus. If
  that fails, M5 is reshaped (carving, #115 P3) before anything ships. Until M5 lands, M4 runs OCCT
  on the whole file as today, uses its meshes for the routed products, and uses them as the
  Hausdorff oracle for the Rust ones.

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

**MVP (this milestone):** G1–G7 and M1–M5 below, with:
- refusal without panics;
- the nesting cap;
- `Untrusted` text;
- the coverage ledger;
- proxy styling and the status line;
- the M4 Hausdorff gate;
- the M5 timing report.

**Later. Each is filed as an issue when this ADR is accepted:**
- **Hardening:**
  - a fuzz target for the scanner and the argument parser (cargo-fuzz), seeded from the malformed set;
  - a heap budget for the graph, admission-controlled like #108, with refusal over the budget;
  - `SIGBUS` on a file truncated while mapped (read small files into memory; handle or pre-check large
    ones);
  - adversarial id ranges and reference fan-out;
  - recovering from malformed records rather than refusing the file;
  - the graph inside the Quick Look extensions (sandbox, memory cap, the capi tripwire).
- **Speed:**
  - the parallel scan, split at record boundaries, equal to the serial scan by test;
  - the graph cached on disk, a mmappable table keyed like the outline artifact;
  - tighter arc, sphere and torus boxes.
- **Coverage:**
  - B-spline edge curves (unless M1b is triggered);
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
- **#124:** text screening, attestation and `reveal`, on top of `Untrusted`.

## 6. Slice plan

Each slice is at most ~400 product lines, plus its tests, and lands as its own PR behind review. Tests
run memory-bounded, one heavy job at a time.

**Order and why:**
- **G1 → G2 → G3 → G4 → G6:** proxies in the viewer as early as possible, which is the first visible
  value.
- **G5 (the CLI)** runs beside G6 once G4 lands, since #124 waits on it.
- **G2's census gates the M slices:** if the product share the fast path could cover comes out far
  below the guess in §2.6, the M plan is re-cut before M1, not after.
- **G7 (colours) comes before M4,** so that the first view combining both meshers already looks
  consistent.
- **M5 comes last,** since it changes the kernel and its acceptance is the timing that justifies the
  fast path.

| Slice | Scope | Acceptance | Test command |
| --- | --- | --- | --- |
| **G1** Scanner + entity table | `src/graph/{mod,lex}.rs`: map, header via `header::read`, records → id, interned type (complex included), arg span; the id → node lookup (dense or sorted); refusal with a byte offset | A table equal to an independent oracle (`ids` and types from a regex pass) on `tests/data`; all malformed fixtures refuse without panicking; Ultimaker/stress within §2.7's scan time and heap | `nix develop -c cargo test graph::` and `nix develop -c cargo run --release --example graph_scan -- <file>` (time and heap) |
| **G2** CSR + lazy args + census | Forward CSR in the scan; `args()` → `Value`, depth cap 64; `referrers()` built lazily; `census()`; `examples/graph_census.rs` writes per-file surface/edge-curve shares and the products all of whose faces are analytic, as a markdown table | Round trip: every `#n` in a span is in `refs`, in order; args parse for every entity of every corpus file; the census table is committed to `docs/adr/0001-census.md` and §2.6's guess is replaced with it | `nix develop -c cargo test graph::` and the census example over the corpus |
| **G3** Structure + placements + units | `structure()`: roots, NAUO children in outline order, naming rules, shapeless products dropped, `ITEM_DEFINED_TRANSFORMATION` and `MAPPED_ITEM`, rep_1/rep_2 matching, units to mm, `Unplaced(reason)` | `Structure::tree()` equals `occt::outline`'s tree (names, shape, leaf count) on every corpus file except the two documented multi-file assemblies; placements equal `--topology`'s `PlacedPart::transform` within 1e-9 relative | `nix develop -c cargo test --test graph_structure` (runs the kernel for the oracle) |
| **G4** Conservative boxes | `bounds()`: the per-type table of §2.3, post-order composition, `Unknown` propagation, instance boxes | On every corpus file, each product's box contains OCCT's prototype `bbox` (within its tolerance enlargement); 0 violations; the tightness ratio per product is reported; §2.7's structure + boxes time | `nix develop -c cargo test --test graph_bounds` |
| **G5** `stepv graph` CLI | `summary` (header, units, census, model box), `tree` (paginated, depth-limited), `census`, `query <jq>` via `jaq` over the lazy view (§2.4); text as `{"untrusted": …}`; `--timings` | Golden JSON on `tests/data`; an injection-style product name comes back only as a data field; the lazy-view test: a one-product face walk on the stress assembly keeps the heap ≤ 2× the graph's (this decides `ValT` vs native functions) | `nix develop -c cargo test --test graph_cli` |
| **G6** The viewer's proxies | `Load::Graph`, proxies drawn translucent with a wireframe, the status line "N parts as boxes", meshes replace proxies per leaf, the mismatch rule, `OutlineArtifact::boxes` filled | Headless viewer test (`tests/view.rs`): first frame with proxies before `Done`; no proxy drawn with the shaded pipeline; a refused graph gives today's behaviour; the artifact's `boxes` length equals `leaves` | `nix develop -c cargo test --test view` |
| **G7** Colours from the graph | `STYLED_ITEM` chain → product and face colours, predefined colours, overrides | On the corpus, the graph's per-product and per-face colours equal the kernel's XCAF colours for every leaf | `nix develop -c cargo test --test graph_colors` |
| **M1** Canonical edge polylines | `mesh::edges`: LINE/CIRCLE/ELLIPSE (direct or via `SURFACE_CURVE`), the §2.6 segment count, one polyline per `EDGE_CURVE`, shared and oriented | Unit tests: chordal error ≤ d and turning angle ≤ a on arcs from 1e-3 to 1e3 mm; two faces sharing an edge get identical points in opposite order | `nix develop -c cargo test mesh::edges` |
| **M1b** *(conditional on G2's census)* B-spline edges | de Boor evaluation, rational weights, flattened to the same contract | As M1, plus curves from the corpus against OCCT's edge polylines from `--edges` | `nix develop -c cargo test mesh::edges` |
| **M2** Planar faces with holes | Plane inversion, `spade` CDT with the outer loop and holes, flood removal, exact normals, routing on `try_add_constraint` failure | Area equals the topology's face area within 1e-6 relative on planar corpus faces; the 1,250-hole plate (ABC `00000046`) meshes, with its time and memory reported against OCCT's 7.7 s and 4.2 GB | `nix develop -c cargo test mesh::plane` |
| **M3** Cylinder, cone, sphere, torus | Closed-form (u, v) inversion, u unwrapped on periodic surfaces, seams, poles, Steiner grid at the deflection step | Fixture solids (full and partial cylinders, cones to the apex, spheres with poles, tori) mesh watertight (every interior edge used by exactly two triangles); area within the deflection's bound of the topology's area | `nix develop -c cargo test mesh::analytic` |
| **M4** Rayon + routing + ledger + the combined view | `classify`, `mesh_product`, the ledger, rayon over products, the viewer and the CLI combining Rust products with OCCT's routed ones (OCCT still runs on the whole file), the cache key's `mesh::VERSION` | **The Hausdorff gate:** every Rust product ≤ 2·d from OCCT's mesh of the same leaf over the corpus, 0 over; the ledger in `--info`; the viewer's status line; triangles and normals well formed (`Part::is_well_formed`) | `nix develop -c cargo test --test fastpath` and `nix develop -c just fastpath-corpus` |
| **M5** The kernel transfers only routed products | `stepv-occt --products <ids>` via `TransferEntity`, face entity ids in the output, the Rust side passing only routed products | Geometry and colours of routed products equal a full XCAF run on the corpus; **the timing report on Ultimaker:** first Rust product, first full view, the Rust share of products and triangles, against 42 s OCCT-only | `nix develop -c cargo test --test kernel_products` and `nix develop -c just fastpath-bench real-assembly` |

`just fastpath-corpus` and `just fastpath-bench` are added by M4 and M5. The `--test` targets are
created by their slices.

## 7. Open questions for the owner

1. **M1b:** the plan adds B-spline edge curves to the MVP if more than 10% of otherwise analytic
   faces are excluded only by them. Is 10% the right trigger, or should B-spline edges simply be in M1?
2. **M5's kernel option** moves names, the tree and colours from XCAF to the graph for routed runs.
   Is it acceptable that the kernel's XCAF run stays only as the oracle and as the fallback when the
   graph is refused?
3. **Proxy style:** a translucent box with a wireframe, plus the status line. Or primitive fits
   (cylinders, spheres) already in the MVP? The plan says boxes only. Fits are later, with the cost
   model.
4. **The Hausdorff gate is per product, at 2·d.** Is that strict enough for the viewer, or should the
   gate be d against an OCCT run at half the deflection?
