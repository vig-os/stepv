---
type: issue
state: open
created: 2026-11-09T02:29:24Z
updated: 2026-11-09T02:29:24Z
author: gerchowl
author_url: https://github.com/gerchowl
url: https://github.com/vig-os/stepv/issues/159
comments: 0
labels: chore, priority:low
assignees: none
milestone: none
projects: none
parent: none
children: none
synced: 2026-11-09T09:29:34.800Z
---

# [Issue 159]: [graph: heap budget with admission control and refusal over budget](https://github.com/vig-os/stepv/issues/159)

Estimate the graph's heap from the file size and entity count before building it, admission-controlled like #108, and refuse over budget with a clear error. Acceptance: a file whose estimate exceeds the budget refuses before allocating.

Deferred from ADR-0001 §5 ("Later", hardening); not in the MVP milestone.

Refs: #115
