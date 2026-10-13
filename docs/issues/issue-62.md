---
type: issue
state: closed
created: 2026-10-12T14:56:40Z
updated: 2026-10-12T21:16:24Z
author: gerchowl
author_url: https://github.com/gerchowl
url: https://github.com/vig-os/stepv/issues/62
comments: 0
labels: feature, priority:medium
assignees: none
milestone: none
projects: none
parent: none
children: none
synced: 2026-10-13T09:11:41.972Z
---

# [Issue 62]: [Cache: atomic complete-only scenes and a header/tree/boxes artifact (#54 step 4)](https://github.com/vig-os/stepv/issues/62)

## Motivation

#54 step 4. A load that is killed partway must never leave a scene the next run trusts. A timed-out load of a big file should still leave something useful behind for next time: the header, the tree and the boxes.

## Decision (from the #54 review, consolidated on #54)

- **Only complete scenes are cached.** Write `<key>.part`, `fsync` it, then `rename` it into place. The cached scene carries `scene_version` and a completeness tag, and a reader rejects anything that is not complete or has the wrong version.
- **Dropped:** a resumable partial-mesh cache. Deflection scales with the bbox, so a resumed load re-tessellates anyway.
- **Added:** a separate **header + tree + boxes artifact**, keyed on (path, len, mtime). It is written as soon as the outline (and the boxes, once known) arrive, even if the run is later killed. It feeds the thumbnail fallback and the macOS header path, and it makes a timed-out load useful next time.

## Existing pieces

- `src/cache.rs`: today's cache.
- `src/header.rs`: the header.
- `occt::run_streaming` and `occt::Event`: #59.
- `occt::outline`: #58.

## Scope (P0)

- [ ] Atomic, complete-only scene cache writes, with a version and a completeness tag.
- [ ] The header/tree/boxes artifact: its format, the key, writing it on outline arrival, reading it back.
- [ ] Docs: plan.md and CHANGELOG.

## Pitfalls

- Concurrent writers of the same key (two processes previewing at once).
- A crash between `fsync` and `rename`, and stale `.part` files left behind.
- mtime granularity, and editors that save in place.
- The cache directory being on another filesystem from where the temporary file is written (`rename` is not atomic across filesystems).

## Acceptance

- A test kills a run mid-stream and checks that nothing is cached as complete, while the header/tree artifact exists and reads back equal to the outline.
- A test checks that a truncated or old-version cache entry is rejected and regenerated.
- Tests show that concurrent writers never yield a torn entry.

Refs: #54

