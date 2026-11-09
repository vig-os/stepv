---
type: issue
state: open
created: 2026-11-08T23:56:34Z
updated: 2026-11-08T23:56:34Z
author: gerchowl
author_url: https://github.com/gerchowl
url: https://github.com/vig-os/stepv/issues/141
comments: 0
labels: feature
assignees: none
milestone: MVP: graph core
projects: none
parent: none
children: none
synced: 2026-11-09T09:29:41.862Z
---

# [Issue 141]: [graph-core G7: Colours from the graph](https://github.com/vig-os/stepv/issues/141)

Slice **G7** of ADR-0001 (`docs/adr/0001-graph-core.md` §6), milestone "MVP: graph core". The design is decided in the ADR; implement it as written, or report a design question.

## Scope
The style chain → product, solid and face colours. **Cases covered:** `STYLED_ITEM` on a `MANIFOLD_SOLID_BREP`/`BREP_WITH_VOIDS`/shell (solid colour), on an `ADVANCED_FACE` (face override), on a shape representation (part colour); `OVER_RIDING_STYLED_ITEM`; `COLOUR_RGB` and `DRAUGHTING_PRE_DEFINED_COLOUR`; `SURFACE_STYLE_USAGE` with `.BOTH.`/`.POSITIVE.` sides through `SURFACE_SIDE_STYLE` → `SURFACE_STYLE_FILL_AREA` → `FILL_AREA_STYLE_COLOUR`; styles held in a `MECHANICAL_DESIGN_GEOMETRIC_PRESENTATION_REPRESENTATION`

## Acceptance
On the corpus, the graph's per-product and per-face colours equal the kernel's XCAF colours for every leaf; one committed fixture per covered case; a case XCAF colours that the graph doesn't is a test failure, never a silent default

## Test
`nix develop -c cargo test --test graph_colors`

## Depends on
G3 (merge order per ADR §6).

## Process
- At most ~400 product lines; over that, split.
- MVP first (owner, 2026-11-08): hardening findings become `chore` issues, not fix rounds.
- Host rules v3 apply: one heavy job at a time; JOBS=2.

Refs: #115
