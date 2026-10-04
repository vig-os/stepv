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

### Changed

### Deprecated

### Removed

### Fixed

### Security
