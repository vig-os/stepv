---
type: issue
state: open
created: 2026-10-12T20:54:49Z
updated: 2026-10-12T20:54:49Z
author: gerchowl
author_url: https://github.com/gerchowl
url: https://github.com/vig-os/stepv/issues/82
comments: 0
labels: bug
assignees: none
milestone: none
projects: none
parent: none
children: none
synced: 2026-10-13T09:11:32.822Z
---

# [Issue 82]: [test: kernel-dependent tests return early and pass when no kernel is available](https://github.com/vig-os/stepv/issues/82)

Found in the review of #78 (at de19529).

`tests/cli.rs:665` has `if !kernel_available() { return; }`. When no kernel is found, the test returns and reports a pass without testing anything. That conflicts with the project's no-skipped-tests rule: in a misconfigured environment, the kernel tests turn silently green.

Fix: when the kernel is missing, fail with its path. This is the same pattern as `STEPV_OCCT_LIB` in #69: CI and the nix checks must always have a kernel. If a test really is optional, use `#[ignore]` with a reason. Check every caller of `kernel_available()`.

A smaller related point from the same review: #80's window description leaves out the JSON serialisation inside `store_outline`. That is negligible, but add it there for completeness.

Refs: #62
