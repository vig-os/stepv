---
type: issue
state: open
created: 2026-11-24T15:12:53Z
updated: 2026-11-24T16:52:22Z
author: gerchowl
author_url: https://github.com/gerchowl
url: https://github.com/vig-os/stepv/issues/213
comments: 1
labels: chore
assignees: none
milestone: Hardening: graph core
projects: none
parent: none
children: none
synced: 2026-11-25T11:03:01.419Z
---

# [Issue 213]: [chore(mesh): a cone with a seam edge to its apex routes rs-fail:degenerate triangle](https://github.com/vig-os/stepv/issues/213)

Found in M3a (#145).

A cone face bounded by its base circle and a **seam edge** running down a ruling to the apex (OCCT's own form) routes as `rs-fail:degenerate triangle`.

**Why.** `mesh::analytic` triangulates in scaled (u, v), where the apex is a pole line. The seam edge is a constraint with no interior points, and every ruling passes through the apex. So the Delaunay triangulation fans the apex's vertices to pairs of grid points on the next ruling, and those triangles are collinear in 3-D. M3a detects these folds (a 3-D area under 1e-9 of the two edges' product) and routes the face.

**Impact.** In the corpus survey, this cone form occurs once (occt-import-js). ABC and nist write the apex as a `VERTEX_LOOP` or leave it implicit; those go through the virtual seam, whose interior points prevent the fan, and they mesh.

**Possible fix.** Triangulate developable surfaces in their isometric development, a polar sector for a cone. There, collinear triples have no circumcircle, so Delaunay never fans them. The open question is how to give the apex a per-ruling normal, since it becomes a single point in the development.

---

# [Comment #1]() by [gerchowl]()

_Posted on November 24, 2026 at 04:52 PM_

No longer reproduces in M3a (#214): the straight rulings take no grid row, so nothing fans the apex to two points of one ruling. OCCT's form (a seam edge down to the apex) now meshes; see the unit test `a_cone_down_to_its_apex`. The fold check stays as a guard. Closed by #214.

