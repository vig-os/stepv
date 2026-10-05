---
type: issue
state: open
created: 2026-10-04T22:19:57Z
updated: 2026-10-04T22:19:57Z
author: gerchowl
author_url: https://github.com/gerchowl
url: https://github.com/vig-os/stepv/issues/30
comments: 0
labels: feature, priority:high
assignees: none
milestone: none
projects: none
parent: none
children: none
synced: 2026-10-05T08:45:46.506Z
---

# [Issue 30]: [viewer: virtualised model tree with show/hide/isolate](https://github.com/vig-os/stepv/issues/30)

## Motivation
The topology carries the assembly tree. Users expect show/hide and isolate per node.

## Scope
- [ ] Flatten the tree once into a node vector with depth, open/closed state and visibility. Render it with `ScrollArea::show_rows`.
- [ ] Checkbox show/hide that propagates down the tree; isolate; select-in-tree ↔ select-in-view.
- [ ] Search/filter by name.

## Pitfalls
#27 measured nested `CollapsingHeader`s at 40k nodes: 38 ms per frame and 382 MB. Virtualised, the same tree costs 2.2 ms. Never build it naively.

## Acceptance
- [ ] A 40k-node synthetic tree stays under 4 ms per frame, measured as in the #27 spike.

_Follow-up of the #27 spike (verdict: egui + wgpu) for #21. Conditional on that verdict being accepted._

