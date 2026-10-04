# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## Unreleased

### Added

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
