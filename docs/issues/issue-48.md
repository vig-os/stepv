---
type: issue
state: open
created: 2026-10-10T23:07:18Z
updated: 2026-10-10T23:07:18Z
author: gerchowl
author_url: https://github.com/gerchowl
url: https://github.com/vig-os/stepv/issues/48
comments: 0
labels: feature, priority:low
assignees: none
milestone: none
projects: none
parent: none
children: none
synced: 2026-10-11T08:35:00.193Z
---

# [Issue 48]: [Viewer: free and multiple section planes](https://github.com/vig-os/stepv/issues/48)

## Motivation
#21 asked for "one or more clipping planes, axis-aligned and free". The viewer (#34, #43) has **one** plane, perpendicular to X, Y or Z, with a flip. A part on a slanted face, or a pocket that only two planes can open (a quarter cut), can't be sectioned the way a CAD user expects.

## What exists
- `Section { axis, offset, flip }` in `src/view/gpu.rs` produces a plane `[n, w]`, and every pass already takes a general plane (`cut()` in the shader, `cap_quad`, the id pass).
- Exact caps (#43): the kernel's `section` op takes any `[nx, ny, nz, w]`. The stencil cap draws any plane's quad.

## Scope
- [ ] **A free plane:** set from a picked planar face (its outward normal, through the pick), then offset along the normal. Optionally rotate it with a gizmo or with two angles in the panel.
- [ ] **Up to three planes at once,** each with its own on/off and flip. The kept region is the intersection.
  - The shader's `cut()` takes an array of planes.
  - Each plane gets its own cap, clipped by the other planes.
- [ ] Exact caps per plane, through the kernel. Each plane's cap is clipped by the other planes, on the CPU or in the shader.

## Pitfalls
- **Caps with several planes:**
  - The stencil-parity cap of one plane must be clipped by the others, or it hatches space that another plane has removed.
  - Exact caps need the same clip.
- **Free planes and the box-based slider:** `Section::plane` maps the slider's 0..1 onto the model box along one axis. A free plane needs the box's extent along its normal.
- **Depth ranks (`CAP_STEP`):** they assume one plane's caps are coplanar. With several planes, caps aren't coplanar and don't overlap, so they need no ranks between planes.

## Acceptance
- [ ] A plane set from a slanted face cuts exactly along it, with exact caps.
- [ ] Two planes (X and Y) open a quarter of the bracket, and both cut faces are capped where solid and open in the hole.

Follow-up from #21.

