---
type: issue
state: open
created: 2026-10-04T22:19:55Z
updated: 2026-10-04T22:19:55Z
author: gerchowl
author_url: https://github.com/gerchowl
url: https://github.com/vig-os/stepv/issues/29
comments: 0
labels: feature, priority:high
assignees: none
milestone: none
projects: none
parent: none
children: none
synced: 2026-10-05T08:45:47.227Z
---

# [Issue 29]: [viewer: GPU id-buffer picking and a face inspector on the exact topology](https://github.com/vig-os/stepv/issues/29)

## Motivation
Measuring on the exact B-rep starts with picking a face and showing what it really is: plane, cylinder ⌀, area, from `--topology` (#26).

## Scope
- [ ] An id pass into R32Uint, `part<<20 | face` (or two channels if part counts exceed 4096), and a one-texel readback on click.
- [ ] An inspector panel: part name, surface type and parameters, face area, part volume and bbox. Edge picking comes with the edges issue.
- [ ] Highlight the picked face in a depth-biased second pass, not a colour mix.

## Pitfalls
- MSAA: the id pass must be single-sample.
- Readback latency: map asynchronously and show it on the next frame.
- Clip planes must apply to the id pass too, so you can't pick through a section.

## Acceptance
- [ ] Clicking the bracket's hole shows a cylinder with r = 4. Clicking the plate's top shows a plane with normal +z and area 1200 − 16π.
- [ ] A test drives a pick at known pixel coordinates against a known camera (headless).

_Follow-up of the #27 spike (verdict: egui + wgpu) for #21. Conditional on that verdict being accepted._

