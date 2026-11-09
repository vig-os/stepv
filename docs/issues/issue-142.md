---
type: issue
state: open
created: 2026-11-08T23:56:35Z
updated: 2026-11-08T23:56:35Z
author: gerchowl
author_url: https://github.com/gerchowl
url: https://github.com/vig-os/stepv/issues/142
comments: 0
labels: feature
assignees: none
milestone: MVP: graph core
projects: none
parent: none
children: none
synced: 2026-11-09T09:29:41.426Z
---

# [Issue 142]: [graph-core M1a: Analytic edge polylines](https://github.com/vig-os/stepv/issues/142)

Slice **M1a** of ADR-0001 (`docs/adr/0001-graph-core.md` §6), milestone "MVP: graph core". The design is decided in the ADR; implement it as written, or report a design question.

## Scope
`mesh::edges`: LINE/CIRCLE/ELLIPSE (direct or via `SURFACE_CURVE`/`SEAM_CURVE`), the clamped circle count and the ellipse bound of §2.6, one polyline per `EDGE_CURVE`, shared and oriented

## Acceptance
Unit tests: chordal error ≤ d and turning angle ≤ a on arcs with radii from 1e-3 to 1e3 mm (no NaN, d > r included) and on ellipses with B/A down to 0.01; two faces sharing an edge get identical points in opposite order

## Test
`nix develop -c cargo test mesh::edges`

## Depends on
G2 (merge order per ADR §6).

## Process
- At most ~400 product lines; over that, split.
- MVP first (owner, 2026-11-08): hardening findings become `chore` issues, not fix rounds.
- Host rules v3 apply: one heavy job at a time; JOBS=2.

Refs: #115
