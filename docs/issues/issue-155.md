---
type: issue
state: open
created: 2026-11-09T02:03:35Z
updated: 2026-11-09T02:03:35Z
author: gerchowl
author_url: https://github.com/gerchowl
url: https://github.com/vig-os/stepv/issues/155
comments: 0
labels: bug
assignees: none
milestone: none
projects: none
parent: none
children: none
synced: 2026-11-09T09:29:36.618Z
---

# [Issue 155]: [ql-e2e: the cache-control thumbnail check fails intermittently on macos-15](https://github.com/vig-os/stepv/issues/155)

**What happened:** on PR #154 (graph only; no Quick Look code touched), the macOS `Kernel (macos-15)` job's `ql-e2e` step failed on attempt 1 and passed on rerun.

```
FAIL: cache control: a cached thumbnail passed
FAIL: cache control: the repeated request was not caught as cached:
== thumb cache-control-link.brep
== ql-e2e: FAILURES
```
Run 42544635545, attempt 1, 2026-11-09T01:38:40Z. Every preview case before it passed. The implementer also saw a thumbnail timeout in the same run.

**Expected:** the cache-control check is deterministic. Either the repeated thumbnail request is reliably served from Quick Look's cache on the runner, or the check accounts for the cache being evicted or bypassed, without loosening what it asserts.

**Next:** diagnose why the second request reached the generator (cache eviction under load? the timing of the first request's completion?) before any retry or timeout change.

Refs: #106

