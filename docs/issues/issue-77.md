---
type: issue
state: open
created: 2026-10-12T19:52:46Z
updated: 2026-10-12T19:52:46Z
author: gerchowl
author_url: https://github.com/gerchowl
url: https://github.com/vig-os/stepv/issues/77
comments: 0
labels: none
assignees: none
milestone: none
projects: none
parent: none
children: none
synced: 2026-10-13T09:11:34.303Z
---

# [Issue 77]: [cache key: same-length in-place rewrites within one mtime tick, and part files outside the key](https://github.com/vig-os/stepv/issues/77)

## Known limits of the cache key (#62)

Every cache key, the entries' and the outline artifact's, is (path, len, mtime) of the top file. Two consequences, documented in plan.md (Cache, Known limits), with no test yet:

- [ ] **A file rewritten in place, at the same length, within one mtime tick**, is taken for the old one and its cached result is served. Editors that save in place and filesystems with coarse mtimes (1 s, 2 s on FAT) make this reachable. Options: add the inode's ctime, or hash the file's head and tail.
- [ ] **A multi-file assembly's part files are outside the key.** A part file edited after the assembly was cached is not seen until the top file changes. Options: record the resolved part files' (path, len, mtime) in the entry and check them on load.

Refs: #62
