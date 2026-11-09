---
type: issue
state: open
created: 2026-11-09T02:29:25Z
updated: 2026-11-09T02:29:25Z
author: gerchowl
author_url: https://github.com/gerchowl
url: https://github.com/vig-os/stepv/issues/160
comments: 0
labels: chore, priority:low
assignees: none
milestone: none
projects: none
parent: none
children: none
synced: 2026-11-09T09:29:34.380Z
---

# [Issue 160]: [graph: SIGBUS when a file is truncated while memory-mapped](https://github.com/vig-os/stepv/issues/160)

`memmap2` maps the file; truncating it while mapped raises SIGBUS. Read small files into memory, and handle or pre-check large ones. Acceptance: a test truncates a mapped file and the process doesn't crash.

Deferred from ADR-0001 §5 ("Later", hardening); not in the MVP milestone.

Refs: #115
