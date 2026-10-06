---
type: issue
state: closed
created: 2026-10-04T22:20:00Z
updated: 2026-10-05T18:43:40Z
author: gerchowl
author_url: https://github.com/gerchowl
url: https://github.com/vig-os/stepv/issues/32
comments: 2
labels: feature, priority:medium
assignees: none
milestone: none
projects: none
parent: none
children: none
synced: 2026-10-06T08:48:25.546Z
---

# [Issue 32]: [viewer: headless CI smoke on lavapipe, and packaging the GPU stack](https://github.com/vig-os/stepv/issues/32)

## Motivation
#27 showed that wgpu runs without a GPU (Debian + Xvfb + Mesa lavapipe: about 6 fps on the stress assembly), but it adds 188 crates and 12–20 MB to the binary.

## Scope
- [x] A CI job on the Kernel workflow (Linux): Xvfb + lavapipe runs `stepv view --frames 30` (a bench mode that exits) on the fixtures and asserts the reported backend.
- [x] A second job with the GPU libraries removed asserts the minifb fallback.
- [x] A binary-size budget (≤ 16 MB macOS, ≤ 24 MB Linux, stripped), and `cargo deny` over the new tree.
- [x] Linux tarball: wgpu's `libvulkan.so.1` and `libGL.so.1` come from the host. Confirm the launcher's ours-then-host library order and document it.
- [x] nix: `stepv view` in the product package; check the closure size.

## Acceptance
- [x] Both CI jobs are green; the size budget is enforced.

_Follow-up of the #27 spike (verdict: egui + wgpu) for #21. Conditional on that verdict being accepted._




---

# [Comment #1]() by [gerchowl]()

_Posted on October 5, 2026 at 01:55 PM_

From #28 (PR #35), to carry into this issue:
- **Panels aren't tested on the macOS CI runner.** Its windows are never presented, so `STEPV_VIEW_SCREENSHOT` falls back to the viewport's render there, and egui's panels go untested (`scripts/test-viewer.sh` prints which path it took). Xvfb does present, so the Linux smoke job here should **require** the full-window path: fail on 'viewport render'.
- **A local Linux loop already exists:** `just linux-view-test` (`scripts/test-linux-viewer.sh`) runs Debian trixie with mesa-vulkan-drivers (lavapipe), xvfb and the X11 libs, with `CARGO_BUILD_JOBS=2` and `STEPV_REQUIRE_GPU=1`. It passes on Vulkan/llvmpipe, including both fallbacks (`WGPU_BACKEND=dx12` and `STEPV_VIEW_REJECT_ADAPTERS`). It's the template for the CI job.

---

# [Comment #2]() by [gerchowl]()

_Posted on October 5, 2026 at 06:43 PM_

Done in #41, merged to main.

