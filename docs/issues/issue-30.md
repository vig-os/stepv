---
type: issue
state: closed
created: 2026-10-04T22:19:57Z
updated: 2026-10-05T18:43:35Z
author: gerchowl
author_url: https://github.com/gerchowl
url: https://github.com/vig-os/stepv/issues/30
comments: 2
labels: feature, priority:high
assignees: none
milestone: none
projects: none
parent: none
children: none
synced: 2026-10-06T08:48:27.114Z
---

# [Issue 30]: [viewer: virtualised model tree with show/hide/isolate](https://github.com/vig-os/stepv/issues/30)

## Motivation
The topology carries the assembly tree. Users expect show/hide and isolate per node.

## Scope
- [x] Flatten the tree once into a node vector with depth, open/closed state and visibility. Render it with `ScrollArea::show_rows`.
- [x] Checkbox show/hide that propagates down the tree; isolate; select-in-tree ↔ select-in-view.
- [x] Search/filter by name.

## Pitfalls
#27 measured nested `CollapsingHeader`s at 40k nodes: 38 ms per frame and 382 MB. Virtualised, the same tree costs 2.2 ms. Never build it naively.

## Acceptance
- [x] A 40k-node synthetic tree stays under 4 ms per frame, measured as in the #27 spike.

_Follow-up of the #27 spike (verdict: egui + wgpu) for #21. Conditional on that verdict being accepted._


---

# [Comment #1]() by [gerchowl]()

_Posted on October 5, 2026 at 02:12 PM_

From #29's review (PR #37): when the model tree hides the picked face's part, the highlight disappears but the inspector keeps showing the face. Clear `picked`, or mark it hidden, when visibility changes. A section that cuts the whole face away has the same effect, but there the user is usually adjusting the cut on purpose, so #37 keeps the selection.

---

# [Comment #2]() by [gerchowl]()

_Posted on October 5, 2026 at 06:43 PM_

Done in #39, merged to main.

