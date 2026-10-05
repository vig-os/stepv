---
type: issue
state: closed
created: 2026-10-04T18:57:33Z
updated: 2026-10-04T20:19:09Z
author: gerchowl
author_url: https://github.com/gerchowl
url: https://github.com/vig-os/stepv/issues/20
comments: 0
labels: bug, priority:high
assignees: none
milestone: none
projects: none
parent: none
children: none
synced: 2026-10-05T08:45:49.243Z
---

# [Issue 20]: [Quick Look preview: flat models edge-on, low-contrast text, scroll doesn't zoom](https://github.com/vig-os/stepv/issues/20)

## Description

Found eyeballing `~/stepv-examples` in Finder (macOS Quick Look preview):

1. **Flat models are shown edge-on.** The preview uses SceneKit's default camera (CAD front view),
   so a sketch or a flat part lying in the XY plane appears as a line. Flat content should be viewed
   **perpendicular to its plane**; 3D content keeps the isometric default. The same rule belongs in
   the thumbnails (`render.rs` default camera) and in `stepv view`'s initial and reset camera.
2. **The info text is barely readable**: `secondaryLabelColor` on the light grey scene background.
3. **The scroll wheel moves the view instead of zooming.** `allowsCameraControl` maps scroll to
   translation; it should zoom.

## Acceptance

- A planar scene (thinnest bounding-box extent ≲ 2% of the diagonal) renders face-on in the
  thumbnail, the preview and the viewer. Tested: the sketch circle renders round, not as a line.
- The info text sits on a contrasting panel.
- Scroll zooms in the preview.

