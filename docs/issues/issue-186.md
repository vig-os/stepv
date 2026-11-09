---
type: issue
state: open
created: 2026-11-09T07:28:23Z
updated: 2026-11-09T07:58:46Z
author: gerchowl
author_url: https://github.com/gerchowl
url: https://github.com/vig-os/stepv/issues/186
comments: 1
labels: chore
assignees: none
milestone: MVP: graph core
projects: none
parent: none
children: none
synced: 2026-11-09T09:29:24.040Z
---

# [Issue 186]: [chore(view): G6a review nits (leaf-count check, no-mesh leaves, test ordering, ≤1.5 s assertion)](https://github.com/vig-os/stepv/issues/186)

These are nits from the fresh review of PR #185 (G6a, #138) at 83fbf59. Under MVP-first they are deferred as a chore.

1. `src/view/placeholder.rs:58-64` `replace`: nothing checks that `delivered.len()` equals `self.leaves`.
   - If the kernel's leaf count differs from the graph's, the numbering may be shifted and the join is wrong, and nothing says so.
   - A leaf past the kernel's tree keeps its box silently.
   - Count these leaves or report them in the status line.
2. After `Done`, a leaf with no mesh keeps its placeholder for the rest of the session. The final view can't tell "no geometry" from "pending", and the status line doesn't count these leaves.
3. Tests:
   - The order "placeholders before Done" is asserted only in `tests/cache.rs` `the_viewer_draws_placeholders_before_the_geometry`, which is `#[ignore]` and needs a window.
   - `tests/view.rs` `the_first_frame_has_placeholders_before_done` renders without a scene.
   - The ≤ 1.5 s of ADR §2.7 is a hand measurement, not an assertion.
4. There's no `missing_docs` lint, so undocumented public items in `src/view` aren't caught. If round 2 doesn't add the rustdoc, it lands here.

Refs: #138
---

# [Comment #1]() by [gerchowl]()

_Posted on November 9, 2026 at 07:58 AM_

These nits are from the re-review of round 2, at 3165ac4, merged as 62dea0e.

5. `src/view/app.rs:1264-1270`, `:1531`: a graph that arrives after Done, in the Ready stage, only shows its status on the next repaint, because the Ready viewer only repaints on input. On a mesh-cache hit, an idle window can go without the status line until the mouse moves. The status is delayed, not lost. Possible fix: `request_repaint_after` until the graph has arrived.
6. `src/view/placeholder.rs:181-241`: the both-orders test drives `GraphState`, but not the routing in `Window::take`. Moving the `Load::Graph` arm back below the stage guard would still pass it.
7. `src/view/gpu.rs:940`: the rustdoc on `Lines` ("Sketch and construction curves") disagrees with `:76` ("sketch and outline lines").
8. `app.rs:1095`: in screenshot mode the screenshot doesn't wait for the graph, so on a cache hit the PNG can be missing the status line. stderr still has it.

