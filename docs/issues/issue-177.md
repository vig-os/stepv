---
type: issue
state: open
created: 2026-11-09T05:26:16Z
updated: 2026-11-09T05:26:16Z
author: gerchowl
author_url: https://github.com/gerchowl
url: https://github.com/vig-os/stepv/issues/177
comments: 0
labels: chore, priority:low
assignees: none
milestone: MVP: graph core
projects: none
parent: none
children: none
synced: 2026-11-09T09:29:27.412Z
---

# [Issue 177]: [graph: G4 boxes miss §2.7's structure + boxes target on Ultimaker (0.76 s vs 0.5 s)](https://github.com/vig-os/stepv/issues/177)

Found in G4 (#136), ADR-0001 §2.7.

## Measured (sage, release, warm)
- Ultimaker 3 (9066.STEP): `structure()` 0.04 s + `bounds()` 0.72 s = **0.76 s** against the ≤ 0.5 s target. The stress assembly: 0.25 s against ≤ 0.75 s (met).

## Profile (`sample`, ~10k samples of `bounds()`)
- Argument parsing (`args::Parser::value`/`list`) about 61%:
  - B-spline control points about 26%;
  - face and edge arguments about 16%;
  - conic frames about 8%.
- malloc/free about 8%, the memo's hashing about 3%.
- Memoising surfaces and curves shared by several faces is already in G4. It saves little.

## Cause
The post-order walk reaches about 1.5 M entities in DAG order, which is random order in the 143 MB map. Parsing Ultimaker's 519k `CARTESIAN_POINT`s takes 0.11 s in file order and 0.31 s shuffled. The walk is bound by memory latency, not by repeated work.

## Options
- Per the owner, parallelism goes into the Rust path: run `rayon` over products (or over the representations of a post-order level), each with its own memo, merged per level.
- Alternatively, a file-order pass that boxes vertices, edges and B-spline poles first, then a cheap composition.

Don't change the target.

Refs: #136
