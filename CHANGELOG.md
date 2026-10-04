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

### Changed

### Deprecated

### Removed

### Fixed

### Security
