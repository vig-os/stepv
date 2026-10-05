---
type: issue
state: open
created: 2026-10-04T14:47:25Z
updated: 2026-10-04T19:00:50Z
author: gerchowl
author_url: https://github.com/gerchowl
url: https://github.com/vig-os/stepv/issues/14
comments: 0
labels: chore, priority:low, needs-human
assignees: none
milestone: none
projects: none
parent: none
children: none
synced: 2026-10-05T08:45:50.651Z
---

# [Issue 14]: [Report the occt-wasm STEP-import gap upstream](https://github.com/vig-os/stepv/issues/14)

## Description

S1 found that `occt-wasm` (Plan A) cannot import STEP from its Rust crate (plan.md §3 "What would
bring Plan A back", §5 "S1 result"). Upstream knows about part of it (PR andymai/occt-wasm#371,
"Known gap"), but **no upstream issue tracks it**. Filing one is outward-facing, on a third-party
repo, so it's left for a human to decide and file.

## What to report

1. crates.io `occt-wasm` 4.0.0 cannot instantiate (`unknown import:
   env::emscripten_get_preloaded_image_data`). It's fixed on `main` as crate 4.1.0, but untagged.
2. On `main` @ `1091f22`, `import_step` / `xcaf_import_step` trap (`uninitialized element`): the
   facade writes input to `/tmp`, and the standalone WASI module imports no `open`. Suggest
   `STEPCAFControl_Reader::ReadStream` over an `std::istringstream`.
3. No way to construct the kernel from a precompiled module (`Module::deserialize`) or with a store
   limiter: `new()` JIT-compiles 23 MB on every process start (2.2 s measured).

## Why it matters to stepv

Plan B is the kernel and it works. These three are exactly the conditions under which Plan A would
come back (better containment, no C++ toolchain). Low priority.

