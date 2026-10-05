---
type: issue
state: open
created: 2026-10-04T22:19:54Z
updated: 2026-10-05T07:41:35Z
author: gerchowl
author_url: https://github.com/gerchowl
url: https://github.com/vig-os/stepv/issues/28
comments: 0
labels: feature, priority:high
assignees: none
milestone: none
projects: none
parent: none
children: none
synced: 2026-10-05T08:45:47.781Z
---

# [Issue 28]: [viewer: egui+wgpu scaffold for stepv view, with the minifb fallback and our own theme](https://github.com/vig-os/stepv/issues/28)

## Motivation
`stepv view` today is minifb plus the CPU rasteriser: it can orbit and nothing else. #27 chose egui 0.36 + wgpu for #21's viewer.

## Scope
- [ ] A new `src/view/` module behind the `viewer` feature (which drops minifb as its only window).
- [ ] Per-part SoA GPU buffers: positions, normals and indices; per-triangle `face_id`; per-face material/status in a storage buffer; per-part visibility as a bitset. No per-vertex colour (the #27 prototype's mistake).
- [ ] An offscreen colour and depth target the viewer owns, blitted into egui. Don't borrow eframe's MSAA/depth attachments.
- [ ] Repaint on input or a dirty flag only, not every frame.
- [ ] `--software`: the existing minifb viewer. `stepv view` falls back to it automatically when no wgpu adapter exists, and reports `"backend": "metal"|"vulkan"|"gl"|"software"` like `"sandbox"`.
- [ ] **Tripwire:** a CI step that fails if `libstepv_capi.a` (the Quick Look extensions) contains any eframe, winit, wgpu or naga symbol, plus a `cargo tree -p stepv-capi` deny-list.

- [ ] **Styling: our own `view::theme`, no component kit.** shadcn-style design tokens: a neutral palette with one accent, light and dark (following the OS), radius 6, a 4-px spacing scale, and a type scale. Applied through egui's `Style`/`Visuals`, so the theme upgrades with egui.
- [ ] **Icons:** `egui-phosphor` (Phosphor, Lucide-like; tracks egui 0.36).
- [ ] **A few in-house components on top:** toolbar icon button, panel section header, key/value property row (for the inspector), and a status pill (for `backend` and `sandbox`).

## Pitfalls
- No shadcn-style egui kit fits: armas and egui-shadcn are both pinned to egui 0.33, three minors behind 0.36.2. A kit would gate every egui upgrade on a one-person project, so the look comes from our own tokens instead.
- Feature unification, as with minifb: capi must stay `--no-default-features`.
- egui and wgpu churn: #27 hit three breaking renames. Pin versions; upgrades are deliberate.
- On a 4 GiB Linux builder, `ash` OOMs at default parallelism.
- Memory: the prototype held 2.1 GB RSS on the stress assembly. Drop the CPU copies once they are uploaded.

## Acceptance
- [ ] Light and dark themes both pass a visual check on macOS and Linux (screenshots in the PR). The tokens live in one module.
- [ ] `stepv view` opens the assembly and the stress assembly on macOS (Metal) and Linux (Vulkan/GL), and the stress assembly orbits at display rate on a GPU.
- [ ] With no adapter, it falls back to minifb and says so.
- [ ] The capi tripwire is green in CI.

_Follow-up of the #27 spike (verdict: egui + wgpu) for #21. Conditional on that verdict being accepted._

