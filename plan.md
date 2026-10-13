# stepv — implementation plan and handoff

**Status:** S1–S5 built and tested; S6 (release) wired and **waiting on human credentials**
(§5 "S6", §7 "Owed"). The kernel is native OCCT 7.9.3. It runs as a subprocess on Linux, and
in-process inside the macOS Quick Look extensions, whose sandbox forbids exec. With the per-face
recovery ladder, 99.5% of a 391-file corpus is shown faithfully, the other 0.5% is drawn with a
flagged overlay, and nothing crashes or hangs. With seven CAD writers and a real 143 MB assembly
added (#12, 2026-10-11) it is 99.2% of 400 (faithful or wireframe; 99.0% fully faithful). The one
new non-pass is the real assembly, which loads correctly but takes 101 s (#51). The CLI, the Linux thumbnailer, MIME and KF6 plugin,
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
`harness-out/results.jsonl` after a run. (Since #12, the exporter matrix and a real large
assembly are in: "Corpus fill (#12)" below.)

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

### Corpus fill (#12, 2026-10-11)

Three gaps in the corpus caveats above, filled as far as public sources allow. Every file is
fetched by `just fixtures`, pinned by sha256, and never committed:

- **`exporter-matrix`**: one file per writer, each writer read from the file's own `FILE_NAME`
  header. They are SolidWorks 2015, Siemens NX 9.0, CATIA V5 R14, Fusion 360 (Autodesk
  Translation Framework), FreeCAD/OCC (KiCad StepUp), Inventor 2021 and Creo (Pro/ENGINEER).
  All come from public repositories, each pinned to a commit URL; the licences are in the
  manifest. The fetcher gained a `files` kind for this.
- **`real-assembly`**: the whole Ultimaker 3 printer, a single 143 MB SolidWorks 2016 STEP
  (AP214) with 258 products, 523 occurrences and every part named and coloured. **CC BY-NC
  3.0**: fine to fetch and test against, never to redistribute. Its zip carries a
  `__MACOSX/._9066.STEP` resource fork, which the fetcher now skips.
- **`mbx-if-public`**: the one AP242 model MBx-IF (the renamed CAx-IF) publishes openly. The full
  CAx-IF rounds are **not** public: they are shared only in the implementor group's members area,
  by invitation. `cax-if` stays hand-collected and reported MISSING.

`just harness`, same settings as S1 (default 60 s per file), sage:

<!-- pyml disable-num-lines 11 line-length -->
| Source | Files | Pass | Wireframe | Degraded | Partial | Clean fail | Bad mesh | Crash | Timeout | Pass rate | Named | Coloured (part / face) | p50 ms | p95 ms | Max RSS MB |
| --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| abc-dataset | 300 | 298 | 1 | 1 | 0 | 0 | 0 | 0 | 0 | 99.7% | 100.0% | 0.0% / 99.2% | 107 | 4963 | 7252 |
| exporter-matrix | 7 | 7 | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 100.0% | 75.0% | 50.0% / 12.5% | 120 | 257 | 96 |
| mbx-if-public | 1 | 1 | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 100.0% | 0.0% | 50.0% / 0.0% | 65 | 65 | 34 |
| nist-pmi | 33 | 33 | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 100.0% | 9.9% | 45.1% / 11.3% | 168 | 364 | 156 |
| occt-import-js | 57 | 56 | 0 | 1 | 0 | 0 | 0 | 0 | 0 | 98.2% | 58.7% | 76.1% / 0.0% | 77 | 119 | 60 |
| real-assembly | 1 | 0 | 0 | 0 | 0 | 0 | 0 | 0 | **1** | 0.0% | — | — / — | — | — | — |
| stress-assembly | 1 | 1 | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 100.0% | 100.0% | 0.0% / 0.0% | 23458 | 23458 | 2078 |
| **all but malformed** | **400** | **396** | **1** | **2** | **0** | 0 | 0 | **0** | **1** | **99.2%** | 94.4% | 7.3% / 80.7% | 101 | 3570 | 7252 |
| malformed (want clean fail) | 10 | — | — | — | — | **10** | 0 | 0 | 0 | — | — | — | — | — | 28 |

- **Every writer loads, fully faithful.** Names and colours vary by writer, as expected; this is
  the first time the columns measure more than Onshape and NIST.
- **The real assembly is correct but slow.** With `--timeout 900` it passes: **100.7 s**,
  1.56 GB peak RSS, 62,011 faces (2 degenerate), 1.25M triangles, and 523/523 parts named and
  coloured. **92.2 s of that is STEP transfer** (read 4.4 s, mesh 3.7 s), against 23 s in total
  for the 221 MB synthetic file. Bytes are not the cost; this file's structure is. At the
  CLI's 20 s default and the thumbnailer's 15 s, the file cannot be previewed today. That is #51.
- No other row moved beyond run-to-run timing noise. *Pass rate* is faithful or wireframe, as in
  the S1 tables: 397/400 here, 396 fully faithful.
- **`corpus.sha256` was not byte-exact for `stress-assembly` until now.** OCCT's writer stamps the
  wall clock into `FILE_NAME`, so every regeneration hashed differently. `stress-gen` now writes a
  fixed time stamp, and a test in `tests/cli.rs` generates twice and compares bytes. The lock's
  `stress-assembly` line changes once, here.
- The fetcher and the harness agree on what a corpus file is: both skip macOS `__MACOSX/` and
  `._*` entries. The harness's own unit tests never ran before (`cargo test` skips example
  tests); `just test` now runs `cargo test --example harness`.

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

**A failure is non-fatal** (#38). The topology comes after the mesh. If it fails (an OCCT
exception in one prototype, an unwritable file), the kernel reports `topology_error`, removes what
it wrote, and the run still succeeds with its mesh. `stepv --png o.png --topology t.json` writes
the PNG and reports `topology_error` instead of `topology`; `--topology` alone fails. `stepv view`
opens without the inspector after one kernel run. The stage shares the run's `--timeout` and
memory cap, and running out of memory stays fatal: the limits protect the machine, and a kill
cannot keep half a run. A file that fits its limits only without the topology fails with it.

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

### Picking (#29, 2026-10-05)

- **The id pass:** a single-sample `Rg32Uint` target, cleared to 0, written as
  `(part + 1, face)`. It uses the same vertex shader, visibility bitset and section plane as the
  image, so a pick hits exactly what is drawn: nothing hidden, nothing through a cut.
  - The issue allowed `part << 20 | face` in an `R32Uint`, which caps an assembly at 4096 parts. Two
    channels cost 4 bytes more per pixel, and only on click.
- **Readback:** a click renders the id pass at the viewport's size and copies one texel, mapped
  asynchronously. The result lands on a later frame: no stall.
- **The highlight** is a second pass over the picked face's part. Its fragment shader keeps only
  that face. It is blended in the accent colour, with a negative depth bias and no depth write, so
  it beats its own face and loses to anything in front. The material is untouched.
- **The inspector** is `view::inspect`: rows from `--topology`, placed into model coordinates by
  the part's transform (a pin's axis is where that pin stands).
  - `stepv view` now runs the kernel with `--topology`. If the topology fails (in the kernel,
    #38), or fails to parse or match, the viewer still opens, says so, and shows mesh ids only.
- **Section plane:** `Section { axis, offset, flip }` across the model box, applied in every pass:
  image, ids, highlight, lines. #34 adds capping.
- **Acceptance**, headless against the real kernel (`tests/view.rs`), at pixels projected through
  the viewer's own matrix:
  - the plate's hole wall reads Cylinder, r = 4 mm;
  - its top reads Plane, normal +z, area 1200 − 16π.
  - `scripts/test-viewer.sh` also clicks through a real window (`STEPV_VIEW_PICK`).

### Model tree (#30, 2026-10-05)

- **`view::tree::Tree`** flattens the assembly tree once into a preorder vector of rows: name,
  depth, part, parent, and `end`, so a subtree is the range `i + 1..end`. Without a topology, the
  tree is a flat list of the mesh's part names.
- **Per frame**, it costs only what is on screen:
  - The display list (expanded rows, or search matches with their ancestors) is rebuilt only when
    expansion or the search changes.
  - Each row's shown-leaf count is recomputed bottom-up only when visibility changes. An
    assembly's tri-state checkbox therefore costs O(1), however many parts it holds.
  - `ScrollArea::show_rows` lays out only the visible slice, which a test checks: 40,401 rows give
    fewer than 60 laid out.
- **Visibility** is the per-part bitset the GPU already honours (`GpuScene::set_visibility`).
  - A new tree generation re-uploads it and re-renders.
  - Hidden parts can't be picked.
  - A selection whose part gets hidden is dropped, as #29's review asked.
- **Selection:**
  - A tree row selects the whole part: `Pick::part`, with `face = u32::MAX`, which the highlight
    pass draws over every face of it. The inspector then shows the part.
  - A face picked in the view opens the row's ancestors and scrolls to it.
- **Measured** (release, M3 Ultra; `forty_thousand_nodes_stay_under_four_ms`): 40,401 rows, all expanded, in a headless egui frame of layout plus tessellation. p50 0.072 ms, p95 0.075 ms, against a 4 ms budget; egui caches the row galleys between frames. #27 measured 2.2 ms for its virtualised tree inside a whole window frame, GPU painting included. The naive nested headers cost 38 ms.

### Edges and the overlay (#31, 2026-10-05)

- **STEPVMSH v4:** `stepv-occt --edges` appends three arrays to each part: `edges`, `edge_ids`
  and `edge_lens`, then the points. The edges are B-rep polylines sampled at the mesh's own
  deflection, in the prototype's frame, then placed.
  - The numbering is `stepv::topology_edges`, now the one function `--topology` uses too, so an
    edge id names its JSON curve by construction. A test checks every id against every
    `tests/data` file's topology.
  - Only `stepv view` asks for v4. Thumbnails, `--glb`, `--mesh` and Quick Look's in-process path
    stay v3, so the issue's "Quick Look keeps v3 behind a flag" holds.
  - The Swift reader also accepts v4 and skips the edges; `just macos-test` decodes a real v4
    file against v3.
  - `assembly.step` has 21 edges.
- **Drawing:** a LineList, pulled 0.0005 depth units toward the eye in the vertex shader (WebGPU
  forbids depth bias on lines), so an edge wins over the faces it bounds.
  - Near-black; E toggles them.
  - A picked edge draws in the accent.
- **Edge picking:** the id pass draws the edges too, writing `EDGE_BIT | edge`.
  - A click reads a 9×9 window, and the nearest edge within 4 px wins over the face under the
    pointer (`resolve_pick`): a one-pixel line is otherwise unclickable.
  - `inspect::edge` gives the curve and its parameters, plus a length scaled with the instance.
  - **Acceptance:** two pixels off the plate's hole rim, the pick reads Circle, r = 4 mm, length 8π.
- **One overlay rule:** `render::overlay(status)` decides which faces are striped. `render.rs`
  uses it, and the GPU materials carry its value (`Material::overlay`).
  - A test holds every status to it.
  - Another checks that both renderers stripe the same pixels (IoU > 0.9).
  - Missing faces are a line (their outline), drawn on top in both.

### Viewer CI and packaging (#32, 2026-10-05)

- **`--frames N`** is the bench mode that exits. Both windows orbit 1° a frame for N frames and
  report the wall-clock intervals as `"frames": {count, p50_ms, p95_ms}`. The first interval,
  which opens the window, is dropped.
  - On a display these are vsync-bound: about 16.7 ms on Metal.
  - On lavapipe under Xvfb they are p50 6–7 ms for the fixtures.
- **CI, `Viewer (Linux, lavapipe)`** (kernel.yml) runs `scripts/test-linux-viewer.sh`, a Debian
  trixie container with mesa-vulkan-drivers and Xvfb:
  - the renderer's tests, with an adapter required;
  - the window checks, where Xvfb presents, so whole-window screenshots are required
    (`STEPV_VIEW_REQUIRE_WINDOW`);
  - `--frames 30` on every fixture, asserting a GPU backend;
  - the 24 MB budget;
  - then the Vulkan ICDs, EGL vendor files and DRI drivers are deleted, and `--frames 5` must
    report `software` with the "no usable GPU adapter" note.
  - The macOS lane already ran the same window checks on Metal (#28). It adds the `--frames` smoke
    test and the 16 MB budget.
- **Size**, stripped (`scripts/check-size.sh`): macOS 11.7 MB, Linux 15.3 MB. The GPU stack
  roughly doubled the CLI.
- **`cargo deny`:** bans, licenses and sources already run over the whole tree in `nix flake
  check` (all features), and advisories in their own step. The viewer's licences were settled in
  #28.
- **Linux tarball:** the launcher runs our binary through our loader with
  `--library-path ours:host`.
  - The viewer's dlopened window and GPU libraries come from the host and load against our glibc.
  - One that needs a newer glibc fails to load, and the viewer falls back to software.
  - `test-linux-tarball.sh` now checks that `stepv view` exits cleanly (3, with its JSON line) in
    the display-less distro containers.
- **nix:** on Linux, the product's `bin/stepv` is wrapped with `LD_LIBRARY_PATH` for
  vulkan-loader, libGL, libxkbcommon, Wayland and the X11 libraries. The real binary stays in
  `bin/`, so the kernel is still found at `../libexec`. CI prints the closure size and holds it to
  1.5 GiB.

### Measurements (#33, 2026-10-05)

- **`stepv-occt --serve`** reads the file once, without meshing, and answers newline-JSON on
  stdin/stdout. The protocol is in `kernel/measure.h`:
  - `distance`: `BRepExtrema_DistShapeShape`'s value and witness points, plus `axis_distance`
    when both entities have an axis line;
  - `angle`: between outward plane normals (0–180°), a line and a plane (0–90°), or two lines
    (acute);
  - `point`: the nearest point on an entity, and a face's normal there.
- **Numbering:** entities are (part, face) or (part, edge), numbered as `--topology` numbers them.
  Faces are in `TopExp_Explorer` order, and edges come from `topology_edges`. The planar normal
  is the shared `outward_normal`, so the inspector and the measurements agree on "outward".
- **Sandbox:** the server runs in the run's sandbox, with nothing writable: the same read root,
  with answers going to the stdout it was given. The test checks it reports the full sandbox.
- **`measure::Server`** (Rust) keeps one kernel per file and holds every query, and the initial
  load, to the run's `Limits`: the wall clock, and the footprint polled every 5 ms.
  - Past either, or when the kernel dies or speaks nonsense, it is killed. The query reports
    `Timeout`, `MemoryCap` or `Crashed`, and the next query starts a new kernel.
  - A refusal ("no such entity") keeps the kernel.
  - Tested against the real kernel: a SIGKILL, and a hung query (`test_sleep`, which only runs
    with `STEPV_OCCT_TEST_HOOKS`).
- **The viewer:** a worker thread owns the server, so neither a query nor the kernel's first load
  blocks a frame.
  - Measure mode (M) takes two picks, faces or edges, asks for distance and angle, and draws the
    witness segment over the view.
  - `STEPV_VIEW_PICK` now takes several clicks, and model points (`@x,y,z`) projected through the
    live camera. With `STEPV_VIEW_MEASURE`, that drives the window test.
- **Acceptance**, against the real kernel (`tests/measure.rs`):
  - the pins' cylinders are 28 mm apart axis to axis, and 24 mm surface to surface;
  - two adjacent faces of the box meet at 90°, and opposite ones at 180°;
  - `test-viewer.sh` gets 28 mm through two clicks in a real window.

### Section caps and fat lines (#34, 2026-10-05)

- **Caps:** a stencil-parity pass, on the GPU.
  - After the image, every surface left by the cut is drawn again with no colour or depth writes,
    inverting the stencil. At a pixel of the section plane inside a solid, the count is odd.
  - Parity, not front/back counting, because CAD exports don't orient faces reliably.
  - A quad on the plane, spanning the fit sphere (`cap_quad`), is then drawn where the stencil is
    odd. It gets its own depth, so anything in front of the cut still hides it. It's hatched at
    45° in two neutral greys.
  - The depth target became `Depth24PlusStencil8`.
  - Each part inverts its own bit (`1 << part % 8`), so overlapping parts don't cancel each
    other: the fixture's pins pass through the plate, and the plate stays capped under them. Only
    parts that share a bit (eight apart) can still cancel.
  - Parts that aren't closed solids (a missing face, or no volume in the topology) stay out of the
    parity, so an open shell can't hatch the plane outside any solid.
  - The id pass runs the same parity and draws the cap as "nothing", so a click on the hatching
    picks nothing rather than the hidden face behind it.
  - Faces carry a slope-scaled depth bias, so an edge's flat quad also wins on tilted and concave
    corners. A test checks a concave edge keeps at least half a convex one's width.
  - Exact, per-part caps from a kernel section are #43 (below); the stencil cap is now their
    fallback.
  - **Acceptance** (`tests/view.rs`, real kernel): cut at y = 15 through the hole and the pins, the
    plate's profile is capped and the hole is open (its wall shows, in the part's colour).
    `test-viewer.sh` checks the cap in a real window.
- **Fat lines:** edges and sketch lines are instanced quads, six vertices per segment, built in
  the vertex shader at a constant pixel width (1.5 px × the display scale; sketches and outlines a
  quarter wider). They are extended past each end, so polylines join, and MSAA anti-aliases them.
  The id pass draws the same quads, so edges are easier to pick.
- **Edges from the triangulation:** the kernel takes an edge's points from
  `BRep_Tool::PolygonOnTriangulation` on an adjacent face, so an edge lies exactly on the triangle
  boundary it bounds.
  - It falls back to curve sampling where a face was recovered (#31's review).
  - The depth pull drops from 0.0005 to 0.0001.
  - The edge-length test against the topology still holds.

### Exact section caps (#43, 2026-10-05)

The stencil cap is one grey for every part, so an interference (the fixture's pins pass through
the plate) is capped, but nobody can see whose section is whose. Now the kernel cuts each part
exactly, and the viewer draws one cap per part in that part's colour.

- **The kernel op:** `stepv-occt --serve` takes `{"op": "section", "plane": [nx, ny, nz, w]}`,
  the viewer's plane (kept: `dot(p, n) <= w`). Protocol at the top of `kernel/measure.h`.
  - For each part with solids whose box the plane crosses: `BRepAlgoAPI_Common` of the solids
    with a face on the plane, well past the box. That gives the section's faces, holes and
    islands included.
  - The faces are meshed with `BRepMesh_IncrementalMesh` and returned as triangles, in model
    coordinates.
  - The issue planned closed polylines with an earcut in the viewer. The kernel already knows
    which loop is a hole and has a mesher, so the viewer gets triangles and needs no
    triangulator.
  - Sheets and sketches have no cap, as under the stencil. A zero normal is refused.
  - If one part's Boolean fails, the whole query fails, so every part keeps the stencil cap. A
    missing exact cap would show that part's inside as if it were hollow.
  - Cost: 900 parts (the fixture × 300) in 0.27 s for 144 caps, about 2 ms a cap. A query is held
    to the run's limits, as a measurement is.
- **The viewer:** the plane must hold still for 150 ms before the query is sent, so a dragged
  slider doesn't queue one per frame. The worker keeps only the newest section query, apart from
  the newest measurement pair.
  - Exact caps are drawn only for the plane they were cut by. Any other plane, while a query is in
    flight or after the kernel refuses, gets the stencil cap. The panel says "Exact section" or
    "Approximate section", with the reason on hover.
  - A refusal holds for its plane until the plane moves. A timeout, the memory cap, or caps larger
    than the GPU's buffers turn exact caps off for the session. Otherwise every stop of the slider
    would cost another kernel restart.
  - The kernel is untrusted: the server reads at most `MAX_LINE` (16 MiB) per answer line. A
    kernel that writes more is killed (`Error::TooLong`), and that also turns exact caps off. The
    caps' total size is checked against the device before anything is built.
  - Each cap is hatched in its part's colour (fill ×0.6, hatch ×0.22).
  - Each cap is drawn `CAP_STEP` (2⁻¹⁸ of NDC depth) nearer the eye than the earlier caps whose
    boxes it overlaps, and no others. Ranks stop at 63, so the offset stays under 0.05% of the
    model however many parts are cut. Where two parts overlap, the later one shows cleanly, with
    no z-fighting mix of the two.
  - The caps are an index buffer, sized against the device's limit.
  - The id pass draws exact caps as "nothing", as it does the stencil cap.
- **Acceptance:**
  - `tests/measure.rs`: at y = 15, the plate's cap is 160 mm² (40 × 5 less the 8 × 5 hole) and
    each pin's is 60 mm² (4 × 15).
  - `tests/view.rs`: the pins are orange *inside* the plate too, the plate is blue around them,
    and another plane falls back to the grey stencil cap.
  - `test-viewer.sh`: the window counts both parts' cap colours, after the kernel reported
    "exact caps for 3 parts".

### Outline before geometry (#54, step 1, 2026-10-12)

#51 showed where a big file's time goes. The Ultimaker 3 assembly reads in about 5 s, then
spends about 90 s building p-curves. The tree, the names and the part count are all in the parsed
entities already. So `stepv-occt --outline` (`kernel/outline.cpp`) walks the parsed
`StepData_StepModel`, with **no shape transfer**, and prints one JSON line in `--topology`'s tree
format. It runs in the kernel's sandbox with nothing writable, like `--serve`. It is also
`stepv_occt_outline` in the C API, for macOS, and `occt::outline` in Rust, held to the run's
`Limits`.

- **What it mirrors:**
  - the transfer's roots (`XSControl_Reader::RootForTransfer`) and its child order (the NAUO
    list `STEPControl_ActorRead::TransferEntity` builds from the graph's sharings);
  - `STEPCAFControl_Reader::ReadNames`' naming: a product's name, else its id; an instance's
    description, else name, else id;
  - the kernel's own `walk` rules for which name a node shows.
- **The contract** is the *product* level of `--topology`'s tree, `topology::products`. A product
  the transfer splits into unnamed bodies (AP203's hybrid models: a solid plus a wireframe, or a
  nested group of them) is one leaf. Shapeless products, such as dm1's materials, are left out,
  as the transfer leaves them out. Leaf numbers count products, so the geometry's tree replaces
  the outline when it arrives rather than renumbering it.
- **Checked over the local corpus** (390 STEP files, malformed excluded, plus the 143 MB
  Ultimaker file): **388 agree exactly** with `products()` of the full run's tree. The other two
  are multi-file assemblies (`s1-c5-214`, `s1_pe_214`), whose sub-assemblies live in the files
  they reference. The transfer reads those files; the outline reads only the one it is given.
  That's deliberate, and documented in `outline.h`.
- **Time:** Ultimaker 3 takes **4.6 s for the outline against 98.6 s** for the full run. Small
  files take 0.05–0.2 s. The 221 MB stress assembly takes 5.1 s against 22 s, since the parse
  itself is most of its cost.
- **Next in #54:** the `SceneEvent` contract and progress, then a per-face healing budget
  (#51), then caching only complete scenes; then the viewer and the CLI show the outline first.

### Events and progress (#54, step 2, 2026-10-12)

`stepv-occt --stream` turns a run's stdout into a stream of frames: `SVEV`, a kind byte, a
little-endian u32 length, and the payload (`kernel/stepv_occt.h`). Every payload today is one
JSON object.

- **The kinds:**
  - `TREE`: the outline, sent between the parse and the transfer. It is built from the run's own
    reader (`outline_json(STEPControl_Reader&)`), so the file is parsed once. It is left out for
    IGES and BREP, and when the outline itself fails; the run goes on.
  - `PROGRESS`: `{"stage", "done", "total"}`. Stages come in order (`transfer`, then `mesh`).
    Within a stage, `done` never falls and ends at `total`. The kernel sends at most one per
    stage every 50 ms, besides the first and the last. `transfer` is thousandths of OCCT's own
    `Message_ProgressIndicator`; `mesh` counts two passes over the prototypes (the mesher, then
    the recovery ladder).
  - `DONE`: the summary, with `sandbox`, exactly as on the plain line. It is always the last
    frame.
  - Kinds 4 and up are reserved for per-part boxes and meshes (P1). Those payloads will be
    binary, never base64 inside JSON.
- **The C API** has the same events through a callback (`stepv_occt_run_stream`); the CLI's
  callback is what writes the frames. `stepv_occt_run_ex` is that call with no callback.
- **The Rust side:**
  - `occt::read_events` decodes a stream. It refuses a frame over `MAX_FRAME` (64 MiB) on its
    header alone, before allocating, and rejects a bad magic, an unknown kind, a malformed
    payload, bytes after `DONE`, and a stream that ends without it.
  - `occt::run_streaming` runs it under the same `supervise()` as `run`, with `Limits`. Decoding
    happens on the reader thread, which passes events over a bounded channel; the wait loop hands
    them to the caller. Progress is dropped when the caller falls behind (`try_send`), the
    outline never is.
  - Outcomes: a kill (wall clock, memory) keeps the events that arrived and has **no summary**,
    so nothing downstream can mistake it for a complete scene. A kernel that exits with a broken
    stream has no summary and the outcome of its exit, as with `run`: an exit 0 or 3 without its
    summary is a broken contract that stepv reports as an error, not a crash (#62); any other exit
    is `Crashed`.
- **Edges of the contract (from the review):**
  - A stage's first and last progress events are never dropped (`Progress::droppable`), so a slow reader's bar still reaches its end. A run that fails partway leaves its stage unfinished.
  - The kernel leaves out an outline over `STEPV_OCCT_MAX_FRAME`, rather than send a frame the reader would refuse.
  - A killed run has no summary, even when the summary was read before the kill landed.
  - `supervise()` kills and reaps the kernel on every way out, including a panicking callback.
  - `--stream` ignores SIGPIPE, so a reader that hangs up produces a failed write rather than a signal.
  - The outline's parse counts against the same `Limits.timeout`, so a run close to its cap can time out when streamed and not when plain.
- **Measured on Ultimaker 3:** the tree arrives at 4.3 s and the summary at 101 s, in 101 frames.
  Transfer progress reaches 15.6% by 15 s and then **does not move for 80 s**. OCCT's indicator
  does not advance inside one solid's shape healing, and solid 221's p-curve projection is that
  80 s (#51). Progress is honest about where the time goes, but it can't make that wait shorter.
  The per-face budget (step 3) is what does.
- **Tests:** `tests/stream.rs` checks:
  - the outline arrives first and equals `--outline`;
  - progress is monotonic and every stage finishes;
  - the summary and mesh match a plain run;
  - IGES and BREP send no tree;
  - a clean failure still sends its summary;
  - a kill after the outline keeps the outline and has no summary (via the
    `STEPV_OCCT_TEST_STALL_MS` hook);
  - six fake kernels' broken streams are crashes;
  - `--stream` refuses `--serve` and `--outline`.

  The decoder's unit tests cover every truncation point and each rejection.

### Per-face healing budget (#51, #54 step 3, 2026-10-12)

OCCT's readers heal every transferred shape with the `FixShape` operator. On a file without
p-curves that means projecting every edge onto each face it bounds, and on a few faces
projection is pathological: two B-spline ribbon faces of Ultimaker 3 (2 × 1,079 poles) take
about 25 s each, at about 350 ms per edge, while their twins on surfaces of the same size take
1 s. `GeomProjLib::Curve2d` is no way out: 1.3 s per edge on the same faces.

- **The mechanism (`kernel/heal.{h,cpp}`):**
  - stepv registers its own `FixShape` before any reader exists. OCCT's registry keeps the
    first operator registered under a name, and `install_face_budget()` checks that ours won.
  - It sets `ShapeFix_Shape` up from the same context parameters as OCCT's operator. The table
    is derived from OCCT's `fixshape()` (keys, order, defaults) and says so; whether that needs
    more than the comment, given that stepv is Apache-2.0 and OCCT LGPL, is an open question for
    a human.
  - It fixes the faces itself, one at a time, as `ShapeFix_Shell` and `ShapeFix_Shape` do: shell
    by shell, then free faces with the two modes `ShapeFix_Shape` sets for one.
  - The edge tool's projector is a subclass with a clock. It is set through a member pointer,
    since `myProjector` is protected and has no setter.
  - Once a face has spent `STEPV_OCCT_FACE_BUDGET_MS` (default 2000) projecting, the next edge
    throws `stepv::BudgetSpent`. That is not a `Standard_Failure`, so OCCT's handlers let it pass
    and it abandons the whole face fix.
  - The face is taken out through the shared `ShapeBuild_ReShape`. Then `ShapeFix_Shape` runs
    with face fixing off, for the shells and solids.
- **Why the face must leave the shape:** caught as an ordinary failed projection, the face would
  stay in its solid without p-curves, and `ShapeFix_Solid::SolidFromShell`'s classification
  dereferences the missing curve. That is a segfault, not an exception.
- **The unhealed faces come back** as loose faces in a compound beside the healed shape, and are
  recorded. The mesher draws each as its outline (`Missing`), without the recovery ladder, and
  counts it in `faces_over_budget`. The topology still numbers it: loose faces come after the
  solid's. The ladder's ShapeFix rung carries the same budget.
- **What the review (round 1) fixed:**
  - **The bounding box.** Without p-curves OCCT bounds a face by its whole surface, and a
    cylinder's is infinite: the scene's box became ±1e100. `stepv::add_bounds` bounds an
    unhealed face by its edges, for the scene, the mesh header and the topology. The topology's
    area for such a face is still 0, since it is unknown.
  - **Measurement.** `--serve` re-reads the file, and a clock does not repeat: a face near the
    budget could be healed in one process and not the other, and every later face in its part
    would be numbered differently. So faces are numbered in the order they are fixed. The run
    reports the numbers it gave up on (`unhealed_faces`), and `STEPV_OCCT_UNHEALED` replays them
    exactly, with no clock on the rest. The viewer hands the list to its measurement server
    (`measure::Server::with_unhealed`).
    - Faces are numbered by their place in the shape as it reached the reader, so fixing can't
      shift the numbers (review round 2).
    - Its one limit: the fix of a face given up on may already have changed shared edges, and
      a replay doesn't repeat that. So the neighbours of an unhealed face can differ slightly
      between the drawn model and the measured one.
    - An older kernel reports no list, and then the server goes by its own clock.
  - **Colours and names on an unhealed face.** Removed, the face was mapped to nothing in the
    healing history, so the reader lost its own colour. It is mapped back to itself.
  - **Tails.** As in OCCT, the second pass finds the faces already fixed.
  - **The install check is no longer silent.** If OCCT's `FixShape` was registered first, the
    kernel warns on stderr.
- **Measured on Ultimaker 3:**

  | | before | after |
  | --- | ---: | ---: |
  | wall | 100.7 s | 42.3 s |
  | transfer | 92.2 s | 34.2 s |
  | faces over budget | — | 4 |
  | peak RSS | 1.56 GB | 1.52 GB |

  It is still 523/523 parts named and coloured, with 62,011 faces and 1.24M triangles.
- **The corpus is unchanged:** all 401 files give an identical summary, field for field, against
  `origin/main`'s kernel, and transfer times match.
- **Still over the CLI's 20 s:** the transfer's remaining time is mostly OCCT's wire
  self-intersection check. It walks a linked sequence by index (`NCollection_BaseSequence::Find`
  under `ShapeExtend_WireData::Edge`, quadratic in edges per wire), then ordinary projection
  (`Extrema_GenExtPS::BuildGrid`), and about 49 s of system time in allocation. Neither is a slow
  face, so the budget rightly doesn't cover them. Next would be per-solid parallel healing with
  `MMGT_OPT=0` (#54 review); until then the front-ends show the outline and progress (#54).
- **Tests:** `tests/heal.rs`, on `tests/data/no-pcurves.step` (the bracket assembly written
  without p-curves):
  - the default budget heals and meshes it all, colours intact;
  - a zero budget leaves exactly its two cylinders unhealed, drawn as outlines, with the faces,
    names and colours all still there;
  - a file that carries its p-curves spends nothing;
  - a budget that isn't a number is the default;
  - the green face colour on an unhealed cylinder survives;
  - an unhealed face's box is its edges', in the summary and the topology;
  - a replay gives up on the same faces with no clock, and on none with a clock already run out;
  - the measurement server replaying the run numbers the pin's cylinder as the run does.

### Cache (#62, #54 step 4, 2026-10-12)

A killed or failed run must never leave an entry a later run trusts, and a run that got as far
as the outline should leave something a preview can show at once.

- **Entries (`cache::store`, `cache::load`):** `STEPVCE\0`, `ENTRY_VERSION` (u32 LE), the
  payload's length (u64 LE), the payload, then its blake3 hash as the completeness tag. `load`
  treats a short, corrupt or other-version entry (and a raw pre-#62 one) as a miss, and the run
  regenerates it. Bumping `ENTRY_VERSION` invalidates the whole cache.
- **Writes (`cache::write_atomic`):** `<name>.<pid>.<nonce>.part` in the target's own directory
  (`rename` is atomic only within a filesystem), `fsync`, `rename`, then `fsync` of the
  directory. Every output stepv writes goes through it, the user's `--png`/`--topology` too.
  Concurrent writers of one key each have their own `.part`; the last rename wins whole.
  A failed directory `fsync` after the rename is not an error: the entry is complete, and at worst
  a crash loses it and it is regenerated. The hash catches truncation, not forgery: the cache
  directory is the user's own.
- **Stale `.part` files:** a writer killed between its write and its rename leaves one. `store`
  sweeps those older than `STALE_PART_AGE` (1 h) from the cache directory; never in the user's.
  The age is by wall clock, so a writer stopped longer than that loses its `.part`: its rename
  fails with `ENOENT`, harmlessly.
- **Only complete results:** an entry is written only when the kernel's outcome is `Ok` with a
  summary. A kernel that exits 0 or 3 without its summary broke the contract, on the plain and
  the streamed path alike: status `error`, "bad kernel output: the kernel exited without a
  summary", not a crash, and nothing cached. A multi-file assembly with a part file
  that did not read (`external_missing > 0`) still draws, but is not cached: the part files are
  outside the key.
- **The negative cache (`<key>.fail`)** keeps only a failure the kernel puts down to the file.
  - The summary's `cause` (`occt::Cause`) is `input` when the file, once opened, does not read or
    transfer, has an unsupported extension, has nothing to draw, nests its assembly more than 64
    deep, or makes OCCT throw.
  - It is `environment` when the kernel cannot open the input (it opens the file itself before
    the reader, so a permission error is never the file's), for path resolution, output I/O and
    missing part files (nothing to draw because of them included), for out of memory (OCCT's
    `Standard_OutOfMemory` included), and for invalid arguments or any other exception.
  - Only the run's summary carries `cause`: the `--serve` and `--outline` error lines don't.
  - Only `Outcome::Failed` with cause `input` is cached. A
  timeout or a memory cap depends on the limits (`--timeout`, `--memory-mb` are not in the key),
  a crash may not repeat, an environment failure is outside the key, and a kernel without
  `cause` says nothing: none of them is cached, since each would block a run that could
  succeed.
- **One snapshot of the file per run (`cache::FileId`):** its canonical path, length and mtime,
  taken once before anything reads the file. The entry's key and the artifact's are both made
  from it, so a file saved mid-run gets nothing from that run: its new mtime is another key.
  Content read after the snapshot can only be newer, under the old key, which the saved file
  never looks up.
- **The outline artifact (`cache::OutlineArtifact`):** JSON `{format: "stepv-outline-artifact",
  version: 1, header, tree, leaves, bbox, boxes}` at `<key>.outline.json`, keyed on the snapshot
  alone in its own hash domain. Cached CLI runs (`--png`/`--glb`/`--mesh`/`--topology`, without
  `--no-cache`) and `view` (#63) are streamed (`occt::run_streaming`) and write it on
  `Event::Tree`, before the transfer. A run killed after the outline keeps it. Once the mesh has decoded, the run rewrites
  it with the model's `bbox` (the B-rep's box, so the same at any deflection). Read back with
  `cache::load_outline` (#63, the thumbnail fallback), which rejects another format or version.
  `boxes` is always absent until the kernel streams per-part boxes (#66).
  - **Why JSON through `write_atomic`, without the entries' magic and checksum:** the front-ends
    (Swift included) read it without stepv, and JSON is self-delimiting, so a truncated file
    does not parse. `write_atomic` means no reader sees a partial write, and the `format` and
    `version` fields reject another schema. A checksum would only catch a complete file
    corrupted in place. That is not worth a binary frame every front-end must parse, for a hint
    shown until a run replaces it, never a result.
  - **Concurrent runs:** each write is whole, and the last rename wins. A run writes the outline
    only when no current-version artifact is at its key: one already there is of the same file,
    and may carry the box of a run that completed, which a later run killed after its outline
    would otherwise drop. The box write always goes through.
- **Known limits:**
  - the keys share `(len, mtime)`'s blind spot: a file rewritten in place within one mtime tick
    at the same length (#77);
  - a multi-file assembly's part file edited after it was cached is not seen: only the top file
    is in the key (#77);
  - streaming costs the run its parse again inside the timeout (about 5% on a large assembly,
    see #54 step 2; #76);
  - a non-STEP file has no outline, so no artifact (#76);
  - a box can still be lost (#80): run A checks for an artifact at the key (a read and a JSON
    parse) and finds none; run B, completing, renames its artifact with the box into place; then
    A's outline write (create, write, `sync_all`, rename) replaces it. The box is missing until
    the next complete run. The window runs from A's read to A's rename, which is milliseconds or
    more on a real disk; the directory `fsync` follows the rename and is outside it;
  - a cache hit returns before the kernel runs, so it writes no artifact: a file cached before
    #62's artifact gets no outline until its entry misses (#76).
- **Tests:**
  - `tests/cache.rs`:
    - a truncated and an old-version entry are regenerated;
    - a timeout, a memory cap, a kernel exit 0 without a summary, an unwritable mesh output
      (`environment`) and missing part files (`environment`) cache nothing, and the next run
      succeeds;
    - a garbage file, a file with no geometry and an OCCT exception (`input`, through the test
      hook `STEPV_OCCT_TEST_THROW`) cache their failure;
    - a `chmod 000` copy (`environment`, skipped as root) and OCCT's out of memory
      (`environment`) cache nothing, and the readable copy then succeeds;
    - invalid arguments to the kernel's C library (`environment`, through `dlopen`);
    - a run killed after its outline leaves the artifact, equal to `occt::outline`, and no entry,
      and the next run adds the box; the artifact follows the file's length and its mtime;
    - a file touched mid-run (same length, later mtime) gets neither the run's artifact nor its
      entry, which stay under the file the run read, the box included;
    - a mesh that doesn't decode leaves the artifact without a box;
    - a later run killed after its outline keeps an earlier run's box;
    - a kernel exit 0 or 3 without a summary is `error`, "the kernel exited without a summary",
      both streamed and with `--no-cache`.
  - `cache.rs`'s unit tests:
    - 4 processes and 4 threads write 64 KiB entries to one key while a reader verifies each raw
      frame. Swapping `write_atomic` for an in-place `std::fs::write` fails it, with 14 to 20 torn
      reads in each of 5 runs;
    - only a complete current-version entry loads;
    - the sweep takes stale `.part` files only.

### Outline and progress first (#63, #54 step 5, 2026-10-12): the CLI, then the viewer

#63 is done in three parts: #84, the CLI; #85 (2026-10-13), the viewer opens on the outline;
#89 (2026-10-13), the viewer keeps the tree, with the reason, when a run fails.

- **`view` writes the artifact:** its load is a cached run like `--png`'s. It is streamed, writes
  `cache::OutlineArtifact` on `Event::Tree` under the `cache::FileId` taken before the run, and
  adds the box once the mesh has decoded. A viewer whose kernel is killed after the outline (the
  timeout, the memory cap, the user's Ctrl-C) leaves the tree for the next preview. With
  `--no-cache` it writes nothing.
- **`view` needs a window session, and checks before the kernel (`view::session`, #84):** on
  macOS a window-server (Aqua) session (`CGSessionCopyCurrentDictionary` is not NULL), elsewhere
  on Unix `DISPLAY` or `WAYLAND_DISPLAY`. Without one, `view` fails at once, before the kernel
  starts: exit 3, status `error`, `"no window session: …"` with the reason, and the header
  metadata as ever. A user over ssh or in a sandbox is told at once instead of after the whole
  run, and macOS's window layer, which aborts a sessionless process outright, is never reached:
  that abort lost the stdout report (#84's CI, where nix's builder runs as `_nixbld` with no
  session). `STEPV_VIEW_TEST_NO_SESSION=1` forces the answer for the tests.
- **Progress on a terminal (`Progress` in `src/main.rs`):** when stderr is a terminal
  (`IsTerminal`), the run is streamed even with `--no-cache`, and one line on stderr is redrawn
  in place (`\r`, then erase to the end of the line):
  - `stepv: reading` while the file parses, as soon as the kernel starts;
  - `stepv: N parts · reading` once the outline arrives, or at once when a cached artifact
    already names them, before the kernel says anything;
  - `stepv: N parts · transferring P%`, then `· meshing P%`, redrawn on each whole percent;
  - the line is erased when stepv finishes the run, whatever its outcome (timeout, memory cap,
    crash and failure included), so stderr's warnings and the viewer's notes start on a clean
    line. The outcome is the stdout JSON's to say. stepv itself interrupted (Ctrl-C) or killed
    erases nothing: it installs no signal handler, so the last state stays on the line and the
    shell's prompt follows it.
  - A file without an outline (IGES, BREP, a STEP file without product structure) shows no part
    count. A cache hit runs nothing and shows nothing.
- **Stdout is unchanged:** one JSON line, the same keys whether stderr is a terminal or not.
  When stderr is not a terminal (a pipe, a file, Quick Look, the thumbnailer), nothing is
  written to it, and an uncached `--no-cache` run is not streamed.
- **The viewer opens on the outline (#85):** `view` opens its window at once, past the session
  check, and runs the kernel on a thread of its own (`run_view` and `load_view` in
  `src/main.rs`), through the CLI's own `tessellate`: the artifact, the box and the progress line
  are the same code. The loader tells the window `view::Load`s over a channel:
  - `Tree`: a cached outline artifact's tree, sent before the kernel starts, then the run's
    `Event::Tree`. The window draws it in the tree panel (`Tree::from_nodes`); until one
    arrives the panel says "Reading the file…";
  - `Progress`: the centre shows the file's name, "Transferring the shapes: P%" or
    "Meshing: P%", and a bar. Before any progress it says "Reading the file…";
  - `Done`: the scene, its topology, the title with the part count, the kernel's sandbox and
    unhealed faces. The window builds the GPU scene, swaps the viewer in, and sets the title;
  - `Failed(why)`: see the next point (#89).
- **A failed run keeps the tree, and says why (#89):** the window stays open on the tree it has
  (the cached artifact's or the run's; with neither, the tree panel says "No outline"), and the
  centre says "The model could not be shown", the reason, and "The tree is the file's outline;
  there is no geometry." The reason is the report's own `status`, its `error` and the kernel's
  `cause` when it gave one (`failure` in `src/main.rs`), so the window and the CLI say the same
  thing in the same words:
  - a timeout: "timeout: wall-clock cap exceeded";
  - the memory cap: "memory-cap: memory cap exceeded";
  - a crash: "crashed: kernel crashed (signal Some(11), exit None)";
  - a kernel failure: "failed: <the kernel's error> (cause: input)", or `environment`;
  - no summary: "error: bad kernel output: the kernel exited without a summary".
  A cancel closes the window itself, so there is nothing left to show. The window is idle, with
  nothing more to come, until the user closes it; the process then exits with the run's code
  and report, as before. The software viewer has no tree (#90): there a failed run still opens
  no window, and only the report says why.
- **Closing the window mid-load cancels the run (#85):** `occt::run_streaming` takes a `cancel`
  flag, which `supervise`'s tick checks every 2 ms and then kills the kernel: the outcome is
  `Outcome::Cancelled`, with no summary. `view` sets it once its window closes, so nothing runs
  on for nobody. The report says status `cancelled`, exit 0, with the reason in `message`
  ("the viewer was closed before the run finished"), not `error`: the user chose it, nothing
  failed. Nothing is cached for it. The loader owns the channel's sender, so a loader that
  panics closes it, and nothing waits on it for ever.
- **The pipelines are checked before the window opens (#85 review):** eframe's creator builds
  the viewer's pipelines inside a wgpu validation scope (`pipelines` in `src/view/app.rs`). A
  GPU that refuses them fails the creator, which hands the load back, and `view::run` falls
  back to the software viewer, as it did when the whole viewer was built there. So the window
  opens only on a GPU that can draw. What is left to refuse after `Done` is the scene's
  buffers (a scene past the device's limits): the window then hands the scene back
  (`app::Back::Loaded`), closes, and the software viewer draws it. So every GPU failure that
  fell back before #85 still does. `STEPV_VIEW_REJECT_PIPELINES` and `STEPV_VIEW_REJECT_UPLOAD`
  force the two refusals, for the tests, beside `STEPV_VIEW_REJECT_ADAPTERS`.
- **Known limits:**
  - `--software`, or a machine with no usable GPU adapter, has no tree panel: the software
    viewer still waits for the whole run before it opens (#90);
  - streaming for the progress line costs the run its parse again inside the timeout, as for
    the artifact (#76);
  - a non-STEP file has no outline, so the line has no part count (#76).
- **Tests:**
  - `tests/progress.rs`, through a real pseudo-terminal (`openpty`): the line names the parts and
    reaches `transferring 100%` and `meshing 100%`, never starts a new line, and is erased at
    the end. The same run into a pipe writes no progress, and its JSON has the same keys and
    values. A cached artifact names the parts with a kernel that never writes a byte; without
    one, no part count shows. The terminal's master and slave are close-on-exec from the
    moment they exist (`posix_openpt(O_CLOEXEC)`, the slave opened by std), and asserted to be:
    the tests run in parallel, and an inheritable slave passed to another test's stepv and
    kernel would hold this terminal open until that process tree exits.
  - `tests/stream.rs`: a run stalled after its outline and cancelled there is
    `Outcome::Cancelled`, without a summary, within seconds of a 60 s cap.
  - `tests/cache.rs`, window tests for #85. The window reports its frames on stderr under
    `STEPV_VIEW_SCREENSHOT`: "loading: tree N rows, …, no geometry" for the first frame drawn with
    the tree, then "scene: N parts" on the swap. A macOS CI runner never presents a window, so
    those lines, not a picture, are what the tests read:
    - with the kernel stalled 8 s after the outline, the loading frame shows the outline's
      rows and no geometry, and the scene follows (exit 0, `ok`);
    - with a cached artifact and a kernel that never writes a byte, the loading frame shows the
      artifact's rows, and no scene; the run times out as before (exit 4);
    - closed once it has drawn the tree (`STEPV_VIEW_CLOSE_LOADING=1`), with the kernel stalled
      30 s: exit 0, `cancelled` with its `message` and no `error`, in under 20 s, and the
      kernel's process is gone;
    - #89: a timeout (the stall hook, the tree from the run), the memory cap (the balloon hook
      under `--memory-mb 64`), a crash (a kernel that sends itself SIGSEGV), a kernel failure
      of cause `input` (the throw hook) and a kernel exit without a summary each leave the
      window's report "failed: tree N rows, <status>: <error>[ (cause: <cause>)]", the report's
      own words, with the tree of N rows still there (from the cached artifact where the kernel
      fails before its outline), and no scene. On `main` before #89, all five fail;
    - with the pipelines refused (`STEPV_VIEW_REJECT_PIPELINES`), or the scene's buffers once
      it comes (`STEPV_VIEW_REJECT_UPLOAD`), the software viewer draws the scene (status `ok`,
      backend `software`, the fallback said on stderr).
  - `view`'s unit tests: a loader that panics ends the software viewer's wait.
  - `tests/cache.rs`: `view` with no window session (`STEPV_VIEW_TEST_NO_SESSION`, and no
    `DISPLAY`) fails with `no window session:` and the header, and never starts the kernel.
    Without the check, it fails.
  - `tests/cache.rs`, needing a window session and so `#[ignore]`d in nix's build sandbox, which
    has none. `scripts/test-window-tests.sh` runs them (`--ignored`), and fails unless every one
    of them ran and passed: in `just view-test` on the macOS runner (kernel.yml's "GPU viewer on
    Metal") and under Xvfb in the Linux viewer job. Each `view` through `Home::view` (#86): the kernel under `--memory-mb 1024`,
    stepv's own `--timeout 30` unless the test gives another, `STEPV_VIEW_SCREENSHOT` set so a
    window that opens closes itself, and the viewer's own footprint polled. stepv runs in its own
    process group: past 2 GiB or 120 s the watchdog kills the whole group, never stepv alone
    (the kernel's caps are stepv's sampler, so a kernel orphaned by killing stepv would run on
    uncapped), and fails naming the limit. However a run ends, no member of its group may
    outlive it.
    - the watchdog, at a 10 s deadline over a kernel that never finishes, kills that kernel too;
    - a `view` whose kernel is killed after its outline leaves the artifact under the pre-run
      `FileId`, equal to `occt::outline`, without a box;
    - `view --no-cache` leaves no artifact, where the same run cached leaves one (a kernel that
      crashes after a full run, so no window opens). Making `view` ignore `--no-cache` fails it;
    - a complete `view` stores the box, equal to the summary's, whether or not a window can
      then open.

### Input from a descriptor (#91, #64 step 1, 2026-10-13)

[#64](https://github.com/vig-os/stepv/issues/64) runs Quick Look's kernel as the child of an XPC
service whose sandbox opens nothing by path: it holds the file only as a descriptor (#79 round 2:
`open()` by path is EPERM, `open("/dev/fd/3")` works). The kernel used to `realpath` its input
(macOS resolves `/dev/fd/3` to the device, not the file) and to pick the reader by extension.

- **`/dev/fd/N`, not `--fd N`:** the input stays a path, so OCCT's path-based readers (STEP,
  IGES, BREP) work unchanged, on macOS (devfs) and Linux (`/proc/self/fd`) alike, and the
  supervisor needs no second spelling. An input that is exactly `/dev/fd/N` is a descriptor
  (`kernel/input.h`); it is never resolved, and it is rewound before reading (on macOS opening
  `/dev/fd/N` shares N's offset, so wherever the caller left it would be where the reader starts).
- **`--format step|iges|brep`:** required with a descriptor, optional with a path, where it
  overrides the extension. `--outline` and `--serve` take both too. The C ABI's
  `stepv_occt_run_stream`, `stepv_occt_outline` and `stepv_occt_serve` gained a `format`
  argument (NULL: by extension); `stepv_occt_run`, the Quick Look extensions' entry, is unchanged.
- **Causes (#62):** a descriptor without `--format`, a `--format` with no reader, or a
  descriptor that is not open or cannot seek (a pipe) fails at stage `read` with cause
  `environment`: the caller asked wrongly, the same file reads when asked right, and a front-end
  must not remember the file as bad. A file that does not read as the format named is `input`,
  as a STEP file named `.igs` always was.
- **Sandbox:** a descriptor's read grant is `/dev/fd/N` itself, and nothing by path: Seatbelt
  matches that name, and Landlock's rule, opened through the magic link, lands on the file
  alone. The file's directory stays closed, and so do the `$HOME` rules' concerns. (#102
  replaced this: the descriptor is read directly, never re-opened, and granted nothing.)
- **Rust:** `occt::Input` (`Path`, or `Fd { fd, format }` with `occt::Format`) for `occt::run`,
  `run_streaming` and `outline`; `&Path` and `&PathBuf` convert, so path callers are unchanged.
  The descriptor is inherited at its own number (close-on-exec cleared in the child alone).
- **Known limit:** a multi-file assembly's part files are unreachable from a descriptor; the run
  reports them in `external_missing` and fails with cause `environment`, as for missing files by
  path. Handing the parts over too is
  [#92](https://github.com/vig-os/stepv/issues/92).
- **Tests:** `tests/fd.rs`: STEP, IGES and BREP as fd 3 with the file deleted after opening give
  the summary (minus timings and peak memory) and mesh bytes of a run by path; no `--format`,
  `--format dwg` and a wrong format fail with the causes above; `occt::run` and `occt::outline`
  through `Input::Fd` match the path's, and a pipe fails with `environment`.
  `tests/sandbox.rs`: an fd input's sibling file is refused by the sandbox. All fail on the
  previous kernel.

### The descriptor read directly (#102, 2026-10-13)

[#102](https://github.com/vig-os/stepv/issues/102): #91 passed `/dev/fd/N` as a path and let
OCCT's path readers re-open it.
- On the `macos-15-arm64` CI runner, the sandbox the kernel inherits from #64's Quick Look
  service denies that re-open: "cannot open input: Operation not permitted" (#98's CI).
- macOS 26.4 allows it, which is why #79's probe passed.
- The root cause is the re-open by name. Reading the descriptor that is already open goes
  through no path check, so **the kernel reads the descriptor itself and never opens
  `/dev/fd/N`** (`kernel/input.h`).

- **STEP and BREP: an `istream` over the descriptor** (`FdStreamBuf`, which reads with `pread`
  from offset 0, so the caller's shared offset is left as it was).
  - The kernel uses `STEPCAFControl_Reader::ReadStream`, `STEPControl_Reader::ReadStream` (for
    the outline) and `BRepTools::Read(shape, istream, builder)`.
  - OCCT 7.9.3's path readers open an `istream` and run the same parser on it: `StepFile_Read`
    and `BRepTools::Read`. Only the open differs, and there is no copy.
- **IGES: a private temporary copy.**
  - OCCT 7.9.3 has no IGES stream reader: `IGESSelect_WorkLibrary` doesn't override
    `IFSelect_WorkLibrary::ReadStream`, whose default fails.
  - The kernel copies the descriptor into `$TMPDIR/stepv-input-XXXXXX.igs` (its real path:
    macOS's `/var` is a symlink) and reads that by path.
  - `stepv-occt` makes the copy before its sandbox, which grants that one file to read.
  - The sandbox can't let the kernel remove a file it may read (Landlock removes through the
    parent directory). So a child forked before the sandbox, holding only its end of a pipe,
    unlinks the copy when the pipe closes. On a normal exit the kernel waits for it; a kernel
    killed outright still has its copy removed.
  - In-process C ABI callers, which have no sandbox of their own, get a copy that is unlinked
    when the read returns.
- **Sandbox:** a descriptor input is granted nothing by path now, not even `/dev/fd/N`. Only
  the IGES copy is granted.
- **Kept from #91:**
  - A descriptor that is not open or can't seek (a pipe) fails with cause `environment`, and
    so does a read error on the descriptor.
  - A file that doesn't read as the format named is `input`.
- **Tests (`tests/fd.rs`):**
  - The descriptor's file is set to mode 000 once it is open, so re-opening `/dev/fd/N` fails
    on macOS (fdesc) and on Linux (`/proc/self/fd`) alike, for any user but root. A run,
    `--outline` and `--serve` must give what the path gives, for all three formats.
  - The previous kernel fails this with "cannot open input: Permission denied", the same
    failure as macOS 15's.
  - With a private `$TMPDIR`, the IGES copy is gone after a successful run and after a failed
    one.
- **Path inputs:** unchanged code. The corpus diff against the previous kernel is in #102's PR.

### #64 macOS kernel: an XPC supervisor and its child (2026-10-13): part 1, the service

[#64](https://github.com/vig-os/stepv/issues/64) takes Quick Look's kernel out of the extension.
In-process, a cancelled heavy load leaves the extension about 0.9 GB fatter, and jetsam kills
extensions under memory pressure. The kernel must run in a process that can be killed, so its
memory is freed at once. Two designs failed before this one:

- **An XPC service with the kernel inside it:** `invalidate()` doesn't kill a busy service,
  a sandboxed client can't `kill()` it, and launchd throttles restarts by about 10 s (#64's
  pre-mortem).
- **The extension spawns the kernel itself:** a Quick Look extension can't `posix_spawn`
  (EPERM, 22 of 22, [#79](https://github.com/vig-os/stepv/issues/79) round 1).

**The design** (owner, 2026-10-13), proven by #79 round 2's harness:

- Each extension embeds `Contents/XPCServices/StepvSupervisor.xpc`: a small, long-lived
  service (`ServiceType` Application).
- The extension sends the file over as a `FileHandle`: a URL carries no permission across XPC,
  and the service can't open the file by path.
- The service `posix_spawn`s its own `Contents/Helpers/stepv-occt /dev/fd/3 --format … --stream
  --mesh …` (#91). The kernel inherits the descriptor as fd 3, besides
  stdin and stderr (`/dev/null`) and stdout (the frames), and nothing else
  (`POSIX_SPAWN_CLOEXEC_DEFAULT`).
- The service forwards TREE and PROGRESS as the kernel writes them. It replies with the summary
  and the mesh, which it writes in its container and hands back as an open, unlinked file, so no
  large XPC message carries it.
- **Events and the reply:** XPC doesn't order the events (messages to the extension's exported
  object) against the reply block, which can arrive first. Review measured this at 4 of 200
  stand-in runs, with the last PROGRESS dropped. So the reply carries how many events were
  sent, and the client completes only once it has received that many.
- The protocol is in `macos/Shared/StepvSupervisor.swift`.

- **One run per connection; the connection is the cancel.** When the extension invalidates the
  connection, or the extension dies, the service SIGKILLs that run's kernel. Concurrent runs
  are concurrent connections, each with its own kernel.
- **Every run is bounded, and well formed, or refused unrun:**
  - a deadline (`deadlineMs`, 1 to 600 000 ms) and a memory cap (`memoryMB`, 1 MB to 1 TB)
    are both required;
  - `linearRel` must be in (0, 1] and `angularDeg` in (0, 180], with NaN and infinity refused;
  - an earlier `Int.max` deadline overflowed and crashed the service after the spawn. A
    deadline of 0 meant no deadline, so a kernel that closed stdout without exiting ran
    forever.
- **Bounds, in the service:**
  - at the deadline, the kernel is killed and reaped;
  - the memory cap is polled every 5 ms.
  - The cap counts **resident size**, not the footprint the CLI caps (`src/occt.rs`). The
    sandbox denies `proc_pid_rusage` even on the service's own child (EPERM) and allows
    `proc_pidinfo(PROC_PIDTASKINFO)`.
  - Resident size includes the shared dylibs and drops as the compressor takes pages, so under
    memory pressure this cap acts later than the CLI's.
- **The kernel dies with its service: `stepv-occt --lifeline N`.**
  - A service killed mid-run (`kill -9`, by jetsam or a crash) used to leave its kernel running
    unsupervised: 45 s more in review, with no deadline or cap, and a 148 MB mesh left in the
    container.
  - Now each run gets a pipe. The kernel holds the read end as fd 4; the write end is the
    service's alone (close-on-exec, and `POSIX_SPAWN_CLOEXEC_DEFAULT` keeps it from other
    kernels).
  - A thread in the kernel blocks in `read(4)`. When the service dies, by any means, the
    write end closes, the read returns, and the kernel `_exit`s with status 4. That status is
    its own: 3 is a failure its summary explains, and a lifeline exit writes no summary.
  - **Why a pipe and not kqueue `EVFILT_PROC NOTE_EXIT` on the parent pid:**
    - it is the same on Linux;
    - it needs no sandbox right to watch another process;
    - it has no race: if the writer is already gone, the first read returns EOF;
    - it costs one blocked thread and nothing per frame.
  - **Run files:** each run's files are in a directory of its own, `stepv-run-<uuid>`, in the
    service's tmp: the mesh, and the kernel's `$TMPDIR`, where #102's IGES copy goes.
    - Its `lock` is created already locked (`O_EXLOCK`) and held until the run removes the
      directory. A process that dies releases its locks.
    - At start-up each service sweeps only the run directories whose lock it can take, so only
      dead runs' are swept: a killed service's, or one whose kernel's job a teardown killed
      along with the IGES copy's remover.
    - Two client processes start two instances of the service in one container. A sweep that
      went by file name alone removed the other instance's live mesh (review).
- **The read loop drains:** the kernel's exit doesn't end the reading. What it wrote before
  exiting is read to the end of the pipe, under the deadline, and only a stream that ends with
  DONE is a finished run. Before this, frames still in the pipe at the exit could be dropped,
  giving a `done` with no summary.
- **The kernel's frames:** `macos/Shared/StepvFrames.swift` is the Swift twin of `read_events`,
  with the same framing bounds (`STEPV_OCCT_MAX_FRAME`, kinds 1 to 3, nothing after DONE). It
  decodes incrementally, as the pipe delivers. The payloads stay JSON bytes: the extension that
  shows them parses them, so `read_events`' payload checks (valid JSON, progress done ≤ total)
  belong to part 2's consumer. A broken stream is a crashed kernel: the service kills
  it and reports it.
- **Bundling:** each service carries its own copy of the OCCT closure, 31 dylibs and 51 MB, in
  its own `Contents/Frameworks`, at `@rpath`.
  - The first build pointed the service's kernel at the app's shared `Contents/Frameworks`;
    dyld's open failed with "file system sandbox blocked open()". A service's sandbox reads its
    own bundle and nothing of the app around it.
  - The app grows from 57 to 174 MB: three copies of the closure, +117 MB. The options and
    their evaluation are in [#101](https://github.com/vig-os/stepv/issues/101). Part 2 drops
    the extensions' in-process copy of libstepvocct.
- **Signing:** the kernel is signed `app-sandbox` + `inherit`, so it runs in the service's
  sandbox; without `inherit` a sandboxed child traps. The service is signed `app-sandbox` alone.
  - The CLI's `Contents/MacOS/stepv-occt` stays a separate copy without these entitlements: an
    `inherit` binary can't run from an unsandboxed parent.
  - Ad-hoc signing leaves the hardened runtime off, because ad-hoc library validation refuses
    the bundled dylibs ("different Team IDs"). Developer ID with the hardened runtime and
    library validation on is [#97](https://github.com/vig-os/stepv/issues/97).
- **A trap, now in the code:** XPC delivers the file's descriptor as the lowest free number,
  often 3 itself.
  - `posix_spawn_file_actions_adddup2(3, 3)` is no file action, so `CLOEXEC_DEFAULT` closed it.
  - The kernel's own `dup(stdout)` then landed on fd 3, and the kernel failed with "cannot
    rewind input descriptor: Illegal seek".
  - The service first moves the descriptor to 10 or higher.
- **Known limits:**
  - **Multi-file assemblies:** their part files are unreachable from one descriptor
    ([#92](https://github.com/vig-os/stepv/issues/92)).
  - **Size:** three copies of the OCCT closure, +117 MB
    ([#101](https://github.com/vig-os/stepv/issues/101)).
  - **Developer ID and the hardened runtime** ([#97](https://github.com/vig-os/stepv/issues/97)).
  - **Containment** is the service's sandbox. The kernel's own Seatbelt profile can't nest
    inside it, so the summary reports `macos-outer`.
- **Tests:**
  - `scripts/ql-e2e/supervisor.sh` (run by `scripts/test-macos.sh`) puts a sandboxed probe into
    a copy of the built preview extension. The probe reaches that extension's real service,
    kernel and Frameworks through the extensions' own client code, and checks:
    - the service killed with `kill -9` under a running kernel: the kernel is gone within 1 s
      (16 ms on sage);
    - STEP (TREE first, PROGRESS, both before the reply, `macos-outer`), IGES and BREP;
    - a deadline kill at 1.5 s with a second run finishing beside it (about 65 ms);
    - the next run right after the kill (about 65 ms, so no throttle);
    - the memory cap at 1 MB, and at 256 MB on a run that peaks near 500 MB;
    - cancel (signal 9);
    - 11 malformed or unbounded requests, all refused;
    - from the service's log, the kills, and that each of the 9 kernels it spawned is gone;
    - a dead run's directory, planted with a free lock, swept by the next instance;
    - a live run that finishes with its mesh while a second instance, started beside it, sweeps
      (`kept 1 live`);
    - no run directory left in the service's container.
  - A second copy puts `macos/Tests/KernelStandIn` in place of the kernel, to hold the service
    to streams the real kernel doesn't write on cue:
    - frames then an immediate exit;
    - an exit before its frames arrive. This one is deterministic: a grandchild writes them
      200 ms later. A service that decides on the exit fails it, with a `done` that has no
      summary;
    - no output, and half a frame, both crashes;
    - stdout closed with no exit, ended by the deadline.
  - `STANDIN_ROUNDS=500 scripts/ql-e2e/supervisor.sh` repeats the stand-in step: events must
    all reach the client before its completion, every time.
  - `tests/fd.rs`: the kernel exits within 1 s of its lifeline closing (the control, without
    a lifeline, lives its 5 s).
  - `macos/Tests/FramesTests`: a real kernel stream decodes the same in 1-, 7-, 9- and
    4096-byte chunks, and each broken stream fails as `read_events` fails it. Dropping the
    MAX_FRAME guard or the trailing-bytes check fails it.
  - `scripts/test-macos.sh` holds each service and its kernel to exactly their entitlements and
    each kernel's dylibs to its own service.
- **`ql-e2e`** (`just ql-e2e`, `scripts/ql-e2e/`, from #79):
  - It installs the build. Quick Look itself then runs the extensions, through
    `QLThumbnailGenerator` and a `QLPreviewView` host app, on a fresh copy of each fixture.
  - Each extension's own log line for that path is the verdict. It must come from
    `/Applications/stepv.app`, not from a process left running from an older install, which
    Quick Look keeps using until it exits.
  - `qlmanage` is never used: it hangs when a script drives it.
  - **Quick Look's thumbnail cache is keyed on the file's inode** (sage, measured): the same
    path asked twice, or a hard link to a thumbnailed file, is answered in 7–9 ms with no call
    to the extension; a new inode, even at an old path or with the same mtime and size, runs it.
    So each request gets freshly written bytes (never a clone). The extension logs each
    thumbnail request at notice level, and a thumbnail with no request line fails as "QL
    served a cached thumbnail; the extension did not run". A control in `run-all.sh` asks for
    one file twice and requires the second to be caught this way. A failed verdict prints the
    window's stepv log: #98's one CI failure (`thumb box.brep`, a thumbnail in 65 ms and no log
    line) left no evidence of which it was.
  - It fails if any of the extension's lines for a file fails. It polls the log for that line
    under a timeout, rather than sleeping. It refuses to run when it can't tell which build a
    running extension process is.
  - Part 1: a thumbnail and a preview of STEP, IGES and BREP, all pass locally (sage, macOS
    26.4) and on CI's `macos-15-arm64` runner, whose window session runs Quick Look's
    extensions: ALL PASS in the Kernel workflow's "Quick Look end to end" step (run 38353253049).
- **Part 2:** both extensions switch to the service, with `handler(nil)` once the tree is shown
  (or on the first mesh, or on failure, when there is no tree), and their own deadlines. The
  in-process kernel goes, and `ql-e2e` gains the deadline kill, the next request and concurrent
  requests through the real extensions. The deadline values and their reasoning will be added
  here.

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
   #28 (scaffold, done: "Viewer scaffold" above) → #29 picking (done: "Picking" above) → #30 tree (done: "Model tree" above) → #31 overlay and edges (done: "Edges and the overlay" above) →
   #32 CI and packaging (done: "Viewer CI and packaging" above) → #33 measurements (done: "Measurements" above) → #34 capping (done: "Section caps and fat lines" above).

#21 is closed (2026-10-11), with the viewer built and #43's exact caps on top. Its open parts are
#48 (free and multiple section planes) and #49 (vertex picking).

Needs a human (`needs-human`): #10 the Apple and crates.io credentials. #16's graph is on,
#3's grants are made (§6), and #14 is filed upstream. #12 is filled from public sources ("Corpus
fill (#12)" above); only the full CAx-IF rounds, which are members-only, are still missing, and
the real assembly's transfer time is #51.

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

1. ~~**Approve the org-config apply**~~: done. org-config#317 applied, and stepv has its four
   rulesets (Main, Release and Tag protection, Signed commits).
2. ~~**Grant the workflow credentials** (#3)~~: done. `stepv` is in all six org-secret lists
   (checked 2026-10-11).
   - The commit App proves it: Sync Issues has pushed verified `commit-action-bot` commits to
     `sync/issue-mirror` every day since 2026-10-06.
   - The release App's grant is in place, but only a release run will exercise it.
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
7. ~~**Upstream report** (#14)~~: filed as
   [andymai/occt-wasm#393](https://github.com/andymai/occt-wasm/issues/393), re-checked on its
   `main` @ `89fb058` (2026-10-08):
   - crates.io is still at 4.0.0, which can't instantiate;
   - STEP import still traps with `uninitialized element`, and upstream ignores its own STEP tests
     for the same `/tmp` reason;
   - `new()` still JIT-compiles every time, in 2.6–2.9 s.
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
not deliberately. Its apply ran on 2026-10-04. stepv now has its four rulesets: Main protection,
Release protection, Tag protection and Signed commits.

**The apply did not deliver the workflow credentials** (stepv#3). This plan once said it would. `otterdog apply` does not reconcile `selected_repositories` for org secrets
whose declared value is a `'********'` dummy, and the #317 plan is "4 to add, 11 to change", with
zero secret actions. `revkit` proves it: it was added to the same six lists in org-config#312 and
is still absent from them days after a successful apply. That is tracked upstream as
[`vig-os/org-config#318`](https://github.com/vig-os/org-config/issues/318), since closed.

The six grants were then made, and stepv is in every list (checked 2026-10-11). Sync Issues proves
the commit App: its token step succeeds, and it pushes verified commits daily. A future repo
still needs the same check. Without the grants, the devkit workflows fail with an **empty
credential and no error message**.

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
