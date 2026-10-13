---
type: issue
state: open
created: 2026-10-12T16:23:33Z
updated: 2026-10-12T16:23:33Z
author: gerchowl
author_url: https://github.com/gerchowl
url: https://github.com/vig-os/stepv/issues/72
comments: 0
labels: feature, priority:medium
assignees: none
milestone: none
projects: none
parent: none
children: none
synced: 2026-10-13T09:11:36.894Z
---

# [Issue 72]: [kernel: mesh prototypes in parallel, one per thread (#51)](https://github.com/vig-os/stepv/issues/72)

## Motivation

#51 / #65. The kernel meshes one prototype at a time with BRepMesh's `InParallel`. On Ultimaker 3 that costs 3.8 s of wall time and about 50 s of system time, which is the "49 s of sys" from #51. Meshing whole prototypes in parallel, one per thread with `InParallel` off, took **1.24 s with 2 s of sys** and gave the same triangles (#65).

## Decision (from #65's measurements)

- Mesh distinct prototypes on `min(8, cores)` worker threads, one prototype per thread, with `IMeshTools_Parameters::InParallel = false`.
- The per-face recovery ladder stays per prototype. Its counters go through per-thread `Summary` partials that are merged afterwards; there is no shared mutable state.
- **Thread safety:**
  - prototypes that share TShapes (edges or faces) must not be meshed at the same time, because BRepMesh writes triangulations onto shared edges. Partition by shared edges (union-find over prototypes); a component is one unit;
  - mesh progress events stay serialised through `Events`.

## Acceptance

- The 401-file corpus diff against `main` shows **zero** differences (corpus-diff.py).
- Ultimaker's `t_mesh_ms` is ≤ 1.5 s and its sys time drops (`/usr/bin/time`). Record both numbers in plan.md.
- A ThreadSanitizer run of the kernel on `tests/data` and a few corpus files is clean, or every report is understood and documented.
- A test with two prototypes sharing an edge (a fixture-gen file) meshes them in one unit, with the same result as serial.

Refs: #51, #65

