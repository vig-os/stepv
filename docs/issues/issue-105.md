---
type: issue
state: open
created: 2026-10-13T05:42:59Z
updated: 2026-10-13T05:42:59Z
author: gerchowl
author_url: https://github.com/gerchowl
url: https://github.com/vig-os/stepv/issues/105
comments: 0
labels: bug, priority:low
assignees: none
milestone: none
projects: none
parent: none
children: none
synced: 2026-10-13T09:11:24.159Z
---

# [Issue 105]: [kernel fd input: follow-ups from #103's review](https://github.com/vig-os/stepv/issues/105)

Nits from the review of #103 (#102) at b22f756:
- **Untested claim:** the CHANGELOG says the IGES temporary copy is 'removed even if the kernel is killed', but no test covers a kill. `the_iges_copy_is_unlinked` covers only a normal exit and a failed run. Add a SIGKILL test.
- `pipe()` followed by `fcntl(FD_CLOEXEC)` in `RemoveWhenGone` is safe only while `main` is single-threaded. Use `pipe2(O_CLOEXEC)`, which Darwin lacks, or set CLOEXEC before any thread exists, and document that.
- **No XCAF parity test:** names and colours by fd vs by path for STEP (only the mesh and outline are compared).
- The never-re-open test **skips under root**. Make it fail instead, or run a root-proof variant.
- **Corpus determinism:** the #103 corpus diff showed 2 of 401 files differing because of a timing-dependent heal in the base kernel (the #60 per-face budget under load). `corpus-diff.py` should pin `STEPV_OCCT_FACE_BUDGET_MS` (for example to 0, disabled) so that diffs are deterministic. Related to #83.

Refs: #102, #103, #83
