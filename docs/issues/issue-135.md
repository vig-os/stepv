---
type: issue
state: closed
created: 2026-11-08T23:56:23Z
updated: 2026-11-09T04:20:19Z
author: gerchowl
author_url: https://github.com/gerchowl
url: https://github.com/vig-os/stepv/issues/135
comments: 0
labels: feature
assignees: none
milestone: MVP: graph core
projects: none
parent: none
children: none
synced: 2026-11-09T09:29:44.607Z
---

# [Issue 135]: [graph-core G3: Structure as a DAG + placements + units](https://github.com/vig-os/stepv/issues/135)

Slice **G3** of ADR-0001 (`docs/adr/0001-graph-core.md` §6), milestone "MVP: graph core". The design is decided in the ADR; implement it as written, or report a design question.

## Scope
`structure()`: the product DAG, the cycle check, shown/leaf counts, `leaf_path`/`leaf_of`, lazy capped `occurrences()`, both naming rules, shapeless products dropped, `ITEM_DEFINED_TRANSFORMATION` (`T = S_mm(item_child)⁻¹ · S_mm(item_parent)` applied to the child's shape, OCCT's change-of-coordinates convention; item_k with rep_k, parent and child by matching against the NAUO, swapped items corrected, `T⁻¹` when reversed) and `MAPPED_ITEM` (the same convention), each item scaled to mm, `Unplaced(reason)`

## Acceptance
`Structure::tree()` equals `occt::outline`'s tree (names, shape, leaf numbers) on every corpus file except the two documented multi-file assemblies; placements equal `--topology`'s `PlacedPart::transform` within 1e-9 relative; **the inch sub-assembly inside a mm assembly** fixture and a reversed-relationship (rep_1 = parent) fixture and a non-commuting fixture (the child's item a rotation that doesn't commute with the parent's) place exactly as `--topology` does; the root rule matches the outline's roots; a cyclic fixture refuses; a shared-sub-assembly fixture whose expansion exceeds the cap gives the outline's error while `structure()` stays O(products)

## Test
`nix develop -c cargo test --test graph_structure` (runs the kernel for the oracle)

## Depends on
G2 (merge order per ADR §6).

## Process
- At most ~400 product lines; over that, split.
- MVP first (owner, 2026-11-08): hardening findings become `chore` issues, not fix rounds.
- Host rules v3 apply: one heavy job at a time; JOBS=2.

Refs: #115
