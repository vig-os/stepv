---
type: issue
state: open
created: 2026-11-22T15:36:58Z
updated: 2026-11-22T16:23:20Z
author: gerchowl
author_url: https://github.com/gerchowl
url: https://github.com/vig-os/stepv/issues/206
comments: 1
labels: chore
assignees: none
milestone: Hardening: graph core
projects: none
parent: none
children: none
synced: 2026-11-23T10:32:53.621Z
---

# [Issue 206]: [chore(mesh): M1b B-spline polyline nits (#205 review)](https://github.com/vig-os/stepv/issues/206)

Nits from the fresh review of PR #205 (M1b, #143), deferred under the MVP rule:

- `bspline.rs:177` `zero tangent`: the threshold comes from the rounding of 48 halvings, scaled by the largest coordinate. That is justified, but the 10⁻³·θ factor is a chosen constant, and it lets the turn reach a·(1+10⁻³). Write that into the ADR §2.6 soundness argument, or remove the slack.
- `zero tangent` routing: 85 Ultimaker edges are routed as near-cusps. The extreme-weights test (`tests/bspline.rs:481`) routes a *valid* curve whose sub-resolution hook sits at its end (a parametric stall, not a cusp). That's a limit of the method, not a defect in the input. Look at the geometry behind the 85, and handle stalls properly (for example by reparametrising, or by measuring the turn in arc length).
- `invert` (`bspline.rs:222`, `:234`, `:258`) allocates Vecs for every node, which isn't data-oriented, and it's in the way for M4a's rayon/SIMD work.
- `fold(f64::max)` turns a NaN deviation into 0 without any sign. Use a NaN-propagating max, or check first.
- The `vertex inversion` route (`MAX_NODES`) has no test.
- The test oracle `off` (`tests/bspline.rs:209`) measures only polyline vertices. That's fine for now, because curve→polyline already covers sag. A note would help.

Refs: #143
---

# [Comment #1]() by [gerchowl]()

_Posted on November 22, 2026 at 04:23 PM_

Nits from the round-2 re-review of #205 (head a2be93f):
- `a_nearly_closed_curve_steps_across_its_seam_gap_within_d` compares against sampled points with a 1.01·d threshold. Because of sample sag, it accepts a true deviation of up to about 1.02·d. Use d plus an analytic sag bound, or denser sampling.
- Polylines now also have exempt corners where an off-curve vertex steps onto the curve. M2/M3 face meshing must know this; spell it out in ADR §2.6 next to the C0-knot exemption.
- Check whether a silently dropped `EDGE_CURVE` (neither emitted nor routed) is caught. If not, add a count invariant: emitted + routed = all edges.

