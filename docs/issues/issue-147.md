---
type: issue
state: open
created: 2026-11-08T23:56:43Z
updated: 2026-11-08T23:56:43Z
author: gerchowl
author_url: https://github.com/gerchowl
url: https://github.com/vig-os/stepv/issues/147
comments: 0
labels: feature
assignees: none
milestone: MVP: graph core
projects: none
parent: none
children: none
synced: 2026-11-09T09:29:39.203Z
---

# [Issue 147]: [graph-core M4b: The combined view](https://github.com/vig-os/stepv/issues/147)

Slice **M4b** of ADR-0001 (`docs/adr/0001-graph-core.md` §6), milestone "MVP: graph core". The design is decided in the ADR; implement it as written, or report a design question.

## Scope
The viewer and the CLI combining Rust products with OCCT's routed ones (OCCT still runs on the whole file), the viewer's ledger status line, the cache key's `mesh::VERSION`, an entry only for a complete composed scene

## Acceptance
Headless viewer test: Rust products replace their placeholders before `Done`, OCCT's routed ones at `Done`, and nothing is drawn twice; changing `mesh::VERSION` misses the cache; a killed OCCT run caches nothing

## Test
`nix develop -c cargo test --test view` and `nix develop -c cargo test --test cache`

## Depends on
M4a, G7, G6b (merge order per ADR §6).

## Process
- At most ~400 product lines; over that, split.
- MVP first (owner, 2026-11-08): hardening findings become `chore` issues, not fix rounds.
- Host rules v3 apply: one heavy job at a time; JOBS=2.

Refs: #115
