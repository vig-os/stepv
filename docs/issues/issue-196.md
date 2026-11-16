---
type: issue
state: open
created: 2026-11-15T21:31:35Z
updated: 2026-11-15T22:21:12Z
author: gerchowl
author_url: https://github.com/gerchowl
url: https://github.com/vig-os/stepv/issues/196
comments: 1
labels: chore
assignees: none
milestone: Hardening: graph core
projects: none
parent: none
children: none
synced: 2026-11-16T09:18:50.405Z
---

# [Issue 196]: [chore(graph): G6c fit nits (MAPPED_ITEM recursion memo, face-order-dependent axis)](https://github.com/vig-os/stepv/issues/196)

These nits are from the fresh review of PR #195 (G6c, #140) at f062231:

1. `src/graph/bounds/fit.rs:293-298`: the fit's `MAPPED_ITEM` recursion has no memo and no cycle set; only the depth cap of 64 stops it. A hostile fan-out costs about 2^depth work, while the box walk is memoised. Memoise it as the box walk does.
2. `fit.rs:440-441`: the fitted axis is the first non-planar face's, so face order decides whether a product fits. This is recorded in the ADR. A face-order-independent choice, such as the axis that minimises ε, would fit more products.

Refs: #140
---

# [Comment #1]() by [gerchowl]()

_Posted on November 15, 2026 at 10:21 PM_

These nits are from the re-review of round 2 (48b03ba, merged):
3. `tests/graph_fits.rs:335-342`: the 9×/11× pair tests the ε threshold, not the new angle rule. The only angle test sits at 1.4× the limit. Add a pair just either side of sin θ · extent = 10·unc.
4. `fit.rs:502`: the angle rule uses `hi - lo` without the ε padding, so it's slightly looser than ADR §2.8's "the fit's axial extent". This has no containment effect, but align the code with the ADR text.

