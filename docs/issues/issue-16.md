---
type: issue
state: open
created: 2026-10-04T14:50:47Z
updated: 2026-10-04T19:00:40Z
author: gerchowl
author_url: https://github.com/gerchowl
url: https://github.com/vig-os/stepv/issues/16
comments: 0
labels: chore, priority:high, needs-human
assignees: none
milestone: none
projects: none
parent: none
children: none
synced: 2026-10-05T08:45:50.305Z
---

# [Issue 16]: [Enable the dependency graph: Dependency Review fails on every PR](https://github.com/vig-os/stepv/issues/16)

## Description

CI's **Dependency Review** job fails on every PR (first seen on #15):

> Dependency review is not supported on this repository. Please ensure that Dependency graph is
> enabled, see https://github.com/vig-os/stepv/settings/security_analysis

stepv's dependency graph is **off**: `gh api repos/vig-os/stepv/dependency-graph/sbom` returns 404.
On the siblings it is on (`scitadel`: 497 packages, `tessera`: 831). The likely reason is that
stepv was created with `gh repo create` before its org-config declaration (plan.md §7), so it
never got the org's defaults. The pending org-config#317 apply does **not** change it; its plan
only touches secret scanning, vulnerability reporting and rulesets.

## Remediation (human: there is no REST API for this toggle)

1. Open https://github.com/vig-os/stepv/settings/security_analysis
2. Enable **Dependency graph**.
3. Re-run the Dependency Review job on the open PR.

Optionally, make org-config declare it so the next repo cannot drift the same way.

## Acceptance

`gh api repos/vig-os/stepv/dependency-graph/sbom` returns an SBOM, and Dependency Review passes.

