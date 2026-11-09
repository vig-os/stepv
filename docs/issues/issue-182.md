---
type: issue
state: open
created: 2026-11-09T06:58:15Z
updated: 2026-11-09T06:58:15Z
author: gerchowl
author_url: https://github.com/gerchowl
url: https://github.com/vig-os/stepv/issues/182
comments: 0
labels: chore
assignees: none
milestone: none
projects: none
parent: none
children: none
synced: 2026-11-09T09:29:25.439Z
---

# [Issue 182]: [chore(view): the graph read can't be cancelled; closing the window mid-read waits for it](https://github.com/vig-os/stepv/issues/182)

Found in G6a (#138). `stepv view` reads the graph on a scoped thread beside the kernel. Closing the window cancels the kernel at once, but the scope still joins the graph thread, so the process exits only when the read finishes: about 0.9 s on Ultimaker, longer on larger files.

Fix direction: a cancel flag checked by the scan/structure/bounds loops, or a detached thread whose result is dropped.

Refs: #138
