---
type: issue
state: open
created: 2026-11-08T17:08:52Z
updated: 2026-11-08T17:08:52Z
author: gerchowl
author_url: https://github.com/gerchowl
url: https://github.com/vig-os/stepv/issues/120
comments: 0
labels: none
assignees: none
milestone: none
projects: none
parent: none
children: none
synced: 2026-11-09T09:29:50.476Z
---

# [Issue 120]: [macOS: .p21 is declared as STEP but no extension or kernel path accepts it](https://github.com/vig-os/stepv/issues/120)

Found in #107's final review (nit, not fixed there).

`macos/Host/Info.plist:28` declares `p21` as a filename extension of `ch.exoma.stepv.step`, so Quick Look routes `.p21` files to stepv's extensions. But `StepvKernel.format(of:)` (`macos/Shared/StepvKernel.swift`) accepts only step/stp, iges/igs and brep. So a `.p21` file gets "No preview: not a STEP, IGES or BREP file" and no thumbnail.

Decide one way: accept `p21` as `step` in `format(of:)`, with a test (check that the kernel and the CLI's own extension handling agree), or drop it from the UTI declaration.

Refs #64
