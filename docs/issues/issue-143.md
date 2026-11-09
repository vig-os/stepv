---
type: issue
state: open
created: 2026-11-08T23:56:37Z
updated: 2026-11-08T23:56:37Z
author: gerchowl
author_url: https://github.com/gerchowl
url: https://github.com/vig-os/stepv/issues/143
comments: 0
labels: feature
assignees: none
milestone: MVP: graph core
projects: none
parent: none
children: none
synced: 2026-11-09T09:29:41.005Z
---

# [Issue 143]: [graph-core M1b: B-spline edge polylines (MVP, required)](https://github.com/vig-os/stepv/issues/143)

Slice **M1b** of ADR-0001 (`docs/adr/0001-graph-core.md` §6), milestone "MVP: graph core". The design is decided in the ADR; implement it as written, or report a design question.

## Scope
De Boor evaluation and knot-insertion subdivision, rational weights > 0, complex-form curves, flattened to the same contract

## Acceptance
As M1a, plus corpus curves against OCCT's edge polylines from `--edges`: within d

## Test
`nix develop -c cargo test mesh::edges`

## Depends on
M1a (merge order per ADR §6).

## Process
- At most ~400 product lines; over that, split.
- MVP first (owner, 2026-11-08): hardening findings become `chore` issues, not fix rounds.
- Host rules v3 apply: one heavy job at a time; JOBS=2.

Refs: #115
