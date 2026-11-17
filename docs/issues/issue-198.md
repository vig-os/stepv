---
type: issue
state: open
created: 2026-11-16T13:54:41Z
updated: 2026-11-16T13:54:42Z
author: gerchowl
author_url: https://github.com/gerchowl
url: https://github.com/vig-os/stepv/issues/198
comments: 0
labels: chore
assignees: none
milestone: Hardening: graph core
projects: none
parent: none
children: none
synced: 2026-11-17T10:27:39.513Z
---

# [Issue 198]: [graph colours: SURFACE_STYLE_RENDERING colours](https://github.com/vig-os/stepv/issues/198)

From G7 (#141). XCAF colours a part from a `SURFACE_STYLE_RENDERING`'s `surface_colour` (observed on a fixture); the graph reads only `SURFACE_STYLE_FILL_AREA` → `FILL_AREA_STYLE_COLOUR`. None of the corpus uses it, and `tests/graph_colors.rs::a_colour_the_graph_misses_fails_the_gate` proves the gate fails on it rather than defaulting.

Done when: the graph reads rendering colours (and `SURFACE_STYLE_RENDERING_WITH_PROPERTIES`) with XCAF's precedence against a fill-area colour in the same side style, with fixtures, and the gate test moves to another uncovered style.
