---
type: issue
state: closed
created: 2026-10-05T16:41:29Z
updated: 2026-10-05T21:43:56Z
author: gerchowl
author_url: https://github.com/gerchowl
url: https://github.com/vig-os/stepv/issues/43
comments: 0
labels: feature, priority:low
assignees: none
milestone: none
projects: none
parent: none
children: none
synced: 2026-10-06T08:48:23.713Z
---

# [Issue 43]: [Exact section caps from the kernel (interference-proof)](https://github.com/vig-os/stepv/issues/43)

## Motivation
#34 caps sections with a GPU stencil-parity pass (PR to follow #42). Overlapping solids cancel each other's parity, so interference isn't capped. In `tests/data/assembly.step` the pins pass through the plate, and the overlap shows the pins' insides. Thin or open shells, and cracked tessellations, also miscount.

## Proposal
Ask the kernel for the exact section: `BRepAlgoAPI_Section` of each part's shape with the plane, through `stepv-occt --serve` (#33). Fill each part's closed wires, triangulated, in the part's own colour, hatched.
- The cap is exact and per part, so interference shows as two overlapping caps, not a hole.
- It costs a query per section change. Keep the stencil cap meanwhile, debounced while the slider moves.

## Scope
- [x] A `section` op in the serve protocol: the plane in, closed polylines per part out, in model coordinates.
- [x] Triangulate the wires, with holes (earcut on the plane), and draw them per part.
- [x] Fall back to the stencil cap while a query is in flight, or when the kernel refuses.

## Acceptance
- [x] A section through the bracket at y = 15 caps the pins *inside* the plate as well.

