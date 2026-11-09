---
type: issue
state: open
created: 2026-11-08T20:05:31Z
updated: 2026-11-08T20:05:31Z
author: gerchowl
author_url: https://github.com/gerchowl
url: https://github.com/vig-os/stepv/issues/125
comments: 0
labels: none
assignees: none
milestone: none
projects: none
parent: none
children: none
synced: 2026-11-09T09:29:48.246Z
---

# [Issue 125]: [Adopt nextest wrapper scripts for the per-test memory cap once they are stable](https://github.com/vig-os/stepv/issues/125)

Follow-up from #83 (83a). The plan for 83a (ii): each test caps its own kernel launches through a `capped(label, …)` helper (`stepv-cap --label <test>`), and one aggregate `stepv-cap` wraps the whole test run as a backstop. nextest's **wrapper scripts** (`[scripts.wrapper.*]`, `run-wrapper`) would let `.config/nextest.toml` wrap each test process in `stepv-cap` instead, so every test gets a cap on its whole process tree, test code included.

They are still experimental (`experimental = ["wrapper-scripts"]`, added in 0.9.98, tracked upstream in nextest-rs/nextest#2384), so 83a doesn't depend on them.

- Docs: https://nexte.st/docs/configuration/wrapper-scripts/
- When they are stable: wrap each test in `stepv-cap` from `.config/nextest.toml`, using the same wrapper-path helper that `just test` and the nix check use, and keep the `capped()` labels for the kernel launches.

Refs: #83
