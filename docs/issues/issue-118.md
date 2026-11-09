---
type: issue
state: open
created: 2026-11-08T16:12:56Z
updated: 2026-11-08T16:12:56Z
author: gerchowl
author_url: https://github.com/gerchowl
url: https://github.com/vig-os/stepv/issues/118
comments: 0
labels: chore, priority:low
assignees: none
milestone: none
projects: none
parent: none
children: none
synced: 2026-11-09T09:29:50.927Z
---

# [Issue 118]: [test(viewer): tell 'no X server' apart from the GPU pipeline-refusal fallback in a_gpu_that_refuses_the_pipelines_falls_back_to_software](https://github.com/vig-os/stepv/issues/118)

From the review of #116 (Fixes #104). When the X connection itself fails, `a_gpu_that_refuses_the_pipelines_falls_back_to_software` reports it as a mismatched fallback note: 'GPU viewer could not start (winit EventLoopError … Failed to open connection to X server); using the software viewer'. A broken test environment then reads like a product regression.

## Scope
- [ ] Before asserting the pipeline-refusal path, the test checks that a display connection works (or the harness guarantees it). If there is no X server, it fails with a clear environment error that names `DISPLAY`.
- [ ] No skip and no silent pass when X is missing.

Refs #104, #116
