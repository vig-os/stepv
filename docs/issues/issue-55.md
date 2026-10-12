---
type: issue
state: open
created: 2026-10-12T00:18:25Z
updated: 2026-10-12T00:18:25Z
author: gerchowl
author_url: https://github.com/gerchowl
url: https://github.com/vig-os/stepv/issues/55
comments: 0
labels: feature, priority:low
assignees: none
milestone: none
projects: none
parent: none
children: none
synced: 2026-10-12T08:55:37.920Z
---

# [Issue 55]: [Viewer: assembly totals and an exact/mesh label on measured values](https://github.com/vig-os/stepv/issues/55)

## Motivation

An outside comment on #21 (https://github.com/vig-os/stepv/issues/21#issuecomment-6058325131) asked for measurement provenance. Most of it is already handled:
- instance transforms when mapping a pick to its prototype (`src/view/inspect.rs`, `tests/measure.rs` pins at 28 mm);
- witness segments for distances (#33);
- mesh-based stand-ins kept apart from exact results: the Section panel says which cap is shown (#43).

Two gaps remain:

1. **No assembly totals.** The inspector shows per-part values only: "Part volume", "Part size" and "Part min" (`src/view/inspect.rs` `part()`). Nothing shows the whole model's volume, surface area or overall box, or how many instances a total counts. Area is computed per prototype (`--topology`) but isn't shown for a part either.
2. **Provenance isn't visible.** Every inspector and measure number is exact, from the B-rep, but the panel doesn't say so. When a mesh-derived value is ever shown, nothing would tell the two apart.

## Proposal

- **Assembly row** (the tree's root selected, or a "Model" section):
  - total volume and total area, summed over placed parts with each instance counted and scaled by its placement;
  - the overall box, made from every part's placed box;
  - the part and instance counts, for example "523 parts (258 distinct)".
- **Part rows** gain "Part area".
- **Provenance:** a quiet "exact (B-rep)" tag on inspector and measure values. Anything computed from the mesh gets "≈ mesh". Today that's only the stencil section cap, which already says so.

## Pitfalls

- **Instances:** a total must count each placement, not each prototype. Scaling uses the placement's scale (`Placement::scale()`, as `part()` does).
- **Volume is only defined for solids.** A model with sheets or faceless parts needs "volume of N solids; M parts have none", not a silently low total.
- **Overlapping parts** (interference) make the summed volume more than the enclosed volume. Say "sum of parts", never "volume".
- **Units** come from `topo.units`, as the rows already use.

## Acceptance

- A unit test in `inspect.rs` checks the totals for the test assembly: two instances of one pin count twice, and scaling is applied.
- "Part area" is shown.
- Values carry an exact or mesh tag, and the stencil cap's mesh status is still labelled.
- plan.md "Measurements" is updated.

Refs: #21

