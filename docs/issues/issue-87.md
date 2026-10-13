---
type: issue
state: open
created: 2026-10-12T22:41:51Z
updated: 2026-10-13T00:45:26Z
author: gerchowl
author_url: https://github.com/gerchowl
url: https://github.com/vig-os/stepv/issues/87
comments: 2
labels: bug, priority:low
assignees: none
milestone: none
projects: none
parent: none
children: none
synced: 2026-10-13T09:11:30.729Z
---

# [Issue 87]: [test: harden #84's view watchdog test (ESRCH check, deadline after the pidfile)](https://github.com/vig-os/stepv/issues/87)

Nits from the review of #84 at efbd319:
- `tests/cache.rs:704`: the survivor loop treats any `killpg(group, 0) == -1` as 'gone'. Check `errno == ESRCH`, so that EPERM doesn't pass vacuously.
- **Flake risk:** the watchdog test's 2 s deadline has to cover stepv starting and spawning the stand-in, and the stand-in writing its pidfile. A loaded runner could fail with 'the kernel never started'. Start the deadline only once the pidfile exists.

Refs: #63, #84
---

# [Comment #1]() by [gerchowl]()

_Posted on October 13, 2026 at 12:00 AM_

More nits from the final review of #84 (f391227):
- `tests/progress.rs:176-190`: the test doesn't check that only the sandbox warning follows the erase, so any stray stderr after it would pass. Assert the exact allowed lines.
- `src/view/mod.rs:229`: the session check has no override to skip it (there's only one to force 'no session'), so a macOS false negative can't be worked around. Add a documented override, for example `STEPV_VIEW_ASSUME_SESSION=1`, with a test.

---

# [Comment #2]() by [gerchowl]()

_Posted on October 13, 2026 at 12:45 AM_

From #93's final review (5389cbc): after an upload refusal, stderr says 'the GPU viewer could not start', but the window had already opened. Reword it, for example 'the GPU refused the scene; drawing it in software', and update the test's prefix match.

