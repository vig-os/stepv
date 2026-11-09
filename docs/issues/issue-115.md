---
type: issue
state: open
created: 2026-11-08T14:12:51Z
updated: 2026-11-09T04:48:30Z
author: gerchowl
author_url: https://github.com/gerchowl
url: https://github.com/vig-os/stepv/issues/115
comments: 11
labels: discussion
assignees: none
milestone: MVP: graph core
projects: none
parent: none
children: none
synced: 2026-11-09T09:29:51.934Z
---

# [Issue 115]: [spike: budgeted preview tiers — raw STEP scan for per-part proxies (box/cylinder/sphere), a cost model from file counts, culling hidden parts before transfer](https://github.com/vig-os/stepv/issues/115)

## Motivation
Today every file pays the full price: OCCT reads the whole file, transfers everything (S1: **72% of wall time on slow files**; #51: p-curves about 90 s on one thread), meshes everything, then draws. Quick Look gives a thumbnail 15 s and 4 GB. Big assemblies blow that budget and end with a deadline or an error image, though a thumbnail only needs the outer silhouette at about 256 px.

The owner asked for three things that compose:
1. **"Disk streaming" with proxies:** a fast pass over the file that yields per-part boxes, or better a cylinder or sphere, drawn at once, before any geometry exists.
2. **Hide internals for previews:** parts that can't be seen from outside (bolts inside housings, internal mechanisms) are skipped for thumbnails and previews.
3. **A tier chosen from the file itself:** its size and entity counts predict cost, so the work is chosen to fit the budget instead of running into the deadline.

## Proposed approach (best guess, for review)
1. **Raw STEP scan, without OCCT.** A single streaming pass over the Part 21 text, linear and with bounded memory, yields:
   - **counts:** bytes, entity counts by type (ADVANCED_FACE, B_SPLINE_SURFACE*, CARTESIAN_POINT…), products, and instances (NEXT_ASSEMBLY_USAGE_OCCURRENCE);
   - **per-product proxies:** the extents of the CARTESIAN_POINTs reachable from each product's shape representation, placed by the assembly transforms. Where a product's faces are all CYLINDRICAL_SURFACE or SPHERICAL_SURFACE, or one dominates, fit that primitive (axis plus radius from the surface entity itself) instead of a box.
   
   #54 already lists "boxes before transfer would need a cheaper pass, e.g. CARTESIAN_POINTs per product; unproven". This spike proves it or kills it.
2. **Cost model from data.** Fit predicted transfer and mesh time, and peak RSS, from the scan's counts, using the per-file measurements that #83's profiling gates record over the corpus. This is a fitted regression: "learned" means fitted to our own corpus, not ML. It's checked into the repo and re-fitted when the ratchet moves.
3. **Tier selection** from that prediction against the front end's budget:
   - **T0:** proxies only. Instant, and always the first frame.
   - **T1:** transfer and mesh only the visible products, at coarse deflection.
   - **T2:** everything at PREVIEW deflection.
   - **T3:** full quality, viewer only.
   
   Thumbnails stop at the tier that fits; the viewer and preview refine progressively, T0 → T1 → T2.
4. **Culling internals before transfer.** Render the T0 proxies into an ID buffer from the thumbnail's fixed view, or from the 6 to 14 directions an orbiting preview needs. Products with no visible pixels aren't transferred at all for T1. STEPControl can transfer roots selectively; XCAF needs checking.
   - Proxies are larger than their parts, so a proxy may only be used as the thing tested for visibility, never as an occluder. Only meshed geometry (or a proven-inner proxy) may hide another part. That keeps the culling conservative: a visible part is never dropped.
   - Order: mesh the large outer parts first, then test smaller parts' proxies against the depth buffer.

## What already exists
- Bbox-relative deflection bands (`Deflection::{THUMBNAIL, PREVIEW}`, plan.md §4).
- The outline artifact with the tree and an optional `boxes` field (#62; boxes are empty until #66 and the PartBox frames from #54 P1).
- Streaming SVEV frames (TREE, PROGRESS, DONE).
- Deadlines and the admission limit for Quick Look (#64, #108).
- The memory cap.

## Scope
- **P0 (spike, measured):**
  - a scan prototype over the corpus (391 files plus Ultimaker), measuring scan time, memory, and box accuracy against the transferred shapes' boxes;
  - which share of products get a usable proxy;
  - how far each product's points land outside or inside its real box.
- **P1:** a cost model fitted on #83's data, with its prediction error per file reported.
- **P2:** a culling prototype on assemblies, measuring the share of products culled, the transfer time saved, and pixels lost (it must be 0 against the full render at thumbnail size).

## Pitfalls
- **Points aren't the shape.** CARTESIAN_POINTs include B-spline control points, which lie outside the surface, so boxes come out loose. Is a loose box acceptable for culling (conservative, yes) and for display?
- **Shared and instanced geometry,** mapped items, and multi-file assemblies (#92). Products whose geometry isn't reachable by a simple reference walk.
- **Occluder correctness:** proxies must never occlude. A cutaway or transparent preview breaks culling.
- **Orbiting:** the preview can rotate, so "hidden" must hold from every direction, or culling is for thumbnails only.
- **The cost model drifts** with OCCT versions and healing settings (#105: the heal budget is nondeterministic). It needs re-fitting tied to the ratchet.
- **The scan duplicates STEP parsing:** a second parser to maintain and secure (fuzz it, and bound its memory).

## Acceptance
- A spike report with corpus numbers for each P item, and a recommendation: adopt, reshape or drop, with an ADR if adopted.
- Follow-ups filed per adopted tier.

## References
- #54 (progressive loading), #66 (per-part boxes), #62 (outline artifact), #83 (profiling gates, the data source), #72 and #73 (parallel mesh and heal), #51 (where the time goes), plan.md §4 "Progressive display" and "Deflection", #92 (multi-file assemblies).

---

# [Comment #1]() by [gerchowl]()

_Posted on November 8, 2026 at 02:13 PM_

**Owner's addition: running bounding boxes, plus winding numbers to identify internals.** This becomes a second culling method beside the visibility approach in P2.

1. **Running boxes during the scan.** While streaming the file, accumulate each product's extent as its points go by, transformed by the assembly placements. Box containment, where part B's box lies inside container A's box, is a cheap test that is *necessary but not sufficient* for "B is internal". Use it as the candidate filter.
2. **Generalized winding number confirms it** (Jacobson et al. 2013, "Robust Inside-Outside Segmentation using Generalized Winding Numbers"; fast evaluation with a Barnes–Hut tree, Barill et al. 2018). For each candidate, evaluate the winding number of A's coarse mesh at sample points of B. B is internal if every sample has w > ½.
   - It's robust to the open and non-watertight meshes that imperfect STEP gives us, where a ray-parity test would flip.
   - It is **view-independent**: "enclosed" holds from every direction, so it is valid for the orbiting preview and not just the fixed thumbnail view, which the ID-buffer method can't promise.
3. **Conservative by construction.** B's samples are its scanned CARTESIAN_POINTs. B-spline control points can lie outside the surface, but that can only stop a cull, never cause a wrong one. A part visible through an opening has samples near w ≈ 0 and stays.
4. **Cost.** Only the candidate containers need a coarse mesh, so only they need transferring. Every culled B skips transfer, healing and meshing entirely. Evaluating the winding number is O(samples × log triangles) with the tree.

**Measure in P2:**
- the share of products culled by enclosure compared with by visibility;
- false keeps;
- **zero false culls**, against a full render from 14 directions;
- the time saved against the cost of transferring the containers.

**Pitfalls:**
- "Enclosed" is stricter than "invisible": parts hidden behind others but not enclosed stay, so the visibility pass is still needed for those.
- Cutaway, section or transparent views turn culling off.
- Containers that are themselves only partly closed, such as a housing with a lid in another part, need the winding number over the *union* of the enclosing parts' meshes, not over a single container.


---

# [Comment #2]() by [gerchowl]()

_Posted on November 8, 2026 at 02:13 PM_

**Framing (owner): the scan *is* a graph model of the STEP file.** This makes P0 concrete.

A Part 21 file is already a graph: every line `#id = TYPE(args)` is a node, and every `#n` in its args is an edge. P0 builds that graph once, without evaluating any geometry, and everything else in #115 becomes a query on it.

**How to build it, with bounded memory:**
- One streaming pass over a memory-mapped file records per entity: the id, a type tag (u16), the byte offset of its args, and its outgoing references in a compact CSR edge array.
- Arguments are parsed lazily, only for the entities a query touches.
- Estimate: about 20–30 bytes per entity plus edges, so roughly 150–250 MB for a 5 M-entity file and far less for typical ones, against OCCT's multi-GB. Measure it.

**What the graph gives directly:**
- **Assembly structure:** PRODUCT → PRODUCT_DEFINITION → SHAPE_REPRESENTATION, with NEXT_ASSEMBLY_USAGE_OCCURRENCE edges and their placements giving the instance tree and transforms.
- **Per-product proxies:** reachability from each product's representation to its CARTESIAN_POINTs gives running boxes. Surface types (CYLINDRICAL_SURFACE, SPHERICAL_SURFACE) give primitive fits.
- **Deduplication beyond explicit instancing:** a structural hash of each product's subgraph (types plus parameters, with ids ignored) finds identical parts that were written out N times as separate products, the same bolt 50×. Each distinct hash is transferred and meshed once and placed N times. This feeds #72 (mesh one prototype per thread) and shrinks the culling work as well.
- **Cost features for the model:** face counts, spline surface counts and degrees, and **the longest EDGE_LOOP per product**. #74's O(n³) is in edges per wire, so the worst case of FixSelfIntersectWire can be predicted from the graph before OCCT ever runs.
- **Containment candidates** for the winding-number culling, from the running boxes.

**Prior art to evaluate, not assume:** Foxtrot's Rust STEP parser (`step` crate) as the tokenizer, and ruststep. Either way the scanner must be fuzzed, memory-capped, and must refuse malformed input cleanly, since it's a second parser on untrusted input.

**Added to P0:** graph build time and memory over the corpus; the share of products whose proxies come from reachability alone; and the share of duplicate products found by hashing beyond explicit instancing.


---

# [Comment #3]() by [gerchowl]()

_Posted on November 8, 2026 at 02:16 PM_

**Owner's addition: stream the refinement as diffs from a background process.** A background process walks the graph and keeps refining; the front end applies a stream of patches. This is the architecture the other pieces slot into, so the spike should end in an ADR.

**1. The scene as a versioned tree of slots, one per graph node** (assembly, product, instance, optionally face). Each slot holds its current best representation. The kernel streams SVEV patch frames:
- `Upsert(node, repr, level)`: a box, then a primitive, then a coarse mesh, then a preview mesh, then the full mesh;
- `Cull(node, reason)`: enclosed, or invisible;
- `Progress` and `Done`.

TREE, PROGRESS and DONE already stream. PartBox (#54 P1, #66) is the first `Upsert` kind. Front ends apply the patches atomically per slot. The viewer redraws, the preview swaps meshes in place, and the thumbnail renders whatever state exists at its deadline. That makes it an *anytime* algorithm: a deadline means "stop refining", never "error".

**2. Bounding volumes at every depth cost nothing extra.** Boxes compose bottom-up: a parent's box is the union of its children's transformed boxes. So after the scan, a single post-order pass gives a box for every node at every depth, in O(entities). Streaming them depth by depth (whole model → sub-assemblies → products → faces) is only a question of what to send first; there's no extra computation. Per-face boxes come from each ADVANCED_FACE's points, in the same pass.

**3. Neither one level at a time nor a fixed number of levels: a priority queue.** Each pending refinement has a benefit and a cost:
- **benefit:** the error it removes on screen, i.e. the node's projected size times the current representation's error (a box is worse than a primitive, which is worse than a coarse mesh);
- **cost:** from the cost model, using the graph features.

Pop the best benefit-per-cost first. Cheap nodes go several levels deep in one go; expensive ones get one step at a time, or wait. The thumbnail's fixed view and the preview's current camera set the projected sizes, and a camera move reprioritises the queue.

**4. Per-product transfer by carving the graph.** The expensive levels need OCCT. From the graph, write each product's subgraph out as a standalone mini STEP file (its entities plus the shared contexts: units, the representation context, and the styles it uses) and transfer only that. This gives:
- **selective transfer:** only what the queue asks for, and never culled parts;
- **parallelism across processes,** each memory-capped and crash-isolated per part. That overlaps with #72 and #73, and complements their in-process threading;
- **a content-addressed cache:** key each result by the subgraph's structural hash, so the same bolt is meshed once across all instances and across *files*.

**Pitfalls to measure:**
- carving has to copy every shared entity: units, contexts, and the presentation and style layers XCAF reads for colours;
- assemblies whose geometry crosses product boundaries;
- the cost of each OCCT process start for many small parts, which suggests batching small products into one carve;
- patch ordering and atomicity in every front end, and memory for the slots on huge assemblies;
- it must keep working when the queue never reaches the full level (the deadline case), and when a part's refinement fails (that slot keeps its last good level, flagged as such).

**Added to the spike as P3:**
- a prototype of the patch stream with box, primitive and mesh levels, driven by the priority queue;
- carving tested on the corpus: what share of products transfer standalone with identical geometry to a full transfer, and the per-process overhead;
- time to first frame and to "visually complete" at thumbnail size, against today's pipeline.

**The end state is an ADR** covering the slot and patch protocol (a SVEV extension), the queue's metric, the carving rules and the cache key.


---

# [Comment #4]() by [gerchowl]()

_Posted on November 8, 2026 at 02:17 PM_

**Owner's question: mesh in Rust with rayon, and drop most of OCCT, since we only visualise and never build geometry.**

**Partly right.** We never need OCCT's *modelling* (booleans, fillets, offsets, construction). But visualising a B-rep still means doing three hard things:
1. evaluating every surface type, including B-splines, surfaces of revolution and extrusion, and offset surfaces;
2. getting each face's trimming loops into its (u,v) parameter space. When p-curves are missing, they have to be built by projecting the 3-D edges. **That is exactly #51's roughly 90 s hot spot**, and it is numerically hard on periodic seams, poles and apexes;
3. **healing** what real files get wrong: self-intersecting wires, gaps, missing edges. plan.md §2 found that this is what makes 99.5% of the corpus faithful.

plan.md §2 already evaluated the pure-Rust path. Foxtrot is precisely "Rust, its own constrained Delaunay triangulation, about 10× faster than OCCT", and it was rejected because it has **no healing, an incomplete surface set, and it fails silently**. Wrong geometry is worse than slow geometry. So a full replacement is the decade of work §2 chose to avoid.

**What is right: a hybrid, chosen per face from the graph.**
- **A Rust fast path, parallel with rayon across faces and products,** for faces that are *provably simple*:
  - planes, cylinders, cones, spheres and tori bounded by lines and circles;
  - their (u,v) trims are closed-form, with no projection and no healing;
  - tessellating them is cheap and robust.
  
  Mechanical parts are dominated by these. P0's graph scan can measure the share per file directly, from the ADVANCED_FACE surface types and the edge curve types.
- **OCCT for everything else,** via the per-product carving above (processes, capped and parallel): B-spline faces, faces missing p-curves, and anything the graph or the fast path flags as suspect.
- **Never a silent fallback.** A face the fast path can't certify goes to OCCT, and that routing is recorded. Correctness is gated in CI by the corpus diff (#105's harness): fast-path faces against OCCT's for the same face, within a Hausdorff tolerance tied to the deflection.

**Pitfalls:**
- **Cracks where a fast-path face meets an OCCT face:** both must use the same discretisation of the shared edge. Discretise edges first, and have both paths consume those polylines, which needs checking against BRepMesh. Invisible in a thumbnail; it matters in the viewer.
- **Licensing:** the original Foxtrot is CC BY-NC and must not be copied (plan.md §1). The diodeinc fork is Apache-2.0/MIT and can be evaluated as a source of its triangulation, or use a Rust CDT crate such as `spade`.
- **Two meshers to maintain,** with drift in tolerances and normals.

**Added to the spike as P4:**
- the share of faces and of time on the fast path, per corpus file;
- fast path compared with OCCT on those faces, for accuracy and wall time, with rayon and with one thread;
- the crack rate at mixed edges.

Adopt only if a large share of time moves to the fast path with zero accuracy failures.


---

# [Comment #5]() by [gerchowl]()

_Posted on November 8, 2026 at 02:19 PM_

**Owner's correction to the crack pitfall: the graph already defines the stitching.** It's right, with one condition.

In STEP the topology is explicit and *shared*. Two adjacent faces both reach the same `EDGE_CURVE` entity, each through its own `ORIENTED_EDGE`, and the edges reach shared `VERTEX_POINT`s. So the graph says exactly which faces meet on which edge.

**The contract:**
- **Edge-first discretisation, once per `EDGE_CURVE` node.** Each edge gets one canonical polyline: its vertex points at both ends, interior points from its curve at the model deflection.
- **Every face mesher uses those polylines as its fixed boundary,** whichever process or mesher (Rust or OCCT) handles the face. Face meshes are then patches that refer to boundary points by `(edge node, index)`, and stitching is an index merge, not a geometric search. No cracks, by construction.

This is how OCCT's own BRepMesh works inside: edges first, then faces. We lift that step out to the graph.

**It also gives invalidation for the streaming design for free.** Refining an edge's polyline marks every face that uses it (edge → faces is a graph query) for re-meshing, as new patches in the priority queue.

**The condition: healing.** When OCCT *repairs* a face (splits or merges edges, rebuilds a self-intersecting wire, ShapeFix), that face's boundary no longer matches the graph's `EDGE_CURVE`s, and the shared-edge contract breaks for it and its neighbours.
- **Mitigation:** the healing step reports which edges it changed. Every neighbour on those edges is re-meshed against the healed polyline, as a follow-up patch. In practice it is rare: about 0.5% of the corpus needs healing.
- The 3-D edge curve and the face's surface may disagree within tolerance. That's harmless here: both faces use the same points.

**To check in the spike:**
- **Can OCCT be given prescribed edge polylines?** IMeshTools' pipeline is pluggable (an edge discretiser and a face discretiser in `BRepMesh_Context`), or OCCT's edge discretisation can serve as the canonical source that the Rust path consumes.
- **Seam edges on periodic surfaces and degenerate pole edges,** where an edge borders the same face twice.

This replaces the "cracks at mixed edges" pitfall in P4 with a measured check: zero open edges in the assembled mesh, counted by matching edge-node indices.


---

# [Comment #6]() by [gerchowl]()

_Posted on November 8, 2026 at 02:22 PM_

**Consolidated pipeline (owner): a multi-pass parallel build over the Rust graph, handing what Rust can't do to OCCT.** This is the shape the ADR should take.

| Pass | Work | Parallel over | Output |
|---|---|---|---|
| 0 Scan | Part 21 text → entity index plus CSR references; memory-mapped, args parsed lazily | file chunks split at entity boundaries (rayon), mind quoted strings | the graph |
| 1 Structure | assembly tree, instances and placements, structural hashes, bottom-up boxes | subtrees (post-order) | **T0 proxies streamed**; dedupe map |
| 2 Classify | each face and edge: *Rust-supported* or *route to OCCT*, with a recorded reason | faces | the routing table |
| 3 Edges | one canonical polyline per `EDGE_CURVE` | edges (rayon) | shared border points |
| 4 Faces | Rust CDT on supported faces; OCCT for the routed ones, in carved mini-STEP files run by a process pool | faces (rayon), products (processes) | face patches, plus the edges OCCT changed |
| 5 Stitch | merge by (edge node, index); re-mesh the neighbours of edges changed by healing | products | crack-free mesh patches **streamed** |
| 6 Refine | the priority queue sends chosen nodes back through passes 3–5 at a finer deflection; culling queries run here | queue items | further patches |

**Run it as a dependency graph, not with global barriers.** An edge needs only its curve; a face needs only its edges; a product's patches go out as soon as its faces are done. With rayon scopes and work stealing, the first products stream while others are still being classified. The only real barriers are per product (stitching) and the healing re-sync.

**Routing classes for "not yet supported", all recorded and never silent:**
- surface types without a Rust evaluator yet (B-spline, offset, swept surfaces at first);
- non-analytic faces missing p-curves;
- faces that fail validation: a uv wire self-intersects, or an edge gap exceeds tolerance;
- a failure at run time in the Rust mesher, such as a failed CDT.

Supported coverage grows over time, and the corpus diff gates each new surface type before it moves to the Rust side.

**v1 simplification, recommended: route whole *products*, not single faces.** A product with any face routed to OCCT goes to OCCT entirely; fully supported products are meshed in Rust. Separate products don't share edges, so the boundary between meshers falls between solids and **v1 needs neither prescribed edge polylines in OCCT nor heal re-sync**. Face-level mixing is v2, adopted only if the corpus shows that many products have just one or two hard faces, and only once OCCT has been proven to accept prescribed edge polylines.

The P0 measurements decide whether v1 pays off: per corpus file, the share of products that are fully supported, and the share of time they account for.


---

# [Comment #7]() by [gerchowl]()

_Posted on November 8, 2026 at 02:25 PM_

**Owner: the queue starts from the boxes; detail is capped by a budget; and degraded output must never lie.**

**1. The queue is seeded as soon as Pass 1 finishes.** Pass 1 gives every node a box at every depth, so the priority queue exists from that point and *drives every later stage*. Classify, edges, faces and refine are per-node stages scheduled by the queue, not sweeps over the whole file.
- Priority = projected screen size of the node's box (for the thumbnail's fixed view, or the preview's or viewer's current camera) × the error of its current level ÷ the predicted cost.
- Large visible parts are processed first, and tiny far-away screws last or never.
- A camera move, a selection, a section plane or a zoom re-prioritises the queue.

**2. A detail budget caps the work.** It is a budget on triangles, memory and time. When the budget is spent, refinement stops and every node keeps its current level: box, primitive, coarse or full.

**3. Configurable, with defaults set by the front end:**

| Front end | Default budget | Override |
|---|---|---|
| thumbnail | Quick Look's deadline and 4 GB; whatever is reached | none |
| preview | the deadline for the first frame, then keep refining until done or capped | none |
| viewer | sized from RAM (like the admission limit L) | `--detail-budget triangles=…,mb=…`, and a "load full detail" action |
| CLI / batch | same as the viewer | `--detail-budget …`; `--strict` **fails** instead of degrading, for automation where a degraded output file would be a silent lie |

**4. Honesty: always show, never pass a proxy off as geometry** (plan.md §4, "Degrade honestly"):
- **Unrefined nodes are drawn in a distinct proxy style,** for example a translucent box or primitive with a wireframe, never shaded to look like real geometry.
- **A status line states it plainly:** "312 of 4,100 parts shown as boxes (detail budget)" or "… (deadline)".
- **No modal dialog to accept it,** since a blocked view is worse. The viewer offers "refine all" or "raise budget" together with the cost model's prediction ("≈ 40 s, ≈ 6 GB"), and refines a part on selection or zoom.
- **Quick Look preview:** a banner such as "Simplified: N parts as boxes. Open in stepv for full detail."
- **Thumbnail:** a small corner badge when the image isn't complete, plus the proxy style itself.
- **Culled internals are view policy, not data.** The tree marks them "not loaded (enclosed)". Selecting one, a section plane or transparency re-queues them. They are never presented as absent.
- The CLI, `--info` and the outline artifact report the levels reached (count per level, the cause of each stop), so scripts can see degradation too.

**Added to the acceptance criteria:**
- no rendered frame shows a proxy styled as geometry;
- every degraded output, image or file, carries a machine-readable and visible marker;
- `--strict` exits non-zero when the budget would degrade the output.


---

# [Comment #8]() by [gerchowl]()

_Posted on November 8, 2026 at 02:27 PM_

**Owner: every case Rust can't solve (OCCT can), and every case neither can, becomes backlog; gaps get uplifted into Rust by investigation, not translation.**

**1. The coverage ledger.** Each routing decision and each failure carries a **reason signature**, a stable, coarse class, never a single file. Examples:
- `surface:B_SPLINE_SURFACE_WITH_KNOTS/rational/deg5`
- `pcurve:missing/periodic-seam`
- `wire:self-intersecting(uv)`
- `edge:gap>tol`
- `rs-fail:cdt`
- `occt-fail:BRepMesh/NotDone`
- `neither:<class>`

Each signature has three outcomes: **rs-ok**, **routed-to-OCCT (OCCT ok)**, and **neither**, which today is drawn as the flagged overlay.

**2. Backlog from the ledger, deduplicated.** A corpus CI job aggregates the signatures. A script keeps **one issue per signature**, labelled `coverage` with the signature in the title:
- it **updates** the counts and sample files in place;
- it never files a new issue per run or per file;
- it closes an issue when the count reaches 0.

Sample files from the members-only CAx-IF corpus (#53) are named by id only, never attached. User runs record the ledger **locally only**: no telemetry upload, though `stepv --info` can print it for a user to paste into a report.

**3. Uplift process per signature (one issue → its own small ADR):**
1. **Trace what OCCT does,** from its outside behaviour: ShapeFix and ShapeExtend status flags per face (which fixes fired), BRepMesh status, the inputs and outputs it changed, and the timing.
2. **Investigate:** OCCT's documentation, the literature (for example trimmed-surface tessellation, p-curve projection and point inversion, wire repair), and the failure's geometry in the corpus.
3. **Design from first principles** in an ADR, which states why the approach is right, not just that it matches OCCT.
4. **Implement in Rust** behind the routing table, with OCCT still the reference.
5. **Gate it:** the corpus diff against OCCT for that signature within tolerance, plus fuzzing. Only then flip the route to rs-ok.

**4. "Neither" cases** get the same issue, plus the visible overlay. They're research items: the file may be genuinely broken, and then the right outcome is an honest flag, not a guess.

**5. Licensing, a hard rule.** stepv is **Apache-2.0**; OCCT is **LGPL-2.1 with an exception**. Translating OCCT source into Rust, even as "a first step", would make that module a derivative of LGPL code, which can't be shipped under Apache-2.0. So:
- **No porting or transliterating of OCCT code.**
- **If reading OCCT's source is needed to understand a behaviour,** use a clean-room split. One agent reads OCCT and writes a **behavioural spec**: inputs, outputs, invariants, edge cases, and *no code or code structure*. A **different** agent, which has never seen that source, implements from the spec plus the literature. Both sides are recorded in the ADR.
- Tracing OCCT as a black box (status flags, inputs and outputs) needs no clean room.


---

# [Comment #9]() by [gerchowl]()

_Posted on November 8, 2026 at 07:55 PM_

Linked: #124 (stepv as an agent tool, CLI + MCP). The graph core here should also serve as #124's query index: the lazy JSON view for jaq, per-node bounding boxes for `relations`, and answer tiers for the honesty contract. Design them together.

---

# [Comment #10]() by [gerchowl]()

_Posted on November 8, 2026 at 11:56 PM_

ADR-0001 merged (#132, f05a906). Slices filed in milestone "MVP: graph core":
- G1 #133, G2 #134, G3 #135, G4 #136, G5 #137;
- G6a #138, G6b #139, G6c #140, G7 #141;
- M1a #142, M1b #143, M2 #144, M3 #145, M4a #146, M4b #147, M5 #148.

Landing order (ADR §6):
- G1→G2→G3→G4→G6a→G6b→G6c, with G5 alongside once G4 lands;
- G2's census gates M;
- G7 before M4b;
- M5 last.

Dispatched one at a time (host rules v3).

---

# [Comment #11]() by [gerchowl]()

_Posted on November 9, 2026 at 04:48 AM_

Owner decision (2026-11-09): **no parallel OCCT kernel pool.** Coverage moves into Rust first, as a clean-room reimplementation (not a port of OCCT code, per ADR-0001 §4). Then the Rust path is parallelised with rayon: M4a's meshing in the MVP, and #165's scan when needed. After the MVP, the coverage issues come first: #171 (starting with SW0/SW1, recognising swept surfaces that are really analytic), then #172 (B-spline surfaces), then #170 (per-face routing).

