---
type: issue
state: open
created: 2026-11-09T02:28:11Z
updated: 2026-11-09T02:28:11Z
author: gerchowl
author_url: https://github.com/gerchowl
url: https://github.com/vig-os/stepv/issues/157
comments: 0
labels: chore
assignees: none
milestone: none
projects: none
parent: none
children: none
synced: 2026-11-09T09:29:35.732Z
---

# [Issue 157]: [chore(graph): structure edge cases that differ from OCCT outside the corpus](https://github.com/vig-os/stepv/issues/157)

Found while implementing G3 (#135, `src/graph/structure.rs`). The graph's structure follows `kernel/outline.cpp` and OCCT's transfer; none of these cases occurs in the local corpus (390 files plus Ultimaker 3 all agree), so per the MVP-first ruling (2026-11-08) they are filed, not built.

1. **Depth-dependent "shown" memo.** `Walker::shown(pd, depth)` returns `true` past depth 60 without memoising, and memoises every other answer at the depth it first meets a product. A shapeless chain deeper than 60 below a product that is also met higher up can make the outline show (or hide) a shapeless product that `structure()` hides (or shows). `structure()` computes "shown" exactly and fails the expansion whenever a visited child's subtree reaches past the cap, which matches the outline whenever the memo is depth-independent.
2. **The placed product.** `STEPControl_ActorRead::TransferEntity(NAUO)` places the product whose SDR uses the CDSR's child representation, not the NAUO's related product definition. With several CDSRs on one NAUO it takes the first such product and the last transform. The graph uses the NAUO's child and the last CDSR.
3. **Contexts without units.** `PrepareUnits` keeps the previous context's factors when a context has no `GLOBAL_UNIT_ASSIGNED_CONTEXT`; the graph uses 1 (mm).
4. **Cycles.** A cycle anywhere refuses the whole structure (ADR §2.2), even where the outline never reaches it (an unreachable shapeless cycle).
5. **A complex `REPRESENTATION_RELATIONSHIP` + `…_WITH_TRANSFORMATION` without `SHAPE_REPRESENTATION_RELATIONSHIP`.** OCCT doesn't read it as an SRRWT and places at the identity; the graph applies the transformation.

Each needs a fixture checked against `occt::outline` / `--topology` before changing anything.

Refs: #135

