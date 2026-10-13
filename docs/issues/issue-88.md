---
type: issue
state: open
created: 2026-10-12T23:17:10Z
updated: 2026-10-12T23:17:10Z
author: gerchowl
author_url: https://github.com/gerchowl
url: https://github.com/vig-os/stepv/issues/88
comments: 0
labels: feature, priority:low
assignees: none
milestone: none
projects: none
parent: none
children: none
synced: 2026-10-13T09:11:30.227Z
---

# [Issue 88]: [spike: terminal (TUI) renderer with quality tiers — kitty/sixel → half-block → braille → ASCII](https://github.com/vig-os/stepv/issues/88)

## Motivation
`stepv view` needs a window session. On headless hosts (anvil or ksb over SSH), in CI, and in the nix sandbox (#84's CI failure) there's no way to look at a model. The kernel already streams everything a terminal front-end needs: the tree first, then progress, then the mesh (STEPVMSH), from #59, #62 and #63. The owner asked for a terminal renderer with quality tiers.

## Proposal: tiered terminal rendering
| Tier | Technique | Quality | Terminals |
|---|---|---|---|
| Pixel | kitty graphics protocol, Sixel, iTerm2 inline images: render off-screen at the real pixel size | close to the GUI | kitty, WezTerm, iTerm2, foot, xterm with Sixel |
| Half-block | `▀` with truecolor foreground and background, 2 px per cell, shaded | a good silhouette, about 200×100 px | any truecolor terminal, **including Alacritty** (which has no Sixel or kitty graphics) |
| Braille | 2×4 dots per cell | edges or wireframe | any Unicode terminal |
| ASCII | luminance ramp | a rough preview | anywhere, including logs and CI output |

- **Renderer:**
  - a small CPU rasterizer (z-buffer, Lambert shading) for the character tiers, which redraws in milliseconds at terminal resolution;
  - the pixel tier reuses the wgpu viewer off-screen (it already runs headless on lavapipe in CI).
- **UI:** ratatui, with the tree pane plus the render pane plus progress, reusing the outline-first flow (#63, #85) and the cached outline artifact (#62). Orbit, zoom and part selection from keys or the mouse.
- **Detection:** `ratatui-image` (kitty, Sixel, iTerm2, half-block), with an explicit override (`--tier`).

## Spike questions (decide before implementation)
1. **The entry point:** `stepv view --tui`, `stepv tui`, or `view` choosing the TUI when there's no window session. The last only as an explicit, documented mode, never a silent fallback (see #84's session check).
2. **Pixel tier:** wgpu off-screen, or the CPU rasterizer at full pixel size? Measure on Ultimaker 3 (523 parts).
3. **Tier detection:** how reliable it is across terminals and through tmux and SSH, and how the override works.
4. **Snapshots:** is a deterministic ASCII or half-block snapshot usable as a visual regression check in CI and PR reports?
5. **Crate choice:** ratatui-image vs viuer vs our own encoder; licences under cargo-deny.

## Pitfalls
- Alacritty, the owner's terminal, tops out at half-block. Design that tier as first-class, not an afterthought.
- Large assemblies: cull or decimate per frame so that a redraw stays interactive.
- Memory: the TUI process obeys the same caps as the viewer (#83).
- Don't duplicate the outline and artifact logic from #63 and #85. Share it.

## Acceptance (spike)
- A prototype renders a fixture in each tier.
- Numbers for redraw time and memory, on a small fixture and on Ultimaker 3.
- A decision comment answering questions 1–5, then an implementation issue.

Refs: #63, #85, #84

