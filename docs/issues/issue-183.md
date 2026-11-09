---
type: issue
state: open
created: 2026-11-09T06:58:17Z
updated: 2026-11-09T06:58:17Z
author: gerchowl
author_url: https://github.com/gerchowl
url: https://github.com/vig-os/stepv/issues/183
comments: 0
labels: chore
assignees: none
milestone: none
projects: none
parent: none
children: none
synced: 2026-11-09T09:29:24.961Z
---

# [Issue 183]: [chore(view): the loading viewport's camera is fixed, and jumps when the scene arrives](https://github.com/vig-os/stepv/issues/183)

Found in G6a (#138). While loading, the placeholders draw with `Camera::default()` framed on the boxes, and the viewport takes no orbit, pan or zoom. On Done the viewer switches to `Camera::for_scene` and the scene's fit, so the view can jump (a flat part seen from its plane normal, or a different fit sphere).

Fix direction: drive the loading viewport with the same `Controls`, homed on the placeholders' fit, and carry the camera into the `Viewer` on Done.

Refs: #138, #85
