---
type: issue
state: open
created: 2026-10-13T00:03:45Z
updated: 2026-10-13T00:03:45Z
author: gerchowl
author_url: https://github.com/gerchowl
url: https://github.com/vig-os/stepv/issues/90
comments: 0
labels: feature, priority:low
assignees: none
milestone: none
projects: none
parent: none
children: none
synced: 2026-10-13T09:11:29.188Z
---

# [Issue 90]: [viewer: the software renderer (--software / no GPU) has no tree panel, so it waits for the whole load](https://github.com/vig-os/stepv/issues/90)

From the #85 design (2026-10-13): the outline-first window covers the GPU viewer only. `--software`, or a machine without a usable GPU, waits for the whole load as before, with no tree and no progress. This is documented as a known limit. The correct end state is that the software viewer also opens on the tree and progress, sharing the loading state with the GPU path.

Refs: #63, #85
