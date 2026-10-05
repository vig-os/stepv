---
type: issue
state: open
created: 2026-10-04T14:47:22Z
updated: 2026-10-04T19:00:47Z
author: gerchowl
author_url: https://github.com/gerchowl
url: https://github.com/vig-os/stepv/issues/12
comments: 0
labels: chore, priority:medium, needs-human
assignees: none
milestone: none
projects: none
parent: none
children: none
synced: 2026-10-05T08:45:51.601Z
---

# [Issue 12]: [Corpus: full CAx-IF rounds, exporter matrix, a real large assembly](https://github.com/vig-os/stepv/issues/12)

## Description

The S1 pass rate (99.5% faithful) is measured on 391 files, but **three of the corpus sources in
`tests/fixtures/manifest.toml` are still missing or synthetic**. Until they are filled, the number
is a lower bound on one exporter (ABC is all Onshape) and a sample of the standard. It is not a claim
about CATIA (plan.md §5 "Corpus caveats").

## Scope

- **`cax-if`**: the full CAx-IF test rounds. They're behind registration, so someone has to register
  and fetch by hand. Today only the public subset in `occt-import-js` is covered.
- **`exporter-matrix`**: real output from SolidWorks, NX, CATIA, Fusion 360, FreeCAD and Onshape.
  AP242 interoperability breaks between writers, and XCAF names/colours are really tested here.
- **A real large assembly** (~200 MB+) beside the synthetic `stress-assembly`, to stress the XCAF
  tree, not just bytes.

`just fixtures` reports each missing source as MISSING on every run. Drop files into
`tests/fixtures/<name>/`, re-run `just fixtures` (it re-hashes into `corpus.sha256`), re-run
`just harness`, and update the plan.md tables.

## Acceptance

All three present; plan.md §5 re-recorded with them.

