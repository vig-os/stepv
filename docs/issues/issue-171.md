---
type: issue
state: open
created: 2026-11-09T02:33:23Z
updated: 2026-11-09T02:33:23Z
author: gerchowl
author_url: https://github.com/gerchowl
url: https://github.com/vig-os/stepv/issues/171
comments: 0
labels: feature, priority:low
assignees: none
milestone: none
projects: none
parent: none
children: none
synced: 2026-11-09T09:29:29.389Z
---

# [Issue 171]: [Swept surfaces in Rust: recognise analytic revolutions/extrusions, then mesh general extrusions, revolutions and offsets](https://github.com/vig-os/stepv/issues/171)

> Design brief written by a read-only design agent (2026-11-09) from ADR-0001 §2.3/§2.6/§5, `docs/adr/0001-census.md`, the M slices (#142–#148), `kernel/` and its own read-only scan of the corpus. Deferred from ADR-0001 §5 ("Coverage: … swept surfaces in Rust"). Not in the MVP milestone. Not yet reviewed.

## Corpus findings
The agent's scan covered the 51 fixtures that use these surfaces, all in ABC.

- **Revolutions (2,647 entities):** every basis curve is a B-spline; none is a LINE or CIRCLE entity. Evaluated:
  - 1,462 are lines parallel to the axis (really cylinders);
  - 30 are oblique lines (cones);
  - 896 are circular arcs off the axis (tori);
  - 207 are arcs centred on the axis (spheres);
  - only about 52 are freeform: 15 cubic, 14 quintic and 23 degree-1 polylines with 3 poles.

  67 profiles touch the axis, and 3 don't lie in a plane through the axis.
- **Extrusions (1,264):** 1,130 freeform non-rational B-splines (degree 2–5), 91 circles (cylinders), 32 ELLIPSE (elliptic cylinders), 11 lines (planes).
- **Offsets (43):** the basis is an extrusion for 29, a B-spline surface for 12 and a revolution for 2. `self_intersect` is `.F.` in the samples read.
- **No PCURVE or SEAM_CURVE in any of the 51 files.** The boundary must be inverted onto the surface.
- **Only 6 of the 51 files have no B-spline surfaces.** Under v1's whole-product routing, swept surfaces alone lift few products.

## 1. Scope
- **First slice: recognition.** Rewrite each swept surface that is really analytic as its M2/M3 surface:
  - a revolution of a line becomes a cylinder, cone or plane annulus;
  - a revolution of an exact circular arc becomes a torus (ring torus only, R > r) or a sphere (centre on the axis);
  - an extrusion of a line becomes a plane, of a circle a cylinder;
  - an offset of a plane, cylinder, cone, sphere or torus (including one recognised above) becomes the same type with an adjusted radius or position.
- **General path:** non-rational and rational B-spline basis curves (degree ≤ 5, weights > 0), and ELLIPSE (extrusions only). LINE and CIRCLE always go through recognition.
- **Offsets:** of analytic surfaces (through recognition), of extrusions and of meridional revolutions. Offsets of B-spline surfaces wait for the B-spline surface issue.

## 2. Algorithm
- **Revolution `S(u,v) = Rot(axis, u)·C(v)`:**
  - invert in closed form: the axial height z, the radius ρ and the angle φ, with u = φ − φ₀;
  - get v by inverting the 2-D curve in the meridian half-plane;
  - route non-meridional profiles (`surface:revolution-nonmeridional`).
- **Extrusion `S(u,v) = C(u) + v·D`:** project along D (not orthogonally) onto the curve, invert for u, then v = (P − C(u))·D.
- **Inversion:**
  - Newton, seeded by subdividing the control polygon;
  - accepted only within the context's uncertainty, else route (`curve:off-surface`);
  - u is unwrapped along loops;
  - a closed profile makes v (revolution) or u (extrusion) periodic, with M3's seam handling.
- **Step bounds:**
  - **u on a revolution:** §2.6's clamped circle count with r = ρ_max over the face, and Δu ≤ a.
  - **v on a revolution, and u on an extrusion:** the breakpoints of M1b's flattening of the basis curve. A rigid sweep preserves sag.
  - **v on an extrusion:** no interior points. The surface is ruled, so S_vv = S_uv = 0.
  - **A provable per-triangle bound:** ≤ ½·M·R², where R is the radius of the smallest disc enclosing the triangle in (u,v), and M comes from ρ_max, max|C′| and max|C″| via the hodograph control polygons.
- **Offset `S + d·N̂`:**
  - its normal is the basis normal, with the direction from the basis surface, never from the face's `same_sense`;
  - it is singular where 1 − d·κᵢ = 0: bound κ over the patch and route if d·κ_max ≥ 1 − margin (`surface:offset-cusp`);
  - also route a C0 basis and `self_intersect ≠ .F.`.
- **Poles:** a profile end on the axis (ρ = 0) is a degenerate point, handled as M3 handles sphere poles. A profile crossing the axis makes the surface self-intersect, so route it (`loop:pole-crossing`).

## 3. OCCT on import (behaviour only)
- **Healing:** the kernel's FixShape builds p-curves for every edge by projection, because these files have none. That is #51's hot spot, so OCCT is costly here and Rust gains the most.
- **Edge tolerances:** healing may enlarge them. The on-surface check then routes the face, which is correct.
- **Probe first:** have `--topology` report which surface type OCCT keeps for each face, to learn whether it converts any on import.
- **Still route to OCCT:**
  - faces left unhealed by the budget, or meshed only at a fallback tier;
  - non-meridional or spindle revolutions;
  - offsets of B-spline surfaces;
  - offset cusps.

## 4. Prerequisites and shared pieces
- **Depends on:** M1b (#143), M3 (#145) and M4a (#146). Per-face routing (#170) isn't required, but most of the payoff waits for it or for Rust B-spline surfaces.
- **Built once and shared with the B-spline surface issue:**
  - B-spline evaluation with derivatives and hodograph bounds;
  - curve and surface point inversion;
  - a generic parametric face mesher (the (u,v) boundary, then the CDT, Steiner refinement driven by M, and the flood);
  - a p-curve reader, used only as an inversion seed;
  - the gate harness.

  The B-spline surface issue owns the mesher.

## 5. Gates and tests
- **Surface gate:**
  - every vertex lies on the *original* swept surface, evaluated exactly, not on the recognised one;
  - each triangle passes the ½·M·R² bound;
  - offsets are sampled, labelled as in §2.6.
- **OCCT gate:** sampled Hausdorff ≤ 2·d, only on faces OCCT meshed at its default tier.
- **Fixtures** (`tests/fixtures/abc-dataset/`):
  - `00000218_…_step_005` (272 revolutions);
  - `00000217_…_step_004` (1,243 revolutions);
  - `00000247_…_step_002` (383 extrusions, no B-spline surfaces);
  - `00000221_…_step_008`;
  - `00000036_…_step_000` (revolutions and offsets).
- **Synthetic fixtures:**
  - a near-circular cubic rational arc;
  - a profile touching the axis;
  - an extrusion whose direction D is oblique to the profile;
  - an offset with d > 1/κ.

## 6. Slices
| Slice | Scope | Acceptance |
|---|---|---|
| **SW0** census (~150) | Ledger sub-signatures (`revolution:line-parallel`, `revolution:arc`, `extrusion:freeform`…); products that would come into scope per signature; the kernel surface-type probe | A committed table in `0001-census.md` |
| **SW1** recognition (~350) | Exact tests: a line test; for arcs, degree reduction of homogeneous poles to a quadratic plus the conic circle condition; frame and sense mapped onto M3 | ≥ 2,550 revolutions and ≥ 100 extrusions recognised; 1,000 samples per surface agree within 1e-9 relative; both gates pass |
| **SW2** shared mesher (~400) | Owned by the B-spline surface issue | That issue's acceptance |
| **SW3** general extrusion (~300) | Inversion along D; ellipse and B-spline profiles | `00000247` meshes watertight; both gates |
| **SW4** general revolution (~350) | Meridional inversion, poles, the M bound | The freeform corpus revolutions and the pole fixture mesh watertight; both gates |
| **SW5** offsets (~300) | Offsets of extrusions and revolutions; curvature-bound routing | `00000036`'s offsets mesh or route with a reason; the cusp fixture routes |

## 7. Pitfalls
1. **The sense flips on recognition.** The revolution's normal is S_u × S_v, which depends on C′'s direction, while a CYLINDRICAL_SURFACE always points outward. Flip the face's sense to match.
2. **Seams differ.** The recognised surface's u origin is the profile's meridian, not the elementary surface's reference direction. Derive the seam from the edges.
3. **Loose, sampled recognition** turns near-circles into tori. Use the exact algebraic test.
4. **Degree-1 polylines with 3 poles** are piecewise cones. Don't recognise them span by span; send them to the general path.
5. **An extrusion direction oblique to the profile** breaks orthogonal projection.
6. **Offset distances are in file units** (e.g. −0.003175 m). Scale them like every other length.
7. **C0 knots in profiles** give creased normals and undefined offsets.
8. **OCCT's fallback-tier faces** fail the 2·d gate spuriously.
9. **An extrusion is unbounded in v.** Take the range only from the boundary.

## Open questions (owner)
1. Under whole-product routing, should only SW0 and SW1 land now, with SW3–SW5 deferred until B-spline surfaces or per-face routing exist?
2. Is a sampled gate acceptable for offsets, as it is for tori?
3. Should recognition use the context's uncertainty, or a fixed 1e-9 relative?

**Critical files:** `docs/adr/0001-graph-core.md`, `docs/adr/0001-census.md`, `src/graph/mod.rs`, `kernel/topology.cpp`, `kernel/heal.cpp`.

Refs: #115, #170

