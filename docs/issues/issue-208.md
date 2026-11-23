---
type: issue
state: open
created: 2026-11-22T17:00:33Z
updated: 2026-11-22T17:00:40Z
author: gerchowl
author_url: https://github.com/gerchowl
url: https://github.com/vig-os/stepv/issues/208
comments: 0
labels: chore
assignees: none
milestone: Hardening: graph core
projects: none
parent: none
children: none
synced: 2026-11-23T10:32:52.847Z
---

# [Issue 208]: [chore(mesh): mesh planar loops that touch themselves or project to one point](https://github.com/vig-os/stepv/issues/208)

Found by M2's planar-face golden (#144). On nist-pmi and occt-import-js, `mesh::plane` routes 6 of about 2,800 planar faces:
- `loop:self-touching` (3): a loop passes one vertex twice, like a figure eight through a shared corner;
- `rs-fail:coincident` (3): two different 3-D points within the uncertainty of the plane project to the same (u, v).

Both are routed by design (ADR-0001 §2.6, M2 notes), and nothing is dropped. They could be meshed:
- split a self-touching loop at its repeated vertex into simple loops before the CDT;
- merge coincident points that are the same `VERTEX_POINT`'s or lie within the uncertainty of each other. This keeps watertightness only if both faces merge the same way, so it belongs with M4a's stitching.

Measure first: M4a's ledger gives the count per corpus. Build this only if that count matters.

