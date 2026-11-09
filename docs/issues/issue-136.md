---
type: issue
state: closed
created: 2026-11-08T23:56:25Z
updated: 2026-11-09T06:43:16Z
author: gerchowl
author_url: https://github.com/gerchowl
url: https://github.com/vig-os/stepv/issues/136
comments: 0
labels: feature
assignees: none
milestone: MVP: graph core
projects: none
parent: none
children: none
synced: 2026-11-09T09:29:44.116Z
---

# [Issue 136]: [graph-core G4: Conservative boxes](https://github.com/vig-os/stepv/issues/136)

Slice **G4** of ADR-0001 (`docs/adr/0001-graph-core.md` §6), milestone "MVP: graph core". The design is decided in the ADR; implement it as written, or report a design question.

## Scope
`bounds()`: the per-type table of §2.3, the 8-corner rule, post-order composition over the DAG, `Unknown` propagation (non-positive weights included), leaf boxes via `leaf_path`

## Acceptance
On every corpus file, each product's box contains OCCT's prototype `bbox` (within its tolerance enlargement); 0 violations; the tightness ratio per product is reported; §2.7's structure + boxes time

## Test
`nix develop -c cargo test --test graph_bounds`

## Depends on
G3 (merge order per ADR §6).

## Process
- At most ~400 product lines; over that, split.
- MVP first (owner, 2026-11-08): hardening findings become `chore` issues, not fix rounds.
- Host rules v3 apply: one heavy job at a time; JOBS=2.

Refs: #115
