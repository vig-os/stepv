---
type: issue
state: open
created: 2026-10-12T16:23:35Z
updated: 2026-10-12T16:23:35Z
author: gerchowl
author_url: https://github.com/gerchowl
url: https://github.com/vig-os/stepv/issues/73
comments: 0
labels: feature, priority:medium
assignees: none
milestone: none
projects: none
parent: none
children: none
synced: 2026-10-13T09:11:36.385Z
---

# [Issue 73]: [kernel: heal after the transfer, per distinct solid, in parallel, with XCAF re-binding (#51)](https://github.com/vig-os/stepv/issues/73)

## Motivation

#51 / #65. Healing inside the STEP transfer is serial: 33.6 s on Ultimaker 3. Healing after the transfer, per distinct solid on `min(8, cores)` threads with stepv's budgeted FixShape, took **about 11 s**, bounded below by solid 221. Faces, edges, unmeshed faces and triangles were the same as serial in all 56 runs (#65). Past about 8 threads, each solid gets slower (ReShape and `TopLoc` allocation contention, plus the `Standard_ErrorHandler` mutex).

## Proposed design (from #65; a **pre-mortem is required before implementation**)

1. **Transfer with a no-op `FixShape`** (stepv registers it, as #60 does), so the transfer is unhealed (about 1.3 s).
2. **Units.**
   - **Grouping:** distinct solids (by TShape), and shells and faces outside solids, partitioned by shared edges and shared `Geom_Surface`s (union-find), so no two units share anything ShapeFix writes.
   - **Order:** largest first, on `min(8, cores)` threads.
   - **What runs:** each unit runs the same budgeted operator body as today's `heal.cpp`, with its own `ShapeProcess_ShapeContext` and history.
3. **Re-binding XCAF.** Healing changes identity: 18,845 of 34,840 distinct faces on Ultimaker get a new TShape. Today OCCT carries that change into the transfer binders (`XSAlgo_ShapeProcessor::MergeShapeTransferInfo`) before STEPCAFControl reads colours and names. After the transfer, stepv must apply the same history to the document, in one serial pass:
   - `XCAFDoc_ShapeTool::SetShape(label, healed)` for top-level and prototype labels, then `UpdateAssemblies()`;
   - each sub-shape label (face, shell and solid colours; GD&T targets) is mapped through the history, with OCCT's semantics:
     - 1:1: replaced;
     - split: one sub-shape label per piece, with the attributes copied;
     - removed: the label is dropped;
     - unhealed (budget): kept, as #60 does.
4. **Face numbering.** Numbers for the `STEPV_OCCT_UNHEALED` replay come from each face's position in the input (a unit's input index plus its offset), never from completion order, so `--serve` replays stay exact.
5. **Progress.** A new stream stage, `heal`, between `transfer` and `mesh` (#59 contract: stages in order, monotonic, first and last never dropped).
6. **IGES.** Same path if IGES's default flags include FixShape; check, and keep in-transfer healing for any format where it isn't measured.

## Risks the pre-mortem must probe

- **ShapeFix thread safety on disjoint units** is not promised by OCCT. Run under TSan.
- **XCAF re-binding fidelity:** names, colours, layers and GD&T, including split faces.
- **Memory:** histories for every unit at once.
- **Cancellation and timeout** mid-heal across threads.
- **Interaction with #60's replay and the recovery ladder.**

## Acceptance

- The 401-file corpus diff against `main` shows **zero** differences, including `parts_named`, `parts_colored` and `parts_face_colored`, plus a per-face colour histogram per file.
- Ultimaker ≤ 20 s end to end through the real kernel (with the parallel mesh, the separate issue), measured 3 times. Record it in plan.md.
- TSan is clean on `tests/data` and a corpus sample.
- The `--serve` replay test from #60 still passes. A new test shows replay numbering is unchanged across thread counts.
- A stream test shows the `heal` stage.

Refs: #51, #65

