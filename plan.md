# stepv — implementation plan and handoff

**Status:** S1–S5 built and tested; S6 (release) wired and **waiting on human credentials**
(§5 "S6", §7 "Owed"). The kernel is native OCCT 7.9.3. It runs as a subprocess on Linux, and
in-process inside the macOS Quick Look extensions, whose sandbox forbids exec. With the per-face
recovery ladder, 99.5% of a 391-file corpus is shown faithfully, the other 0.5% is drawn with a
flagged overlay, and nothing crashes or hangs. The CLI, the Linux thumbnailer, MIME and KF6 plugin,
the viewer window, and the macOS preview and thumbnail extensions all work. On macOS that was
proven through Quick Look itself.
**Audience:** the next agent or human picking this up cold. Read §1–§4, then §5 for what was
built and why, then §7 "Owed" for what only a human can do.
**Written:** 2026-10-04. **S1 recorded:** 2026-10-04. **S2–S6:** 2026-10-04.

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
| **`occt-wasm`** (OCCT V8 to WASM, run on wasmtime) | Plan A — **failed S1** | Has the full data-exchange surface on paper; in the Rust crate STEP import traps (no FS in the WASI build). See §3, §5 "S1 result" |
| **Native OCCT in C++** | Plan B — **CHOSEN after S1** | 97.2% on the S1 corpus, 0 crashes. OCCT comes from nixpkgs, so the "own a C++ build" cost is one CMake file |
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

> **Outcome (S1, 2026-10-04): rejected on evidence; Plan B is the kernel.** The reasoning below
> is kept as written because it was sound *as a plan*. What it got wrong was the packaging, which
> only running it could show. In the Rust crate, STEP import does not work at all. See §5
> "S1 result" for the measurements and §3 "What would bring Plan A back" for the way back.

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

The first two were observed: a 0% pass rate (STEP import traps), and a cold start of 2.2 s.

### What would bring Plan A back

All three of these, verified by re-running `just harness` against it, not by reading a changelog:

1. A crates.io release whose `OcctKernel::new()` instantiates. 4.0.0 does not; the fix is on
   upstream `main` as crate 4.1.0, untagged as of 2026-10-04.
2. STEP/XCAF import that does not go through a file. The facade writes the bytes to
   `/tmp/*.step` and the standalone WASI module has no filesystem; its import list has no `open`
   at all, so no host shim can fix it. OCCT has `STEPCAFControl_Reader::ReadStream`, so this is a
   small facade change upstream, but it *is* upstream's change to make.
3. A constructor that takes a precompiled module (`Module::deserialize`) and a store limiter.
   4.x only has `new()`, which JIT-compiles the 23 MB module on every process start.

Even then, Plan B's subprocess already gives the crash containment that was Plan A's best argument.

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

**As built in S1:** `kernel/stepv-occt.cpp` (~600 lines with the readers and the mesh writer),
CMake, OCCT 7.9.3 from the pinned nixpkgs. It runs as a **subprocess** of the Rust side
(`src/occt.rs`), not as a linked library, for three reasons:

- **Containment.** A file that crashes OCCT kills a child process, and the caller gets a reported
  `Crashed` outcome. That was Plan A's strongest argument, and a process boundary provides it.
- **Hard limits for free.** The parent kills the child at the deadline. No cooperative
  cancellation inside OCCT is needed.
- **LGPL.** OCCT stays dynamically linked into a separate executable. Replacing it means replacing
  shared libraries next to a binary we never link into.

Contract: one input path; a one-line JSON summary on stdout (also on failure, with exit 3); with
`--mesh`, the planar buffers in the `STEPVMSH` format specified at the top of the C++ file and
decoded by `occt::read_mesh`. OCCT's own stdout chatter is redirected to stderr so it cannot
corrupt the summary.

---

## 4. Architecture

Split hard at the mesh boundary. One CLI is the product boundary; both front-ends shell out to it.

```text
                  ┌──────────────────────────────────────┐
  .step/.stp  ──▶ │  stepv (CLI)                         │
  .iges/.igs      │                                      │
  .brep           │   stepv-occt subprocess (OCCT 7.9)   │
                  │     STEPCAF/IGESCAF reader → XCAF    │
                  │     BRepMesh, bbox-relative defl.    │
                  │   ↓  STEPVMSH buffers + JSON         │
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
(linear 0.1–0.5% of the diagonal, angular 20–30°). **Correction from S1:** this plan used to say
`occt-wasm`'s `tessellateRelative` "does this natively". It does not. It passes OCCT's
`isRelative` flag, which scales deflection **per edge**, by each edge's own size, not by the
model. The kernel computes the whole model's bbox diagonal and passes an absolute deflection
with `Relative = false`.

**S1 finding, mesher:** on these files, angular deflection costs far more than linear. The worst
file in the corpus (ABC `00000046`, a perforated plate) spends 14.8 s and 7.2 GB in BRepMesh on
two planar faces with ~1,250 holes each. That is the same at linear 0.1% and 0.5%, but drops to
7.7 s and 4.2 GB at 30°. OCCT's alternative Delabella triangulator was still running after nine
minutes on that file. Watson (the default) stays. The S2 timeout and memory cap are what make
this file safe, not tuning.

**Cache or be a fan event.** Finder re-requests thumbnails constantly. Key on
`(path, len, mtime, deflection, output-kind)` — already implemented and unit-tested in
`src/cache.rs`. Deliberately **not** content-hashed: reading a 400 MB STEP file to decide whether to
read a 400 MB STEP file is self-defeating. Cache roots are
`~/Library/Caches/ch.exoma.stepv` and `$XDG_CACHE_HOME/stepv`.

**Degrade honestly — this is the actual product differentiator.** On tessellation failure, parse only
the STEP header (`FILE_DESCRIPTION`, `FILE_NAME`, originating system, schema, part count) and render
that as text alongside the bounding box. A header parse never fails. Foxtrot-based viewers show
nothing. Ship `--info` (exit 0, JSON on stdout) before shipping pretty rendering.

**Progressive display.** The bounding box is available before any tessellation. Show the box plus
header metadata immediately, swap in geometry when it arrives. S1 shows where the wait actually
is: on files that take over 1 s, **72% of wall time is STEP transfer** (entity translation plus
OCCT's default shape healing) and 22% is meshing. A box-first display therefore has to come from
a cheaper pass than a full transfer, which makes it an S2 design question.

**Hard limits in the CLI, not the extension.** Wall-clock timeout (default 20 s) and a memory cap,
with a non-zero exit. A Quick Look extension that hangs is a worse bug than one that shows an icon.

**The kernel CLI sandboxes itself (#18).** The input is untrusted, and a memory-corruption bug in
OCCT's readers would run the attacker's code with the kernel's rights. Before it opens the input,
`stepv-occt` confines itself to what a run needs: reading the input's directory and below
(multi-file assemblies resolve their external references there), and writing the one mesh file,
which it creates first so that no right to create files is needed. The sandbox is
`kernel/sandbox.cpp`: Landlock plus a seccomp deny list (sockets, exec, any clone without
`CLONE_THREAD`, ptrace, namespaces, io_uring) on Linux, and a deny-by-default Seatbelt profile on
macOS. It lives in the kernel rather than the CLI, so that every caller gets it: the thumbnailers,
KDE, and the harness. The summary reports it (`"sandbox"`). An OS that can provide only part of it
gets a warning, not a silent pass. In-process, `libstepvocct` is left alone: the App Sandbox is
already the boundary there.

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

Every step and owed item has a GitHub issue; this file is the design record, the issues are the
tracker.

| Step | Issue | Owed / related | Issue |
| --- | --- | --- | --- |
| S1 kernel (done) | [#1](https://github.com/vig-os/stepv/issues/1) | Memory cap | [#4](https://github.com/vig-os/stepv/issues/4) |
| S2 CLI | [#5](https://github.com/vig-os/stepv/issues/5) | Kernel + harness in CI | [#11](https://github.com/vig-os/stepv/issues/11) |
| S3 limits | [#6](https://github.com/vig-os/stepv/issues/6) | Corpus gaps | [#12](https://github.com/vig-os/stepv/issues/12) |
| S4 Linux | [#7](https://github.com/vig-os/stepv/issues/7) | `deny.toml` | [#13](https://github.com/vig-os/stepv/issues/13) |
| S5 macOS | [#9](https://github.com/vig-os/stepv/issues/9) | LGPL NOTICE | [#8](https://github.com/vig-os/stepv/issues/8) |
| S6 release | [#10](https://github.com/vig-os/stepv/issues/10) | Workflow credentials | [#3](https://github.com/vig-os/stepv/issues/3) |
| | | occt-wasm upstream report | [#14](https://github.com/vig-os/stepv/issues/14) |
| | | devkit `mkRustProject` gap | [devkit#1810](https://github.com/vig-os/devkit/issues/1810) |

### S1 — Prove the kernel (do this before anything else)

```bash
cd ~/Projects/stepv
direnv allow                      # or: nix develop
just fixtures                     # builds kernel/, fetches + generates the corpus
just harness                      # examples/harness.rs over tests/fixtures/
just harness --cold-start 30      # spawn-to-result latency
```

(As originally written, this step began with `cargo add occt-wasm@4.1`; that crate version does
not exist on crates.io — see "S1 result".) For each input file, `examples/harness.rs`:

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

### S1 result (2026-10-04, sage: Mac Studio, arm64)

**Plan A, `occt-wasm`: failed, 0% pass rate.** Measured in a scratch crate, not inferred:

| Build | `OcctKernel::new()` | `import_step` / `xcaf_import_step` on a 10 mm cube |
| --- | --- | --- |
| crates.io 4.0.0 (latest published) | **fails**: `unknown import: env::emscripten_get_preloaded_image_data` | unreachable |
| upstream `main` @ `1091f22` (crate 4.1.0, untagged) | OK, **2.2 s** (JIT, `--release`) | **wasm trap: `uninitialized element`** |

The `cargo add occt-wasm@4.1` this plan prescribed is impossible: 4.1 is not on crates.io, and
4.0.0 cannot instantiate at all (upstream PR #371: the crate's tests had been skipping silently
since its first commit). On `main`, primitives and `tessellate` work, but both STEP importers
write the input to `/tmp` inside a WASI module whose import list has no `open`. The facade's
`ShapeFix` question (§6) is answered — `fix_shape` / `heal_*` exist — and is moot. §3 "What would
bring Plan A back" lists the conditions.

**Plan B, native OCCT 7.9.3: passed.** `just fixtures && just harness`, at `Deflection::PREVIEW`
(0.1% of the bbox diagonal, 20°). One kernel process per file, run serially:

<!-- pyml disable-num-lines 9 line-length -->
| Source | Files | Pass | Partial | Clean fail | Bad mesh | Crash | Timeout | Pass rate | Named | Coloured (part / face) | p50 ms | p95 ms | Max RSS MB |
| --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| abc-dataset | 300 | 290 | 9 | 1 | 0 | 0 | 0 | 96.7% | 100.0% | 0.0% / 99.2% | 84 | 3205 | 7255 |
| nist-pmi | 33 | 33 | 0 | 0 | 0 | 0 | 0 | 100.0% | 9.9% | 45.1% / 11.3% | 146 | 329 | 158 |
| occt-import-js | 57 | 56 | 1 | 0 | 0 | 0 | 0 | 98.2% | 58.7% | 76.1% / 0.0% | 61 | 92 | 60 |
| stress-assembly | 1 | 1 | 0 | 0 | 0 | 0 | 0 | 100.0% | 100.0% | 0.0% / 0.0% | 20232 | 20232 | 2078 |
| **all but malformed** | **391** | **380** | 10 | 1 | 0 | **0** | **0** | **97.2%** | 94.5% | 7.1% / 81.0% | 80 | 2694 | 7255 |
| malformed (want clean fail) | 10 | — | — | **10** | 0 | 0 | 0 | — | — | — | — | — | 27 |

*Partial* = loaded and drew, but some faces produced no triangles (10 files, 1–28 faces each, out of
hundreds to thousands). These count **against** the pass rate, because they are exactly the
silent failure §2 holds against Foxtrot. Here they are counted, and S2 has to surface them. The one
clean fail is ABC `00000092`, a file that tessellates to zero triangles.

Against the gate:

| Metric | Target | Result |
| --- | --- | --- |
| Load + tessellate pass rate | Record it | **97.2%** (380/391); 99.7% load and draw something |
| Parts with names resolved | Record it | 94.5% overall; exporter-dependent as expected: ABC/Onshape 100%, NIST 9.9% |
| Parts with colours resolved | Record it | 7.1% at part level, 81.0% per face. Onshape colours faces, not parts, so the front-ends must render per-face colour or they will lose it |
| p95 wall-clock, typical part | < 400 ms | **337 ms** for files under 1 MB (284 files). See the size breakdown below |
| Cold start | Measure; decides precompiling | **62–69 ms** (min–p95, 30 runs, 6.6 KB file): spawn, OCCT init, read, mesh, write. With no WASM, there is nothing to precompile, so the `.cwasm` question is gone |
| 200 MB assembly | Fail cleanly, never hang or OOM | **221 MB** synthetic assembly (60 distinct copies of NIST CTC 02): **loads**, 20.2 s, 2.1 GB peak. No hang. It needs S2's limits to be safe in Quick Look |
| Cliff probe (one-off, not in the corpus) | Find it | A **745 MB** file (200 copies, 800 parts) also **loads**: 65.6 s, **5.5 GB** peak, 5.1M triangles. Native 64-bit has no 4 GB arena, so the cliff is now the host's RAM. That makes S2's memory cap mandatory |

Wall-clock by input size (passing files, parent-side, spawn included):

| File size | Files | p50 ms | p95 ms | max ms |
| --- | ---: | ---: | ---: | ---: |
| < 0.1 MB | 171 | 57 | 87 | 204 |
| 0.1–1 MB | 113 | 109 | 337 | 836 |
| 1–5 MB | 75 | 522 | 1456 | 4249 |
| > 5 MB | 21 | 5604 | 21068 | 21164 |

82.6% of passing files finish under 400 ms. The slow tail is the reason for the cache, the
timeout and the bbox-first display, not a kernel defect. It is dominated by STEP transfer, not
meshing (§4).

**Two kernel bugs found and fixed by the harness:** (1) multi-file CAx-IF assemblies resolved
to *no geometry* when given a relative path, because OCCT resolves external references only
against an absolute one. The kernel now `realpath`s its input, and occt-import-js went from 78.9% to
98.2%. (2) OCCT's auto-naming invented part names ("SOLID") for unnamed shapes, inflating the
names column. It is now disabled.

**Corpus caveats, stated so the number is not over-read.** `cax-if` (full rounds, behind
registration) and `exporter-matrix` (hand-collected SolidWorks/NX/CATIA/Fusion/FreeCAD output)
are **not yet in the corpus**. The CAx-IF rounds are covered only through the public subset in
`occt-import-js`. The stress assembly is synthetic. ABC is all Onshape output. 97.2% is
therefore a lower bound on robustness against one exporter and a sample of the standard, not a
claim about CATIA. Exact bytes are in `tests/fixtures/corpus.sha256`; per-file results are in
`harness-out/results.jsonl` after a run.

### S1 follow-up — recovery ladder, sketch handling, broken-face overlay (2026-10-04)

Every one of the 2.8% non-passes above was looked at face by face, using the mesher's own status
flags, `ShapeAnalysis_Wire` checks, `BRepCheck` and the face's area. There were four causes:

| Cause | Files | What it was |
| --- | ---: | --- |
| No surfaces at all | 1 | ABC `00000092` is 3 trimmed curves and 2 B-splines: a sketch exported as STEP |
| Recoverable by `ShapeFix` | 4 | Valid B-spline faces that mesh once healed. A clean re-mesh alone does **not** fix them |
| Face far smaller than the model, or a self-intersecting discretized boundary | 4 | Tori and cones of ~0.0075 mm² (3×10⁻¹⁴ of diag²); planes flagged `SelfIntersectingWire` at the model-relative deflection |
| Zero-area slivers | 3 | Zero parametric width or negative/vanishing area (e.g. 00000085's 23 cylinders, u-range exactly 0). The exporter's leftovers. **Not holes**: nothing a renderer could show |
| Genuinely unmeshable, visible | 2 | 00000229: 2 planes still self-intersecting at fine deflection. `conical-surface`: a cone whose boundary has a 2-D gap and lacks its apex edge. Explicit `ShapeFix_Face` (degenerate/lacking/seam fixes) does not save it either |

The kernel now runs a **ladder** on every face the first pass leaves without triangles. Each rung
works on an isolated copy, so neighbours' shared edges are never disturbed:

1. clean re-mesh
2. `ShapeFix_Shape` + re-mesh
3. re-mesh with deflection relative to the **face's** size
4. degenerate check: zero parametric width, or |area| under 1e-8 of diag², which is less than one
   pixel on a 10,000-pixel render. Runs *after* rung 3, so a tiny real fillet is meshed, not
   discarded
5. relaxed (10× deflection, 45°)
6. **approximate**: a UV-grid sample of the surface, clipped to the face by `BRepClass_FaceClassifier`
7. **missing**: outline only

Each face records which rung produced it as a `FaceStatus` in the contract (`src/lib.rs`, mesh
format `STEPVMSH` v2). That is what makes **a broken-face overlay** possible: a renderer draws
`Approx` faces with a warning material and `Missing` faces as an outline (`LineKind::MissingOutline`),
and badges the part. `FaceStatus::is_faithful()` is the line between drawing normally and drawing
with a warning. A throwaway SVG prototype confirmed the data supports it: amber hatching on the
approximated faces, plus a "⚠ N faces approximated" badge.

Faceless parts are now drawn as curves. If the whole file has no faces, they are `LineKind::Sketch`
(the file is a sketch: draw it). In a file that also has solids (43 files, 149 parts:
construction geometry, axes, the NIST files' PMI-related curves), they are
`LineKind::Construction`, which renderers hide by default. Drawing them would clutter every
preview of a part that happens to carry its construction sketch.

Result, same corpus, same settings:

<!-- pyml disable-num-lines 9 line-length -->
| Source | Files | Pass | Wireframe | Degraded | Partial | Clean fail | Bad mesh | Crash | Timeout | Pass rate |
| --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| abc-dataset | 300 | 298 | 1 | 1 | 0 | 0 | 0 | 0 | 0 | 99.7% |
| nist-pmi | 33 | 33 | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 100.0% |
| occt-import-js | 57 | 56 | 0 | 1 | 0 | 0 | 0 | 0 | 0 | 98.2% |
| stress-assembly | 1 | 1 | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 100.0% |
| **all but malformed** | **391** | **388** | **1** | **2** | **0** | 0 | 0 | **0** | **0** | **99.5%** |
| malformed (want clean fail) | 10 | — | — | — | — | **10** | 0 | 0 | 0 | — |

| Faces failing first pass | Remeshed | Healed | Refined | Coarse | Degenerate | Approx | Missing |
| ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| 54 | 0 | 5 | 18 | 0 | 28 | 3 | 0 |

*Pass* = every face faithful. *Wireframe* = a sketch, fully drawn, counted as a pass. *Degraded*
= drawn, but some faces approximated and flagged. Not a pass. The ladder costs no measurable
latency: p95 for files under 1 MB is 335 ms (was 337 ms). The two rungs that never fired on this
corpus, re-mesh and coarse, are kept as cheap first attempts; nothing here argues for them, and
the next exporter might.

### S2 — The CLI for real (#5): done

`stepv <file> --info | --png | --glb | --mesh`, and `stepv view`. Every run past argument parsing
prints one JSON line (header metadata, outcome, kernel summary), including on failure. Exit codes
are as documented: `0`; `2` usage; `3` no geometry (unreadable, tessellation failed, memory cap),
with the metadata intact; `4` timeout.

- **`--info`** (`src/header.rs`) parses the Part 21 header directly (with its string escapes), the
  IGES Start/Global sections and the BREP banner, plus a bounded streaming PRODUCT count. It never
  touches the kernel and cannot fail.
- **`--png`** (`src/render.rs`) is a CPU rasteriser with per-face colour, a z-buffer and 2×
  supersampling, and it draws the **broken-face overlay**: amber stripes for approximated faces, a
  red outline for missing ones, and a badge. Construction curves are hidden.
- **`--glb`** (`src/glb.rs`) writes glTF 2.0: a node per named part, primitives per (colour,
  status), an `extras` face status and a `stepv:approximated` material. It is validated by the
  `gltf` crate.
- **`--mesh`** writes the validated STEPVMSH buffers, for front-ends that build their own scene.
- **Cache:** hits and failures are cached, timeouts are not, and writes are atomic.
- **Limits:** the timeout kills the child. The **memory cap is the child's footprint** (Linux RSS;
  macOS `ri_phys_footprint`), sampled every 5 ms, with a 4 GiB default. `RLIMIT_AS` was tried and
  rejected: macOS ignores it, and on Linux it caps virtual address space, which OCCT and glibc
  arenas reserve by the hundreds of MB. A small file's footprint stays under 5 MB, because shared
  dylib pages don't count.
- **Kernel:** per-face XCAF colours (STEPVMSH v3); `kernel/fixture-gen` writes the committed test
  corpus in `tests/data/`.
- **`stepv view`** (`src/viewer.rs`, default-on `viewer` feature) is an orbit viewer over the same
  rasteriser. `--no-default-features` builds a headless CLI.

Tests: 66 in total. `tests/cli.rs` runs the real binary against the real kernel and *fails*, not
skips, without it. On the 391-file corpus with default limits (`just cli-sweep`), every real file
exits 0 and every malformed file exits 3 with metadata.

### S3 — Limits and failure modes (#6): done

- **Timeout:** fires, exit 4 (tested; `--timeout 0.001`). The 221 MB stress assembly takes 19.5 s
  against the 20 s default, and the `.thumbnailer` uses 15 s, so it fails there as a timeout. That
  is the intended behaviour.
- **Memory cap:** fires, `memory-cap`, exit 3. The test is deterministic, using a kernel test hook
  that allocates 512 MiB against a 64 MiB cap. ABC `00000046` (7.2 GB uncapped) is killed at a
  1 GB cap.
- **Broken files:** empty, truncated, noise and wrong-format files all exit 3 with header metadata
  (tested; plus the 10-file `malformed` set in the sweep).
- **Decision (perforated-face pathology):** no automatic coarser retry. It would double worst-case
  latency, and at thumbnail settings (30°) that file already fits the 4 GiB cap. The front-ends
  show header metadata on failure instead.

### S4 — Linux front-ends (#7): done

`packaging/linux/` contains:

- a freedesktop `.thumbnailer` (GNOME, XFCE);
- MIME XML with content magic for STEP and a new `model/x-brep` type;
- a `.desktop` entry opening the formats in `stepv view`;
- `install.sh` (`PREFIX`/`DESTDIR`, `--integration-only` for an AppImage user).

`packaging/kde/` is a KF6 `ThumbnailCreator` that shells out to the same CLI, so the kernel's
containment and limits apply in Dolphin. `scripts/test-linux-integration.sh` runs the thumbnailer
exactly as the desktop does (4 formats, plus a clean failure with no image on a broken file), and
checks the compiled MIME database and the desktop entry. CI builds the KF6 plugin and runs that
test on Linux.

### S5 — macOS front-ends (#9): done

`macos/` contains a host app declaring the STEP/IGES/BREP types, a `QLThumbnailProvider` and a
`QLPreviewingController`. `scripts/build-macos-app.sh` builds it **without Xcode**: the CLT's
`swiftc` and SDK suffice, with the `_NSExtensionMain` entry point and hand-assembled bundles. It
relocates OCCT out of `/nix/store` into one shared `Frameworks/` (57 MB) and signs inside-out.

- **The extensions run the kernel IN-PROCESS.** The Quick Look extension sandbox forbids exec:
  `posix_spawn` fails with EPERM, while `stat` of the same file succeeds. So the kernel gained a C
  ABI (`kernel/stepv_occt.h`, libstepvocct, serialised because OCCT's readers share global state),
  and the renderer and header reader gained one (`capi/`, `stepv-capi`). That was the "second
  consumer" this plan named as the split trigger. Containment on macOS is the extension process,
  which the system runs apart from Finder and kills on hang or memory pressure.
- **Buffers, not USDZ:** the preview builds an `SCNGeometry` straight from STEPVMSH, with
  per-face colour and the overlay. The thumbnail is pixel-identical to Linux's.
- **Two traps, now in the code comments:**
  - The types must not conform to `public.3d-content`: Apple's SceneKit thumbnailer claims it,
    wins the dispatch for BREP, and fails.
  - `ThumbnailsAgent` caches type graphs until logout. After changing a type declaration, change
    its identifier or log out.

`scripts/test-macos.sh` checks three things. The Swift reader agrees with Rust on parts and
triangles. The preview scene renders offscreen with its colours. With `--quicklook`, Quick Look
itself thumbnails all 4 formats through the installed app. All pass. CI runs the first two on
macOS; the third needs a logged-in session.

### S6 — Release (#10): wired, waiting on credentials

- **`prepare-release-extension.yml`** sets the crate versions on the release branch
  (`scripts/set-version.py`); devkit's freeze only covers CHANGELOG.md.
- **`release-extension.yml`**, the seam that blocks publication on failure, builds and gates the
  release artefacts:
  - a relocatable Linux tarball: binaries, every library and nix's own loader. Not an AppImage:
    nix-appimage needs unprivileged user namespaces, which Ubuntu 24.04+ forbids. It is proven in
    debian:11, ubuntu:22.04/24.04 and fedora:41 containers without /nix;
  - a DMG, Developer-ID signed, notarised and stapled when the Apple secrets exist (a *final*
    release refuses to ship without them; a candidate falls back to ad-hoc);
  - build-provenance attestations for both;
  - `cargo publish -p stepv` from the gated `crates-io` environment, by token for the first
    publish and by Trusted Publishing (OIDC) after.
- **`release-assets.yml`** attaches the attested artefacts to the published Release. The seam's
  token ceiling, `contents: read`, can't.
- The Kernel CI builds the Linux tarball (proven in clean debian/ubuntu/fedora containers) and the macOS app on every PR, so neither is first built on
  release day. The crate packages to 34 files (98 KB) and verifies from its own tarball.

What only a human can do is listed in §7 "Owed".

### Sandbox (#18): done

The tests came first (`tests/sandbox.rs`). The kernel hook `STEPV_OCCT_TEST_ESCAPE` tries one
forbidden action from inside the sandbox: a TCP connect, a UDP datagram, exec, a write outside the
mesh, or a read outside the input's directory. The tests assert that the action is refused and has
no effect, that the input's directory stays readable, and that the run still produces its mesh.
Against the unsandboxed kernel, all seven refusal tests failed. With the sandbox:

- **macOS** (Seatbelt): every action is refused (EPERM). `harness --strict` is unchanged at 99.5%
  (388/391), cli-sweep is clean, and `s1-c5-214` still loads all 11 parts from its 12 sibling
  files. `scripts/test-macos.sh` also holds both extensions to exactly app-sandbox + read-only
  files, and checks that the bundled CLI's kernel runs sandboxed.
- **Linux 7.0** (Landlock ABI 6 + seccomp, run with `scripts/test-linux-sandbox.sh` in a Debian
  container with OCCT 7.8.1): network and exec are refused by seccomp (EPERM), files by Landlock
  (EACCES). Over all 401 corpus files, the sandboxed and unsandboxed kernels give the same summary.
  The meshes are byte-identical with one exception, ABC `00000143`: its parallel meshing differs
  from run to run on the unsandboxed kernel too. Nothing printed a permission error.
- **Found on the way (#22):** an early version broke relative input paths, every file then failed
  cleanly, and `harness --strict` still exited 0. `--min-pass` now puts a floor under the pass
  rate, and CI uses it.

---

### Topology export (#21, kernel side): v1

`stepv-occt --topology <out.json>` (and `stepv --topology`) writes the exact facts a viewer needs
for a model tree and for measuring on the B-rep instead of the mesh. The format is at the top of
`kernel/topology.cpp`, and its Rust types are in `src/topology.rs`:

- the assembly **tree**, with names, as XCAF has it (the mesh only has the flattened parts);
- per placed **part**: its prototype and its 3x4 placement;
- per **prototype**: area, volume (solids only) and bbox;
- per **face**: its surface type and parameters (a plane's normal, a cylinder's axis and radius,
  cones, spheres, tori), its area, and the indices of its edges;
- per **edge**: its curve type and parameters (a circle's centre and radius), its length, and its
  vertices; and the vertex positions.

The indices line up with the mesh: `parts[i]` is the mesh's part `i`, and a prototype's
`faces[j]` is mesh face id `j`. A picked triangle therefore names its exact face.
`Topology::check_against` enforces this, and the CLI refuses a file that fails it. The tests check
the numbers against hand calculations: the bracket plate's volume is 6000 − 80π, its hole radius
is 4, and the pin's volume is 60π.

Not in v1: edge polylines, which a viewer needs in order to pick edges on screen. They belong
with the mesh (a STEPVMSH v4) once the viewer exists. Point-to-point distances between shapes
(`BRepExtrema_DistShapeShape`) are kernel queries a viewer would make on demand, not data to
precompute.

### Viewer stack (#27 spike, 2026-10-05): egui + wgpu

**Decided:** #21's cross-platform viewer (option B) is built on egui + wgpu (eframe), in-process in
`stepv view`, with the minifb viewer kept as the `--software` fallback. The alternative was
three.js in a browser fed by a loopback server.

Both were prototyped against the same kernel output (`spikes/` on `feature/27-viewer-stack-spike`)
and measured. The full table is on #27.

- **Rendering speed did not decide it.** At matched settings (the 1.77M-triangle stress assembly,
  1280×800, MSAA, GPU-synchronised), both draw a frame in under 1 ms on Apple Silicon. On Linux
  with no GPU, wgpu runs on lavapipe at about 6 fps for that model.
- **What decided it:**
  - No loopback listener beside a kernel that #18 just sandboxed.
  - One language and toolchain. The minified JS failed the repo's hooks.
  - No browser launch: 5.6 s cold.
  - The tested Rust (camera, overlay rules, topology checks) is reused, not re-implemented in JS.
- **What it costs:**
  - 188 crates and a 12–20 MB binary, so it stays behind the `viewer` feature, with a CI tripwire
    keeping it out of the Quick Look capi.
  - Real API churn: three breaking renames hit in one afternoon. Versions are pinned.
  - A heavier Linux build: a 4 GiB VM needs `-j2`.
- **The condition that would have flipped it:** an egui model tree failing at scale. It doesn't when
  virtualised: 40k nodes cost 2.2 ms per frame. Built naively from nested headers, the same tree
  costs 38 ms.
- **Reviews:** two rounds of fresh agent reviews. Round 1 covered graphics, packaging/security and
  CAD product. Round 2 covered methodology, which caught that the first frame-time comparison was
  invalid, and productisation, which supplied the follow-ups.

The prototypes' data plane is throwaway. The product is a fresh `view::` module: #28 (scaffold,
fallback, tripwire), #29 (id-buffer picking), #30 (virtualised tree), #31 (overlay, edges,
STEPVMSH v4), #32 (CI, packaging), #33 (measurements through a sandboxed kernel query channel),
and #34 (capping, fat lines).

### Viewer scaffold (#28, 2026-10-05)

`src/view/` replaces `src/viewer.rs`. `stepv view` draws on the GPU, and reports `"backend"`.

- **GPU layout:** struct-of-arrays buffers for positions, normals, a face id per vertex, and
  indices, with each part a range drawn as its own instance.
  - Per face: a material (colour and `FaceStatus`) in a storage buffer.
  - Per part: the first face in that table, and a visibility bitset.
  - No per-vertex colour.
  - STEPVMSH writes each face's vertices separately, so a per-vertex face id costs nothing. The
    layout would split a shared vertex, and `tests/view.rs` checks that every committed file needs
    none.
- **The camera** is `render.rs`'s, as an orthographic matrix with the same sphere fit. The tests
  hold the GPU silhouette to the software one (IoU > 0.95 for every `tests/data` file) and the
  shading to within 3/255.
- **Rendering:** into the viewer's own target, 4× MSAA resolved to an `Rgba8Unorm` texture that
  egui samples. The shaders encode sRGB themselves: egui blends in gamma space, and an `Srgb`
  target with a `Unorm` view needs view formats, which wgpu's GL backend lacks.
  - A frame renders only when its `RenderKey` (camera, toggles, size, theme) changes.
- **Fallback:** `--software` is the minifb window. It is also used, with a note on stderr, when no
  usable adapter exists.
  - A usable adapter grants the limits the viewer asks for and reads storage buffers in fragment
    shaders, so WebGL2-class GL doesn't qualify. It also supports a base vertex.
  - The probe and eframe's adapter selector apply the same test. When the window's adapter fails
    after the probe passed, the window fails before it opens, and the scene still reaches the
    software viewer. The code review caught this path missing; `scripts/test-viewer.sh` exercises
    it through `STEPV_VIEW_REJECT_ADAPTERS`.
- **Approximated faces** keep `render.rs`'s amber stripes, drawn from `FaceStatus` in the storage
  buffer. The rest of the overlay (missing-face badge, edges) is #31.
- **The capi tripwire** (`just capi-tripwire`, CI on both OSes) fails on any eframe, egui, winit,
  wgpu, naga or minifb crate in stepv-capi's graph, or symbol in `libstepv_capi.a`. It was checked
  red: a `-p stepv -p stepv-capi` build unifies `viewer` on, and the tripwire names nine crates.
- **Theme:** shadcn-style tokens (zinc neutrals, one blue accent, radius 6, a 4-pt grid) applied
  through egui's `Style` and `Visuals`.
  - Tested for WCAG AA. That caught zinc-500 text on the muted fill at 4.4:1, now a shade darker.
  - Phosphor icons, and four in-house components.
- **Measured** on the stress assembly, Apple M3 Ultra, Metal:
  - 1.77M triangles upload in 43 ms.
  - A GPU-synchronised frame (1280×800, 4× MSAA) costs p50 0.88 ms and p95 1.37 ms.
  - The viewer's steady footprint is about 360 MB, 98 MB of it GPU. The rest is system frameworks
    (CoreUI, ICU, Metal caches): malloc_history shows no stepv allocation of 1 MB or more left
    after upload.
  - The 2.1 GB the prototype was charged with is the kernel child's peak (2078 MB), not the
    viewer's.
- **Licences:** egui embeds fonts under OFL-1.1 and the Ubuntu Font Licence. They are allowed in
  `deny.toml`, and their texts are in `licenses/`.

### Work queue (ordered, 2026-10-04)

Agent work, in order:

1. ~~**#18 Sandbox the kernel on every path**~~: done, see "Sandbox (#18)" above. Found on the
   way: #22, the harness's pass-rate floor.
2. ~~**#20 Quick Look preview bugs**~~: done (#24). Every front-end opens on
   `render::Camera::for_scene`; the preview gained scroll zoom and a readable info panel.
3. **#19 Multi-file assemblies blank in Quick Look** (`priority:medium`, #25): the honest message.
   Widening Quick Look's read access stays undecided; §6 "Sandbox read scope" has the trade-off.
4. **#21 Viewer: model tree, sections, measurements** (`priority:medium`). The kernel side is
   `--topology`. The platform is decided: B, egui + wgpu ("Viewer stack" above). The work is
   #28 (scaffold, done: "Viewer scaffold" above) → #29 picking → #30 tree → #31 overlay and edges →
   #32 CI and packaging → #33 measurements → #34 capping.

Needs a human (`needs-human`): #3 org-secret grants (`priority:blocking`), #16 dependency-graph
toggle, #10 the Apple and crates.io credentials, #12 corpus collection, #14 the upstream report.

## 6. Open questions

- ~~**Precompiled `.cwasm` distribution.**~~ Moot: Plan B has no WASM module (S1).
- ~~**Shipping OCCT.**~~ Answered in S5/S6. stepv.app bundles one shared copy, 57 MB in total. The
  Linux release is an AppImage of the nix closure, which is distro-independent. Source builds
  supply their own OCCT.
- ~~**LGPL compliance wording** (#8).~~ Done: `NOTICE`, plus OCCT's LGPL-2.1 and exception
  verbatim in `licenses/`, shipped in every package.
- **`occt-import-js` for a web path.** Same kernel, browser-first, LGPL-2.1. Likely the right answer
  if a web preview is ever wanted; explicitly out of scope now.
- ~~**Does `occt-wasm` expose `ShapeFix`?**~~ Answered in S1: yes (`fix_shape`, `heal_solid`,
  `heal_face`, …), and moot, since its STEP import does not work. Under Plan B the STEP reader
  applies OCCT's default shape processing during transfer.
- ~~**Perforated-face meshing.**~~ Decided in S3: the limits contain it, with no automatic coarser
  retry.
- **Sandbox read scope (#18, #19).** The kernel may read the input's directory and below. An
  external reference that points *above* it (`../parts/x.stp`) is refused and shows as missing.
  Widening that would mean trusting more of the disk to a hostile file. Quick Look is narrower
  still: it grants the one file, which is why multi-file assemblies are blank there (#19).
- **macOS containment.** In-process, the extension cannot cap one file's time or memory itself.
  It relies on the system killing a hung or ballooning extension. If that proves too coarse in
  practice, the next step is an XPC service inside the `.appex`: launchd may start one where exec
  is forbidden, and it brings back per-file kill.
- **Upstream report for `occt-wasm`** (#14). The STEP-import-needs-a-filesystem gap (§3) is known
  upstream (PR #371 "Known gap"), but no issue tracks it. Filing one is outward-facing, so it is
  left to a human.
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
- **S1 (2026-10-04):** `kernel/` (the Plan B C++ kernel plus the `stress-gen` fixture generator, both
  built by `just kernel`); `src/occt.rs` (subprocess driver, timeout, `STEPVMSH` decoder, tests);
  `examples/harness.rs`; `just fixtures` implemented by `scripts/fetch-fixtures.py`, with every
  download pinned by sha256 and the resulting bytes recorded in the committed
  `tests/fixtures/corpus.sha256`; `flake.nix` with `pkgs.opencascade-occt` and the `native` module
  enabled.

### Owed — only a human can do these

Everything else is built and tested. Each item below needs a credential, an account or a click
that an agent must not take. Each one is the last step before something ships.

1. **Approve the org-config apply** (org-config#317's `Apply` run waits on the `production`
   environment). It gives stepv its rulesets: branch and tag protection, signed commits.
2. **Grant the workflow credentials** (#3): add `stepv` to the six org-secret repository lists by
   hand. The apply does not do this (org-config#318), and devkit's sync, upgrade and release
   workflows need it.
3. **Enable the dependency graph** (#16): the repo-settings toggle, which has no API. Until then
   Dependency Review is red on every PR.
4. **Apple Developer ID** (#10), for a notarised macOS release. Store these as repo or org secrets:
   `MACOS_SIGNING_P12`, `MACOS_SIGNING_P12_PASSWORD`, `MACOS_SIGNING_IDENTITY`,
   `APPLE_NOTARY_KEY_P8`, `APPLE_NOTARY_KEY_ID` and `APPLE_NOTARY_ISSUER`. The header of
   `release-extension.yml` lists them. A final release refuses to ship without them.
5. **crates.io** (#10):
   - Create the `crates-io` environment, ideally with a required reviewer, and give it a
     `CARGO_REGISTRY_TOKEN` for the **first** publish.
   - After that release, configure stepv's trusted publisher on crates.io and delete the token.
     The workflow then switches to OIDC by itself.
   - Declare the environment in org-config if environments are governed there.
6. **Corpus** (#12): the full CAx-IF rounds (registration) and real SolidWorks, NX, CATIA, Fusion
   and FreeCAD exports, collected by hand. They're needed before the pass rate can be claimed for
   those exporters.
7. **Upstream report** (#14): file the occt-wasm STEP-import gap with andymai/occt-wasm, if wanted.
8. **Devkit fixes** to watch, not do:
   - vig-os/devkit#1810: `mkRustProject` drops hook knobs, and the scaffold fails its own lint. It's
     worked around here with `# deadnix: skip`.
   - vig-os/devkit#1811: the Rust pack's CI lanes ran no Rust. It's worked around in
     `justfile.project` and `kernel.yml`.

Done since S1, for the record: kernel CI (#11, `kernel.yml`: nix build, flake check, strict
harness, CLI sweep, Linux tarball, macOS app, Linux integration, advisories) and `deny.toml` (#13:
bans/licenses/sources inside flake check, advisories in CI).

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

The declaration merged as [`vig-os/org-config#317`](https://github.com/vig-os/org-config/pull/317)
on 2026-10-04 (content reproduced in §8). It superseded #316, which was closed by a branch rename,
not deliberately. As of 2026-10-04 its `Apply` run is **waiting on the human-gated `production`
environment**. Until someone approves that deployment, stepv has **no branch protection, no
signed-commit rule and no tag protection** (`gh api repos/vig-os/stepv/rulesets` returns 0).

**Approving the apply will not deliver the workflow credentials either** (stepv#3). This plan used
to say the opposite. `otterdog apply` does not reconcile `selected_repositories` for org secrets
whose declared value is a `'********'` dummy, and the #317 plan is "4 to add, 11 to change", with
zero secret actions. `revkit` proves it: it was added to the same six lists in org-config#312 and
is still absent from them days after a successful apply. That is tracked upstream as
[`vig-os/org-config#318`](https://github.com/vig-os/org-config/issues/318). Until an org owner
adds stepv to the six lists by hand (read-modify-write: the endpoint replaces the whole list; the
commands are in stepv#3), the devkit workflows fail with an **empty credential and no error
message**. That is this gap, not a bug in the workflows. Nothing in S2–S3 needs them.

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
