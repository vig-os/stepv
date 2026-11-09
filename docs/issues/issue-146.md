---
type: issue
state: open
created: 2026-11-08T23:56:42Z
updated: 2026-11-08T23:56:42Z
author: gerchowl
author_url: https://github.com/gerchowl
url: https://github.com/vig-os/stepv/issues/146
comments: 0
labels: feature
assignees: none
milestone: MVP: graph core
projects: none
parent: none
children: none
synced: 2026-11-09T09:29:39.675Z
---

# [Issue 146]: [graph-core M4a: Classify + ledger + rayon meshing](https://github.com/vig-os/stepv/issues/146)

Slice **M4a** of ADR-0001 (`docs/adr/0001-graph-core.md` §6), milestone "MVP: graph core". The design is decided in the ADR; implement it as written, or report a design question.

## Scope
`classify`, `mesh_product`, the ledger with reason signatures, rayon over products, the ledger in `--info`; `just fastpath-corpus` runs both gates

## Acceptance
**The surface gate** (every triangle within d of the exact surface) and **the OCCT gate** (sampled symmetric Hausdorff ≤ 2·d per product, distances recorded): 0 failures over the corpus; `Part::is_well_formed` on every Rust part

## Test
`nix develop -c cargo test --test fastpath` and `nix develop -c just fastpath-corpus`

## Depends on
M3 (merge order per ADR §6).

## Process
- At most ~400 product lines; over that, split.
- MVP first (owner, 2026-11-08): hardening findings become `chore` issues, not fix rounds.
- Host rules v3 apply: one heavy job at a time; JOBS=2.

Refs: #115
