---
type: issue
state: open
created: 2026-10-04T22:20:00Z
updated: 2026-10-04T22:20:00Z
author: gerchowl
author_url: https://github.com/gerchowl
url: https://github.com/vig-os/stepv/issues/32
comments: 0
labels: feature, priority:medium
assignees: none
milestone: none
projects: none
parent: none
children: none
synced: 2026-10-05T08:45:45.209Z
---

# [Issue 32]: [viewer: headless CI smoke on lavapipe, and packaging the GPU stack](https://github.com/vig-os/stepv/issues/32)

## Motivation
#27 showed that wgpu runs without a GPU (Debian + Xvfb + Mesa lavapipe: about 6 fps on the stress assembly), but it adds 188 crates and 12–20 MB to the binary.

## Scope
- [ ] A CI job on the Kernel workflow (Linux): Xvfb + lavapipe runs `stepv view --frames 30` (a bench mode that exits) on the fixtures and asserts the reported backend.
- [ ] A second job with the GPU libraries removed asserts the minifb fallback.
- [ ] A binary-size budget (≤ 16 MB macOS, ≤ 24 MB Linux, stripped), and `cargo deny` over the new tree.
- [ ] Linux tarball: wgpu's `libvulkan.so.1` and `libGL.so.1` come from the host. Confirm the launcher's ours-then-host library order and document it.
- [ ] nix: `stepv view` in the product package; check the closure size.

## Acceptance
- [ ] Both CI jobs are green; the size budget is enforced.

_Follow-up of the #27 spike (verdict: egui + wgpu) for #21. Conditional on that verdict being accepted._

