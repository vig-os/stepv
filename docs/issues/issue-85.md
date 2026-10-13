---
type: issue
state: closed
created: 2026-10-12T22:09:23Z
updated: 2026-10-13T01:38:50Z
author: gerchowl
author_url: https://github.com/gerchowl
url: https://github.com/vig-os/stepv/issues/85
comments: 0
labels: feature, priority:medium
assignees: none
milestone: none
projects: none
parent: none
children: none
synced: 2026-10-13T09:11:31.813Z
---

# [Issue 85]: [viewer: open on the outline — tree and progress first, meshes on Done (#63 part 2)](https://github.com/vig-os/stepv/issues/85)

Part 2 of #63, split off because of the size cap. Part 1 is #84: CLI progress on a TTY, and `view` writing the outline artifact.

## Scope (#63 acceptance items 1–3)
- [ ] **Tree first:** the viewer window opens before the run finishes. It shows the assembly tree from the streamed `Event::Tree` and a progress indicator from `Event::Progress`, then swaps in the scene on `Done`.
- [ ] **Cached:** with a cached outline artifact, the tree shows before any kernel event.
- [ ] **Killed or timed out:** the window keeps the tree and shows an honest message (what happened and why), never an empty window.

## Tests
These are window tests through the `STEPV_VIEW_SCREENSHOT` hook, with the `STEPV_OCCT_TEST_STALL_MS` stall hook. No existing window test uses that hook yet, so they start from scratch. Every test must bound its own window and kernel: a regression closes the window and fails, never hangs, and all memory stays capped (#83).

## Context
- The decision is in #54, step 5.
- The artifact rules are in plan.md ("#62"), with their limits in #76, #77 and #80.
- Per-part boxes don't exist yet (#66).

Closes #63 when merged, together with #84.

---
**Split (2026-10-13):** this issue now covers acceptance items 1, 2 and 4: the window opens first, the tree comes from `Event::Tree` or the cached artifact, progress shows, the scene swaps in on `Done`, and closing the window mid-load cancels the kernel. Item 3 (keep the tree on a killed or timed-out run) moved to **#89**, whose PR closes #63.

Decisions:
- **Closing the window mid-load:** it cancels the kernel through a cancel hook in `occt::supervise`, with status `cancelled` and exit 0. `run_streaming` gains a documented cancel.
- **The software viewer** waits for the load as before. This is a known limit, filed as **#90**.
