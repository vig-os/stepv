---
type: issue
state: open
created: 2026-11-09T10:04:56Z
updated: 2026-11-10T00:12:35Z
author: gerchowl
author_url: https://github.com/gerchowl
url: https://github.com/vig-os/stepv/issues/191
comments: 1
labels: chore
assignees: none
milestone: Hardening: graph core
projects: none
parent: none
children: none
synced: 2026-11-10T09:31:58.250Z
---

# [Issue 191]: [chore(view): G6b follow-ups (same-count tree mismatch, untopologised scene, CLI graph cost)](https://github.com/vig-os/stepv/issues/191)

Found while doing G6b (#139). Under MVP-first these are filed, not built.

1. **A tree mismatch with the same leaf count isn't detected.** `Placeholders::replace` detects a mismatch by leaf count only (ADR-0001 §2.4, the G6b notes). Two trees with the same count but another order (an external file swapped for one with as many leaves) join boxes to the wrong leaves. The same holds for `OutlineArtifact::boxes`. A structural check (the leaf names along `leaf_path` against the kernel tree's) would catch it.
2. **A scene without a topology tree drops the boxes uncounted.** `GraphState::scene(None)` keeps G6a's rule, "the scene replaces every box", so leaves without geometry and a mismatch go unsaid in the status line then.
3. **`parts_per_leaf` counts a part as delivered even when its mesh is empty** (0 triangles), so such a leaf is neither replaced visibly nor counted as without geometry.
4. **CLI cost:** a cached CLI run of a STEP file (`--png`, `--glb`, thumbnails through the CLI) now reads the graph on a thread beside the kernel, for the artifact's boxes. On Ultimaker that's about 1.3 s of CPU and the graph's heap (~100–200 MB) during the run. Nobody has measured it against §2.7's budgets yet. Skip it when an artifact with boxes for this file already exists.

Refs: #139
---

# [Comment #1]() by [gerchowl]()

_Posted on November 9, 2026 at 10:49 AM_

These nits are from the architect's fresh review of PR #192 (G6b) at e3034d7:

1. `tests/cache.rs:676-686`: the `multifile-nested` case passes only because one leaf is `Unknown`. The leaf-count filter at `src/main.rs:~823` (`.filter(|b| b.len() == a.leaves)`) is never exercised: deleting it keeps every test green.
2. `src/view/placeholder.rs:47-58`: when the scene arrives first (a mesh-cache hit), the status has no mismatch suffix. A warm and a cold `view` of `multifile-nested` give different status lines.
3. `src/main.rs:293-304`: the CLI loads the graph beside the kernel with no memory cap, and `tessellate` blocks on `rx.recv()` until it finishes.
4. `src/view/app.rs:948`: the render call's placeholders argument is now always `None`. Remove it.

