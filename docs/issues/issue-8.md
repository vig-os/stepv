---
type: issue
state: closed
created: 2026-10-04T14:47:17Z
updated: 2026-10-04T17:05:00Z
author: gerchowl
author_url: https://github.com/gerchowl
url: https://github.com/vig-os/stepv/issues/8
comments: 0
labels: docs
assignees: none
milestone: none
projects: none
parent: none
children: none
synced: 2026-10-05T08:45:53.034Z
---

# [Issue 8]: [LGPL NOTICE for the bundled OCCT before the first public binary](https://github.com/vig-os/stepv/issues/8)

## Description

Under Plan B, stepv ships **OCCT (LGPL-2.1)**: dynamically linked into the separate
`stepv-occt` executable and, on macOS, bundled inside the app (plan.md §3 "As built", §6).
Dynamic linking into a separate executable makes it replaceable, but the actual `NOTICE` text,
covering attribution, the licence text, where to get the OCCT source and how to replace the
libraries, has to be written **before the first public binary**, not after.

## Scope

- `NOTICE` (or `THIRD_PARTY.md`) covering OCCT 7.9.x and its own third-party components as built in
  nixpkgs.
- The replacement procedure on each platform (which dylibs/.so, where they live in the bundle).
- Include it in both the release artifacts and the macOS bundle.

Blocks S5 and S6.

