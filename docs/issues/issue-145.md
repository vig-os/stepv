---
type: issue
state: open
created: 2026-11-08T23:56:40Z
updated: 2026-11-08T23:56:40Z
author: gerchowl
author_url: https://github.com/gerchowl
url: https://github.com/vig-os/stepv/issues/145
comments: 0
labels: feature
assignees: none
milestone: MVP: graph core
projects: none
parent: none
children: none
synced: 2026-11-09T09:29:40.135Z
---

# [Issue 145]: [graph-core M3: Cylinder, cone, sphere, torus](https://github.com/vig-os/stepv/issues/145)

Slice **M3** of ADR-0001 (`docs/adr/0001-graph-core.md` §6), milestone "MVP: graph core". The design is decided in the ADR; implement it as written, or report a design question.

## Scope
Closed-form (u, v) inversion after the on-surface check, u unwrapped on periodic surfaces, seams, poles, Steiner grid at the deflection step

## Acceptance
Fixture solids (full and partial cylinders, cones to the apex, spheres with poles, tori, a cylinder–cylinder intersection with B-spline edges) mesh watertight (every interior edge used by exactly two triangles); area within the deflection's bound of the topology's area

## Test
`nix develop -c cargo test mesh::analytic`

## Depends on
M2 (merge order per ADR §6).

## Process
- At most ~400 product lines; over that, split.
- MVP first (owner, 2026-11-08): hardening findings become `chore` issues, not fix rounds.
- Host rules v3 apply: one heavy job at a time; JOBS=2.

Refs: #115
