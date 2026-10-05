---
type: issue
state: closed
created: 2026-10-04T14:47:23Z
updated: 2026-10-04T17:05:01Z
author: gerchowl
author_url: https://github.com/gerchowl
url: https://github.com/vig-os/stepv/issues/13
comments: 0
labels: chore
assignees: none
milestone: none
projects: none
parent: none
children: none
synced: 2026-10-05T08:45:51.275Z
---

# [Issue 13]: [Add deny.toml without turning nix flake check red](https://github.com/vig-os/stepv/issues/13)

## Description

`mkRustProject` turns `cargo deny` on automatically once `deny.toml` exists. It was left out on
purpose (plan.md §7): the advisories check wants network access, which the nix build sandbox
does not have, so adding the file unchecked turns every `nix flake check` red.

## Proposed solution

Add `deny.toml` with licences (the dependency tree is small: `blake3`, `serde`, `serde_json`),
bans and sources. Check how devkit's `deny` tool handles advisories in the sandbox: an offline
advisory DB, or advisories run outside `nix flake check`. Confirm `nix flake check` stays green.

