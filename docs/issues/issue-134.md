---
type: issue
state: closed
created: 2026-11-08T23:56:21Z
updated: 2026-11-09T02:05:40Z
author: gerchowl
author_url: https://github.com/gerchowl
url: https://github.com/vig-os/stepv/issues/134
comments: 0
labels: feature
assignees: none
milestone: MVP: graph core
projects: none
parent: none
children: none
synced: 2026-11-09T09:29:45.034Z
---

# [Issue 134]: [graph-core G2: CSR + lazy args + complex parts + census](https://github.com/vig-os/stepv/issues/134)

Slice **G2** of ADR-0001 (`docs/adr/0001-graph-core.md` §6), milestone "MVP: graph core". The design is decided in the ADR; implement it as written, or report a design question.

## Scope
Forward CSR in the scan; `args()`/`args_of()` → `Value`, depth cap 64; `is_a`/`parts`/`of_type` over complex parts; `referrers()` built lazily; `census()`; `examples/graph_census.rs` writes per-file surface and edge-curve shares (3-D only, p-curves excluded) and the share of products all of whose faces are analytic

## Acceptance
Round trip: every `#n` in a span is in `refs`, in order; args parse for every entity and every complex part of every corpus file; the unit, context, relationship and rational-B-spline walkthroughs of §2.4 as unit tests; the census table is committed to `docs/adr/0001-census.md`, replacing §2.6's guess; the "with `referrers`" heap

## Test
`nix develop -c cargo test graph::` and the census example over the corpus

## Depends on
G1 (merge order per ADR §6).

## Process
- At most ~400 product lines; over that, split.
- MVP first (owner, 2026-11-08): hardening findings become `chore` issues, not fix rounds.
- Host rules v3 apply: one heavy job at a time; JOBS=2.

Refs: #115
