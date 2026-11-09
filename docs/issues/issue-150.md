---
type: issue
state: open
created: 2026-11-09T00:13:05Z
updated: 2026-11-09T00:13:05Z
author: gerchowl
author_url: https://github.com/gerchowl
url: https://github.com/vig-os/stepv/issues/150
comments: 0
labels: chore
assignees: none
milestone: MVP: graph core
projects: none
parent: none
children: none
synced: 2026-11-09T09:29:37.928Z
---

# [Issue 150]: [graph: the dense id lookup can cost 16 B per entity at the ADR's 4x threshold](https://github.com/vig-os/stepv/issues/150)

Found in G1 (#133).

ADR-0001 §2.1 uses a dense `Vec<u32>` lookup "when the largest id ≤ 4× the count", and costs the lookup at 4–8 B per entity. At the threshold itself the dense table is 4 × 4 B = **16 B per entity**.

That's 18 B of G1 columns plus 16 B, so 34 B. With G2's 4.5 B of references it comes to about 38.5 B, which is over §2.7's 36 B "without `referrers`" target.

**Measured on real files** (`examples/graph_scan.rs`): Ultimaker 3 and the stress assembly both have dense ids (max id ≈ count), at 22.0 B/entity in total. So nothing in the corpus hits this today. A file with sparse ids (gaps up to 4×) would.

**Options:**
- lower the dense threshold to 2× (at most 8 B, which matches the ADR's estimate);
- or re-state the budget.

This needs a decision before G2 measures the "with references" heap.

Refs: #133
