---
type: issue
state: closed
created: 2026-10-12T15:38:46Z
updated: 2026-10-12T19:50:37Z
author: gerchowl
author_url: https://github.com/gerchowl
url: https://github.com/vig-os/stepv/issues/70
comments: 0
labels: chore, priority:low
assignees: none
milestone: none
projects: none
parent: none
children: none
synced: 2026-10-13T09:11:37.897Z
---

# [Issue 70]: [cache: follow-ups from #68's review (test helper, sweep cost, fsync on hits, copies)](https://github.com/vig-os/stepv/issues/70)

## Follow-ups from the review of #68 (#62 A1)

- [x] **The cross-process test can silently stop testing.** The helper (`src/cache.rs` ~332–341) does nothing and exits 0 when its env vars are missing or the test filter stops matching. Make it panic when the env is absent, and have the parent check the child's output for `1 passed`. (Done in #69.)
- [ ] **The sweep stats every cache file** before checking the `.part` extension, on every `store` (~233–243). Check the extension first.
- [ ] **Old `<key>.tmp-<pid>` files** from the pre-#68 writer are never swept. Sweep them once, or document the limit.
- [ ] **The cache-hit path now fsyncs.** `from_cache` (`main.rs` ~614) does a full `sync_all` (F_FULLFSYNC on macOS) plus a directory fsync just to copy a hit to the user's output. Measure it on a Finder-sized thumbnail burst, decide whether user outputs need durability, and record the choice in plan.md.
- [ ] **The payload is copied twice** (`load` `.to_vec()`, and `store` framing a second copy). This doubles peak memory for big meshes.

Refs: #62


