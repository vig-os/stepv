---
type: issue
state: closed
created: 2026-11-08T23:56:28Z
updated: 2026-11-09T07:58:39Z
author: gerchowl
author_url: https://github.com/gerchowl
url: https://github.com/vig-os/stepv/issues/138
comments: 0
labels: feature
assignees: none
milestone: MVP: graph core
projects: none
parent: none
children: none
synced: 2026-11-09T09:29:43.237Z
---

# [Issue 138]: [graph-core G6a: The viewer opens on the graph](https://github.com/vig-os/stepv/issues/138)

Slice **G6a** of ADR-0001 (`docs/adr/0001-graph-core.md` §6), milestone "MVP: graph core". The design is decided in the ADR; implement it as written, or report a design question.

## Scope
`Load::Graph`, the graph's tree, box placeholders (translucent, wireframe), the status line, the fallback when the graph is refused

## Acceptance
Headless viewer test (`tests/view.rs`): first frame with placeholders before `Done`; no placeholder drawn with the shaded pipeline; a refused graph gives today's behaviour, with the reason in the status line

## Test
`nix develop -c cargo test --test view`

## Depends on
G4 (merge order per ADR §6).

## Process
- At most ~400 product lines; over that, split.
- MVP first (owner, 2026-11-08): hardening findings become `chore` issues, not fix rounds.
- Host rules v3 apply: one heavy job at a time; JOBS=2.

Refs: #115
