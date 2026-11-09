---
type: issue
state: open
created: 2026-11-08T23:56:38Z
updated: 2026-11-08T23:56:38Z
author: gerchowl
author_url: https://github.com/gerchowl
url: https://github.com/vig-os/stepv/issues/144
comments: 0
labels: feature
assignees: none
milestone: MVP: graph core
projects: none
parent: none
children: none
synced: 2026-11-09T09:29:40.602Z
---

# [Issue 144]: [graph-core M2: Planar faces with holes](https://github.com/vig-os/stepv/issues/144)

Slice **M2** of ADR-0001 (`docs/adr/0001-graph-core.md` §6), milestone "MVP: graph core". The design is decided in the ADR; implement it as written, or report a design question.

## Scope
Plane inversion, `spade` CDT with the outer loop and holes, flood removal, exact normals, routing on `try_add_constraint` failure

## Acceptance
Area equals the topology's face area within 1e-6 relative on planar corpus faces; the 1,250-hole plate (ABC `00000046`) meshes, with its time and memory reported against OCCT's 7.7 s and 4.2 GB

## Test
`nix develop -c cargo test mesh::plane`

## Depends on
M1b (merge order per ADR §6).

## Process
- At most ~400 product lines; over that, split.
- MVP first (owner, 2026-11-08): hardening findings become `chore` issues, not fix rounds.
- Host rules v3 apply: one heavy job at a time; JOBS=2.

Refs: #115
