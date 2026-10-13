---
type: issue
state: open
created: 2026-10-13T07:51:21Z
updated: 2026-10-13T08:53:42Z
author: gerchowl
author_url: https://github.com/gerchowl
url: https://github.com/vig-os/stepv/issues/106
comments: 1
labels: bug, priority:low
assignees: none
milestone: none
projects: none
parent: none
children: none
synced: 2026-10-13T09:11:23.765Z
---

# [Issue 106]: [macOS supervisor and ql-e2e: follow-ups from #98's final review](https://github.com/vig-os/stepv/issues/106)

Nits from the review of #98 at 121f4fb:
- `macos/Supervisor/main.swift` `RunDirectory.init`: `open(O_CREAT|O_EXLOCK)` creates the lock file before it takes the lock, so a sweep in that gap could empty and `rmdir` a new run's directory. The run then fails visibly. Separately, a crash between `mkdir` and `open` leaves a lock-less directory that the sweep keeps forever. Create the lock in a temporary name and rename it in, or have the sweep treat a lock-less directory older than the max deadline as stale. Also correct the comment.
- **No client timeout:** a service death after its reply but before the events land reports a crash, and there is no client timeout. The service deadline is the only bound. Consider a client-side bound of the deadline plus a margin.
- **`ql-e2e.sh`:** `log show --start` has one-second resolution, so the cache control can see the previous request's lines (a false failure). Use sub-second marks, or a per-request id in the log line. The header comment still says 'a new path, so no Quick Look cache answers', which is wrong.
- **An untested branch in ql-e2e:** 'got the request but logged no result'.

Refs: #64, #98
---

# [Comment #1]() by [gerchowl]()

_Posted on October 13, 2026 at 08:53 AM_

#107 (#64 PR 2) resolves point 2 here, **No client timeout**, for both extensions. The thumbnail replies by itself after 17 s (its run's deadline is 15 s). The preview's `StepvPreviewSession` gives up after 122 s (its run's deadline is 120 s): it cancels the run, invalidates the connection, shows why, and drops a late reply exactly once (`macos/Tests/PreviewSessionTests`, which fails without the backstop). Points 1, 3 and 4 remain open.

