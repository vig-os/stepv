---
type: issue
state: closed
created: 2026-10-13T00:03:44Z
updated: 2026-10-13T02:03:13Z
author: gerchowl
author_url: https://github.com/gerchowl
url: https://github.com/vig-os/stepv/issues/89
comments: 0
labels: feature, priority:medium
assignees: none
milestone: none
projects: none
parent: none
children: none
synced: 2026-10-13T09:11:29.701Z
---

# [Issue 89]: [viewer: keep the tree with an honest message when a load is killed or times out (#63 part 3)](https://github.com/vig-os/stepv/issues/89)

Split from #85 because of the size cap. #63 acceptance item 3: a killed or timed-out `view` load keeps the tree that is already on screen and shows a message saying what happened and why (timeout, memory cap, crash, kernel failure, each with its reason). It never shows an empty window. Window tests run through `Home::view` and `just view-test`.

The PR for this issue closes #63, after #85 lands.

Refs: #63, #85
