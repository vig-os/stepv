---
type: issue
state: open
created: 2026-11-09T06:04:08Z
updated: 2026-11-09T06:04:08Z
author: gerchowl
author_url: https://github.com/gerchowl
url: https://github.com/vig-os/stepv/issues/179
comments: 0
labels: chore, priority:low
assignees: none
milestone: none
projects: none
parent: none
children: none
synced: 2026-11-09T09:29:26.867Z
---

# [Issue 179]: [graph: G4 bounds review nits (Empty vs Unknown child, per-type mm tests, expect)](https://github.com/vig-os/stepv/issues/179)

Review nits from #178 (G4 bounds), deferred under MVP-first:

- **A child with no geometry reports one thing and is treated as another.**
  - `src/graph/bounds.rs:124-125`: a child with no B-rep geometry adds nothing to its parent, yet `product(child)` reports `Unknown`.
  - Callers can see that this breaks "any Unknown child makes the parent Unknown".
  - For multi-file external parts, the model box leaves out geometry the kernel shows (#92).
  - Decide on a distinct `Empty` vs `Unknown` and document it in §2.4.
- **mm-scaling unit test covers points only.** Radii, semi-axes, torus, sphere and offset are scaled correctly by reading the code (`* s`), but no test covers them. Add an inch fixture per type.
- **`bounds.rs:90` uses `expect`** on `leaf_path`. Believed unreachable; replace it with an error or document the invariant.

Refs: #136

