---
type: issue
state: closed
created: 2026-11-09T09:15:19Z
updated: 2026-11-09T09:47:41Z
author: gerchowl
author_url: https://github.com/gerchowl
url: https://github.com/vig-os/stepv/issues/190
comments: 1
labels: chore
assignees: none
milestone: none
projects: none
parent: none
children: none
synced: 2026-11-10T09:31:58.789Z
---

# [Issue 190]: [chore(graph): G5 review leftovers (tree --depth cost, heap-test cross-checks, skip reported as ok)](https://github.com/vig-os/stepv/issues/190)

Deferred from the G5 review (PR #188, #137); MVP first.
- `stepv graph tree --depth n` still expands the whole outline (up to the caps) and filters by depth, so a shallow page of a huge DAG costs the full expansion.
- The heap test's face walk is cross-checked against `faces_of` only on `tests/data`; the stress run checks only that faces > 0.
- With `STEPV_CORPUS` unset, the heap test prints SKIPPED on stderr but reports "ok"; a skip should be visible in the test summary (e.g. `#[ignore]` plus an explicit opt-in).

Refs: #137
---

# [Comment #1]() by [gerchowl]()

_Posted on November 9, 2026 at 09:47 AM_

Duplicate of #189 (the same G5 review nits, filed by the architect in parallel).

