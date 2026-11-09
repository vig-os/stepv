---
type: issue
state: open
created: 2026-11-09T06:43:18Z
updated: 2026-11-09T06:43:18Z
author: gerchowl
author_url: https://github.com/gerchowl
url: https://github.com/vig-os/stepv/issues/180
comments: 0
labels: bug, priority:medium
assignees: none
milestone: none
projects: none
parent: none
children: none
synced: 2026-11-09T09:29:26.339Z
---

# [Issue 180]: [graph bounds: GEOMETRIC_CURVE_SET and unlisted curve types become Unknown (ADR says excluded)](https://github.com/vig-os/stepv/issues/180)

From the re-review of #178 (fix round 51a88f6). The code fails safe, never giving a wrong box, but it contradicts ADR §2.3.

**What happens:** `GEOMETRIC_CURVE_SET` now goes through `geometric_set()` too. Element names that `wire()`'s patterns miss end up as `Unknown("unsupported surface")`:
- `B_SPLINE_CURVE_WITH_KNOTS` (the most common B-spline curve);
- `OFFSET_CURVE_3D`, `CURVE_REPLICA`, `COMPOSITE_CURVE_ON_SURFACE`;
- the `AXIS2_PLACEMENT_3D` and `PLANAR_BOX` elements found in nist-pmi `GEOMETRIC_SET`s.

That `Unknown` then spreads up to assemblies, whereas ADR §2.3 says curve sets and wireframe are excluded. No corpus file hits it today; the totals are unchanged.

**Fix:**
- Return `Empty` for `GEOMETRIC_CURVE_SET` directly, since the schema allows it no surfaces.
- Classify `GEOMETRIC_SET` elements by `is_a` against the surface supertypes rather than by name patterns, so that only real surfaces are boxed or made `Unknown`, and curves, points and placements are skipped.
- Fixtures: a curve set with a B-spline curve, and a set holding a placement.

Refs: #136

