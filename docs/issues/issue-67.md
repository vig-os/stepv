---
type: issue
state: open
created: 2026-10-12T15:16:21Z
updated: 2026-10-12T15:16:21Z
author: gerchowl
author_url: https://github.com/gerchowl
url: https://github.com/vig-os/stepv/issues/67
comments: 0
labels: bug
assignees: none
milestone: none
projects: none
parent: none
children: none
synced: 2026-10-13T09:11:38.382Z
---

# [Issue 67]: [test: stream's killed-after-outline test flakes under load (3 s cap)](https://github.com/vig-os/stepv/issues/67)

`tests/stream.rs::a_run_killed_after_the_outline_keeps_the_outline_but_no_summary` holds the stalled kernel to a 3 s wall clock. That cap has to cover the kernel's start, its sandbox and the parse before the outline is out. During #62's local gate on a loaded Mac (load average 16.7) no outline arrived within 3 s:

```
panicked at tests/stream.rs:206:5:
only the outline arrived: []
```

3 of 3 re-runs passed at load average about 12. The stall is 30 s, so the cap can sit well above the parse and still kill the kernel inside the stall (#62's `tests/cache.rs` uses 10 s for the same reason). The fix is to choose the cap by that margin, not to retry.

Refs: #59, #62
