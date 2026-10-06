---
type: issue
state: closed
created: 2026-10-04T22:20:03Z
updated: 2026-10-05T18:43:45Z
author: gerchowl
author_url: https://github.com/gerchowl
url: https://github.com/vig-os/stepv/issues/34
comments: 2
labels: feature, priority:low
assignees: none
milestone: none
projects: none
parent: none
children: none
synced: 2026-10-06T08:48:24.422Z
---

# [Issue 34]: [viewer: section capping and fat lines](https://github.com/vig-os/stepv/issues/34)

## Motivation
v1 sections clip without caps (the #27 decision). Real section views fill the cut. Thin LineList edges alias.

## Scope
- [x] Capping: per section plane, build the cut boundary from the exact B-rep (a kernel section through the query channel) or a stencil cap pass, and draw it hatched.
- [x] Fat, anti-aliased lines (expanded quads) for edges and sketches.

## Acceptance
- [x] A section through the bracket shows a filled plate profile with the hole open.

_Follow-up of the #27 spike (verdict: egui + wgpu) for #21. Conditional on that verdict being accepted._


---

# [Comment #1]() by [gerchowl]()

_Posted on October 5, 2026 at 03:51 PM_

From #31's review (PR #40): edges are now sampled from the curve (GCPnts_TangentialDeflection) and drawn as 1-px lines, pulled a constant 0.0005 depth units (0.1% of the fit diameter) toward the eye, because WebGPU allows no depth bias on lines. At grazing angles and on sheets thinner than that, an edge can dash or show through. When fat lines land here, take edge points from `BRep_Tool::PolygonOnTriangulation`, so they coincide with the triangle boundaries, and make the pull slope-aware (or much smaller).

---

# [Comment #2]() by [gerchowl]()

_Posted on October 5, 2026 at 06:43 PM_

Done in #44, merged to main.

