<!-- Seeded by vigOS devkit — yours to edit; upgrades never overwrite this file. -->
<!-- Bugs / missing tools: https://github.com/vig-os/devkit/issues -->

# stepv

Fast STEP/IGES/BREP previews and thumbnails: macOS Quick Look, Linux file managers (GNOME, XFCE,
KDE), and one CLI underneath both.

Press space on a `.step` file in Finder and you get an interactive 3D preview. Icon view, Nautilus
and Dolphin show rendered thumbnails.

![stepv thumbnails: a coloured assembly, an IGES part and a sketch](docs/images/thumbnails.png)

## What makes it different

STEP is a boundary-representation format, so showing one means evaluating trimmed NURBS and
tessellating them while tolerating whatever the exporter wrote. stepv uses
[Open CASCADE Technology](https://dev.opencascade.org) for that, which is the only reliable open
kernel. [`plan.md`](plan.md) has the survey and the evidence.

- **It doesn't pass broken geometry off as the model.** Every face that fails to mesh goes
  through a recovery ladder: re-mesh, heal, refine at the face's own scale, detect a zero-area
  sliver, coarsen, approximate. Each face records which step produced it. Anything short of exact
  is drawn with a warning overlay (amber stripes, a red outline for a missing face, and a badge)
  instead of silently.
- **It degrades honestly.** If there's no geometry at all, the file's header metadata (originating
  system, schema, names) is still reported.
- **It is contained.** On Linux the kernel runs as a child process under a wall-clock and memory
  cap. On macOS the Quick Look extension process is the boundary.

On a 391-file robustness corpus (NIST PMI, 300 ABC models, CAx-IF rounds, a 221 MB assembly),
99.5% of files show faithfully and none crash or hang. The details are in `plan.md` §5.

## Install

**macOS 14+.** Download the DMG from
[Releases](https://github.com/vig-os/stepv/releases), drag `stepv.app` to Applications, and open
it once. If previews don't appear, enable *stepv* under System Settings → General → Login Items &
Extensions → Quick Look.

**Linux.** Download `stepv-<version>-x86_64.AppImage` from Releases and put it on your `PATH` as
`stepv`. For thumbnails and the file-type association, also install the integration files from a
checkout with `PREFIX=~/.local packaging/linux/install.sh --integration-only`. With Nix:

```bash
nix profile install github:vig-os/stepv
```

**From source.** You'll need OCCT ≥ 7.8, CMake and Rust.

```bash
cargo install stepv
cmake -S kernel -B build && cmake --build build   # the OCCT kernel
export STEPV_OCCT=$PWD/build/stepv-occt           # or install it as <prefix>/libexec/stepv/stepv-occt
```

## Use

```bash
stepv part.step --png part.png --size 512   # thumbnail, with the broken-face overlay
stepv part.step --glb part.glb              # glTF 2.0: named nodes, per-face colours
stepv part.step --info                      # header metadata as JSON; never fails
stepv view part.step                        # interactive viewer window
```

Every run prints one line of JSON on stdout, describing the outcome, the header metadata and the
kernel summary. Exit codes are `0` ok, `2` usage error, `3` no geometry (the metadata is still
valid), and `4` timeout. Results and failures are cached. `stepv --help` lists the limits and
options.

## Develop

```bash
direnv allow                 # or: nix develop
just test                    # kernel + fmt + clippy + every test, against the real kernel
just fixtures && just harness    # the robustness corpus and its pass-rate table
just cli-sweep               # the real CLI over the corpus, held to its exit-code contract
just macos-app && just macos-test --quicklook   # macOS app, through Quick Look itself
```

`plan.md` is the design record and the GitHub issues are the tracker.

## License

stepv is Apache-2.0 (see [`LICENSE`](LICENSE)). It makes use of facilities provided by the Open
CASCADE Technology software, which is LGPL-2.1 with the Open CASCADE exception. OCCT is always
dynamically linked and replaceable; [`NOTICE`](NOTICE) explains how, and lists the other bundled
libraries.
