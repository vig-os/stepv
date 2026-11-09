---
type: issue
state: open
created: 2026-11-09T02:29:23Z
updated: 2026-11-09T02:29:23Z
author: gerchowl
author_url: https://github.com/gerchowl
url: https://github.com/vig-os/stepv/issues/158
comments: 0
labels: chore, priority:low
assignees: none
milestone: none
projects: none
parent: none
children: none
synced: 2026-11-09T09:29:35.274Z
---

# [Issue 158]: [graph: fuzz target for the scanner and argument parser (cargo-fuzz)](https://github.com/vig-os/stepv/issues/158)

A cargo-fuzz target over `Graph::open` and `args()`/`args_of()`, seeded from `tests/fixtures/malformed/`. Acceptance: runs in CI for a bounded time; any panic is a bug.

Deferred from ADR-0001 §5 ("Later", hardening); not in the MVP milestone.

Refs: #115
