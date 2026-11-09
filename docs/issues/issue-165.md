---
type: issue
state: open
created: 2026-11-09T02:29:32Z
updated: 2026-11-09T02:29:32Z
author: gerchowl
author_url: https://github.com/gerchowl
url: https://github.com/vig-os/stepv/issues/165
comments: 0
labels: chore, priority:low
assignees: none
milestone: none
projects: none
parent: none
children: none
synced: 2026-11-09T09:29:32.154Z
---

# [Issue 165]: [graph: parallel scan split at record boundaries](https://github.com/vig-os/stepv/issues/165)

Split the mapped file at safe record boundaries and scan in parallel; the result must equal the serial scan, by test. Acceptance: equality on the corpus; speedup measured on the stress file.

Deferred from ADR-0001 §5 ("Later", speed); not in the MVP milestone.

Refs: #115
