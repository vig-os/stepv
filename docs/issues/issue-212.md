---
type: issue
state: open
created: 2026-11-24T14:57:49Z
updated: 2026-11-24T14:57:49Z
author: gerchowl
author_url: https://github.com/gerchowl
url: https://github.com/vig-os/stepv/issues/212
comments: 0
labels: none
assignees: none
milestone: MVP: graph core
projects: none
parent: none
children: none
synced: 2026-11-25T11:03:01.884Z
---

# [Issue 212]: [graph-core M3b: sphere and torus faces](https://github.com/vig-os/stepv/issues/212)

Split from #145 (M3), as pre-agreed. **M3a** (#145) covers cylinder and cone, the shared face/seam/pole machinery, and the #210 floor. This issue covers **sphere and torus**: poles, two radii, and the doubly-curved deflection rule.

## Scope
`SPHERICAL_SURFACE` and `TOROIDAL_SURFACE` faces on the M3a machinery:
- closed-form (u, v) inversion;
- poles (sphere);
- a Steiner grid at the deflection step in both curved directions.

## Decided (architect, 2026-11-24, ADR §2.6 amended in this PR)
**Boundary triangles on doubly-curved surfaces.** A boundary chord already sags up to d from its curve (M1). The triangle across it can deviate from the surface by up to about 4/3·d.
- **Decision:** prove the bound and document it. Interior triangles must stay ≤ d. Triangles on a boundary chord must stay ≤ the proven bound (about 4/3·d), with the proof written in §2.6.
- **Rejected:**
  - meshing every edge at d/2: a global cost, roughly 1.4× the edge points on every face, to fix two surface types;
  - Steiner points next to each boundary chord: more machinery, and it still only approaches d.

## Acceptance
- Fixture solids (spheres with poles, full and partial tori) mesh watertight, meaning every interior edge is used by exactly two triangles.
- Area is within the deflection's bound of the exact area.
- The interior and boundary deviation bounds are checked against the exact surface.
- Golden: within that bound of OCCT's topology area. A file that can't be compared fails.

## Test
`nix develop -c cargo test mesh::analytic`

## Depends on
M3a (#145).

## Process
- At most ~400 product lines; over that, split.
- MVP first: hardening becomes `chore` issues.
- Host rules v3.

Refs: #115 #145
