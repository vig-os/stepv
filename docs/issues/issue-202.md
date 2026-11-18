---
type: issue
state: open
created: 2026-11-17T16:33:41Z
updated: 2026-11-17T16:33:41Z
author: gerchowl
author_url: https://github.com/gerchowl
url: https://github.com/vig-os/stepv/issues/202
comments: 0
labels: chore
assignees: none
milestone: Hardening: graph core
projects: none
parent: none
children: none
synced: 2026-11-18T09:57:19.126Z
---

# [Issue 202]: [chore(graph): Ultimaker's graph model diagonal is 16× OCCT's (§2.6 diagonal rule)](https://github.com/vig-os/stepv/issues/202)

Found in M1a (#142, PR #201), and recorded as data there.

On the Ultimaker 3 assembly, the graph's model box (G4, `graph::bounds`) has a diagonal of **42,076 mm**. OCCT's is **2,572 mm** (kernel summary), so the ratio is about **16×**. ADR-0001 §2.6 ("The diagonal") says the fast path must use the OCCT-equivalent diagonal of the B-rep's edges when the ratio is over 1.05 on corpus files, or the Rust mesh is coarser than OCCT's. With d from the graph's diagonal, Ultimaker's d is 42 mm instead of 2.57 mm.

In M1a's edge counts on Ultimaker, the 20° angle bound dominates at either d, so the counts are identical. Faces (M2/M3) and other files won't be that lucky.

## To do (before M4a's gates rely on d)
- Find what inflates the graph's box on Ultimaker: a far-off conic's whole-curve box, wireframe, or a placement.
- Implement §2.6's OCCT-equivalent diagonal (from the edge polylines, now that M1a has them) when the ratio is over 1.05.

Refs: #115

