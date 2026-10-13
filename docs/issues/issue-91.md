---
type: issue
state: closed
created: 2026-10-13T00:08:52Z
updated: 2026-10-13T01:02:20Z
author: gerchowl
author_url: https://github.com/gerchowl
url: https://github.com/vig-os/stepv/issues/91
comments: 0
labels: feature, priority:medium
assignees: none
milestone: none
projects: none
parent: none
children: none
synced: 2026-10-13T09:11:28.703Z
---

# [Issue 91]: [kernel: read the input from an inherited fd (/dev/fd/N) with an explicit --format (#64 step 1)](https://github.com/vig-os/stepv/issues/91)

## Why
#64 (owner decision, 2026-10-13): Quick Look runs the kernel as a child of an XPC supervisor service. The service can't open the file by path (the sandbox denies it); it only has an **fd** handed over XPC (#79 round 2: open-by-path gives EPERM, `/dev/fd/3` works). Today the kernel:
- calls `realpath` on its input path, and on macOS `realpath("/dev/fd/3")` gives `/dev/fd/<name>`, which breaks;
- picks the reader (STEP, IGES, BREP) by file extension, which an fd doesn't have.

## Scope
- [ ] `stepv-occt` accepts `/dev/fd/N`, or `--fd N`. Pick one and justify it; `/dev/fd/N` keeps OCCT's path-based readers. The path isn't `realpath`ed for fd inputs.
- [ ] `--format step|iges|brep`, required for fd input and optional otherwise (it overrides the extension).
- [ ] The sandbox (Seatbelt and Landlock) still applies; an fd input needs no extra file access.
- [ ] The Rust side (`src/occt.rs`) can pass an fd input. That is an API addition: rustdoc it.
- [ ] Multi-file assemblies with an fd input: the sibling part files can't be reached. This is a documented limit with its own issue; the run reports which parts are missing, as it does for missing files today.

## Acceptance
- A test runs the kernel on an fd (open the fixture, pass the fd as fd 3, and point the path at a nonexistent file) for STEP, IGES and BREP. Each yields the same output as a run by path.
- A wrong or missing `--format` with fd input fails clearly.
- Corpus behaviour is unchanged for path inputs.

Refs: #64, #79
