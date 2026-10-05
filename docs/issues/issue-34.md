---
type: issue
state: open
created: 2026-10-04T22:20:03Z
updated: 2026-10-04T22:20:03Z
author: gerchowl
author_url: https://github.com/gerchowl
url: https://github.com/vig-os/stepv/issues/34
comments: 0
labels: feature, priority:low
assignees: none
milestone: none
projects: none
parent: none
children: none
synced: 2026-10-05T08:45:44.123Z
---

# [Issue 34]: [viewer: section capping and fat lines](https://github.com/vig-os/stepv/issues/34)

## Motivation
v1 sections clip without caps (the #27 decision). Real section views fill the cut. Thin LineList edges alias.

## Scope
- [ ] Capping: per section plane, build the cut boundary from the exact B-rep (a kernel section through the query channel) or a stencil cap pass, and draw it hatched.
- [ ] Fat, anti-aliased lines (expanded quads) for edges and sketches.

## Acceptance
- [ ] A section through the bracket shows a filled plate profile with the hole open.

_Follow-up of the #27 spike (verdict: egui + wgpu) for #21. Conditional on that verdict being accepted._

