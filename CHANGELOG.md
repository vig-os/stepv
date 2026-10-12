# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## Unreleased

### Added

- **Atomic, versioned cache entries** ([#62](https://github.com/vig-os/stepv/issues/62)):
  an entry is either whole and current or ignored
  - Entries carry a format version and a checksum; a truncated, corrupt or old-version entry is
    a miss and is regenerated. Existing caches are regenerated once
  - Every write is a `.part` file, `fsync`ed and renamed, then the directory `fsync`ed;
    concurrent writers of one key never tear an entry, and stale `.part` files are swept
  - Failures cached before this (raw `.fail` files) are ignored and regenerated
- **Only complete results, and only failures of the file, are cached** ([#62](https://github.com/vig-os/stepv/issues/62)):
  the kernel's summary says why a run failed in `cause` (`input` or `environment`), and only a
  failure of the input itself is remembered. Timeouts, memory caps, crashes, unreadable inputs,
  unwritable outputs and missing part files no longer are. Neither is a kernel exit without a summary, or a
  multi-file assembly drawn without some of its parts
- **An outline artifact, left as soon as the outline arrives** ([#62](https://github.com/vig-os/stepv/issues/62)):
  cached runs are streamed and write `cache::OutlineArtifact` (`<key>.outline.json`: the file's
  header and assembly tree, and the model's box once a run completes), so a run killed after
  the outline still leaves something a preview can show at once. Read it with
  `cache::load_outline`. A run keys the artifact and its output on the file as it was before the
  run read it, so a file saved mid-run gets nothing from that run. Per-part boxes follow in #66
- **A time budget for healing each face** ([#51](https://github.com/vig-os/stepv/issues/51)):
  a file written without p-curves (SolidWorks writes none) has the reader rebuild every edge's
  2D curve on each face by projection, and on a few faces that is very slow. The kernel now heals
  faces one at a time with a clock on each, `STEPV_OCCT_FACE_BUDGET_MS` (default 2000). A face
  past it is left unhealed and drawn as its outline (Missing), and the summary counts it in
  `faces_over_budget`. On the Ultimaker 3 assembly four ribbon faces go over: the run takes
  42 s instead of 101 s (transfer 34 s instead of 92 s), with all 523 parts still
  named and coloured
  - stepv registers its own `FixShape` operator ahead of OCCT's, set up from the same reader
    parameters; outside the budget it heals as before
  - The recovery ladder's ShapeFix rung has the same budget
  - The run lists the faces it gave up on (`unhealed_faces`), and the viewer's measurement
    server replays exactly that list (`STEPV_OCCT_UNHEALED`), so both number faces alike
- **Streamed runs: the outline, then progress, then the summary** ([#54](https://github.com/vig-os/stepv/issues/54)):
  `stepv-occt --stream` writes a run's events to stdout as frames in place of its one JSON line.
  A STEP file's outline arrives once the file is parsed, from the same parse the run uses; then
  progress through the transfer and the meshing; then the summary. On the Ultimaker 3 assembly
  the tree arrives at 4.3 s of 101 s
  - In Rust as `occt::run_streaming`, which hands each `occt::Event` to the caller as it arrives.
    A run killed partway keeps what arrived and has no summary; a stream that breaks the frame
    contract has no summary either, and an exit 0 or 3 without one is reported as an error
    ("the kernel exited without a summary"), not a crash
  - In the C API as `stepv_occt_run_stream`, with a callback, for in-process front-ends
  - Transfer progress is OCCT's own, so it stalls inside one slow solid: Ultimaker 3 sits at
    15.6% for 80 s ([#51](https://github.com/vig-os/stepv/issues/51))
- **The assembly outline before the geometry** ([#54](https://github.com/vig-os/stepv/issues/54)):
  `stepv-occt --outline` reads a STEP file's product tree and names straight from its parsed
  entities, with no shape transfer. On the 143 MB Ultimaker 3 assembly that takes 4.6 s against
  98.6 s for the full run ([#51](https://github.com/vig-os/stepv/issues/51)), so a front-end can
  show the tree while the geometry loads
  - It is the product level of the tree `--topology` writes: the same products, names and order.
    A product the transfer splits into unnamed bodies is one leaf (`topology::products`)
  - In the C API as `stepv_occt_outline`, and in Rust as `occt::outline`
  - `harness --outline` checks it against each STEP file's full run; CI's `--strict` gate fails
    on a mismatch
- **Corpus: seven CAD writers and a real large assembly** ([#12](https://github.com/vig-os/stepv/issues/12)):
  `just fixtures` now fetches one file each from SolidWorks, NX, CATIA, Fusion 360, FreeCAD,
  Inventor and Creo, the whole Ultimaker 3 printer (143 MB, 523 parts) and MBx-IF's public AP242
  model, each pinned by sha256. The harness draws 99.2% of 400 files faithfully or as
  wireframe; the real assembly loads correctly but needs 101 s
  ([#51](https://github.com/vig-os/stepv/issues/51))
  - The fetcher and the harness skip macOS `__MACOSX` resource forks, and `just test` now runs
    the fetcher's tests and the harness's own unit tests
  - `stress-gen` writes the same bytes on every run, so `corpus.sha256` really is exact
- **Exact section caps** ([#43](https://github.com/vig-os/stepv/issues/43)): a cut model is capped
  per part, in each part's colour, from the kernel's exact section (`stepv-occt --serve`'s new
  `section` op). Parts that interfere are both capped in their own colours, not as one grey
  patch, and where they overlap, the later part's cap shows
  - While the slider moves, or if the kernel refuses, the GPU's stencil cap stands in, and the
    Section panel says which one is shown
- **Section caps and fat edges** ([#34](https://github.com/vig-os/stepv/issues/34)): a cut model
  shows its solid cross-section hatched, as a drawing's section, with holes left open; "Cap the
  cut" turns it off
  - Edges and sketches are drawn as anti-aliased quads 1.5 px wide at the display's scale, not
    1-px lines
  - The kernel takes edge points from the mesh's own triangulation, so edges sit exactly on the
    faces they bound
- **Measurements** ([#33](https://github.com/vig-os/stepv/issues/33)): in `stepv view`, M (or
  Measure) and two clicks on faces or edges give the distance between them, the distance between
  their axes (cylinders, cones, lines, circles), and their angle, with the witness points drawn
  - Measured on the exact B-rep by a long-lived kernel, `stepv-occt --serve`
  - The server is sandboxed as a run is, and held to the run's time and memory limits per query.
    A hang, a balloon or a crash is killed, restarted and reported, never taking the viewer down
- **Viewer CI and packaging** ([#32](https://github.com/vig-os/stepv/issues/32))
  - `stepv view --frames N`: orbit for N frames, exit, and report the frame intervals in the JSON
    line
  - A Linux CI job on Mesa's lavapipe and Xvfb runs the viewer for real: whole windows,
    `--frames` on every fixture, and the software fallback with the GPU drivers removed
  - A stripped binary-size budget: 16 MB on macOS (11.7 today), 24 MB on Linux (15.3)
  - The nix product's viewer finds its window and GPU libraries on Linux, and the tarball's
    launcher documents why the host's come after its own
- **B-rep edges and edge picking** ([#31](https://github.com/vig-os/stepv/issues/31)): `stepv view`
  draws the model's exact edges (E toggles them), and clicking one shows its curve (a circle's
  radius, ⌀, centre and normal) and its length
  - The kernel writes them with `--edges` as STEPVMSH v4, numbered as `--topology` numbers them.
    Quick Look stays on v3, and its reader also accepts v4
  - The viewer flags faces by the thumbnails' own rule (`render::overlay`), tested to stripe the
    same faces
- **Model tree** ([#30](https://github.com/vig-os/stepv/issues/30)): the assembly tree beside the
  view in `stepv view`
  - Show/hide per node, which carries down to every part below; Isolate and Show all; search by name
  - Selection both ways: a row selects its part in the view, and a face clicked in the view
    reveals its row
  - Virtualised: only the rows on screen are laid out, so 40,000 nodes cost a frame what 40 do
- **Picking and the inspector** ([#29](https://github.com/vig-os/stepv/issues/29)): click a face in
  `stepv view` to see what it exactly is, from `--topology`
  - The surface type and parameters (a plane's normal, a cylinder's radius, ⌀ and axis, …) in
    model coordinates, the face's area, and the part's volume and size
  - The face is highlighted; Esc clears it
  - A section plane (X, Y or Z, flippable) cuts the model, and picks and highlights honour it
- **GPU viewer** ([#28](https://github.com/vig-os/stepv/issues/28), for
  [#21](https://github.com/vig-os/stepv/issues/21)): `stepv view` draws on the GPU through egui +
  wgpu (Metal, Vulkan or GL)
  - Per-face colours come from a storage buffer, and parts can be hidden, without touching the
    geometry. The framing is the thumbnails' own, and approximated faces keep their amber stripes
  - A toolbar, a properties panel and a status bar
  - Light and dark themes from shadcn-style design tokens, following the OS or `--theme`
  - The pre-#28 software window is kept as `--software`, and taken automatically, with a note, when
    there is no usable GPU adapter. The JSON line reports `"backend"`
  - CI checks that the Quick Look library never links the window or GPU stack
- **Exact topology export** ([#21](https://github.com/vig-os/stepv/issues/21)): `--topology <path>`
  writes the assembly tree, and per prototype its faces' surface types and parameters (plane,
  cylinder radius, …), edges' curves and lengths, vertices, area and volume, as JSON whose indices
  line up with the mesh's. It is the kernel side of a viewer with a model tree and measurements
- **Native OCCT kernel** ([#1](https://github.com/vig-os/stepv/issues/1))
  - `kernel/stepv-occt`: STEP/IGES/BREP through XCAF (names, colours, instances), meshed at a
    bbox-relative deflection, run as a subprocess so crashes are contained and the deadline is a kill
  - Per-face recovery ladder (re-mesh, heal, face-relative refine, degenerate check, coarse,
    approximate, outline) with a `FaceStatus` per face for a broken-face overlay
  - Sketch-only files drawn as curves; construction curves beside solids kept but hidden by default
- **Kernel harness and corpus** ([#1](https://github.com/vig-os/stepv/issues/1))
  - `just fixtures` fetches a sha256-pinned corpus (NIST PMI, ABC, occt-import-js) and generates
    stress and malformed files; `just harness` prints the acceptance table
- **The `stepv` CLI** ([#5](https://github.com/vig-os/stepv/issues/5))
  - `--info` from the file header alone (STEP Part 21, IGES, BREP); cannot fail
  - `--png` via a CPU rasteriser with per-face colour and the broken-face overlay; `--glb` (glTF 2.0)
  - One JSON line on stdout for every run; exit 3 keeps the metadata; results and failures cached
  - Timeout (exit 4) and a memory cap by child footprint ([#4](https://github.com/vig-os/stepv/issues/4))
  - `--mesh` writes the raw STEPVMSH buffers for front-ends
- **`stepv view`**: an interactive orbit viewer over the same rasteriser ([#7](https://github.com/vig-os/stepv/issues/7))
- **Linux front-ends** ([#7](https://github.com/vig-os/stepv/issues/7))
  - freedesktop `.thumbnailer` (GNOME, XFCE), MIME types (with a new `model/x-brep`), desktop entry
  - KF6 `ThumbnailCreator` for Dolphin; `packaging/linux/install.sh`
- **macOS Quick Look** preview and thumbnail extensions ([#9](https://github.com/vig-os/stepv/issues/9))
  - The kernel runs in-process (the extension sandbox forbids exec) via `libstepvocct` and the
    `stepv-capi` C ABI; the interactive SceneKit preview uses per-face colour and the overlay
  - Built without Xcode; one shared copy of OCCT (57 MB app)
- **Release train** ([#10](https://github.com/vig-os/stepv/issues/10)): relocatable Linux tarball, Developer-ID
  signed and notarised DMG, build-provenance attestations, crates.io publish (Trusted Publishing
  after the first release)
- **`NOTICE`** with OCCT's LGPL-2.1 and exception, shipped with every package ([#8](https://github.com/vig-os/stepv/issues/8))
- **CI that runs the kernel** ([#11](https://github.com/vig-os/stepv/issues/11)) and `cargo deny`
  ([#13](https://github.com/vig-os/stepv/issues/13))

### Changed

### Deprecated

### Removed

### Fixed

- **A failed topology no longer fails the run** ([#38](https://github.com/vig-os/stepv/issues/38)):
  the kernel reports `topology_error` and keeps the mesh, so `--png --topology` still writes the
  PNG; `--topology` alone still fails
  - `stepv view` opens such a file without the inspector after one kernel run, not two
- Quick Look preview ([#20](https://github.com/vig-os/stepv/issues/20)): flat parts and sketches
  open face-on, not edge-on (also in thumbnails, `stepv --png` and `stepv view`); the scroll wheel
  zooms; the info text sits on a light panel and reads in dark mode
- `harness --strict` passed a pass-rate collapse in which every file failed cleanly. `--min-pass`
  now sets a floor, and CI uses it ([#22](https://github.com/vig-os/stepv/issues/22))
- A multi-file STEP assembly whose part files cannot be read no longer fails as an unexplained "no
  geometry": the kernel counts the referenced part files (`external_files`, `external_missing`),
  the error names the cause, and the Quick Look preview explains that it can open only the one
  file ([#19](https://github.com/vig-os/stepv/issues/19))

### Security

- **The kernel CLI sandboxes itself** ([#18](https://github.com/vig-os/stepv/issues/18)): before
  opening the input it can read only the input's directory and write only its mesh, with no
  network and no exec. On Linux this is Landlock + seccomp, on macOS a Seatbelt profile. The
  summary reports it (`"sandbox"`), and the CLI warns when the OS provides only part of it.
