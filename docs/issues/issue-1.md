---
type: issue
state: closed
created: 2026-10-04T10:58:01Z
updated: 2026-10-04T15:13:51Z
author: gerchowl
author_url: https://github.com/gerchowl
url: https://github.com/vig-os/stepv/issues/1
comments: 1
labels: none
assignees: none
milestone: none
projects: none
parent: none
children: none
synced: 2026-10-05T08:45:55.392Z
---

# [Issue 1]: [S1: prove the kernel against a real STEP corpus (occt-wasm failed → native OCCT)](https://github.com/vig-os/stepv/issues/1)

## Description

The kernel for `stepv` is **chosen but unproven**: OCCT V8 via
[`occt-wasm`](https://github.com/andymai/occt-wasm) on wasmtime. The survey, the rejected
alternatives (Foxtrot, `opencascade-rs`, `cadrum`, `truck`/`ruststep`, STEPcode, build123d), the
risks and the Plan B fallback are all written up in [`plan.md`](../blob/main/plan.md).

This issue is step **S1** of `plan.md` §5 — the step whose entire purpose is to find out whether
Plan A holds up. **No other work should start before it has a recorded pass rate.**

## Scope

1. `cargo add occt-wasm@4.1`.
2. A harness that, per input file: `xcafImportSTEP` → walk XCAF labels for names/colours →
   `meshBatch` with `tessellateRelative` at `Deflection::PREVIEW` → build a `Scene` → assert
   `Mesh::is_well_formed()` on every part.
3. Record per file: pass/fail, triangle count, names resolved, colours resolved, wall-clock,
   peak RSS.
4. Corpus (`just fixtures` + `tests/fixtures/manifest.toml`): CAx-IF / NIST PMI AP242 files, a few
   hundred ABC-dataset samples, real exporter output (SolidWorks / NX / CATIA / Fusion / FreeCAD),
   and one ~200 MB+ assembly to find the `wasm32` 4 GB arena cliff.

Always `--release` — debug-mode wasmtime is ~100x slower and will look like a broken kernel.

## Acceptance

Fill the table in `plan.md` §5 with real numbers. The gate:

- p95 wall-clock for a typical part < 400 ms **with a precompiled `.cwasm`**
- cold start measured both with and without the precompiled module
- the 200 MB assembly fails **cleanly** — never hangs, never OOMs the host

Also answer the open question in `plan.md` §6: **does `occt-wasm` expose `ShapeFix`?** The facade
header was confirmed to carry STEP/XCAF/mesh/glTF; healing was not confirmed either way. If it is
absent, dirty exporter output will fail more often than native OCCT would — which is a Plan B
argument.

## Switch to Plan B if

Any of: pass rate materially below a native-OCCT baseline on the same files; p95 cold start above
~400 ms even precompiled; the 4 GB arena is hit by file sizes users actually have; or an API break
that cannot be absorbed in a day, twice running. Record the reason in `plan.md` §3.
---

# [Comment #1]() by [gerchowl]()

_Posted on October 4, 2026 at 02:47 PM_

## S1 result: done, with Plan B

**Plan A (`occt-wasm`) failed its gate.** crates.io 4.0.0 can't instantiate its module. On upstream
`main`, both STEP importers trap, because the WASI build has no filesystem. That's a 0% pass rate
and a 2.2 s JIT cold start. The `ShapeFix` question is answered (it exists) and moot.

**Plan B (native OCCT 7.9.3, run as a subprocess) passed.** On 391 files (NIST PMI, 300 ABC
models, the occt-import-js CAx-IF subset, a 221 MB synthetic assembly), with the per-face recovery
ladder:

| | |
| --- | --- |
| Faithful (pass + sketch wireframe) | **99.5%** (389/391) |
| Degraded (some faces approximated, flagged for an overlay) | 2 |
| Holes / crashes / timeouts / bad meshes | **0 / 0 / 0 / 0** |
| Malformed files failing cleanly | 10/10 |
| p95, files < 1 MB | 335 ms (target < 400 ms) |
| Cold start | 62–69 ms |

Full tables, the face-by-face diagnosis and the caveats are in `plan.md` §5 "S1 result" and "S1
follow-up". Work is on `feature/1-s1-kernel-harness`; the PR closes this issue.

Follow-ups now tracked: S2 #5, S3 #6, S4 #7, S5 #9, S6 #10, memory cap #4, kernel CI #11,
corpus gaps #12, deny.toml #13, LGPL NOTICE #8, occt-wasm upstream report #14, devkit
vig-os/devkit#1810.


