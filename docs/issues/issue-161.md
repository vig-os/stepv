---
type: issue
state: open
created: 2026-11-09T02:29:27Z
updated: 2026-11-09T02:29:27Z
author: gerchowl
author_url: https://github.com/gerchowl
url: https://github.com/vig-os/stepv/issues/161
comments: 0
labels: chore, priority:low
assignees: none
milestone: none
projects: none
parent: none
children: none
synced: 2026-11-09T09:29:33.896Z
---

# [Issue 161]: [graph: adversarial id ranges and reference fan-out](https://github.com/vig-os/stepv/issues/161)

Bound the dense id lookup's memory for sparse or huge ids (see #150) and the work done for entities with huge reference fan-out. Acceptance: fixtures with ids near u32::MAX and an entity with 10^6 references stay within the heap target.

Deferred from ADR-0001 §5 ("Later", hardening); not in the MVP milestone.

Refs: #115
