---
type: issue
state: open
created: 2026-10-04T22:20:01Z
updated: 2026-10-04T22:20:01Z
author: gerchowl
author_url: https://github.com/gerchowl
url: https://github.com/vig-os/stepv/issues/33
comments: 0
labels: feature, priority:medium
assignees: none
milestone: none
projects: none
parent: none
children: none
synced: 2026-10-05T08:45:44.611Z
---

# [Issue 33]: [viewer: measurements through a sandboxed kernel query channel](https://github.com/vig-os/stepv/issues/33)

## Motivation
Single-entity facts (radius, area, length, volume) come from `--topology`. Measuring *between* entities (point-to-point, face-to-face distance, angle between faces or edges) needs the kernel: `BRepExtrema_DistShapeShape` on the exact shapes.

## Scope
- [ ] A long-lived kernel subprocess (`stepv-occt --serve`), sandboxed as in #18: same read root, no writable files. It reads newline-JSON queries on stdin and answers on stdout: distance(a, b), angle(a, b), plus point coordinates on a face.
- [ ] Measure tools in the viewer: pick two entities and show the result with the witness points drawn.
- [ ] Time and memory limits per query, as for a run.

## Pitfalls
- The kernel stays one process per file. A crash in a query must not take down the viewer: restart it and report.
- Entities are addressed as (part, face/edge) indices, so the server must reload the same file and number entities identically to `--topology`.

## Acceptance
- [ ] The distance between the two pins' axes (via their cylinders) reads 28 mm. The angle between two adjacent box faces reads 90°.

_Follow-up of the #27 spike (verdict: egui + wgpu) for #21. Conditional on that verdict being accepted._

