---
type: issue
state: open
created: 2026-10-11T14:41:43Z
updated: 2026-10-12T00:28:52Z
author: gerchowl
author_url: https://github.com/gerchowl
url: https://github.com/vig-os/stepv/issues/51
comments: 3
labels: none
assignees: none
milestone: none
projects: none
parent: none
children: none
synced: 2026-10-12T08:55:38.845Z
---

# [Issue 51]: [Real 143 MB assembly: STEP transfer takes 92 s (Ultimaker 3, SolidWorks)](https://github.com/vig-os/stepv/issues/51)

## Problem

The first **real** large assembly in the corpus (#12) loads correctly but takes **100.7 s**, and **92.2 s of that is STEP transfer**. The file is `real-assembly/9066.STEP`: the whole Ultimaker 3 printer, 143 MB, written by SolidWorks 2016 (AP214), with 523 parts, all named and coloured. The CLI's default timeout is 20 s and the `.thumbnailer` uses 15 s, so **a user can't preview this file at all**. In the harness it times out at the default 60 s.

The synthetic 221 MB `stress-assembly` takes 23 s in total. Bytes aren't the problem; something in this file's structure is.

Measured on sage (Mac Studio, arm64), with `just harness --timeout 900 tests/fixtures/real-assembly`, release build, OCCT 7.9.3:

| Phase | ms |
| --- | ---: |
| read | 4,423 |
| **transfer** | **92,247** |
| mesh | 3,694 |
| extract | 48 |
| **wall** | **100,710** |

Peak RSS is 1.56 GB. There are 62,011 faces, of which 2 are degenerate, and 1.25M triangles.

## What to find out first (hypotheses, not findings)

- **Bisect the transfer** by turning off XCAF's colour, name and layer modes one at a time on `STEPCAFControl_Reader`. The file has about 8,000 `COLOUR_RGB` entities and 258 products in 523 occurrences, and style transfer is the usual suspect.
- **Profile one run** (Instruments or `perf`) and name the hot OCCT function before changing anything.
- **Check whether shared sub-assemblies are transferred once per occurrence** rather than once per product.

## Acceptance

- The hot spot is named, with a profile.
- `real-assembly` loads within the CLI's 20 s default, or plan.md §5 records why it can't and what the front-ends show instead, for example bbox-first or a header-only fallback.
- Nothing is lost: the name and colour coverage stays at 100% / 100%.

Refs: #12

---

# [Comment #1]() by [gerchowl]()

_Posted on October 11, 2026 at 11:00 PM_

## Profile and first experiments (2026-10-12, sage)

`sample` of the kernel during transfer, 25 s at 1 ms. **One thread** was busy.

- **About two-thirds of the samples (16,914) are OCCT's transfer-time healing**, not STEP translation: `STEPControl_ActorRead::TransferEntity → XSAlgo_ShapeProcessor::ProcessShape → ShapeProcess::Perform → ShapeFix_Shape → ShapeFix_Solid → ShapeFix_Shell → ShapeFix_Face → ShapeFix_Wire`. It runs on every transferred shape and walks down to every wire. There are two hot spots:
  - `ShapeFix_Wire::FixSelfIntersection → ShapeFix_IntersectionTool::FixSelfIntersectWire → ShapeExtend_WireData::Edge(i) → NCollection_BaseSequence::Find(int)` (4,865 samples). That is index access into a linked sequence inside a loop, which is quadratic in the edges per wire.
  - `ShapeConstruct_ProjectCurveOnSurface::ApproxPCurve → ShapeAnalysis_Surface::ValueOfUV → Extrema_GenExtPS::BuildGrid` (about 5,000 samples): rebuilding 2D p-curves by projecting points onto surfaces.
- The actual STEP-to-topology translation (`StepToTopoDS_*`) is under 1% of the samples.
- `time` reports **64 s of system time** in a 99 s run, along with `memset` and malloc in the profile. Allocation churn is worth its own look.

**Switching the healing off did not work.** A standalone `STEPCAFControl_Reader` test program on OCCT 7.9.3, with colour, name and layer modes on as the kernel has them, gave:

| Attempt | transfer ms | faces | unmeshed |
| --- | ---: | ---: | ---: |
| default | 93,343 | 62,011 | 2 |
| `ChangeReader().SetShapeProcessFlags({})` | 89,473 | 62,011 | 2 |
| `ChangeReader().SetShapeProcessFlags({DirectFaces})` | 94,276 | 62,011 | 2 |
| `STEPCAFControl_Reader::SetShapeProcessFlags({DirectFaces})` | 88,473 | 62,011 | 2 |
| `Interface_Static "read.step.sequence" = "NoSuchSequence"` | 89,144 | 62,011 | 2 |

A profile of the third switch's run still shows `ShapeProcess::Perform → ShapeFix_Shape`, so on this path 7.9.3 ignores all three. **Next:**
1. Read the OCCT 7.9.3 source of `STEPControl_ActorRead` / `XSAlgo_ShapeProcessor` to find where the operation sequence actually comes from.
2. With healing really off, run the whole corpus. The kernel already heals per face, on demand: ladder rung 2, `ShapeFix` on only the faces that fail to mesh. So the blanket pass may be mostly redundant. The risk is the things healing fixes that still mesh but come out wrong (tolerances, orientation), so the faithful percentage and a visual check decide, not the timing alone.
3. If some healing must stay, heal each part on its own thread after a healing-free transfer. Parts are independent, so this is plain C++ threads in the kernel.

`Message_ProgressRange` on `Transfer()` also gives progress reporting and cancellation, which the in-process macOS path lacks today: the C API has no deadline or cancel.


---

# [Comment #2]() by [gerchowl]()

_Posted on October 11, 2026 at 11:51 PM_

## Found the switch; the cost is p-curves (2026-10-12)

**Why the earlier attempts did nothing:** the flags live on the work session's actor, and `ReadFile` resets them. `XSControl_Reader::InitializeMissingParameters` then puts back the default, which for STEP is `{FixShape}` (`STEPControl_Reader::GetDefaultShapeProcessFlags`). Calling `STEPCAFControl_Reader::SetShapeProcessFlags(...)` **after** `ReadFile` does take effect.

| Ultimaker 3 (`9066.STEP`) | transfer | mesh | unmeshed faces |
| --- | ---: | ---: | ---: |
| default (`FixShape`) | 93.3 s | 5.6 s | 2 |
| **no healing** (empty flags, set after `ReadFile`) | **1.3 s** | — | **62,011 (all)** |
| `FixShape`, `FixSelfIntersectionMode` and `FixIntersectingWiresMode` = `NotFix` | 93.2 s | 5.6 s | 2 |
| no healing, then `ShapeFix_Edge::FixAddPCurve` per distinct solid on 32 threads | 1.7 s + **119.8 s** | 5.7 s | 1,694 |

Everything else is 62,011 faces and 689 solids, of which 331 are distinct.

**Why the time goes where it does:**
- **The file has no p-curves.** It has no `PCURVE`, `SURFACE_CURVE` or `SEAM_CURVE` entities. Without healing, **143,319 of 333,846 edge uses have no 2D curve on their face**, on 33,630 of the 62,011 faces, and BRepMesh meshes nothing. So the healing is necessary on this file; what costs time is *building p-curves by projection*.
- STEP translation itself takes 1.3 s for 62k faces. Reading takes about 4.6 s.
- Turning the self-intersection fixes off saved nothing measurable, even though they showed up hot in the profile. Either the parameter map doesn't reach this path the same way, or the time is inside the p-curve work. **Not settled.**

**The naive parallel attempt failed:** it ran at 191% CPU on 32 threads, with 83 s of system time, and left 1,694 faces unmeshed. Likely causes, none checked yet:
- allocator or OCCT-internal lock contention;
- one dominant solid;
- `FixAddPCurve` alone being less than what `ShapeFix_Wire` does (it also fixes seams, degenerate edges and the like).

**What to try next:**
1. Per-solid timing, to rule out one dominant part.
2. `ShapeFix_Wire::FixEdgeCurves` per face instead of a bare `FixAddPCurve`.
3. Separate *processes* per part, to remove allocator contention entirely, if threads don't scale.
4. Lazy p-curves: build them only for the faces being drawn at the current level of detail.

The test program is in the session scratchpad (`heal-exp/heal-2026-10-12.cpp`).


---

# [Comment #3]() by [gerchowl]()

_Posted on October 12, 2026 at 12:28 AM_

## It's one solid, and in it four faces (2026-10-12)

Per-solid timing of p-curve building, on one thread, after a healing-free transfer:

- **Solid 221, with 397 faces, takes 116.6 s of the 128.2 s total.** The next largest is 1.7 s. **Parallelism can't fix this**: the run is bounded by its slowest part. That's also why 32 threads ran at 191% CPU, although the allocator lock (`Standard_MMgrOpt`) is a second reason (review on #54).
- **Inside it, four faces take about 28 s each (111 s in all).** Each is a `Geom_BSplineSurface` with **2 × 1,079 poles, degree 1 × 2, and 540 knot spans**: a long ruled ribbon, probably a belt or cable sweep. Each has one wire of 75–77 B-spline edges. About 370 ms goes on each edge: `ShapeConstruct_ProjectCurveOnSurface` → `Extrema_GenExtPS::BuildGrid`, a grid search over the whole surface for each projected point. Other faces on surfaces of the same size take about 1.2 s.
- The p-curve pass peaks at about 4.9 GB RSS (`/usr/bin/time -l`).

**What that means for the fix:**
1. **A per-face budget beats parallelism.** Build p-curves lazily, per face, and give each face a time budget. Past the budget, the face is drawn as `Approx` (the UV-grid rung that already exists) or `Missing`, and the rest of the model shows. One pathological face must never hold up 62,000 good ones.
2. **The projection itself:** for a degree-1 × N ruled surface, project per knot span from a seeded guess (`Extrema_GenExtPS` with a sub-domain, or `ShapeAnalysis_Surface::NextValueOfUV` with a good start) instead of the global grid. That's an upstream-worthy OCCT issue if it reproduces in isolation.
3. Parallel per distinct solid still helps everything else, with `MMGT_OPT=0` and `ShapeFix_Wire::FixEdgeCurves` per face (#54 review). It's secondary.

The other 330 solids together take about 11.6 s on one thread, which is about 1 s on 16 cores if they scale.


