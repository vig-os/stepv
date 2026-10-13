---
type: issue
state: closed
created: 2026-10-12T14:56:42Z
updated: 2026-10-13T02:03:13Z
author: gerchowl
author_url: https://github.com/gerchowl
url: https://github.com/vig-os/stepv/issues/63
comments: 1
labels: feature, priority:medium
assignees: none
milestone: none
projects: none
parent: none
children: none
synced: 2026-10-13T09:11:41.476Z
---

# [Issue 63]: [Viewer and CLI show the outline and progress before the geometry (#54 step 5)](https://github.com/vig-os/stepv/issues/63)

## Motivation

#54 step 5. The kernel streams the outline and progress (#59). The front-ends still wait for the whole run.

## Decision (#54)

- The viewer shows the assembly tree and the part boxes first, then progress, then the meshes.
- The CLI reports progress while it runs.
- Both use the header/tree/boxes artifact from step 4 when it is cached.

## Scope (P0)

- [ ] The viewer opens on the tree (and the boxes, where known), with a progress indicator from `occt::Event::Progress`, and swaps in the scene on `Done`.
- [ ] The CLI prints progress on stderr when it is a TTY, and stays quiet otherwise.
- [ ] A killed or timed-out run leaves the viewer showing the tree with an honest message, never an empty window.

## Acceptance

- Window tests (the existing `STEPV_VIEW_SCREENSHOT` hooks) show the tree before the geometry, using the `STEPV_OCCT_TEST_STALL_MS` hook.
- A CLI test checks the progress output.

Refs: #54. Depends on the step 4 issue.

---

# [Comment #1]() by [gerchowl]()

_Posted on October 12, 2026 at 07:52 PM_

From #62: the outline artifact (`cache::OutlineArtifact`, `<key>.outline.json`) is written by cached CLI runs only. `view` should write it too when it streams a load, so a viewer killed after the outline leaves it for the next preview.

