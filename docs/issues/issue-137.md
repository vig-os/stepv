---
type: issue
state: open
created: 2026-11-08T23:56:27Z
updated: 2026-11-08T23:56:27Z
author: gerchowl
author_url: https://github.com/gerchowl
url: https://github.com/vig-os/stepv/issues/137
comments: 0
labels: feature
assignees: none
milestone: MVP: graph core
projects: none
parent: none
children: none
synced: 2026-11-09T09:29:43.668Z
---

# [Issue 137]: [graph-core G5: `stepv graph` CLI](https://github.com/vig-os/stepv/issues/137)

Slice **G5** of ADR-0001 (`docs/adr/0001-graph-core.md` §6), milestone "MVP: graph core". The design is decided in the ADR; implement it as written, or report a design question.

## Scope
`summary` (header, units, census, model box), `tree` (paginated, depth-limited, capped), `census`, `query <jq>` via `jaq` over the lazy view (§2.4); text as `{"untrusted": …}`; `--timings`

## Acceptance
Golden JSON on `tests/data`; an injection-style product name comes back only as a data field; the lazy-view test: a one-product face walk on the stress assembly keeps the heap ≤ 2× the graph's (this decides `ValT` vs native functions)

## Test
`nix develop -c cargo test --test graph_cli`

## Depends on
G4 (merge order per ADR §6).

## Process
- At most ~400 product lines; over that, split.
- MVP first (owner, 2026-11-08): hardening findings become `chore` issues, not fix rounds.
- Host rules v3 apply: one heavy job at a time; JOBS=2.

Refs: #115
