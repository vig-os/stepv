---
type: issue
state: open
created: 2026-11-22T17:34:10Z
updated: 2026-11-22T17:34:11Z
author: gerchowl
author_url: https://github.com/gerchowl
url: https://github.com/vig-os/stepv/issues/210
comments: 0
labels: chore
assignees: none
milestone: Hardening: graph core
projects: none
parent: none
children: none
synced: 2026-11-23T10:32:52.532Z
---

# [Issue 210]: [design: ABC files declare a 1e-17 mm uncertainty, so the on-surface check routes ~20% of their planar faces](https://github.com/vig-os/stepv/issues/210)

Found by M2's planar-face golden (#144, PR #209), run locally on the ABC subset: 298 files, leaving out `00000046` (Rust only) and `00000048` (kernel exit 86).

Every one of the 300 ABC files (Onshape exports) declares `UNCERTAINTY_MEASURE_WITH_UNIT(LENGTH_MEASURE(1.E-17), …)`. f64 can't hold coordinates to that, and the files' own geometry isn't that consistent either. ADR §2.6 checks boundary points against the declared uncertainty. Since M2, that check allows f64's rounding of the distance as well, `8ε·(|p| + |o|)`. Even so:
- **5,703 planar faces route `curve:off-surface`.** Their points lie more than the rounding off their plane, for example from 15-digit coordinates on tilted planes.
- **4,788 meshed faces have a vertex off its line or conic beyond 1e-17 mm.** The golden counts them and doesn't compare them (#207).

The other 18,173 faces match OCCT's area exactly (worst 3.2e-9 relative) and 1,971 match within their bound, with 0 failing. So ABC's coverage (75% by the census) will be capped by this, not by the surfaces.

**Design question for the owner:** should a declared uncertainty below some floor (for example OCCT's confusion precision of 1e-7 mm, or a multiple of the model's f64 resolution) be raised to that floor? Or should ABC faces route as they do now? M3's on-surface check for analytic faces hits the same question.

