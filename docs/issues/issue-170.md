---
type: issue
state: open
created: 2026-11-09T02:32:46Z
updated: 2026-11-09T02:32:46Z
author: gerchowl
author_url: https://github.com/gerchowl
url: https://github.com/vig-os/stepv/issues/170
comments: 0
labels: feature, priority:low
assignees: none
milestone: none
projects: none
parent: none
children: none
synced: 2026-11-09T09:29:29.823Z
---

# [Issue 170]: [Per-face routing (v2): mesh analytic faces in Rust, carve hard faces for OCCT, stitch at shared edges](https://github.com/vig-os/stepv/issues/170)

> Design brief written by a read-only design agent (2026-11-09) from ADR-0001 §2.5/§2.6/§5, `docs/adr/0001-census.md`, the M slices (#142–#148) and `kernel/`. Deferred from ADR-0001 §5 ("Coverage: per-face routing (v2)"). Not in the MVP milestone. Not yet reviewed; the recommendation is the agent's.

**Recommendation:** don't build v2 yet. Build two small slices now: **V0**, which measures, and **V3**, a spike that tests whether OCCT will use prescribed edge polylines. Build the rest only if V0's gate passes. The reason: before Rust B-spline surfaces exist, the faces v2 sends to OCCT are the costly ones, so they keep OCCT's critical path.

## 1. The problem and the approach
OCCT heals what it transfers, so its edges may stop matching the graph's `EDGE_CURVE`s. Three ways to have OCCT mesh only some faces:

- **Mesh the whole product in OCCT and keep only the hard faces' triangles.** Rejected. This is v1's cost (parse, transfer and healing of the whole product) with seams added. It saves nothing.
- **Prescribe polygons through `BRepMesh`.** This doesn't choose which faces OCCT reads, so it doesn't cut parse or transfer. It is the edge contract, and v2 uses it inside the next option.
- **Carve a mini-STEP per representation (chosen).** It holds:
  - the hard `ADVANCED_FACE`s and everything they reference;
  - copies of the representation context (units, uncertainty);
  - a synthetic `OPEN_SHELL` → `SHELL_BASED_SURFACE_MODEL`.

  Original `#id`s are kept, and synthetic entities are numbered above the file's largest id. All mixed representations go into one carve, run by one kernel process.

**Why carving:** OCCT's cost is the parse plus the transfer. On Ultimaker the parse takes 4.6 s and the result arrives at 42 s. On slow files, the transfer (healing included) is 72% of the time and meshing 22%. Only shrinking OCCT's input reduces both. M5's `--products` still parses the whole file; a carve doesn't.

**Edges:** the Rust side prescribes every edge of a carved face, including edges between two hard faces. The kernel puts those points into the mesher in place of its own edge discretisation, through the public, pluggable discretiser stage of OCCT's mesh context. Whether OCCT honours them is unproven (#115), so V3 tests it.

## 2. The join
- **Ids:** because the carve keeps the original ids, a STEP entity id means the same on both sides. After transfer, the kernel asks the reader's entity → shape bindings for each requested face and edge.
- **Healing history:** stepv's own budgeted FixShape (`heal.cpp`) records the re-shape history, so each edge id can be reported as `kept`, `modified`, `split[n]`, `merged` or `removed`.
- **Checking each prescribed point:** project it onto the healed curve; it must land within the edge's tolerance. Never compare curve parameters: STEP `LINE` magnitudes and degree-valued angles change OCCT's parametrisation.
- **Outcomes:**
  - `kept` or `modified` edges that pass the check are fine.
  - For `split` or `merged` edges, the kernel returns the polyline it actually used, and the Rust side re-meshes the analytic neighbours against it (the re-sync). This is allowed only if every point passes the neighbour's on-surface check.
  - `removed`, an unmapped face, a face mapped to several faces, or a failed re-sync makes the whole product fall back to v1. The ledger records why: `seam:split-unsynced`, `seam:unmapped`, `seam:removed` or `seam:not-honoured`.

## 3. Watertightness
- **Rust's polyline wins.** It exists before OCCT runs, so analytic faces never wait. OCCT's polyline wins only after a re-sync.
- **Stitching is an index merge.** The kernel returns, per face boundary, the vertex indices it used for each edge id. The Rust side replaces them with the canonical `(edge, k)` points. If any point would move by more than max(edge tolerance, d), that is a mismatch.
- **No T-junctions, by construction:**
  - both sides use one identical vertex sequence per edge;
  - points are never matched geometrically, and never compared as f32;
  - V4's gate checks it: every interior edge of a mixed product is used by exactly two triangles.

## 4. Kernel protocol
- **Request:** `stepv-occt /dev/fd/N --format step --seam /dev/fd/M --linear-abs <d>`.
  - The carve is an unlinked temporary file or a memfd. A pipe won't work, because the reader needs to seek.
  - `STEPVSEA` (binary) lists, per edge id: a u64 id, a u32 count, then the points as f64, in mm and representation coordinates.
- **Response:** STEPVMSH v5, only when `--seam` is given; Quick Look keeps v3. Each part is one carved representation. It adds:
  - a step id per face;
  - per face, a boundary table: for each edge use, the edge id, an `honoured`/`resynced`/`mismatch` status and the vertex indices;
  - the heal history per edge id;
  - `--topology` fields with the step ids;
  - a `seam` block in the summary.
- **Unchanged:** the sandbox (read access is the descriptor only, the existing #91 path), the 4 GiB footprint cap and the per-face healing budget.

## 5. Expected speedup
- **Which faces stay in OCCT:** before Rust B-spline surfaces, v2 only takes the analytic faces of mixed products off OCCT's list. 84% of hard faces are B-spline (19,472 of 23,172). They carry most of the control points and all the slow p-curve projection, including Ultimaker's 25 s ribbon faces.
- **Coverage upside:**

  | File | Hard faces | Representations out of scope |
  | --- | ---: | ---: |
  | Ultimaker 3 | ~10% (≈ 3,520) | 44.7% |
  | Stress assembly | ~5% (≈ 2,030) | 50.0% |

  In addition, 204 representations (8.9%) miss scope by only one or two faces.
- **Time:** by face count the saving could be large, but hard faces likely dominate OCCT's time per face. Expect a small wall-time gain and earlier display, not a halving.
- **The parse saving comes from carving, not from per-face routing.** Product carving (#115 P3) gets it with no seams.

## 6. Gates, tests and slices
Each slice is ≤ ~400 product lines.

| Slice | Scope | Acceptance |
| --- | --- | --- |
| **V0** Measure (graph only) | Per out-of-scope representation: hard and analytic face counts, and the bytes and entities reachable from each | Table committed. **Go only if** the projected OCCT time for the carved hard faces is ≤ 75% of M5's routed-product time on Ultimaker or the stress assembly |
| **V1** Carver | `graph::carve(rep, faces)` | Every corpus carve transfers. Per step id, area within 1e-6 relative and box equal to the full transfer's. Duplicate-id files are refused |
| **V2** Join and history | Step ids, heal history, the v5 face table | Fixtures from `fixture-gen.cpp` for split, merged and removed edges are each reported correctly. No unmapped face goes unreported |
| **V3** Prescription (spike) | `--seam`, the pluggable edge discretiser, boundary tables | ≥ 99% of corpus prescribed edges honoured exactly, and every failure flagged. If it fails, re-plan as "OCCT wins" using V2 only |
| **V4** Stitch and fallback | Per-face classify, index merge, re-sync, `seam:*` ledger reasons | The watertight gate with 0 failures. Hausdorff ≤ 2·d against OCCT's whole product. Every mismatch fixture falls back |
| **V5** View and bench | Mixed products in the viewer; cache key (`mesh::VERSION`, kernel version, seam mode) | `fastpath-bench` reports v1 against v2. v2 is never more than 5% slower than v1 on any corpus file |

## 7. Pitfalls
1. **Deflection:** the carve's box is smaller than the model's, so the kernel must take d as an absolute (`--linear-abs`).
2. **Parameters:** never send curve parameters (STEP vector magnitudes, angle units). Send 3-D points and project them.
3. **Healing context:** an open shell doesn't heal like a solid. It gets no shell or solid fixes, and its orientation fixes differ. Take orientation from the graph.
4. **Recovery ladder:** rungs 1–5 mesh isolated copies and drop the prescription. Those faces must report `not-honoured`.
5. **Seam and pole edges:** a seam edge appears twice in one face, and `topology_edges` drops degenerate edges.
6. **Vertex drift:** healing grows tolerances and moves vertices. Bound the snap distance.
7. **Faces past the budget, and Approx faces:** whole-product fallback doesn't fix them. Mark the seam open and count it.
8. **Instancing:** carve per representation, not per occurrence (`MAPPED_ITEM`).
9. **Clean room:** use OCCT's public API only. Describe its behaviour; copy no code.

## Open questions (owner)
1. Is V0's ≤ 75% threshold the right go criterion?
2. Should product carving (#115 P3), or Rust extrusion and revolution surfaces, come before v2?
3. May a mixed product show its Rust faces before OCCT's faces arrive, or only all at once?

**Critical files:** `kernel/stepv-occt-core.cpp`, `kernel/heal.cpp`, `kernel/stepv-occt.cpp`, `src/occt.rs`, `docs/adr/0001-graph-core.md`.

Refs: #115

