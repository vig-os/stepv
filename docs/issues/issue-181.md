---
type: issue
state: open
created: 2026-11-09T06:58:14Z
updated: 2026-11-09T06:58:14Z
author: gerchowl
author_url: https://github.com/gerchowl
url: https://github.com/vig-os/stepv/issues/181
comments: 0
labels: chore
assignees: none
milestone: none
projects: none
parent: none
children: none
synced: 2026-11-09T09:29:25.879Z
---

# [Issue 181]: [chore(view): placeholders walk leaf_path per leaf, O(leaves × fan-out) on flat assemblies](https://github.com/vig-os/stepv/issues/181)

Found in G6a (#138). `view::placeholder::Placeholders::of` calls `Bounds::leaf(leaf)` for every leaf, and each call walks `Structure::leaf_path` (O(depth × fan-out)). On a flat assembly with n leaves under one parent that is O(n²): fine on Ultimaker (523 leaves, 0.85 s to the first placeholder frame) and the stress assembly (120), but a 100k-part flat assembly would stall the first frame.

Fix direction: one `occurrences()` pass that places each product's box with the composed placement (the 8-corner rule), so the walk is O(occurrences). Needs a public 'place a product box' on `Bounds`.

Refs: #138, ADR-0001 §2.7
