---
type: issue
state: closed
created: 2026-11-08T23:56:20Z
updated: 2026-11-09T00:59:23Z
author: gerchowl
author_url: https://github.com/gerchowl
url: https://github.com/vig-os/stepv/issues/133
comments: 0
labels: feature
assignees: none
milestone: MVP: graph core
projects: none
parent: none
children: none
synced: 2026-11-09T09:29:45.536Z
---

# [Issue 133]: [graph-core G1: Scanner + entity table](https://github.com/vig-os/stepv/issues/133)

Slice **G1** of ADR-0001 (`docs/adr/0001-graph-core.md` §6), milestone "MVP: graph core". The design is decided in the ADR; implement it as written, or report a design question.

## Scope
`src/graph/{mod,lex}.rs`: map, header via `header::read`, records → id, interned type (complex as a sorted part list), arg span; the id → node lookup (dense or sorted); edition 3 `DATA(…)` parameters, `ANCHOR`/`SIGNATURE` skipped; CR/LF stripped in strings (the header path too); duplicate ids kept as OCCT keeps them; refusal with a byte offset

## Acceptance
A table equal to an independent oracle (`ids` and types from a regex pass) on `tests/data`; the duplicate-id fixture agrees with `occt::outline`; all malformed fixtures refuse without panicking; Ultimaker/stress within §2.7's scan time and the "without `referrers`" heap

## Test
`nix develop -c cargo test graph::` and `nix develop -c cargo run --release --example graph_scan -- <file>` (time and heap)

## Depends on
none (merge order per ADR §6).

## Process
- At most ~400 product lines; over that, split.
- MVP first (owner, 2026-11-08): hardening findings become `chore` issues, not fix rounds.
- Host rules v3 apply: one heavy job at a time; JOBS=2.

Refs: #115
