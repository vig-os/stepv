---
type: issue
state: closed
created: 2026-10-04T14:47:18Z
updated: 2026-10-04T17:04:59Z
author: gerchowl
author_url: https://github.com/gerchowl
url: https://github.com/vig-os/stepv/issues/9
comments: 0
labels: feature
assignees: none
milestone: none
projects: none
parent: none
children: none
synced: 2026-10-05T08:45:52.696Z
---

# [Issue 9]: [S5: macOS front-ends (Quick Look preview + thumbnail extensions)](https://github.com/vig-os/stepv/issues/9)

## Description

Step **S5** of [`plan.md`](../blob/main/plan.md) §5: the Swift host app, plus the
`QLPreviewingController` and `QLThumbnailProvider` extensions. It's the largest step and the
only one that cannot be validated headlessly in CI.

## Scope

- **Decide buffers-over-FFI versus USDZ** (plan.md §4: SceneKit and Model I/O cannot read glTF).
  The `STEPVMSH` buffers are already planar, the shape `SCNGeometrySource` takes.
- **Ship OCCT inside the app bundle.** New with Plan B (plan.md §6): bundle and sign the OCCT
  dylibs plus the `stepv-occt` executable inside a sandboxed extension. **Measure the bundle size
  first.**
- Render the broken-face overlay (`FaceStatus`) and per-face colour the same way `--png` does.
- Respect the CLI's timeout and memory cap; the extension must never hang Finder.

## Depends on

S2 (#5), S3 (#6), and the LGPL NOTICE (#8) (a bundled OCCT is the first public binary that carries it).

