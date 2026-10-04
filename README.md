<!-- Seeded by vigOS devkit — yours to edit; upgrades never overwrite this file. -->
<!-- Bugs / missing tools: https://github.com/vig-os/devkit/issues -->

# stepv

Fast STEP/IGES/BREP preview and thumbnails for macOS Quick Look and Linux file managers.

Press space on a `.step` file in Finder and see the part. Get rendered thumbnails in icon view, in
Nautilus, and in Dolphin. One CLI underneath both platforms.

> **Status: scaffolded, kernel unproven.** Nothing here has rendered a STEP file yet. The kernel has
> been *chosen* — OCCT V8 via [`occt-wasm`](https://github.com/andymai/occt-wasm) on wasmtime — with
> the survey, the rejected alternatives, the risks and the fallback all written down. The next step
> is a robustness harness against a real corpus.
>
> **Start at [`plan.md`](plan.md).** It is the handoff: §1–§3 are settled decisions with evidence,
> §5 is the work, §6 is what is deliberately unresolved.

## Why this is not just a renderer

STEP is a boundary-representation format. Showing one means evaluating trimmed NURBS surfaces and
their topology, then tessellating with tolerance-based healing of whatever the exporter produced.
That evaluation is the whole problem — and it is why the kernel decision came before any UI.

The existing macOS previewer in this space builds on Foxtrot, whose own authors describe it as "a
proof-of-concept demo, not an industrial-strength CAD kernel". `plan.md` §2 explains what was
surveyed instead and why OCCT is the only reliable open answer today.

## Development

```bash
direnv allow      # or: nix develop
cargo test
stepv --help
```

The dev environment is [vigOS devkit](https://github.com/vig-os/devkit) in `direnv` mode with the
Rust language pack (`vigos.lib.mkRustProject`). `nix flake check` runs fmt, clippy, nextest,
doctests and `cargo doc`.

## License

Apache-2.0. See [`LICENSE`](LICENSE).

Note that the compiled OCCT WebAssembly module this tool will execute is LGPL-2.1-only and is
shipped as a separate replaceable file rather than embedded — see `plan.md` §3.
