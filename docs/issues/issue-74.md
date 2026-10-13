---
type: issue
state: open
created: 2026-10-12T16:23:37Z
updated: 2026-10-12T19:47:51Z
author: gerchowl
author_url: https://github.com/gerchowl
url: https://github.com/vig-os/stepv/issues/74
comments: 1
labels: feature, priority:medium, needs-human
assignees: none
milestone: none
projects: none
parent: none
children: none
synced: 2026-10-13T09:11:35.924Z
---

# [Issue 74]: [OCCT patch: FixSelfIntersectWire is O(n³) in edges per wire (#51)](https://github.com/vig-os/stepv/issues/74)

## Motivation

#51 / #65. OCCT 7.9.3's `ShapeFix_IntersectionTool::FixSelfIntersectWire` fetches `sewd->Edge(num1)` and `sewd->Edge(num2)` inside its pair loop. `Edge(i)` walks an `NCollection_Sequence` by index, and its position cache is lost on every alternation, so the loop is O(n³).
- On Ultimaker 3, two 2,485-edge wires (solid 207) take about 3.1 s each.
- Timing only that fetch pattern on the 2,485-edge wire: **2,617 ms as written, 19 ms with `edge1` hoisted**.

## Proposal

- A small patch: hoist `edge1` out of the inner loop, or copy the edges into an array once. The results are identical by construction.
- Carry it on OCCT 7.9.3 in stepv's flake (an override of the nixpkgs OCCT), and send it upstream.

## Needs an owner decision

- Carrying a patch means stepv no longer uses OCCT "unmodified" (NOTICE). LGPL then requires the modified source to be available; the patch would sit in stepv's public repo.
- Sending it upstream is outward-facing.

## Acceptance (once approved)

- The corpus diff against `main` shows zero differences.
- Ultimaker's transfer is measured faster.
- NOTICE describes the patch.
- An upstream PR or issue to Open-Cascade-SAS/OCCT is linked.

Refs: #51, #65

---

# [Comment #1]() by [gerchowl]()

_Posted on October 12, 2026 at 07:47 PM_

## Correctness gate (falsifier, 2026-10-12)

**A plain hoist of `edge1` is NOT equivalent.** In the big-segment branch (`ShapeFix_IntersectionTool.cxx` V7_9_3, lines 1345–1627), `num1` is split and `Remove(numseg2); Remove(numseg1)` runs at 1622–1623 with `numseg1 ∈ {num1, num1+1}`. The `num1--; break;` there is commented out (1625–1626), so the inner loop continues with a stale `edge1`. Copying the edges into an array once is wrong too, because splits at `num2` shift the indices.

**The corrected patch:** hoist `edge1` as a copy (not a reference) before the `num2` loop, and **re-fetch it right after the removals** at line 1624. Invariant: wherever the original fetches, `edge1` holds the same handle as `sewd->Edge(num1)`. Patch: `scratch/74/hoist.patch` (architect state).

**Evidence:**
- **The harness copy is faithful:** it matches nixpkgs' library on every wire, and reruns are identical.
- **Fuzz:** 214k random planar wires, with about 470k hits on the segment branch.
  - corrected patch: **0 differences**;
  - plain hoist: about 56k differing wires (e.g. `SEED0=999`: `r1 s28 rm6 n41` vs `r1 s31 rm6 n44`);
  - array: about 191k.
- **Real healing, with `libTKShHealing` rebuilt with a hook:**
  - corpus: 173,459 calls, 3 with fixes applied, **0 differences**;
  - Ultimaker: 37,339 calls, **0 differences**;
  - the 2,485-edge wire: **25.5 s → 3.8 s** summed.

**Limits:**
- Real files never reach the segment branch, so only the fuzz exercises the re-fetch.
- The fuzz wires are planar lines.
- The copies ran with a fresh context, not the real history.
- The build used clang -O2, not OCCT's own flags.

**Next:** carry the corrected patch on OCCT 7.9.3 in the flake, run the corpus with zero differences, and measure Ultimaker. Then send the corrected version upstream, describing the segment branch.

