---
type: issue
state: open
created: 2026-11-08T17:08:53Z
updated: 2026-11-08T17:08:53Z
author: gerchowl
author_url: https://github.com/gerchowl
url: https://github.com/vig-os/stepv/issues/121
comments: 0
labels: none
assignees: none
milestone: none
projects: none
parent: none
children: none
synced: 2026-11-09T09:29:50.043Z
---

# [Issue 121]: [macOS preview: a mesh that fails to decode is reported as "no geometry"](https://github.com/vig-os/stepv/issues/121)

Found in #107's final review (nit, not fixed there).

`PreviewViewController.prepare` (`macos/Preview/PreviewViewController.swift`) does `if let data = result.mesh, let decoded = try? decodeStepvMesh(data)`. A mesh the service did return but that fails to decode (a truncated or corrupt STEPVMSH, a version mismatch) falls through to `StepvPreviewText.why(result)`. For a `.done` outcome that is "no geometry", which hides the real decode error from the user and from the log.

Fix: catch the decode error, then show and log it as its own reason ("the mesh could not be read: …"). Add a unit test with a corrupt mesh.

Refs #64
