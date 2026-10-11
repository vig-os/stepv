---
type: issue
state: open
created: 2026-10-10T23:07:20Z
updated: 2026-10-10T23:07:20Z
author: gerchowl
author_url: https://github.com/gerchowl
url: https://github.com/vig-os/stepv/issues/49
comments: 0
labels: feature, priority:low
assignees: none
milestone: none
projects: none
parent: none
children: none
synced: 2026-10-11T08:34:59.925Z
---

# [Issue 49]: [Viewer: pick vertices — coordinates, and distances to vertices](https://github.com/vig-os/stepv/issues/49)

## Motivation
#21's measurement list includes the **coordinates of a picked vertex** and **point-to-point distance**. The viewer picks faces and edges (#29, #31), and measures between them on the exact B-rep (#33), but vertices can't be picked. So there's no reading a corner's coordinates, and no corner-to-corner distance.

## What exists
- `--topology` already lists each prototype's vertices with positions (`kernel/topology.cpp`), and edges name their vertices.
- The id pass encodes faces, and edges with `EDGE_BIT`. Edges win within `PICK_RADIUS` (`resolve_pick` in `src/view/gpu.rs`).
- The kernel's `distance` op takes `{"part", "face"}` or `{"part", "edge"}` entities (`kernel/measure.h`).

## Scope
- [ ] **Vertices in the id pass:** small screen-space points, a third id kind. Within the pick radius, a vertex beats an edge, and an edge beats a face.
- [ ] **The inspector** shows the picked vertex's model coordinates.
- [ ] **A `{"part", "vertex"}` entity** in the serve protocol, numbered as `--topology` numbers vertices, so `distance` works between any two entities, vertices included.
- [ ] **Hover feedback,** so a user can tell a vertex pick from an edge pick.

## Pitfalls
- **Id space:** `EDGE_BIT` already takes the top bit, so a vertex bit must fit beside it without shrinking the face and edge range below real models' counts.
- **Numbering:** vertex ids must follow `--topology`'s, through `TopExp::MapShapes(TopAbs_VERTEX)`, in the mesh's per-prototype order.
- **Measure mode:** a vertex must win the pick there; elsewhere a face pick usually matters more.

## Acceptance
- [ ] A click on the bracket's corner shows its coordinates `(40, 30, 5)`.
- [ ] Two corner picks measure the exact distance between them, 50 mm across the plate's top diagonal.

Follow-up from #21.

