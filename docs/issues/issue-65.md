---
type: issue
state: closed
created: 2026-10-12T14:56:45Z
updated: 2026-10-12T16:23:47Z
author: gerchowl
author_url: https://github.com/gerchowl
url: https://github.com/vig-os/stepv/issues/65
comments: 2
labels: chore, priority:medium
assignees: none
milestone: none
projects: none
parent: none
children: none
synced: 2026-10-13T09:11:39.435Z
---

# [Issue 65]: [Spike: the rest of #51 — self-intersection fix, per-solid parallel healing, allocator](https://github.com/vig-os/stepv/issues/65)

## Motivation

#51. Ultimaker 3 takes 42 s after #60; the target is the CLI's 20 s. What remains in the transfer (34 s):
- OCCT's wire self-intersection fix, which walks a linked sequence by index (`NCollection_BaseSequence::Find` under `ShapeExtend_WireData::Edge`, quadratic; about 40% of samples);
- ordinary projection;
- about 49 s of system time in allocation.

## Question to answer (a measurement, not a product change)

1. **The self-intersection fix.** Run Ultimaker and the 401-file corpus with `FixSelfIntersectionMode`, `FixIntersectingEdgesMode` and `FixNonAdjacentIntersectingEdgesMode` at NotFix:
   - the time saved;
   - the corpus diff against main (any change in faces, triangles or statuses);
   - edges per wire on the hot wires.
2. **Parallel healing per distinct solid.** In a scratch program: transfer with healing off, then heal the distinct solids on 1, 4, 16 and 32 threads, with and without `MMGT_OPT=0`.
   - wall time and CPU %;
   - whether the result equals serial healing.
3. **The allocation cost:** `MMGT_OPT=0` against the default on a plain run.

## Output

A comment on #51 with the numbers and a recommended design: which of
- skipping or budgeting the self-intersection fix,
- per-solid parallel healing (and how colours and names keep mapping through XCAF),
- an upstream OCCT fix,

gets Ultimaker under 20 s, at what risk. That becomes an ADR or a decided issue before any implementation.

## Acceptance

The comment exists, with commands and numbers; the scratch programs live outside the repo.

Refs: #51

---

# [Comment #1]() by [gerchowl]()

_Posted on October 12, 2026 at 04:22 PM_

## Spike results: the three levers, with numbers

Host: sage (32 cores). Kernel `c2c5d1c` (origin/main, #60). Every row is 3 runs, interleaved, shown as median (min–max), with the 1-min load at the start of each run. Another delegate was building throughout, so the load is 6–23. Scratch programs and raw outputs are in `~/.local/state/architect/stepv/scratch/65/` (`out/*.tsv`), outside the repo. Ultimaker = `9066.STEP`.

**Short version.** Ultimaker can get under 20 s with the 2 s face budget kept: heal out of the transfer per distinct solid on 4–8 threads, and mesh prototypes in parallel, one per thread. That measures **19.0 s**, or **17.5–18.4 s** (after the first launch) with the self-intersection cost removed. Each combination below gives the same faces, edges, unmeshed faces and triangles as today's in-transfer healing. `MMGT_OPT` is not a lever: this OCCT build ignores it. Also, the "49 s of sys time" is not allocation in the transfer. It comes from BRepMesh's own parallel loop.

### 1. The self-intersection fixes off

Scratch kernel: `kernel/heal.cpp`'s `configure()` with `FixSelfIntersectionMode`, `FixIntersectingEdgesMode` and `FixNonAdjacentIntersectingEdgesMode` forced to 0 (`scratch/65/noselfint/`). Base = origin/main's kernel copied to `scratch/65/base/`.

```
scratch/65/bench.sh base#i 1 base/stepv-occt 9066.STEP          # interleaved with
scratch/65/bench.sh noselfint#i 1 noselfint/stepv-occt 9066.STEP
python3 bin/corpus-diff.py base/stepv-occt noselfint/stepv-occt ~/Projects/stepv/tests/fixtures
```

| Ultimaker | load | wall s | transfer ms | faces | unmeshed | triangles | over budget |
|---|---|---|---|---|---|---|---|
| base | 9–16 | 43.1 (42.8–43.2) | 34106 (34062–34299) | 62011 | 6 | 1242533 | 4 |
| self-int off | 10–18 | 34.9 (34.6–35.3) | 26126 (26046–26420) | 62011 | 6 | 1242533 | 4 |

The transfer is 8.0 s faster, and the Ultimaker output is identical.

**Corpus: 14 of 401 files differ, in triangle counts only.** No differences in faces, unmeshed faces, statuses or exit codes. The differences are deterministic: the base run twice gives identical counts.

| file | triangles base → off |
|---|---|
| abc 00000013_…_000 | 9623 → 9653 |
| abc 00000044_…_007 | 254306 → 254308 |
| abc 00000085_…_000 | 77069 → 77061 |
| abc 00000148/192/203/209 (same model ×4) | 69756 → 69744 |
| abc 00000149/193/204/210/228 (same model ×5) | 69787 → 69781 |
| abc 00000197_…_000 | 31971 → 31980 |
| abc 00000227_…_001 | 69286 → 69289 |

The geometry the fix produces really does change in these files. Transfer savings elsewhere are large: abc 00000046 7.4 → 2.3 s, 00000048 7.2 → 2.1 s, 00000236 4.1 → 2.2 s, rc-x60 14.1 → 11.9 s.

**Edges per wire.** Program `prog/par.cpp`, `par 9066.STEP wires`: a serial heal, then `ShapeFix_Wire::FixSelfIntersection()` and `ShapeFix_IntersectionTool::FixSelfIntersectWire()` timed on each of the 37,360 wires. Edges per wire: ≤4: 30132 · ≤16: 6726 · ≤64: 405 · ≤256: 95 · ≤1024: 0 · >1024: **2**.

| rank | ms (whole fix) | of which non-adjacent | edges | solid |
|---|---|---|---|---|
| 1 | 3153 | 3126 | **2485** | 207 (2492 faces) |
| 2 | 3015 | 3018 | **2485** | 207 |
| 3–10 | 3.2–4.9 | 0–2.9 | 2–230 | various |

Two wires account for 6.2 of the 8.1 s that the fix costs over the whole file. The cause is in `FixSelfIntersectWire` (OCCT 7.9.3 `ShapeFix_IntersectionTool.cxx`, pair loop): it fetches `sewd->Edge(num1)` **and** `sewd->Edge(num2)` inside the inner loop. That is an `NCollection_Sequence` walked by index, whose position cache is lost on every alternation, so the loop is O(n³). Timing only that fetch pattern on the 2485-edge wire gives **2617 ms as written, 19 ms with `edge1` fetched once per outer iteration** (`par-next3 … wires`). The upstream fix is to hoist `edge1` (or copy the edges to an array). It changes no results.

### 2. Healing per distinct solid, in parallel

`prog/par.cpp`: the STEP reader runs with a no-op `FixShape`, so the transfer is unhealed. The program then heals each distinct solid (by TShape; shells and faces outside solids are their own units, 665 in all) with **stepv's own budgeted FixShape operator**. That is `kernel/heal.cpp` copied, plus a `heal_unit()` entry point, run with the reader's parameters (`DE_ShapeFixParameters` defaults, precision 1e-5, max 1). Work is handed out largest first. The mesh is the kernel's preview (0.001 × diagonal, 20°). Counts are over distinct shapes. `intransfer` is the same program with healing in the transfer, as the kernel does it.

```
prog/par 9066.STEP intransfer
prog/par 9066.STEP par <1|4|8|16|32>                     # ± MMGT_OPT=0
MESH_THREADS=8 prog/par-next 9066.STEP par <4|8>          # mesh units 1 per thread, BRepMesh InParallel off
SPIKE_NOSELFINT=1 …                                       # + lever 1
STEPV_OCCT_FACE_BUDGET_MS=500 …                           # budget probe
```

| run | load | wall s | user s | sys s | transfer ms | heal ms | heal CPU % | slowest solid ms | Σ solid ms |
|---|---|---|---|---|---|---|---|---|---|
| in transfer | 12–21 | 43.6 (43.6–44.4) | 52 | 51 | 34459 | — | — | — | — |
| par 1 | 9–14 | 42.9 (42.8–43.0) | 51 | 51 | 1285 | 32295 | 100 | 9665 | 32295 |
| par 1, MMGT_OPT=0 | 9–11 | 42.7 (42.5–42.7) | 51 | 50 | 1264 | 32374 | 100 | 9684 | 32374 |
| par 4 | 11–15 | 21.8 (21.5–22.3) | 52 | 50 | 1262 | 11538 | 286 | 9574 | 32931 |
| par 4, MMGT_OPT=0 | 9–15 | 21.8 (21.1–22.2) | 52 | 51 | 1280 | 11400 | 284 | 9543 | 32760 |
| par 8 | 7–15 | 21.7 (21.4–22.2) | 59 | 51 | 1313 | 11008 | 374 | 9928 | 41595 |
| par 16 | 9–12 | 21.0 (20.4–21.6) | 126 | 51 | 1300 | 10581 | 1024 | 10580 | 109039 |
| par 16, MMGT_OPT=0 | 8–12 | 21.0 (20.4–21.6) | 126 | 52 | 1263 | 10577 | 1022 | 10576 | 108681 |
| par 32 | 11–18 | 23.0 (22.6–23.7) | 240 | 54 | 1260 | 12545 | 1767 | 12545 | 250607 |
| par 32, MMGT_OPT=0 | 13–22 | 23.0 (22.6–23.4) | 242 | 54 | 1276 | 12570 | 1799 | 12570 | 254794 |
| par 4 + mesh8 | 9–11 | 19.4 (19.4–19.6) | 46 | 3 | 1322 | 11477 | 285 | 9476 | 32609 |
| **par 8 + mesh8** | 8–10 | **19.0 (18.7–19.0)** | 54 | 4 | 1289 | 10921 | 380 | 9854 | 41486 |
| **par 8 + mesh8 + self-int off** | 7–11 | **18.3 (17.5–24.8)**¹ | 45 | 4 | 1270 | 10550 | 319 | 9596 | 34346 |
| par 4 + mesh8 + self-int off | 8–9 | 19.2 (18.9–19.3) | 38 | 1 | 1300 | 10861 | 235 | 9275 | 25773 |
| par 8 + mesh8 + budget 500 | 7–9 | 15.1 (14.9–15.6) | 47 | 4 | 1298 | 7119 | 497 | 7119 | 35149 |
| par 8 + mesh8 + budget 500 + self-int off | 8–9 | 12.8 (12.1–12.9) | 40 | 4 | 1280 | 4777 | 589 | 3825 | 28740 |

¹ 5 runs. The first took 24.8 s: it was the binary's first launch. Two repeats gave 17.5 and 17.7 s. The other columns are from the first 3 runs.

**Equal to serial: yes, in every run (56 out-of-transfer runs).** Each run, at every thread count, with or without the allocator setting, mesh mode or self-int setting, gives faces 34840, edges 96970, unmeshed 6, triangles 731653 and 4 faces over budget, the same as the in-transfer heal. (The budget-500 runs also keep 4 over budget on Ultimaker; see below for the corpus.)

What the table shows:
- **4–8 threads is the knee.** Healing is then bounded by one solid. **Solid 221** (397 faces) takes 9.4–9.7 s serially, and about 8 s of that is its 4 over-budget ribbon faces, each spending the full 2 s budget before it is given up. **Solid 207** (2492 faces, the two 2485-edge wires) is next at 6.5 s, and 0.6 s with self-int off.
- **More threads make each solid slower.** The summed per-solid time grows 3× at 16 threads and 8× at 32. A `sample` of `par 32` shows 32 workers mostly in `ShapeBuild_ReShape::Apply` → `BRep_TEdge::EmptyCopy` → `calloc`, plus `TopLoc_Location`. The global `Standard_ErrorHandler` mutex (`OCC_CATCH_SIGNALS` in `ShapeAnalysis_Surface::ValueOfUV`, `ShapeFix_Edge::FixSameParameter`) accounts for about 5% of worker samples. 4 threads show no inflation (Σ ≈ serial).
- **The 50 s of sys time is the mesher.** The kernel meshes one prototype at a time with BRepMesh's `InParallel`. That costs 3.8 s wall and about 50 s sys, against 5.2 s for a single thread with 0.1 s sys. Meshing whole prototypes one per thread on 8 threads (InParallel off) takes **1.24 s with 2 s sys**, and gives the same triangles. With healing in the transfer and only the mesh changed (`intransfer-mesh8`): 41.2 s wall, 3 s sys.
- **A 500 ms budget** brings solid 221 down to 3.8 s. On the corpus, though (`corpus-diff.py base budget500`), **8 of 401 files then lose 1–2 faces to outlines**: abc 00000011/12/94/205/207 −2 faces each, 00000041 −2, 00000225 −1, 00000234 −1. The faces become missing: 360–2,500 triangles per file become 53–140 outline segments. That is a fidelity loss, so I don't recommend it.

### 3. The allocator alone (`MMGT_OPT=0` against the default, origin/main's kernel)

```
scratch/65/bench.sh ultimaker-default#i 1 base/stepv-occt 9066.STEP
scratch/65/bench.sh ultimaker-MMGT_OPT=0#i 1 env MMGT_OPT=0 base/stepv-occt 9066.STEP   # + 3 corpus files
```

| run | load | wall s | sys s | read ms | transfer ms | mesh ms |
|---|---|---|---|---|---|---|
| Ultimaker default | 6–11 | 42.1 (41.2–44.2) | 52 | 4405 | 33646 (32930–35746) | 3690 |
| Ultimaker MMGT_OPT=0 | 7–10 | 43.7 (43.3–43.8) | 49 | 4407 | 34767 (34504–34779) | 4004 |
| rc-x60 default | 6–7 | 25.3 (24.4–25.7) | 49 | 7479 | 13644 | 3735 |
| rc-x60 MMGT_OPT=0 | 6–7 | 23.2 (22.3–23.7) | 49 | 5078 | 14196 | 3751 |
| abc 00000011 default | 8–9 | 8.5 (8.4–8.5) | 0 | 165 | 8203 | 50 |
| abc 00000011 MMGT_OPT=0 | 7–8 | 8.4 (8.4–8.4) | 0 | 168 | 8166 | 49 |
| abc 00000046 default | 8–9 | 23.2 (22.4–23.6) | 6 | 128 | 7359 | 15514 |
| abc 00000046 MMGT_OPT=0 | 8–11 | 23.4 (22.3–23.6) | 7 | 131 | 7329 | 15697 |

No effect, and none is possible. nixpkgs' OCCT 7.9.3 is built with the **native** allocator: `Standard::Allocate` calls `calloc` directly (checked with `otool -tV libTKernel.7.9.dylib`). No OCCT library contains the string `MMGT_OPT`, so nothing reads it. The earlier "`Standard_MMgrOpt` lock" diagnosis was wrong. rc-x60's faster read under `env MMGT_OPT=0` is reproduced by `env DUMMY=0` (4.6–4.8 s against 6.9–8.1 s without `env`), so it is an effect of how the process is launched, unrelated to the allocator. I didn't investigate it further.

### Recommendation

To get Ultimaker under 20 s with no fidelity loss, combine these three:

1. **Fix `FixSelfIntersectWire` upstream, not by skipping it.** Hoist `edge1` out of the inner loop (or index an array). Carry it as a patch on OCCT 7.9.3 in stepv's flake, and send it upstream. The results are identical by construction. The full self-int cost is 8 s; the measured access-pattern cost on the two hot wires is 2.6 s each, so about 5 s of the 8 s goes. Turning the fixes off instead changes 14 corpus files (triangles) and skips repairs that exist for a reason. The end-to-end gain of the patched build is **not yet measured**; the "self-int off" rows are its upper bound.
2. **Heal after the transfer, per distinct solid, on min(8, cores) threads,** with stepv's budgeted FixShape as it is today. That takes healing from 33.6 s to about 11 s (floor: solid 221). Do not use more than about 8 threads: past that, each solid gets slower.
3. **Mesh prototypes in parallel, one per thread, with BRepMesh `InParallel` off:** 3.8 → 1.2 s, and sys 50 → 2 s.

Measured together in the scratch program: **19.0 s** with 2+3, **17.5–18.4 s** with self-int off standing in for 1. The rest of the time is read 4.4 s (fixed), transfer 1.3 s, heal about 10.5 s (solid 221) and mesh 1.2 s. The margin is thin, and the kernel adds its recovery ladder and output writing to what the scratch program measures, so the decided design should also schedule **headroom**. The next lever is solid 221's 8 s of budget: run the projections of one solid's faces in parallel, so its 4 ribbon faces spend their budgets at the same time (about 2 s instead of 8). Don't lower the budget: 500 ms loses faces in 8 of 401 files.

**How XCAF names and colours keep mapping.** Healing does change identity: on Ultimaker, **18,845 of 34,840 distinct faces get a new TShape** (only 16k keep theirs, because p-curves are added in place). Today OCCT carries that change into the transfer binders (`XSAlgo_ShapeProcessor::MergeShapeTransferInfo`), and only then does STEPCAFControl read colours and names. Healing after the transfer reverses that order, so stepv has to apply the same history to the document itself:
- Each unit's `ShapeProcess_ShapeContext::Map()` (original sub-shape → result) is the same map OCCT merges into the binders. The budgeted operator already patches it for unhealed faces (#60).
- Name, colour and layer attributes live on labels. Healing never moves them; only the shapes the labels hold change.
- After all units are healed, one serial pass re-binds the shapes. Top-level and prototype labels get `XCAFDoc_ShapeTool::SetShape(label, healed)`, then `UpdateAssemblies()` rebuilds the assembly compounds. Each sub-shape label (face, shell and solid colours, which `face_colors()` reads, and the GD&T targets) is mapped through the history, with OCCT's semantics: 1:1 → replaced; split → the attribute applies to every piece (one sub-shape label per piece, attributes copied); removed → dropped, as the binders would do.
- Acceptance would be a corpus diff with zero differences in `parts_named`, `parts_colored` and `parts_face_colored`, plus a per-face colour histogram, against in-transfer healing. Ultimaker has no per-face colours (`parts_face_colored` 0), so the corpus files carry that test.

**Risks to settle in the ADR:**
- (a) **Thread safety.** OCCT doesn't promise that ShapeFix runs safely on distinct solids in parallel. Results matched in all 56 runs, but distinct solids that share a `Geom_Surface` or an edge are legal STEP. Units must be partitioned by shared edges (union-find), and the corpus should run under TSan.
- (b) **Face numbering.** `heal.cpp` numbers faces in fix order from one global counter, so parallel order would break the `STEPV_OCCT_UNHEALED` replay contract for `--serve`. Numbering must come from the unit's index in the input, not from completion order.
- (c) **Progress.** Healing leaves the transfer's progress range and needs its own stage in the #59 stream.
- (d) **IGES.** This was measured on STEP only (its default flags are just FixShape). IGES needs the same check.

The front-ends don't need a fallback message: each combination above (2+3, with or without 1) is under 20 s on Ultimaker. If the kernel's own measurement after implementing them lands above 20 s, the over-budget faces are already drawn as outlines and streamed (#59/#60), so the front-ends would show the streamed outline with a healing progress stage until the mesh arrives, not an error.


---

# [Comment #2]() by [gerchowl]()

_Posted on October 12, 2026 at 04:23 PM_

The spike's done; the results are in the comment above. Follow-ups:
- #72: mesh prototypes in parallel (decided).
- #73: heal after the transfer, per solid, in parallel, with XCAF re-binding (a design proposal; it gets a pre-mortem first).
- #74: an OCCT patch for the O(n³) `FixSelfIntersectWire` (needs an owner decision).

The front-ends need no fallback message: #72 plus #73 measured 19.0 s on Ultimaker in the scratch program.

