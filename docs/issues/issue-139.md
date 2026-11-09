---
type: issue
state: open
created: 2026-11-08T23:56:31Z
updated: 2026-11-08T23:56:31Z
author: gerchowl
author_url: https://github.com/gerchowl
url: https://github.com/vig-os/stepv/issues/139
comments: 0
labels: feature
assignees: none
milestone: MVP: graph core
projects: none
parent: none
children: none
synced: 2026-11-09T09:29:42.778Z
---

# [Issue 139]: [graph-core G6b: Meshes replace placeholders](https://github.com/vig-os/stepv/issues/139)

Slice **G6b** of ADR-0001 (`docs/adr/0001-graph-core.md` §6), milestone "MVP: graph core". The design is decided in the ADR; implement it as written, or report a design question.

## Scope
Per-leaf replacement as meshes arrive, the tree-mismatch rule (the kernel's tree wins, unmapped placeholders dropped and counted), `OutlineArtifact::boxes` filled when every leaf has a box

## Acceptance
Headless test: after `Done` no placeholder remains on a matching tree; a multi-file fixture shows the mismatch rule and its status line; the artifact's `boxes` length equals `leaves`, and it's absent when any box is `Unknown`

## Test
`nix develop -c cargo test --test view` and `nix develop -c cargo test --test cache`

## Depends on
G6a (merge order per ADR §6).

## Process
- At most ~400 product lines; over that, split.
- MVP first (owner, 2026-11-08): hardening findings become `chore` issues, not fix rounds.
- Host rules v3 apply: one heavy job at a time; JOBS=2.

Refs: #115
