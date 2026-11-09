---
type: issue
state: open
created: 2026-11-09T06:58:18Z
updated: 2026-11-09T06:58:18Z
author: gerchowl
author_url: https://github.com/gerchowl
url: https://github.com/vig-os/stepv/issues/184
comments: 0
labels: chore
assignees: none
milestone: none
projects: none
parent: none
children: none
synced: 2026-11-09T09:29:24.459Z
---

# [Issue 184]: [chore(view): the software viewer shows no graph tree or placeholders](https://github.com/vig-os/stepv/issues/184)

Found in G6a (#138). With `--software`, or with no usable GPU adapter, the minifb viewer still waits for the kernel's scene before it opens (#90): `Load::Graph` is skipped like the tree and progress, so the graph's early view is GPU-only.

Fix direction: decide whether the software viewer gets a loading stage at all (#90), and if so draw the boxes with `render.rs`'s line path.

Refs: #138, #90
