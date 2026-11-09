---
type: issue
state: open
created: 2026-11-08T19:17:42Z
updated: 2026-11-08T19:17:48Z
author: gerchowl
author_url: https://github.com/gerchowl
url: https://github.com/vig-os/stepv/issues/123
comments: 0
labels: chore, priority:low
assignees: none
milestone: none
projects: none
parent: none
children: none
synced: 2026-11-09T09:29:49.167Z
---

# [Issue 123]: [ql-e2e/preview session: harden cancel log wording, closed-before-ready ordering, deinit thread assumption](https://github.com/vig-os/stepv/issues/123)

Follow-up nits from the round-6 review of #107 (merged at 08eb056). None block; each is a robustness hardening.

1. **`scripts/ql-e2e/run-all.sh` (~L177), closed-before-ready case:** the host signals on kernel spawn, and the test then requires the close to be logged before the ready line. On a fast machine where the tree arrives quickly, this can fail spuriously. It fails loudly, not silently; sage has about 4.7 s of margin. Order the case on an event that is guaranteed to come before ready, rather than on how fast the machine is.
2. **`macos/Shared/StepvPreviewSession.swift:106`:** `complete()` logs `ready (cancelled)` for a cancellation. `runs.py gone` and the `first "…: ready ("` match both read it as a ready line. That only works because, in the current cases, a cancel can't come before a host act. Use a distinct wording such as `completed (cancelled)` and update `runs.py` and its tests.
3. **`macos/Preview/PreviewViewController.swift` `deinit`:** it calls `cancel()`, which touches main-thread-only state (`live`, `handler`). Nothing enforces that `deinit` runs on main. Add `dispatchPrecondition(condition: .onQueue(.main))`, or document why the last release is always on main.

## Acceptance
- No time-based ordering in the closed-before-ready case.
- A cancellation log line can't be confused with ready; `runs_test.py` covers it.
- The main-thread assumption in `deinit` is asserted or documented.

Refs: #64, #107

