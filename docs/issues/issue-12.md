---
type: issue
state: closed
created: 2026-10-04T14:47:22Z
updated: 2026-10-11T15:23:34Z
author: gerchowl
author_url: https://github.com/gerchowl
url: https://github.com/vig-os/stepv/issues/12
comments: 1
labels: chore, priority:medium, needs-human
assignees: none
milestone: none
projects: none
parent: none
children: none
synced: 2026-10-12T08:55:39.521Z
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

---

# [Comment #1]() by [gerchowl]()

_Posted on October 11, 2026 at 03:23 PM_

Done in #52 (merged as f7c7543), as far as public sources allow:

- **exporter-matrix:** 7 writers (SolidWorks, NX, CATIA, Fusion 360, FreeCAD/OCC, Inventor, Creo), each verified from its own header and pinned per file. All 7 pass.
- **real-assembly:** the whole Ultimaker 3 printer, 143 MB of SolidWorks, 523 parts. It loads correctly with every part named and coloured, but takes 101 s, 92 s of it in STEP transfer. That is #51.
- **mbx-if-public:** MBx-IF's one open AP242 model.
- plan.md §5 "Corpus fill (#12)" has the table: 99.2% of 400 faithful or wireframe.
- On the way, `corpus.sha256` became byte-exact (stress-gen no longer stamps the wall clock), and the harness's own tests now run in `just test`.

The one part public sources can't fill, the members-only CAx-IF rounds, moves to 53.

