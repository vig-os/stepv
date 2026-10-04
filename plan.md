# stepv — implementation plan and handoff

**Status:** scaffolded, kernel unproven. Nothing here has rendered a STEP file yet.
**Audience:** the next agent or human picking this up cold. Read §1–§3, then start at §5 step S1.
**Written:** 2026-10-04.

Everything in §1–§4 is a *settled decision with stated evidence*. §5 is the work. §6 is what is
deliberately unresolved. If you disagree with a decision, §3 names the exact observation that would
overturn it — overturn it on that, not on taste.

---

## 1. What stepv is

A file previewer and thumbnailer for CAD interchange files, on macOS and Linux, from one binary.

- **macOS** — a Quick Look preview extension (`QLPreviewingController`) plus a thumbnail extension
  (`QLThumbnailProvider`), so pressing space in Finder shows the part, and icon view shows a render.
- **Linux** — a `.thumbnailer` for GNOME/XFCE file managers, a KF6 `ThumbnailCreator` for
  Dolphin, and a standalone viewer binary with a MIME association.
- **Formats** — `.step` / `.stp` primarily; `.iges` / `.igs` and `.brep` come free from the chosen
  kernel and should be wired at the same time, not deferred.

The reference point is [`amzoo/quicklook-step`](https://github.com/amzoo/quicklook-step), which does
the macOS half with Foxtrot + SceneKit. Do not vendor or copy from it: it is **CC BY-NC 4.0**, which
is incompatible with shipping this as an Apache-2.0 tool in a vigOS org repo.

### Why this is not a weekend of SceneKit glue

STEP is a boundary-representation format. Displaying it means evaluating trimmed NURBS surfaces and
their topology, then tessellating them with tolerance-based healing of an exporter's sloppiness.
That evaluation is the entire problem, it is decades of work, and no amount of renderer polish
substitutes for it. Pick the kernel first; the front-ends are small.

---

## 2. The question that was actually researched

> Foxtrot is what the existing macOS previewer uses. Is it reliable enough, and if not, what is?

**Foxtrot is not the foundation to build on.** Its own authors say so:

> "Foxtrot is a proof-of-concept demo, not an industrial-strength CAD kernel. It may not work for
> your models!" — and the project's own screenshots contain "a handful of surfaces that it fails to
> triangulate."

It is genuinely impressive work (hand-written EXPRESS parser, its own constrained Delaunay
triangulation, roughly 10x faster load than OCCT) and the maintained fork
[`diodeinc/foxtrot`](https://github.com/diodeinc/foxtrot) is Apache-2.0/MIT, still taking commits as
of 2026-10, and has a WASM target. But it has **no shape healing**, an **incomplete surface set**,
and it fails **silently** — which in a previewer means showing wrong geometry with no signal. Wrong
geometry is worse than slow geometry.

**The conclusion from the survey:** there is exactly one reliable open STEP-to-triangles path, and it
is OCCT. Every other option that works is OCCT in different packaging. So the real question was
never "which kernel" but "which packaging of OCCT".

### Packagings evaluated

<!-- pyml disable-num-lines 13 line-length -->
| Option | Verdict | Why |
| --- | --- | --- |
| **`occt-wasm`** (OCCT V8 to WASM, run on wasmtime) | **CHOSEN — Plan A** | Has the full data-exchange surface; see §3 |
| **Native OCCT in C++** | **Fallback — Plan B** | Correct and proven, but you build OCCT and own a C++ build on two platforms |
| **`occt-import-js`** | Keep for a web path | Same idea as Plan A but browser-first, LGPL-2.1, powers Online3DViewer |
| **`bschwind/opencascade-rs`** | **Rejected** | See below — the binding is missing everything that matters here |
| **`cadrum`** | **Rejected, steal one idea** | See below |
| **`truck` / `ruststep`** | **Rejected** | `truck-stepio` is export-leaning, import "comes further down the road"; `ruststep` parses Part-21/AP203 with no geometry evaluation and no tessellation. Rust-native STEP is parse-only today |
| **STEPcode** | **Rejected** | Mature EXPRESS parser, zero geometry evaluation. You would write trimmed-NURBS tessellation yourself — the decade of work we are avoiding |
| **build123d / CadQuery** | **Rejected** | Modeling DSLs over OCP over OCCT. Add zero reading capability, add a Python runtime. Wrong layer for a space-bar preview (right layer for a CI thumbnailer) |
| **three.js** | **Category error** | A renderer. No STEP loader exists for it; the real recipe is `occt-import-js` **plus** three.js |
| **CAD Exchanger / HOOPS / Datakit** | Deferred | Genuinely better than OCCT on dirty files. Revisit only if STEP robustness becomes a paid product promise |

### Why `opencascade-rs` was rejected (this one needs stating, it looks right)

It is the obvious choice — "Rust bindings to OCCT" — and it is the wrong one. The `cxx` bridge was
read directly, not the README. The module registry in `crates/opencascade-sys/src/lib.rs` and the
headers under `include/` show that **every** capability this project depends on is absent:

- **No `STEPCAFControl_Reader`.** Only `STEPControl_Reader`, and the sole binding is `read_step()`
  plus `one_shape_step()` wrapping `reader.OneShape()` — which collapses an entire file into a single
  `TopoDS_Shape`. **No assembly tree, no part names, no per-face colours.**
- **No XCAF at all** — no `xcaf_doc`, no `t_doc_std` module.
- **No `ShapeFix`** — `shape_analysis` and `shape_upgrade` exist; the healing pass does not.
- **`b_rep_mesh.hxx` is a bare `#include`** with no helper functions, and there is no
  `IMeshTools_Parameters` module, so the deflection controls (§4, the single biggest quality lever)
  are not reachable.
- **No `RWGltf_CafWriter` / `RWMesh`.**
- Where triangles *are* exposed, the shape is wrong: `poly.hxx` offers
  `Poly_Triangulation_Node/Normal/UV(i)`, each returning a heap-allocated `gp_Pnt` behind a
  `unique_ptr` — one FFI call and one allocation **per vertex**.

Project health confirms the direction is elsewhere: created 2022-08, **171 commits in four years**,
`opencascade` 0.1.0 (2023-01) to 0.2.0 (2023-08) to **0.3.0 (2026-08)** — a three-year release gap —
265 stars, 64 open issues, LGPL-2.1, maintainer describes it as "a major work in progress" developed
in spare time. Recent commits are modeling features (faces with holes, transforms, downcasts). It is
becoming CadQuery-in-Rust, which is a fine goal and not ours.

Adopting it would mean re-implementing the CAF layer inside someone else's LGPL binding at their
release cadence. Plan B (plain C++) is strictly less work than that.

### Why `cadrum` was rejected, and the one thing to take from it

`lzpel/cadrum` (MIT, 5.8k downloads, active) is modeling-first again — its examples are primitives,
booleans, lofts, fillets, chamfers. Two specific disqualifiers:

- Its STEP colour support is a **non-standard trailer appended after the STEP data**
  (`read_color_trailer` / `write_color_trailer`). That round-trips *its own* colours. It is not XCAF
  reading a third-party exporter's colours, which is what a previewer needs.
- `write_gltf_binary` deliberately hand-rolls glTF JSON — the source comments that
  "`RWGltf_CafWriter` is intentionally not used".

**But `Mesh::scene` is a software rasteriser** — per-triangle shaded RGB to PNG/SVG, no GPU. That is
exactly the piece the Linux thumbnailer otherwise has to solve with offscreen GL on a machine with no
X server. Borrow the approach (§4); do not take the dependency.

---

## 3. Plan A: `occt-wasm`

[`andymai/occt-wasm`](https://github.com/andymai/occt-wasm) — OCCT **V8** compiled to WebAssembly,
with a Rust crate that embeds the brotli-compressed module (~4.7 MB) and executes it on **wasmtime**.

### Why it wins

Its facade (`facade/include/occt_kernel.h`, 515 lines, 170+ methods) binds every single thing
`opencascade-rs` is missing:

```text
xcafImportSTEP(stepData) -> docId          // = STEPCAFControl_Reader
xcafGetLabelInfo(docId, labelId)           // names + hasColor
xcafExportGLTF(docId, linDefl, angDefl)    // = RWGltf_CafWriter
tessellate(id, linDefl, angDefl)
tessellateRelative(id, linDefl, angDefl)   // bbox-relative deflection
meshBatch(ids[], linDefl, angDefl)         // bulk MeshData, not per-vertex FFI
hasTriangulation(id)
getBoundingBox / getBoundingBoxLoose       // instant bbox-first preview
exportStl / exportStlBinary
```

And the packaging is better than native **for this specific job**:

- **No C++ toolchain required.** For a small tool that must build on macOS and Linux CI and ship to
  users, not building OCCT from source is worth a great deal.
- **Licensing is cleaner than native OCCT.** Tooling is MIT OR Apache-2.0; the compiled WASM output
  is LGPL-2.1-only. Ship the `.wasm` as a **separate file beside the binary** rather than embedded —
  the user can replace it, which satisfies the LGPL's replaceability obligation trivially. Compare
  that to static-linking native OCCT into a sandboxed Quick Look extension.
- **A hostile STEP file is contained by the WASM sandbox.** In a Quick Look extension parsing
  arbitrary downloaded files, that is a feature, not a nicety.
- **Newer OCCT** (V8) than `occt-sys`'s 7.8.1.
- Active: created 2026-03-26, pushed 2026-10-03, 0 open issues, releases roughly weekly.

### The risks, stated plainly

These are why §5 starts with a harness rather than a feature:

1. **Young and churning.** Four months old and already at 4.1.0 (0.3 to 4.x since May). Expect
   breaking changes; pin the exact minor and read the CHANGELOG on every bump.
2. **Thin adoption.** 58 stars, 352 downloads, one maintainer. Zero open issues could mean
   well-maintained or could mean no users. Nobody on this project has run it yet.
3. **`wasm32` caps the address space at 4 GB.** A monster assembly can exhaust the arena. Find the
   cliff deliberately (§5 step S3) rather than in a user's Finder window.
4. **wasmtime must compile a ~15 MB module.** Use `Module::deserialize` of a precompiled `.cwasm`
   or you pay JIT cost on *every single preview*. This is the difference between a snappy previewer
   and an unusable one.
5. **`--release` is mandatory.** Debug-mode wasmtime compilation is ~100x slower; a debug run will
   look like the kernel is broken when it is not.

### What would overturn this decision

Go to Plan B if **any** of these is observed in §5:

- Pass rate on the corpus materially below a native-OCCT baseline on the same files.
- p95 cold-start latency above ~400 ms for a typical part even with a precompiled `.cwasm`.
- The 4 GB arena is hit by files in the size range users actually have.
- An API break that cannot be absorbed in under a day, twice in a row.

### Plan B: native OCCT in C++

Roughly 200 lines. `STEPCAFControl_Reader` to an XCAF document, walk `XCAFDoc_DocumentTool`'s
`ShapeTool`/`ColorTool` for names and colours, `BRepMesh_IncrementalMesh` with `IMeshTools_Parameters`,
then either pull `Poly_Triangulation` arrays in bulk or hand off to `RWGltf_CafWriter`. The
[`gkv311/occt-demo-examples` step2gltf example](https://github.com/gkv311/occt-demo-examples/blob/master/_examples/xde/export/step2gltf.md)
is essentially the whole program.

**Plan B is C++, not a Rust `cxx` bridge.** This was decided deliberately. Since stepv is a
standalone repo, there is no `cxad-mesh` buffer format or blake3 cache to share, which was the only
argument for Rust at the kernel layer. Once native, you write identical OCCT calls either way, so a
bridge is pure maintenance overhead on a 200-line tool — and the front-ends are not Rust regardless
(Swift on macOS, a PNG write on Linux). Build it with CMake; `pkgs.opencascade-occt` is already
commented into `flake.nix`'s `extraPackages`, and devkit's `native` module (`cc`, `c++`, `cmake`,
`make`, `pkg-config`) is commented into `modules` beside it. Devkit's `native` module explicitly
leaves third-party libraries like OCCT to consumer `extraPackages`, which is why both lines are
needed.

Only choose a Rust bridge over plain C++ if stepv grows substantial non-OCCT logic. A previewer
will not.

---

## 4. Architecture

Split hard at the mesh boundary. One CLI is the product boundary; both front-ends shell out to it.

```text
                  ┌──────────────────────────────────────┐
  .step/.stp  ──▶ │  stepv (CLI)                         │
  .iges/.igs      │                                      │
  .brep           │   occt-wasm (OCCT V8 on wasmtime)    │
                  │     xcafImportSTEP                   │
                  │     tessellateRelative / meshBatch   │
                  │   ↓                                  │
                  │   Scene { bbox, parts[] }  (lib.rs)  │
                  │   ↓                                  │
                  │   cache: blake3(path,len,mtime,…)    │
                  └───────┬───────────────┬──────────────┘
                          │               │
         ┌────────────────┘               └──────────────────┐
         ▼                                                   ▼
  macOS front-end                                   Linux front-ends
  QLPreviewingController                             .thumbnailer (GNOME/XFCE)
  QLThumbnailProvider                                KF6 ThumbnailCreator (Dolphin)
  SceneKit via SCNGeometrySource                     software rasteriser → PNG
```

### Decisions inside that picture

**The CLI exists first.** Anything it cannot do, the previewer cannot do. Its argument surface is
already fixed in `src/main.rs` and its exit codes are part of the contract (`3` = tessellation
failed but metadata is still valid — the front-ends depend on that distinction).

**Deflection is relative to the bounding-box diagonal.** This is the single most important number in
the pipeline and the reason most CAD previewers are either visibly faceted on small parts or hang on
big assemblies. `Deflection::{THUMBNAIL, PREVIEW}` in `src/lib.rs` encode the useful bands
(linear 0.1–0.5% of the diagonal, angular 20–30°). `occt-wasm`'s `tessellateRelative` does this
natively — use it, do not reimplement it.

**Cache or be a fan event.** Finder re-requests thumbnails constantly. Key on
`(path, len, mtime, deflection, output-kind)` — already implemented and unit-tested in
`src/cache.rs`. Deliberately **not** content-hashed: reading a 400 MB STEP file to decide whether to
read a 400 MB STEP file is self-defeating. Cache roots are
`~/Library/Caches/ch.exoma.stepv` and `$XDG_CACHE_HOME/stepv`.

**Degrade honestly — this is the actual product differentiator.** On tessellation failure, parse only
the STEP header (`FILE_DESCRIPTION`, `FILE_NAME`, originating system, schema, part count) and render
that as text alongside the bounding box. A header parse never fails. Foxtrot-based viewers show
nothing. Ship `--info` (exit 0, JSON on stdout) before shipping pretty rendering.

**Progressive display.** `getBoundingBox` is available before any tessellation. Show the box plus
header metadata immediately, swap in geometry when it arrives.

**Hard limits in the CLI, not the extension.** Wall-clock timeout (default 20 s) and a memory cap,
with a non-zero exit. A Quick Look extension that hangs is a worse bug than one that shows an icon.

**macOS note that will otherwise cost a day:** SceneKit and Model I/O **cannot read glTF**. Either
emit USDZ (Quick Look renders it natively and the thumbnail comes free) or hand raw buffers across
FFI into `SCNGeometrySource`. For a preview, raw buffers are less machinery — which is why `Mesh` in
`src/lib.rs` is flat planar buffers and not a vertex struct.

**Linux note:** the GNOME/XFCE path is nearly free — a `.thumbnailer` file pointing at the CLI,
which writes a PNG. Dolphin needs a small KF6 `ThumbnailCreator` shelling to the same binary. Register
the `model/step` MIME type (`shared-mime-info` carries it for `.step`/`.stp`).

### Crate layout

One crate (`stepv`, lib + bin) on purpose while the kernel is unproven. Split into `stepv-core` plus
`stepv` when a **second** consumer of the library appears — the expected trigger is the C ABI for the
Swift Quick Look extension. Splitting earlier buys nothing and costs a workspace.

---

## 5. The work

Tracked as [`vig-os/stepv#1`](https://github.com/vig-os/stepv/issues/1).

### S1 — Prove the kernel (do this before anything else)

```bash
cd ~/Projects/stepv
direnv allow                      # or: nix develop
cargo add occt-wasm@4.1
```

Write `tests/harness.rs` (or `examples/harness.rs` — it needs to run on real files, not fixtures in
the cargo source filter). For each input file:

1. `xcafImportSTEP` the bytes.
2. Walk XCAF labels; record how many parts resolved a **name** and a **colour**.
3. `meshBatch` with `tessellateRelative` at `Deflection::PREVIEW`.
4. Build a `Scene`; assert `Mesh::is_well_formed()` on every part.
5. Record: triangle count, wall-clock, peak RSS.

Always `--release` (risk 5 above).

**Corpus** — `just fixtures` should fetch, and `tests/fixtures/manifest.toml` should record:

- **CAx-IF / NIST PMI AP242 test files** — the nasty corners of the standard. Small, high signal.
- **ABC dataset samples** — ~1M CAD models in STEP; take a few hundred for a does-it-load sweep.
- A handful of real exporter output: SolidWorks, NX, CATIA, Fusion, FreeCAD. These are where
  AP242 interoperability actually breaks.
- One deliberately enormous assembly (~200 MB+) to find the 4 GB arena cliff.

**Acceptance gate — write the numbers down in this file when you have them:**

| Metric | Target |
| --- | --- |
| Load + tessellate pass rate | Record it. This is *the* number |
| Parts with names resolved | Record it (exporter-dependent, not a pass/fail) |
| Parts with colours resolved | Record it |
| p95 wall-clock, typical part | < 400 ms *with* a precompiled `.cwasm` |
| Cold start, `.cwasm` vs JIT | Measure both; the delta decides whether precompiling is mandatory |
| Behaviour on the 200 MB assembly | Must fail cleanly, never hang or OOM the host |

If the gate fails on the §3 criteria, switch to Plan B and record why here. **Do not proceed to S2
until S1 has a recorded pass rate.**

### S2 — The CLI for real

Implement `--info` first (header metadata as JSON, exit 0) — it is the honest-degradation path and it
cannot fail. Then `--glb`, then `--png` with the software rasteriser, then the cache wired through,
then the timeout and memory cap. Keep the exit codes in `src/main.rs` exactly as documented.

### S3 — Limits and failure modes

Find the arena cliff. Confirm the timeout fires. Confirm exit 3 still prints valid metadata. Confirm
a truncated/corrupt STEP file fails cleanly rather than panicking across the wasmtime boundary.

### S4 — Linux front-ends

The `.thumbnailer` (cheapest possible win — do it first and the project is useful), MIME registration,
then the KF6 `ThumbnailCreator`.

### S5 — macOS front-ends

Swift host app plus `QLPreviewingController` and `QLThumbnailProvider`. Decide buffers-over-FFI
versus USDZ at this point, with the §4 note in hand. This is the largest single step and it is last
because it is the one that cannot be validated headlessly in CI.

### S6 — Release

`crates.io` publish for `stepv`, and binaries for both platforms. The repo already has the devkit
release train; §"Repo setup" below records what still has to be wired for it.

---

## 6. Open questions

- **Precompiled `.cwasm` distribution.** It is host-arch and wasmtime-version specific. Build at
  install time, ship per-arch, or build on first run and cache? S1's cold-start delta decides whether
  this matters at all.
- **LGPL compliance wording.** Shipping the `.wasm` as a separate replaceable file is the plan (§3).
  Someone should write the actual `NOTICE` text before the first public binary, not after.
- **`occt-import-js` for a web path.** Same kernel, browser-first, LGPL-2.1. Likely the right answer
  if a web preview is ever wanted; explicitly out of scope now.
- **Does `occt-wasm` expose `ShapeFix`?** The facade header was read for STEP/XCAF/mesh/glTF and
  those are all present. Healing was *not* confirmed either way. Check during S1 — if absent, dirty
  exporter output will fail more often than native OCCT would, and that is a Plan B argument.
- **Shared cache with cxad.** `src/cache.rs` deliberately uses blake3, the same function cxad's node
  store uses, so a future shared cache needs no migration. No such sharing is designed yet.

---

## 7. Repo setup — what exists and what is still owed

### Done

- **`vig-os/stepv`**, public, Apache-2.0, created 2026-10-04.
- **devkit 1.17.0 scaffold**, `DEVKIT_MODE=direnv`, `DEVKIT_WORKFLOW=trunk` (topic branches merge
  straight to `main`; releases still fork `release/X.Y.Z` from `main` and merge back).
  `DEVKIT_LANGUAGES=rust` is declared, which arms CI's language gate — if `Cargo.toml` ever
  disappears, CI fails loudly instead of reporting a green "Tests" check over nothing.
- **`flake.nix` rewired to `vigos.lib.mkRustProject`.** This is the only supported Rust adoption: the
  `rust` capability module **refuses to load bare** (`modules = [ "rust" ]` is an eval-time error by
  design, because it would otherwise hand you a toolchain plus a green `nix flake check` that builds
  nothing). The one call provides `devShells.default`, `checks` and `packages`, all three assigned.
  `flake.nix` is in devkit's `PRESERVE_FILES`, so upgrades never overwrite this.
- **`rust-toolchain.toml`** pinned to 1.96.0, byte-identical to cxad's, so the fenix `toolchainHash`
  is shared and neither repo discovers it twice.
- **`src/lib.rs`** — the kernel-agnostic `Scene` / `Part` / `Mesh` / `BBox` / `Deflection` contract,
  with tests. Both plans must produce this, which is what keeps the kernel decision reversible.
- **`src/cache.rs`** — cache keys, implemented and unit-tested.
- **`src/main.rs`** — the CLI argument surface and exit codes, fixed before either front-end exists.
  It exits 3 with an honest "not implemented" rather than faking a render.

### Owed

- **The `occt-wasm` dependency is deliberately not in `Cargo.toml`.** Adding it is S1. Nothing in
  this repo should claim a kernel works before the harness has run.
- **`just fixtures` is declared but not implemented.** `tests/fixtures/manifest.toml` records every
  corpus source with its reason, licence posture and expectation — that file is committed and is
  the record of what any pass rate was measured against. The recipe itself **exits 1 with a pointer
  to this plan** rather than succeeding silently, because a `fixtures` recipe that no-ops makes an
  empty corpus look green. Implementing the fetch is part of S1.
- **`deny.toml`** — `mkRustProject` turns `cargo deny` on automatically once the file exists. Left
  out deliberately for now: the advisories check wants network access, and a nix build sandbox does
  not have it, so adding the file without checking that first turns every `nix flake check` red.
- **`CARGO_REGISTRY_TOKEN`** and the `crates-io` environment for the S6 publish, mirroring
  `vig-os/scitadel`'s shape. Not declared yet on purpose: declaring a repo secret whose live value
  does not exist is how otterdog plans go wrong.
- **A devkit issue for the `mkRustProject` forwarding gap.** `mkRustProject` has no `branchTypes` /
  `commitTypes` / `refsPolicy` / `refsOptionalTypes` arguments, so those `.vig-os` knobs do not reach
  the flake-generated hooks the way they do through `mkProjectShell`. Inert here — all four keys are
  empty, so each resolves to its devkit default — but set one and it is silently ignored. The gap is
  commented at the call site in `flake.nix`.
Checked and **not** owed, recorded so nobody re-investigates:

- **CodeQL default setup** — the scaffold warns that its advanced config conflicts with GitHub's
  default code-scanning setup and does not change that API setting for you. Verified via
  `gh api /repos/vig-os/stepv/code-scanning/default-setup`: already `not-configured`, so the
  advanced config's uploads will not reject. Nothing to do.
- **No Rust leg in `codeql.yml`** — the matrix is `['actions']` and that is deliberate. Devkit's own
  comment states Rust omits its CodeQL leg; `actions` is always analyzed. Not a mis-render from the
  first scaffold running before `Cargo.toml` existed.

### org-config

vig-os is governed as code by [`vig-os/org-config`](https://github.com/vig-os/org-config) (Otterdog;
plan on PR, apply on merge behind a human-gated `production` environment). A repo is supposed to be
**declared there and created by `otterdog apply`** — this one was created with `gh repo create`
first, which is exactly the bypass that config warns about. `apply` never deletes what the config
omits, so the consequence is that stepv shows up as an **inventory drift issue** until its
declaration lands.

The declaration is open as
[`vig-os/org-config#316`](https://github.com/vig-os/org-config/pull/316) (content reproduced in §8).
**Until it merges, stepv has no branch protection, no signed-commit rule and no tag protection, and
its devkit workflows have no credentials** — the org secrets are `visibility: selected` and stepv is
not in any of their repository lists until that PR applies. Expect the scaffolded workflows to fail
with empty credentials and no error message before then; that is this gap, not a bug in the
workflows.

`apply` runs only on merge to org-config's `main`, behind a human-gated `production` environment. So
merging the PR is not the end of it — someone has to approve the deployment.

---

## 8. The org-config declaration

A `trunk`-workflow repo has no `dev` branch, so it takes `mainProtection`, `releaseProtection`,
`signedCommits` and `tagProtection` — and **no** `devProtection`. New repo, so it goes on the
**client-ID** credential form from day one (as `tessera` did) and must **not** be added to the legacy
numeric `*_APP_ID` secrets, which are being retired.

Org secret lists to add `'stepv'` to:

- `COMMIT_APP_CLIENT_ID`, `COMMIT_APP_PRIVATE_KEY`
- `DEVKIT_UPGRADE_APP_CLIENT_ID`, `DEVKIT_UPGRADE_APP_PRIVATE_KEY`
- `RELEASE_APP_CLIENT_ID`, `RELEASE_APP_PRIVATE_KEY`

Repository declaration in `otterdog/vig-os/vig-os.jsonnet`, inserted alphabetically between
`scitadel` and `sync-issues-action`:

<!-- pyml disable-num-lines 25 line-length -->
```jsonnet
orgs.newRepo('stepv') {
  allow_auto_merge: true,
  allow_update_branch: true,
  custom_properties+: {
    type: ['tools'],
  },
  description: 'Fast STEP/IGES/BREP preview and thumbnails for macOS Quick Look and Linux file managers — OCCT-backed, sandboxed',
  private_vulnerability_reporting_enabled: true,
  rulesets: [
    orgs.mainProtection(['15368:CI Summary']),
    orgs.releaseProtection(
      checks=['15368:CI Summary'],
      bypass=['commit-action-bot'],
    ),
    orgs.signedCommits(),
    orgs.tagProtection(['vig-os-release-app']),
  ],
},
```

`15368` is the github-actions app; `CI Summary` is the managed `ci.yml` aggregator job. No
`devProtection` — that is the whole point of `DEVKIT_WORKFLOW=trunk`.

---

## 9. References

Kernel survey:

- [Foxtrot project page](https://www.mattkeeter.com/projects/foxtrot/) — the author's own scope
  statement
- [`diodeinc/foxtrot`](https://github.com/diodeinc/foxtrot) — maintained fork, Apache-2.0/MIT
- [`andymai/occt-wasm`](https://github.com/andymai/occt-wasm) and
  [its facade header](https://github.com/andymai/occt-wasm/blob/HEAD/facade/include/occt_kernel.h)
- [`bschwind/opencascade-rs`](https://github.com/bschwind/opencascade-rs) and
  [`step_control.hxx`](https://github.com/bschwind/opencascade-rs/blob/main/crates/opencascade-sys/include/step_control.hxx)
  — the evidence for the rejection in §2
- [`kovacsv/occt-import-js`](https://github.com/kovacsv/occt-import-js) — the web path
- [`lzpel/cadrum`](https://github.com/lzpel/cadrum) — the software rasteriser idea
- [`truck-stepio`](https://lib.rs/crates/truck-stepio)
- [OCCT step2gltf example](https://github.com/gkv311/occt-demo-examples/blob/master/_examples/xde/export/step2gltf.md)
  — effectively all of Plan B
- [`amzoo/quicklook-step`](https://github.com/amzoo/quicklook-step) — prior art, CC BY-NC 4.0, do not
  copy

Project context:

- [`vig-os/devkit`](https://github.com/vig-os/devkit) — `docs/MIGRATION.md` for every `.vig-os` knob,
  `docs/NIX.md` for the flake-input policy
- [`vig-os/org-config`](https://github.com/vig-os/org-config) — `docs/adr/0008-*` for the ruleset
  tiers used in §8
- `gerchowl/cxad` — shares the toolchain pin and the blake3 cache-key choice; its `ARCHITECTURE.md`
  §2 on topological naming is the reason `Mesh` carries a per-triangle face-id buffer
