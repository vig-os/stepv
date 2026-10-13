---
type: issue
state: open
created: 2026-10-12T20:43:59Z
updated: 2026-10-12T20:43:59Z
author: gerchowl
author_url: https://github.com/gerchowl
url: https://github.com/vig-os/stepv/issues/80
comments: 0
labels: bug
assignees: none
milestone: none
projects: none
parent: none
children: none
synced: 2026-10-13T09:11:33.267Z
---

# [Issue 80]: [cache: a concurrent run's outline write can drop another run's box from the artifact](https://github.com/vig-os/stepv/issues/80)

## A concurrent run's box can be lost from the outline artifact (#62, #78)

Two cached runs of the same file (same `cache::FileId`, so the same `<key>.outline.json`):

1. Run A gets its outline (`Event::Tree`). It checks for an artifact at the key: `cache::load_outline_at` reads and parses the file (`src/main.rs`, the `Event::Tree` callback). It finds none, or one of another version.
2. Run B, which got its outline earlier, completes. Its box write (`store_outline`, then `write_atomic`) renames an artifact **with** the box into place.
3. Run A goes on to write its outline **without** a box. `write_atomic` does `create_new` → `write_all` → `sync_all` → `rename`, and A's rename replaces B's artifact.

**Consequence:** the artifact has no box until the next run of the file completes. The tree and header are unaffected, since both runs are of the same file. Nothing is served wrong; a box is missing.

**The window:** from A's read and JSON parse to A's rename. That covers the read, the parse, the `.part`'s create, write and `sync_all`, then the rename: milliseconds or more on a real disk. The directory `fsync` comes after the rename, so it is outside the window.

**Proposed fix**, either of:
- A lock file per key (`<key>.outline.lock`, `flock`), held across the check and the write, and taken by the box write too.
- A compare-and-swap on the entry: write the outline only with `link`/`renameat2(RENAME_NOREPLACE)` (macOS: `renamex_np(RENAME_EXCL)`), which fails if an artifact appeared since the check. The box write keeps a plain rename.

**Test:** hold A between its check and its rename (a test hook, as `STEPV_OCCT_TEST_STALL_MS` holds the kernel), complete B meanwhile, and assert the box survives.

Refs: #62

