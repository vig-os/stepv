---
type: issue
state: open
created: 2026-10-13T03:10:43Z
updated: 2026-10-13T07:50:08Z
author: gerchowl
author_url: https://github.com/gerchowl
url: https://github.com/vig-os/stepv/issues/104
comments: 2
labels: bug, priority:high
assignees: none
milestone: none
projects: none
parent: none
children: none
synced: 2026-10-13T09:11:24.596Z
---

# [Issue 104]: [Viewer CI (lavapipe): a_gpu_that_refuses_the_pipelines_falls_back_to_software flaked on 'Failed to open connection to X server'](https://github.com/vig-os/stepv/issues/104)

Seen on #103's CI (run 38337928702, job 115102161034, 2026-10-13T02:47Z). `tests/cache.rs` `a_gpu_that_refuses_the_pipelines_falls_back_to_software` failed at tests/cache.rs:1034:

```
stepv: the GPU viewer could not start (winit EventLoopError: os error at …/winit-0.30.13/src/platform_impl/linux/mod.rs:788: Failed to open connection to X server); using the software viewer
```

- **What happened:** the test sets `STEPV_VIEW_REJECT_PIPELINES=1` and expects the fallback note to say the GPU refused the pipelines. Here winit couldn't connect to Xvfb at all, so the fallback came for a different reason and the note didn't match.
- **What it isn't:** #103 changes only how the kernel reads an input descriptor. This test reads a path. The same job passed on `main` at the same base (`1f65bbc`), and the other viewer tests in the run passed.
- **Likely cause:** Xvfb was unavailable for a moment mid-suite. The root cause isn't diagnosed yet.
- **Next:** make the viewer job wait for Xvfb, or have the test tell 'no X server' apart from its own refusal path, rather than retrying.

Refs: #103
---

# [Comment #1]() by [gerchowl]()

_Posted on October 13, 2026 at 07:49 AM_

Recurred on #98 at 121f4fb (run 38362262033, job 115178993393, 2026-10-13T07:30Z): the same test, `a_gpu_that_refuses_the_pipelines_falls_back_to_software`, and the same `Failed to open connection to X server` at tests/cache.rs:1034. #98's changes are macOS-only, plus the kernel's `--lifeline` flag, which this test doesn't use. That's 2 occurrences in about 6 Viewer runs today.

---

# [Comment #2]() by [gerchowl]()

_Posted on October 13, 2026 at 07:50 AM_

Second occurrence (#98 at 121f4fb). Raised to priority:high: a CI flake that fails PRs at random has to be root-caused (for example Xvfb not ready when the window tests start, or a display number collision between parallel tests), not retried away.

