---
type: issue
state: open
created: 2026-11-09T02:33:59Z
updated: 2026-11-09T02:33:59Z
author: gerchowl
author_url: https://github.com/gerchowl
url: https://github.com/vig-os/stepv/issues/172
comments: 0
labels: feature, priority:low
assignees: none
milestone: none
projects: none
parent: none
children: none
synced: 2026-11-09T09:29:28.905Z
---

# [Issue 172]: [B-spline surfaces in Rust: project boundaries from shared edge polylines, bound every triangle within d, route whole products](https://github.com/vig-os/stepv/issues/172)

> Design brief written by a read-only design agent (2026-11-09) from ADR-0001 §2.6/§4/§5, `docs/adr/0001-census.md`, the M slices (#142–#148), `src/graph/` and `kernel/`. Deferred from ADR-0001 §5 ("Coverage: B-spline … surfaces in Rust"). Not in the MVP milestone. Not yet reviewed. Swept and offset surfaces are #171, which reuses the evaluator and mesher built here (B1–B4); per-face routing is #170.

## 1. Scope
**In:** 19,477 faces, 10.9% of the corpus:
- `ADVANCED_FACE` on `B_SPLINE_SURFACE_WITH_KNOTS` (7,717 faces);
- the complex rational form `(BOUNDED_SURFACE() B_SPLINE_SURFACE(…) B_SPLINE_SURFACE_WITH_KNOTS(…) … RATIONAL_B_SPLINE_SURFACE(…) …)` (11,755);
- `QUASI_UNIFORM_SURFACE` (5, all in `occt-import-js/cax-if/dm1-id-214.stp`), simple or rational.

The 31 `QUASI_UNIFORM_CURVE` edges come along: M1b's curve code gets the same implicit-knot expansion.

**Reading the entities:**
- Each part is read through `args_of`.
- In the complex form, `B_SPLINE_SURFACE`'s arguments start at the degrees, because the name lives in `REPRESENTATION_ITEM`. G2's `walkthrough_rational_b_splines` pins this.
- Control points and weights are indexed `[i_u][j_v]`.

**Out:** `UNIFORM_SURFACE`, `BEZIER_SURFACE` and `PIECEWISE_BEZIER_SURFACE`; none occur in the corpus.

**Minimal first meshing slice:** open surfaces with no poles. That is 14,154 of the 14,370 surfaces scanned in ABC and nist-pmi.

**Corpus fact that shapes the design:** only 11 of the 397 fixtures contain `PCURVE`s. The ones that also have B-spline surfaces are all OCCT-written (the stress assembly and `cax-if/*-oc-214`); ABC, nist-pmi and Ultimaker have none. So projection is the main path, and p-curves come last, as a cross-check.

## 2. Algorithm
**Evaluation:**
- Cox–de Boor basis functions with derivatives up to order 2, in homogeneous coordinates (wP, w); rational derivatives by the quotient rule.
- Span search and knot expansion are shared with M1b.
- Validate before use: knots non-decreasing, Σ multiplicities = n + p + 1, weights > 0. Otherwise route as `bspline:invalid`.
- Quasi-uniform knots follow ISO 10303-42: end multiplicity p + 1, interior knots at integer spacing.

**Bounds, per Bézier span:**
- **Non-rational:** `Muu`, `Muv` and `Mvv` are the largest norms of the second-derivative control nets. By the convex-hull property they hold over the whole span.
- **Rational:** with A = wS, use `S'' = (A'' − w''c − 2w'S' − w''(S − c)) / w`, centred on the span's hull centre c. Bound each term by its homogeneous control net, divided by `w_min`.
- When the weight ratio makes a bound too loose, subdivide by knot insertion.

**Boundary in (u, v):**
- Invert each shared edge-polyline point by Newton on `(S − P)·Su = (S − P)·Sv = 0`.
- Seed only each loop's first point from a lazy per-face grid of span samples; seed every later point from its predecessor. This avoids the per-point global grid search that cost OCCT about 28 s per Ultimaker ribbon face (#51).
- Accept a point only if its residual is at most the context uncertainty; otherwise route as `curve:off-surface`.
- A uv jump larger than a quarter of the domain is a wrong branch: route.
- Decide outer loop versus holes by signed area in (u, v), never by assumption.

**CDT and refinement:**
- Scale (u, v) by the face's mean `|Su|` and `|Sv|` before `spade`, so the triangulation is near-isotropic.
- Seed every interior knot line (creases at degree-1 knots).
- Refine with interior centroids until every triangle meets `½(Muu·Δu² + 2·Muv·Δu·Δv + Mvv·Δv²) ≤ d`, where Δu and Δv are its uv extents. This is Taylor's remainder at each vertex, so it bounds every point of the triangle, not a sample.
- Then split any interior edge whose exact vertex normals differ by more than a.
- Normals are exact (`Su × Sv`), taken from the triangle's own span (one-sided at C0 knots) and flipped by `same_sense`.
- A per-face point cap routes the face as `rs-fail:budget`.

**Boundary coarseness (a second edge pass):**
- If a boundary segment fails the bound with margin (`½Q ≤ d/2`), the face sends an edge-refinement request.
- Order: edges → faces (project and check) → refine the requested edges at curve-parameter midpoints → re-project → mesh.
- Points are only ever added to the shared polyline, so stitching stays an index merge and the mesh stays watertight.

**Seams, closed surfaces and poles:**
- **Closure** is decided geometrically: the first and last control rows coincide within tolerance. The STEP closed flag is advisory only; a disagreement routes the face.
- **A seam edge** (the same `EDGE_CURVE` used twice) takes u_min on one use and u_max on the other, chosen by continuity.
- **A loop that wraps round with no seam edge** routes as `loop:seam-unresolved`.
- **A collapsed control row is a pole.** Its boundary point takes its u from its neighbours, and a degenerate uv segment along the iso-line maps to the one 3-D vertex.
- **An interior singularity** (`Su × Sv = 0` away from the boundary) routes the face.

## 3. Healing
**What OCCT's default `FixShape` does to such faces, described by behaviour:**
- builds missing p-curves by projection (#51's cost);
- reorders wires and closes gaps by growing vertex and edge tolerances;
- adds degenerate edges at poles;
- adds missing seams, choosing the right period near a seam;
- removes small edges;
- fixes self-intersecting and intersecting wires;
- re-orients wires and adds natural bounds;
- makes 3-D curves and p-curves agree (SameParameter).

**Rust replicates only the local, checkable part:** projection, seam unwrap, degenerate pole segments and loop orientation. It never grows a tolerance or edits topology.

**Routed to OCCT:**
- boundaries off the surface;
- uv crossings;
- a missing seam;
- edges shorter than the tolerance;
- Newton non-convergence or an ambiguous inversion;
- invalid knots or weights;
- interior singularities;
- faces over the budget.

## 4. Prerequisites and dependencies
- **Required:** M1b (#143), M2/M3 (#144, #145) and M4a (#146). M5 (#148) is what turns this into saved time, since B-spline faces without p-curves are where OCCT's transfer time goes.
- **Per-face routing (#170) is not a prerequisite.** Products stay the unit of routing; the census per product (#153) measures the gain.
- **P-curve parsing is deferred** to B6: `PCURVE` → `DEFINITIONAL_REPRESENTATION` → a 2-D curve.
- **Crates:** `spade` (MIT OR Apache-2.0) and `rayon` (MIT OR Apache-2.0), both already planned.
  - No NURBS crate: `curvo` and `truck-geometry` pull large dependency trees and give no rigorous bounds.
  - Evaluation is written from The NURBS Book and Part 42 only (the clean-room rule).

## 5. Gates and tests
**Exact surface (no OCCT needed):**
- every interior vertex equals `S(u, v)` within 1e-9 relative;
- every boundary residual is within the context uncertainty;
- every triangle passes the bound, recomputed by an independent routine in the gate;
- a sampled check: 4×4 barycentric samples per triangle, Newton-inverted against the exact surface.

**Against OCCT:**
- the sampled symmetric Hausdorff distance is ≤ 2·d per product, as in M4a;
- face area matches OCCT's, and orientation agrees (normal dot product > 0);
- products where OCCT left a face as `Approx` or `Missing` are excluded and counted.

**Exact oracles:** synthetic shapes where every vertex must lie on the analytic surface within 1e-9:
- a rational NURBS sphere, which has poles;
- a closed NURBS cylinder, which has a seam;
- a torus.

**Corpus fixtures:**
- ABC `00000021` (3 faces, 0% analytic), `00000033` (802 B-splines) and `00000011`;
- `dm1-id-214` (quasi-uniform);
- `as1-oc-214` and the stress assembly (p-curves);
- the four Ultimaker ribbon faces (2×1,079 poles, degree 1×2) as the timing fixture.

**Watertightness:** every interior edge is used by exactly two triangles.

## 6. Slices
| Slice | Scope | Acceptance |
| --- | --- | --- |
| **B1** Evaluation (~350) | Parse the simple, complex and quasi-uniform forms; basis functions and derivatives; span bounds | Exact on the synthetic shapes; derivatives match finite differences; every bound ≥ the value measured at 10⁵ samples on every corpus surface; every surface parses or gets a reason |
| **B2** Projection (~350) | Seeding, Newton, continuation, residual check, loop orientation | Every corpus face projects or routes with a reason; the ribbon faces project in < 100 ms each |
| **B3** CDT and refinement (~400) | Open surfaces without poles: metric scaling, knot seeds, bound and angle refinement, edge-refinement requests | The surface gate has 0 failures; area matches OCCT; watertight on ABC `00000021` |
| **B4** Seams and poles (~300) | Seam unwrap, degenerate pole segments, closure detection | NURBS sphere and cylinder exact and watertight |
| **B5** Integration (~250) | `classify`, ledger signatures, `fastpath-corpus` | Both gates at 0 failures; the product-share uplift and Ultimaker timing reported |
| **B6** P-curves (~250) | Parse p-curves and compare them with projection | Agreement within tolerance on the OCCT-written files; disagreement routes |

## 7. Pitfalls
1. **Argument offsets** differ between the complex and simple forms: the name is missing in the complex part.
2. **A transposed control net or weights** (u and v swapped) evaluate silently to the wrong surface.
3. **Newton can land on the wrong sheet** on closed or folded ribbons. The continuity check is mandatory.
4. **3,267 bilinear (1×1) surfaces** have no `Muu` or `Mvv`. The twist term `Muv` decides refinement, and their creases need the knot-line seeds.
5. **Raw (u, v) on ribbons** gives slivers, and the bound never converges. Scale by the metric first.
6. **Rational bounds explode** at large weight ratios. Subdivide rather than over-refine.
7. **Boundary vertices sit off the surface** by up to the uncertainty. The gate must hold them to that tolerance, not to 1e-9.
8. **NaN** from a zero Jacobian or a zero weight must route the face, never panic.

## Open questions (owner)
1. Should this ship right after M4a, or wait for M5 so the transfer-time saving is measurable from the first slice?
2. What is the per-face budget: a point cap and a time limit before `rs-fail:budget`?

**Critical files:** `docs/adr/0001-graph-core.md`, `src/graph/mod.rs`, `src/graph/args.rs`, `kernel/heal.cpp`, `kernel/stepv-occt-core.cpp`.

Refs: #115, #170, #171

