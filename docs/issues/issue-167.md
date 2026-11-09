---
type: issue
state: open
created: 2026-11-09T02:29:35Z
updated: 2026-11-09T02:29:35Z
author: gerchowl
author_url: https://github.com/gerchowl
url: https://github.com/vig-os/stepv/issues/167
comments: 0
labels: chore, priority:low
assignees: none
milestone: none
projects: none
parent: none
children: none
synced: 2026-11-09T09:29:31.185Z
---

# [Issue 167]: [graph: tighter boxes for arcs, spheres and tori](https://github.com/vig-os/stepv/issues/167)

ADR §2.3's boxes are conservative but loose for partial arcs, sphere patches and torus patches. Tighten them using the parameter range, keeping the conservative guarantee by test.

Deferred from ADR-0001 §5 ("Later", speed); not in the MVP milestone.

Refs: #115
