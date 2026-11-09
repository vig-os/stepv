---
type: issue
state: open
created: 2026-11-09T03:20:26Z
updated: 2026-11-09T03:20:26Z
author: gerchowl
author_url: https://github.com/gerchowl
url: https://github.com/vig-os/stepv/issues/174
comments: 0
labels: chore, priority:low
assignees: none
milestone: none
projects: none
parent: none
children: none
synced: 2026-11-09T09:29:28.374Z
---

# [Issue 174]: [graph: G3a structure review nits (dangling refs refuse file, tolerance types, dedup, complex formation)](https://github.com/vig-os/stepv/issues/174)

Review nits from #173 (G3a, product structure), deferred under MVP-first:

- **A dangling reference refuses the whole file.** `structure.rs` calls `fields(..)?` on every NAUO that names a product definition, and on every product definition (`product_names`). A dangling reference in either (e.g. a product definition's context) fails the whole file with `Malformed`, where OCCT still produces an outline. It falls back to OCCT and doesn't panic. Degrade per product instead.
- **`is_tolerance` guesses from type names** instead of following OCCT's class tree.
- **`bodies` uses `dedup()`,** which only removes adjacent duplicates.
- **A complex `PRODUCT_DEFINITION_FORMATION` gets no names.**

None of these occur in the corpus. Each needs a fixture that compares with `occt::outline`.

Refs: #135

