---
type: issue
state: open
created: 2026-11-09T04:20:21Z
updated: 2026-11-09T04:20:21Z
author: gerchowl
author_url: https://github.com/gerchowl
url: https://github.com/vig-os/stepv/issues/176
comments: 0
labels: chore, priority:low
assignees: none
milestone: none
projects: none
parent: none
children: none
synced: 2026-11-09T09:29:27.903Z
---

# [Issue 176]: [graph: G3b placement review nits (non-finite placements, degenerate frames doc, test gaps, reverses)](https://github.com/vig-os/stepv/issues/176)

Review nits from #175 (G3b placements), deferred under MVP-first:

- **Non-finite placements.** `src/graph/structure.rs:452` (`frame`) never checks the location is finite, so an overflowing coordinate or a NaN gives `Placed` containing NaN or inf instead of `Unplaced(reason)`. The same goes for a zero, negative or NaN inch conversion factor (`:531`). It doesn't panic, but downstream consumers (G4 boxes, the M4b scene) get a non-finite transform. Fix: refuse non-finite values as `Unplaced(reason)`.
- **Degenerate frames.** `frame` (`:456-470`): a zero-length or NaN axis, or a ref direction parallel to the axis, falls back to OCCT's default direction. That matches the kernel oracle, but ADR §2.2 should say it.
- **Test gaps.**
  - The mapped-item test's mapping origin sits at (0,0,0), so the origin's unit scaling is never exercised.
  - No fixture covers the swapped-items branch (`structure.rs:429`).
- **`reverses`** (`:380`) takes the *first* matching shape definition and accepts only the plain entity type. OCCT takes the last one and accepts subtypes. This case doesn't occur in the corpus.

Refs: #135

