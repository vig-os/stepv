---
type: issue
state: open
created: 2026-10-12T15:06:34Z
updated: 2026-10-12T15:06:34Z
author: gerchowl
author_url: https://github.com/gerchowl
url: https://github.com/vig-os/stepv/issues/66
comments: 0
labels: feature
assignees: none
milestone: none
projects: none
parent: none
children: none
synced: 2026-10-13T09:11:38.881Z
---

# [Issue 66]: [artifact: per-part boxes from PartBox frames (#54 P1)](https://github.com/vig-os/stepv/issues/66)

The outline artifact from #62 (`cache::OutlineArtifact`) has an optional `boxes` field, one box per leaf of the tree. It is always absent for now, because the kernel does not emit per-part boxes yet: those are the `PartBox` frames (stream kinds 4 and up) planned as #54 P1.

## Scope
- Once the kernel streams `PartBox` frames, `occt::Event` carries them and the CLI fills `boxes` in the artifact as they arrive (written atomically, as the tree is).
- Decide whether that bumps `cache::OUTLINE_VERSION` (a v1 reader already accepts the field, so it should not need to).

Refs: #62, #54
