---
type: issue
state: open
created: 2026-11-09T02:29:37Z
updated: 2026-11-09T02:29:37Z
author: gerchowl
author_url: https://github.com/gerchowl
url: https://github.com/vig-os/stepv/issues/168
comments: 0
labels: chore, priority:low
assignees: none
milestone: none
projects: none
parent: none
children: none
synced: 2026-11-09T09:29:30.787Z
---

# [Issue 168]: [mesh: curvature-adaptive step on ellipses](https://github.com/vig-os/stepv/issues/168)

ADR §2.6 uses the uniform step Δt ≤ √(8d/A); eccentric ellipses get many more segments than needed. Use a curvature-adaptive step with the same sag bound d, by test.

Deferred from ADR-0001 §5 ("Later", speed); not in the MVP milestone.

Refs: #115
