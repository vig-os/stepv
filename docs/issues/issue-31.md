---
type: issue
state: closed
created: 2026-10-04T22:19:58Z
updated: 2026-10-05T18:43:37Z
author: gerchowl
author_url: https://github.com/gerchowl
url: https://github.com/vig-os/stepv/issues/31
comments: 1
labels: feature, priority:high
assignees: none
milestone: none
projects: none
parent: none
children: none
synced: 2026-10-06T08:48:26.394Z
---

# [Issue 31]: [viewer: broken-face overlay in WGSL, and model edges](https://github.com/vig-os/stepv/issues/31)

## Motivation
Thumbnails show approximated faces (amber stripes) and missing faces (red outline) honestly (`src/render.rs`). The viewer must not hide what the thumbnail shows. CAD views also need edges.

## Scope
- [x] The FaceStatus overlay in the fragment shader, with the same rules as `render.rs` and one source of truth for the colours.
- [x] Edges: B-rep edge polylines from the kernel. Today only missing-face outlines and sketches exist as segments. They go in **STEPVMSH v4** per-part edge polylines, with an edge id per segment that indexes the topology's edges. Drawn as a depth-biased LineList; fat lines come later.
- [x] Edge picking, using the id buffer with edges drawn into it, so the inspector can show an edge's length and a circle's radius.

## Pitfalls
- STEPVMSH v4 must stay readable by Quick Look (capi/Swift), or Quick Look keeps v3 behind a flag.
- Edge counts on big models: the stress assembly has about 100k edges.

## Acceptance
- [x] The viewer and `stepv --png` agree on which faces are flagged.
- [x] Clicking the bracket's hole edge shows a circle with r = 4 and length 8π.

_Follow-up of the #27 spike (verdict: egui + wgpu) for #21. Conditional on that verdict being accepted._


---

# [Comment #1]() by [gerchowl]()

_Posted on October 5, 2026 at 06:43 PM_

Done in #40, merged to main.

