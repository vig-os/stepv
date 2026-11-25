---
type: issue
state: open
created: 2026-11-24T14:23:53Z
updated: 2026-11-24T14:23:53Z
author: gerchowl
author_url: https://github.com/gerchowl
url: https://github.com/vig-os/stepv/issues/211
comments: 0
labels: chore
assignees: none
milestone: Hardening: graph core
projects: none
parent: none
children: none
synced: 2026-11-25T11:03:02.347Z
---

# [Issue 211]: [chore(mesh): M2 planar-face nits (#209 review)](https://github.com/vig-os/stepv/issues/211)

Nits from the fresh review of PR #209 (M2, #144), deferred under the MVP rule:

- `src/mesh/plane.rs:191`: `owner[i]` remembers only the first loop that reaches a vertex. A loop passing twice through a vertex already used by another loop isn't routed as `loop:self-touching`. It either meshes correctly or routes as `rs-fail:orientation`. Only the reason is inconsistent.
- `plane.rs:169`: a NaN `dot(d, z)` (from inf·0 near 1e308) passes the off-plane check, so spade later routes it as `rs-fail:coordinate`. Again only the reason is wrong. Check finiteness first.
- `tests/mesh_plane.rs:308`: every curve that isn't a line or a conic gets the d·|chord| bound, including a `TRIMMED_CURVE` around a circle. Log the worst bound-to-area ratio, and give trimmed conics the exact segment area.
- `mesh_plane.rs:431-460`: the by-value fallback (350 faces) never checks normals. Compare normals there too.
- `mesh_plane.rs:404`: `reversed` is always false in the golden, so winding is never checked against OCCT's shell orientation. Revisit in M4a.
- `plane.rs:172`: points are inserted one by one rather than bulk-loaded, and constraints are added one at a time. Adversarial input could take quadratic time. Bulk-load, and bound the worst case.

Refs: #144
