---
type: issue
state: open
created: 2026-10-04T18:57:35Z
updated: 2026-10-08T10:55:57Z
author: gerchowl
author_url: https://github.com/gerchowl
url: https://github.com/vig-os/stepv/issues/21
comments: 2
labels: feature, priority:medium, needs-human
assignees: none
milestone: none
projects: none
parent: none
children: none
synced: 2026-10-09T08:46:05.751Z
---

# [Issue 21]: [Viewer: model tree with show/hide, cross sections, measurements on the exact B-rep](https://github.com/vig-os/stepv/issues/21)

## Description

Opening a STEP file (double-click, not just Space) should give a real viewer:

- **model tree**: the XCAF assembly structure (assemblies, instances, parts, by name), with
  per-node **show/hide** (and isolate);
- **cross sections**: one or more clipping planes, axis-aligned and free, with the cut faces capped;
- **measurements** on the exact B-rep, not the mesh: point-to-point distance, edge length,
  radius/diameter of circular edges and cylinders, angle between faces or edges, face area, and
  coordinates of a picked vertex; plus bounding box and volume per part.

## What it needs from the kernel

Today the kernel emits meshes, per-face status/colour and curves. Measurements need the topology
it currently throws away:

- the **assembly tree** (it is walked already; emit it, not just the flattened parts);
- per **face**: surface type (plane, cylinder, cone, sphere, torus, B-spline) and key parameters
  (axis, radius);
- per **edge**: a polyline plus curve type and parameters (line, circle with centre/radius, …);
- **vertices**.

That means a new STEPVMSH version, or a separate topology file, and kernel queries for exact
measurements (`BRepGProp` for area and volume, `BRepExtrema_DistShapeShape` for distances).

## Open decision: where the viewer lives

- **A. Native macOS app**: stepv.app becomes a document-based SwiftUI + SceneKit app. It reuses the
  Quick Look scene builder and gives the best Mac experience, but it's macOS-only.
- **B. Cross-platform Rust viewer** (wgpu + egui) replacing the minifb `stepv view`. One codebase
  for Linux and macOS; more work up front; the GUI is non-native.
- **C. Both, staged**: B first (Linux has no viewer beyond `stepv view` today), with the Quick Look
  preview staying SceneKit.

Picking this is the first step. The kernel topology work is needed under every option, so it can
start regardless.

---

# [Comment #1]() by [gerchowl]()

_Posted on October 4, 2026 at 09:15 PM_

The kernel side is in #26. `--topology` writes the assembly tree and the exact B-rep facts per prototype: surface types and parameters (plane normals, the radii of cylinders, cones, spheres and tori), curves (circle centres and radii), edge lengths, vertices, face areas and volumes. Its indices line up with the mesh's face ids, so a picked triangle names its exact face. Over the 401-file corpus it changes no exit code. The format is at the top of `kernel/topology.cpp`.

**Still needed before the viewer itself:** the platform decision (A native macOS / B cross-platform Rust wgpu+egui / C both, staged). Not in topology v1, deliberately: edge polylines for on-screen edge picking (they belong in a STEPVMSH v4, shaped by the viewer), and shape-to-shape distances (an on-demand kernel query, not precomputed data).

---

# [Comment #2]() by [cadprobs-a11y]()

_Posted on October 8, 2026 at 10:55 AM_

The topology export described here should make measurement provenance visible in the viewer: preserve the assembly instance transform when mapping a picked triangle back to its prototype, and show the witness points or segment for shape-to-shape queries. It may also help to distinguish per-part values from assembly totals for bounding box, area, and volume, and to label any mesh fallback separately from exact B-Rep results.

The [CADProps STEP measurement guide](https://www.cadprops.com/guides/measure-step-file/) uses similar user-facing terminology. Disclosure: I contribute to CADProps. It is a separate hosted tool and processes uploaded files on its servers.

