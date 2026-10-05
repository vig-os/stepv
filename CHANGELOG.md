# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## Unreleased

### Added

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
