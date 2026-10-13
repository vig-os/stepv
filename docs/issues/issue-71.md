---
type: issue
state: open
created: 2026-10-12T16:13:44Z
updated: 2026-10-12T19:10:44Z
author: gerchowl
author_url: https://github.com/gerchowl
url: https://github.com/vig-os/stepv/issues/71
comments: 0
labels: chore, priority:low
assignees: none
milestone: none
projects: none
parent: none
children: none
synced: 2026-10-13T09:11:37.402Z
---

# [Issue 71]: [cache: follow-ups from #69's review (out-of-memory kind, gated test hook, dlsym check, documented limits)](https://github.com/vig-os/stepv/issues/71)

## Follow-ups from the review of #69 (#62 A2)

- [ ] **Out-of-memory is matched on message text.** It's detected by the exact string "out of memory", in both kernel catch sites and in `src/occt.rs` ~376. Editing either string would silently start negative-caching out-of-memory runs again. Give the summary a dedicated `kind` (or a cause plus a constant) and match on that.
- [ ] **`STEPV_OCCT_TEST_THROW` isn't gated** (`kernel/stepv-occt-core.cpp` ~1024). Gate it behind `STEPV_OCCT_TEST_HOOKS` like `measure.cpp`'s hooks, and consider gating the older `TOPOLOGY_FAIL`, `BALLOON_MB` and `ESCAPE` hooks the same way.
- [x] **`tests/cache.rs` `invalid_arguments_are_the_environment`** calls through `dlsym` results without a null check. Assert they're non-null. (Done in #69.)
- [ ] **Document two limits** where the cause is set:
  - The race between the open check and `ReadFile`: a permission revoked in between is classified as input.
  - OCCT can swallow an out-of-memory error inside `ReadFile` or `Transfer` and simply report failure, which is classified as input.
- [ ] **`src/main.rs` ~514:** `format!("{c:?}").to_lowercase()` ties the JSON to the Debug output. Use `Serialize` with `rename_all`.

Refs: #62


