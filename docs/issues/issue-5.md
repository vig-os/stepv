---
type: issue
state: closed
created: 2026-10-04T14:47:12Z
updated: 2026-10-04T17:04:57Z
author: gerchowl
author_url: https://github.com/gerchowl
url: https://github.com/vig-os/stepv/issues/5
comments: 0
labels: feature
assignees: none
milestone: none
projects: none
parent: none
children: none
synced: 2026-10-05T08:45:54.098Z
---

# [Issue 5]: [S2: the CLI for real (--info, --glb, --png + overlay, cache, limits)](https://github.com/vig-os/stepv/issues/5)

## Description

Step **S2** of [`plan.md`](../blob/main/plan.md) §5: turn `stepv` from an argument surface that exits 3
with "not implemented" into the real CLI that both front-ends shell out to. The kernel is decided
and measured (S1, #1): native OCCT in a subprocess, 99.5% faithful on the S1 corpus.

## Scope, in order

1. **`--info`**: header metadata as JSON, exit 0. Parse the Part 21 header (`FILE_DESCRIPTION`,
   `FILE_NAME`, originating system, schema, product count) **directly in Rust**. Do not use the
   kernel's full transfer, which is 72% of wall time on slow files (S1). This is the
   honest-degradation path, so it must not be able to fail.
2. **`--glb`**: binary glTF from the `Scene`.
3. **`--png`**: the software rasteriser (plan.md §2, the `cadrum` idea), including the
   **broken-face overlay**. `FaceStatus::Approx` gets the warning material, `Missing` is drawn
   as its outline (`LineKind::MissingOutline`), the part gets a badge, and
   `LineKind::Construction` is hidden by default. The data contract is in place (S1 follow-up).
4. **Cache** wired through (`src/cache.rs` keys are already implemented and tested).
5. **Timeout → exit 4.** `occt::run` already kills the child at the deadline; map
   `Outcome::Timeout`.
6. **Memory cap**: #4, because it needs a
   per-platform design.
7. **Per-face colour table** from the kernel next to `Mesh::face_ids`. 81% of parts in the S1
   corpus carry colour only per face (Onshape), so without it previews lose their colour.

## Acceptance

- Exit codes exactly as documented in `src/main.rs` (0 / 2 / 3 / 4); exit 3 always carries valid
  metadata on stdout.
- `--info` succeeds on every file in `tests/fixtures/malformed/` that has a parseable header.
- `--png` of `occt-import-js/conical-surface` shows the approximated bore with the overlay;
  `abc-dataset/00000092` (sketch only) renders its curves.

