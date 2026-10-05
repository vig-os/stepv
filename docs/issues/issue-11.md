---
type: issue
state: closed
created: 2026-10-04T14:47:21Z
updated: 2026-10-04T17:05:00Z
author: gerchowl
author_url: https://github.com/gerchowl
url: https://github.com/vig-os/stepv/issues/11
comments: 0
labels: chore
assignees: none
milestone: none
projects: none
parent: none
children: none
synced: 2026-10-05T08:45:51.941Z
---

# [Issue 11]: [CI: build the kernel and run the harness on the redistributable corpus](https://github.com/vig-os/stepv/issues/11)

## Description

`nix flake check` builds and tests the Rust crate (including the `STEPVMSH` decoder tests), but
**not `kernel/`**, and the harness needs a corpus that CI does not fetch. So nothing in CI
proves the kernel builds, let alone that it still passes the S1 gate. (Also: `nix flake check`
has not yet run on the S1 commits; sage's build governor held it, and the equivalent cargo checks
were run by hand.)

## Proposed solution

A CI job that:

1. builds `kernel/` (`just kernel`) against the flake's OCCT;
2. fetches only the **redistributable** sources (`just fixtures nist-pmi malformed`: US
   government work, plus locally generated files);
3. runs `just harness` and **fails on any `crash`, `timeout` or `bad-mesh` verdict**, or on a
   pass rate below the recorded one for those sources (NIST 100%, malformed 10/10 clean fail).

Linux and macOS runners, since both are shipping targets.

## Acceptance

A deliberately broken `kernel/stepv-occt.cpp` turns CI red.

