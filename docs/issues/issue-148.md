---
type: issue
state: open
created: 2026-11-08T23:56:45Z
updated: 2026-11-08T23:56:45Z
author: gerchowl
author_url: https://github.com/gerchowl
url: https://github.com/vig-os/stepv/issues/148
comments: 0
labels: feature
assignees: none
milestone: MVP: graph core
projects: none
parent: none
children: none
synced: 2026-11-09T09:29:38.739Z
---

# [Issue 148]: [graph-core M5: The kernel transfers only routed products](https://github.com/vig-os/stepv/issues/148)

Slice **M5** of ADR-0001 (`docs/adr/0001-graph-core.md` §6), milestone "MVP: graph core". The design is decided in the ADR; implement it as written, or report a design question.

## Scope
`stepv-occt --products <ids>` via `TransferEntity`, face and edge STEP entity ids in the output, the Rust side passing only routed products

## Acceptance
**CI gate:** geometry and colours of routed products equal a full XCAF run on `tests/data`, `nist-pmi` and the G7 fixtures; the full corpus is recorded in the PR; **the timing report on Ultimaker:** first Rust product, first full view, the Rust share of products and triangles, against 42 s OCCT-only

## Test
`nix develop -c cargo test --test kernel_products` and `nix develop -c just fastpath-bench real-assembly`

## Depends on
M4b (merge order per ADR §6).

## Process
- At most ~400 product lines; over that, split.
- MVP first (owner, 2026-11-08): hardening findings become `chore` issues, not fix rounds.
- Host rules v3 apply: one heavy job at a time; JOBS=2.

Refs: #115
