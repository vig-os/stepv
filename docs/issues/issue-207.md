---
type: issue
state: open
created: 2026-11-22T17:00:31Z
updated: 2026-11-22T17:00:31Z
author: gerchowl
author_url: https://github.com/gerchowl
url: https://github.com/vig-os/stepv/issues/207
comments: 0
labels: chore
assignees: none
milestone: Hardening: graph core
projects: none
parent: none
children: none
synced: 2026-11-23T10:32:53.251Z
---

# [Issue 207]: [chore(mesh): route line and conic edges whose vertices are off the curve](https://github.com/vig-os/stepv/issues/207)

Found by M2's planar-face golden (#144, `tests/mesh_plane.rs`).

`mesh::edges` locates a B-spline edge's vertices on the curve and routes one farther than d (`vertex off curve`, M1b). A `LINE`, `CIRCLE` or `ELLIPSE` edge takes its vertex points as its ends without any check, and its interior points from the curve.

In nist-pmi (`nist_ftc_11_asme1_ap242-e2.stp`, faces #1200 to #1232), vertices lie up to 3.75e-3 mm off their lines. That is within the file's uncertainty of 5e-3 mm, but it is 1 to 3% of those small faces' areas. OCCT's face follows the curve, Rust's the vertex. The golden allows for it with a bound of g·(L + 2g) per edge, so nothing fails today.

**Proposal:** check a line's and a conic's vertices against the curve, as `curve:off-surface` checks a face's points against the plane: within the context's uncertainty, else route the edge. That needs the uncertainty in `Polylines::build`. Then fold the golden's vertex-gap bound into a plain 1e-6 comparison.

