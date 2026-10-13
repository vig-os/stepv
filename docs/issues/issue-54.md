---
type: issue
state: open
created: 2026-10-11T23:51:59Z
updated: 2026-10-12T14:31:42Z
author: gerchowl
author_url: https://github.com/gerchowl
url: https://github.com/vig-os/stepv/issues/54
comments: 5
labels: feature, priority:medium
assignees: none
milestone: none
projects: none
parent: none
children: none
synced: 2026-10-13T09:11:42.491Z
---

# [Issue 54]: [Spike: progressive loading — progress, cancel, tree and boxes first, per-front-end fallbacks](https://github.com/vig-os/stepv/issues/54)

## Motivation

A slow file currently gets one of two outcomes: a finished model, or a failure. On the real 143 MB Ultimaker 3 assembly (#51):

| Front-end | Limit | What the user sees today |
| --- | --- | --- |
| Linux thumbnailer | 15 s | the generic icon; the host records a failure and never retries |
| CLI `--png` | 20 s | exit 4, `"timeout"`; timeouts aren't cached, so every attempt pays the full 20 s again |
| macOS Quick Look preview | **none** | the kernel runs in-process, and the C API (`kernel/stepv_occt.h`) has no deadline, cancel or progress, so it spins for about 100 s or until macOS kills the extension; the header info only shows once the kernel returns |
| `stepv view` | 120 s | opens after about 100 s, with no progress shown |

Nothing predicts the problem from file size or part count, and nothing offers "open in the viewer". plan.md §4 already promised "Progressive display: show the box plus header metadata immediately, swap in geometry when it arrives". This spike designs that, now that #51 shows where the time goes: reading the file takes about 5 s, translation about 1.3 s, and building p-curves about 90 s on one thread.

## Proposed approach (to be challenged)

1. **Progress and cancel in the kernel.** Pass a `Message_ProgressRange` to OCCT's `Transfer()` and to meshing. That gives a percentage for the CLI and `--serve`, and a way to cancel. In the C API it becomes a callback, which the in-process macOS path uses for a deadline and for cancelling when the user closes the preview.
2. **Stage-wise output.** After reading (about 5 s here), the kernel already knows the assembly tree, names, part count and colours. It emits them right away, then **each part as it finishes**: first its bounding box, then its mesh, rather than one mesh file at the end. `--serve` is the natural channel; the CLI can write stage records.
3. **Front-ends, each in its own way** (no egui needed outside the viewer):
   - **viewer (egui):** the tree, names and part boxes at about 5 s, then meshes as they arrive, a progress bar, and per-part healing badges using the face-status overlay mechanism;
   - **Quick Look preview:** progress, a hard deadline, header plus tree plus boxes as the fallback, and an "Open in stepv view" action for heavy files;
   - **thumbnail:** a deadline, plus a fallback image (a box render with the header, not the generic icon), and maybe a cheap size or part-count gate up front.
4. **Show what healing changed:** OCCT records each fix (`ShapeExtend` statuses, `XSAlgo` transfer info). Expose it per part in `--topology`, so the viewer can say "healed: N wires".

## What already exists

- `--serve` (newline-delimited JSON) for measurements and sections (`src/measure.rs`, `kernel/measure.cpp`).
- The per-face status contract, with the amber and red overlay (`FaceStatus`, `STEPVMSH` v2).
- `--info` header parsing; Quick Look already shows header lines on failure (`macos/Preview/PreviewViewController.swift`).
- `Scene { bbox, parts[] }` in `src/lib.rs`.

## Scope

- **P0:**
  - progress and cancel through the kernel, the CLI, `--serve` and the C API;
  - a Quick Look deadline;
  - the viewer shows the tree and boxes before meshes.
- **P1:**
  - per-part streaming of meshes;
  - the thumbnail fallback image;
  - "Open in stepv view".
- **P2:**
  - healing reports per part;
  - a size or part-count early-out heuristic.

## Pitfalls

- **Box-first needs geometry.** A part's box comes from its transferred shape, and transfer is where the time goes (#51). Boxes before transfer would need a cheaper pass, for example `CARTESIAN_POINT`s per product. That's unproven.
- **Progress granularity.** OCCT reports per root or per entity. A single huge part may sit at one percentage for a long time.
- **Streaming changes the mesh contract.** The `STEPVMSH` cache and the single-file output assume one complete scene. A partial scene must never be cached as complete.
- **Cancelling OCCT mid-transfer** has to leave the process usable, which matters most for in-process Quick Look. A cancelled in-process load is the riskiest path.
- **Timeouts aren't cached**, by design: "a busy machine is not a property of the file". With progressive results, decide what is cached after a deadline.

## Acceptance

- On `real-assembly`, the viewer shows the tree and names within about 6 s, and the meshes fill in as they finish.
- The macOS preview never runs past its deadline, and shows header plus tree plus "Open in stepv view".
- The thumbnailer shows a box-and-header image instead of the generic icon.
- Tests cover cancellation mid-transfer and the "partial results are never cached as complete" rule.

Refs: #51

---

# [Comment #1]() by [gerchowl]()

_Posted on October 12, 2026 at 12:29 AM_

## Review round 1 (three fresh reviewers: OCCT data exchange, macOS Quick Look, IPC and caching), consolidated

**Where they agree:**
- **Tree, names and colours come before any transfer.** After `ReadFile`, walk the `StepData_StepModel` graph (`ProductDefinition`, `NextAssemblyUsageOccurrence`, `Product::Name`, `StyledItem` → `Colour_Rgb`) with our own walker. `STEPCAFControl`'s readers only run after a transfer.
- **Boxes before transfer are approximate:** a hull of each product's `CARTESIAN_POINT`s, composed through the NAUO placements. Good enough as placeholders and for laying out the tree, not as truth.
- **Only complete scenes are cached.** Write `<key>.part`, `fsync`, then rename, and add `scene_version` plus a completeness tag. **Drop** the "resumable partial mesh cache" idea: deflection scales with the bbox, so it re-tessellates anyway. **Add** a separate header + tree + boxes artifact, keyed on (path, len, mtime). It feeds the thumbnail fallback and the macOS header path, and it makes a timed-out load useful next time.
- **No hard size gate for thumbnails** (#51 shows the cost isn't bytes). Use a deadline bias from `--info` and the part count instead.

**Where they disagree, which is the real decision:**
- The **OCCT** reviewer: cancelling in-process via `Message_ProgressRange` is workable if the reader is cleared (`ClearShapes()`) and the XCAF document thrown away.
- The **macOS** reviewer: in-process cancellation is a "loaded gun". OCCT's global state after an aborted transfer is untested, and 1.5 GB in a QL extension is on borrowed time. **Move the kernel into a bundled XPC service** that can be killed and restarted. The deadline and cancel become `xpc_connection_cancel()`.

**Pitfalls the reviewers added:**
- **Progress goes silent inside `ShapeFix_Wire`.** None of `FixEdgeCurves`, `FixAddPCurve` or `ApproxPCurve` takes a progress range, and `UserBreak()` is only checked at per-solid boundaries. #51 now shows a *single face* can take 28 s, so cancel latency can be that long. A per-face budget is needed regardless (#51).
- **Quick Look's watchdog** is reportedly about 30 s for a preview and 20 s for a thumbnail. These are reverse-engineered numbers, not documented. Calling `handler(nil)` once the header, tree and boxes are on screen (about 6 s) stops it, and SceneKit swaps after that are legal.
- **"Open in stepv view"**: through `extensionContext.open(url)` with an app URL scheme. `NSWorkspace.open` is blocked by the sandbox.
- **Streaming contract:** one `SceneEvent` enum (`Header`, `Tree`, `PartBox`, `PartMesh`, `Progress`, `Done`, `Error`). It has two transports: framed binary on stdout (magic, kind, u32 length, payload) and a C callback. Never base64 meshes into JSON lines, because the 16 MiB `MAX_LINE` breaks on one real part. Use bounded channels, coalesce `Progress`, never drop `PartMesh`, and treat a truncated frame as a crash.
- **Per-root streaming:** `TransferOneRoot` per product, with the XCAF name and colour resolution deferred to one final pass so instancing stays correct.

**Updated recommendation (P0):**
1. A tree and names walker after `ReadFile`, with point-hull boxes.
2. The `SceneEvent` contract, plus `Message_ProgressRange` for per-solid progress.
3. **A per-face healing budget** (#51), so one face can't stall a load, which also bounds cancel latency.
4. The header/tree/boxes cache artifact and atomic complete-scene caching.
5. The viewer and the CLI use it.

**macOS:** an XPC kernel, or in-process cancel. That's for the maintainer to decide.


---

# [Comment #2]() by [gerchowl]()

_Posted on October 12, 2026 at 12:49 AM_

## macOS spike: kernel in-process, or in an XPC service? (2026-10-12)

The two reviewers disagreed. This settles it with measurements, on sage (OCCT 7.9.3).

**Cancel safety, measured.** A test program (`cancel-exp/cancel-2026-10-12.cpp` in the session scratchpad) works in **one** process. Each round it starts the Ultimaker 3 load (143 MB) and cancels it at a random moment through `Message_ProgressIndicator::UserBreak()`, thrown away as the OCCT review advises (the XCAF document is closed). It then reloads two reference files (`tests/data/assembly.step` and NIST CTC 01 AP242) and compares faces, solids, names, colours, triangles and diagonal against a fresh-process baseline.

| | Result |
| --- | --- |
| Correctness | **12/12 rounds identical**, plus 4/4 with `MMGT_OPT=0`. The "loaded gun" didn't go off. |
| Cancel latency | **under 10 ms** when cancelled while reading or during early transfer. **0.6–4.7 s** typically during healing. **Worst 16.6 s** (15.0 s with `MMGT_OPT=0`), stuck inside a face where OCCT doesn't check for a break (#51). |
| Memory | **49 MB → about 800 MB after the first cancelled load, then a plateau around 940 MB** (950 MB with `MMGT_OPT=0`). It doesn't grow per round, so it's retained, not leaked, but it is never handed back to the system. |

**Quick Look itself** couldn't be exercised from this session. `qlmanage -t` hangs with no extension process even on the 28 KB test assembly, and plan.md says Quick Look was proven from Finder, so it's this session's environment. Still to do: one run from a logged-in Terminal, `qlmanage -t -s 512 -o /tmp ultimaker3.step`, plus Console filtered on `Stepv`.

**XPC feasibility** (Apple DTS on the developer forums, threads 703702, 112628 and 677665): an XPC service is visible **only to the bundle it's embedded in**. So it has to live inside **each** `.appex`, as `Contents/XPCServices/...`. That's one copy for the preview and one for the thumbnail. One copy shared through `stepv.app` would need a Service Management login item plus app-group entitlements, which is much heavier. Building `.xpc` bundles with swiftc and no Xcode is the same pattern `scripts/build-macos-app.sh` already uses for the `.appex`es.

**Verdict: an XPC service per extension.** The reason is **memory, not correctness**. In-process cancellation is correct, but every cancelled heavy load leaves the extension process about 0.9 GB fatter, and macOS kills extensions under memory pressure (jetsam). Killing an XPC service returns its memory. Two more reasons:
- The cancel latency (up to 17 s) is bounded only by #51's per-face budget. A killable process bounds it to roughly zero.
- The progress callback on the C API is still needed, across the XPC boundary.

**Order of work:**
1. P0 everywhere else: the tree walker, `SceneEvent`, the per-face budget, the caching.
2. Then macOS: a `KernelXPC` inside each appex that loads `libstepvocct`, with deadline and cancel done by `invalidate()` on the connection.
3. Call `handler(nil)` once the header and tree are on screen.
4. The thumbnail fallback PNG.
5. `extensionContext.open` for "Open in stepv view".


---

# [Comment #3]() by [gerchowl]()

_Posted on October 12, 2026 at 12:28 PM_

**Step 1 done: #58** (merged). `stepv-occt --outline`, `stepv_occt_outline` and `occt::outline` read the product tree and names with no shape transfer: Ultimaker 3 in 4.6 s against 98.6 s. The contract is `topology::products` of the full run's tree. It agrees on 388/390 local corpus files; the two others are multi-file assemblies, a documented gap. CI's `harness --strict --outline` now gates it (37/37 on the NIST corpus and `tests/data`). **Next:** the `SceneEvent` contract and progress.

---

# [Comment #4]() by [gerchowl]()

_Posted on October 12, 2026 at 01:09 PM_

**Step 2 done: #59** (merged as `662d7fd`). `stepv-occt --stream` sends a run's events as frames: the outline, then progress (transfer and mesh), then the summary. Rust consumes it through `occt::run_streaming`, and in-process callers through the C callback `stepv_occt_run_stream`. The contract is in `kernel/stepv_occt.h`, and plan.md has a section "Events and progress (#54, step 2)".

On Ultimaker 3 the tree arrives at 4.3 s of 101 s. Transfer progress then sits at 15.6% for 80 s: OCCT's indicator doesn't advance inside solid 221's p-curve projection. That's step 3, the per-face healing budget from #51.

Gaps the review left open, to cover as consumers arrive:
- no test calls the C callback directly; the Quick Look XPC step will;
- no memory-cap kill mid-stream;
- the mesh comparison against a plain run is counts only.


---

# [Comment #5]() by [gerchowl]()

_Posted on October 12, 2026 at 02:31 PM_

Step 3 (per-face healing budget, #51) is merged as #60: Ultimaker 3 now takes 42 s instead of 101 s, with transfer progress no longer stuck at 15.6%. Details are on #51. Next is step 4: complete-only atomic caching, plus a header/tree/boxes cache artifact.

