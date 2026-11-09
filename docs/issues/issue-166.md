---
type: issue
state: open
created: 2026-11-09T02:29:34Z
updated: 2026-11-09T02:29:34Z
author: gerchowl
author_url: https://github.com/gerchowl
url: https://github.com/vig-os/stepv/issues/166
comments: 0
labels: chore, priority:low
assignees: none
milestone: none
projects: none
parent: none
children: none
synced: 2026-11-09T09:29:31.652Z
---

# [Issue 166]: [graph: on-disk cache of the graph (mmappable table)](https://github.com/vig-os/stepv/issues/166)

Persist the entity table, CSR and type interner as an mmappable file keyed like the outline artifact, so a re-open skips the scan. Acceptance: re-open time and an invalidation test.

Deferred from ADR-0001 §5 ("Later", speed); not in the MVP milestone.

Refs: #115
