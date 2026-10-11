---
type: issue
state: closed
created: 2026-10-04T14:47:25Z
updated: 2026-10-10T23:29:53Z
author: gerchowl
author_url: https://github.com/gerchowl
url: https://github.com/vig-os/stepv/issues/14
comments: 1
labels: chore, priority:low, needs-human
assignees: none
milestone: none
projects: none
parent: none
children: none
synced: 2026-10-11T08:35:01.289Z
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

---

# [Comment #1]() by [gerchowl]()

_Posted on October 10, 2026 at 11:29 PM_

Filed upstream as https://github.com/andymai/occt-wasm/issues/393, after re-checking all three claims on its `main` @ `89fb058` (2026-10-08), with a probe build of the crate:
1. crates.io is still at 4.0.0 (`main` is 4.1.0, unpublished).
2. `import_step` and `xcaf_import_step` trap with `uninitialized element`. Upstream ignores its own STEP tests for the same `/tmp` reason.
3. `new()` only ever calls `Module::new`, which takes 2.6–2.9 s, and there's no store limiter.

