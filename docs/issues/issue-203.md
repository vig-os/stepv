---
type: issue
state: open
created: 2026-11-17T18:11:11Z
updated: 2026-11-17T21:19:40Z
author: gerchowl
author_url: https://github.com/gerchowl
url: https://github.com/vig-os/stepv/issues/203
comments: 1
labels: chore
assignees: none
milestone: Hardening: graph core
projects: none
parent: none
children: none
synced: 2026-11-18T09:57:18.683Z
---

# [Issue 203]: [chore(mesh): M1a edge-polyline nits (#201 review)](https://github.com/vig-os/stepv/issues/203)

Nits from the fresh review of PR #201 (M1a, #142). They fit the MVP rule, so they are deferred here:

- `src/mesh/edges.rs:265`: a closed edge is detected only by the same vertex or `sweep == 0.` exactly. Two distinct but coincident vertices on a full circle can round to a tiny positive sweep (giving one chord) or to ≈2π (giving the full circle), depending on the sign of the rounding. Decide on a tolerance-based closed test, for example the vertex distance compared with the context's uncertainty.
- `edges.rs:98`: `EdgePoint::index` has no rustdoc.
- `edges()` and `routed()` iterate a HashMap, so their order isn't deterministic. Sort them, or use an ordered map, so output and goldens are reproducible.
- `edges.rs:271`: when d/r is below about 1e-16, `acos(1−d/r)` evaluates to 0, so the edge is routed as "too many segments". That's acceptable, but document it.
- `tests/mesh_edges.rs:292-294`: a file whose `check()` fails is only printed as "not compared". It is the same class of problem as #200: corpus gates must count "not compared" files and fail on any of them.

Refs: #142
---

# [Comment #1]() by [gerchowl]()

_Posted on November 17, 2026 at 09:19 PM_

Two more nits from the round-3 re-review of #201 (head ff880d8):
- `src/graph/bounds.rs:492`: the reason text for an overflowing CARTESIAN_POINT changed from "non-finite coordinate" to "not a finite 3-D point", and an overflowing radius now says "non-finite radius" (it used to come out as a NaN corner). Both still come out as Unknown, but CHANGELOG.md doesn't mention the change.
- When `p - c` overflows to ±inf on oblique axes, `atan2(±inf, ±inf)` gives a finite but wrong parameter. The output stays finite, so the postcondition holds, but the parameter is wrong. Route it as non-finite.

