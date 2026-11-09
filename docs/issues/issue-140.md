---
type: issue
state: open
created: 2026-11-08T23:56:32Z
updated: 2026-11-08T23:56:32Z
author: gerchowl
author_url: https://github.com/gerchowl
url: https://github.com/vig-os/stepv/issues/140
comments: 0
labels: feature
assignees: none
milestone: MVP: graph core
projects: none
parent: none
children: none
synced: 2026-11-09T09:29:42.349Z
---

# [Issue 140]: [graph-core G6c: Primitive fits](https://github.com/vig-os/stepv/issues/140)

Slice **G6c** of ADR-0001 (`docs/adr/0001-graph-core.md` §6), milestone "MVP: graph core". The design is decided in the ADR; implement it as written, or report a design question.

## Scope
§2.8: coaxial-cylinder and concentric-sphere fits from the graph's analytic surfaces, conservative radial and axial bounds, circumscribed drawing, fallback to the box

## Acceptance
On the corpus, every fit contains OCCT's mesh vertices of that leaf (0 outside); a fit is used only when smaller than the box; fixtures of a shaft (cylinder), a ball (sphere) and a bracket (box fallback); fits are styled as placeholders

## Test
`nix develop -c cargo test --test graph_fits`

## Depends on
G6b (merge order per ADR §6).

## Process
- At most ~400 product lines; over that, split.
- MVP first (owner, 2026-11-08): hardening findings become `chore` issues, not fix rounds.
- Host rules v3 apply: one heavy job at a time; JOBS=2.

Refs: #115
