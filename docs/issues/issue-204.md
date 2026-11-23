---
type: issue
state: open
created: 2026-11-22T14:55:20Z
updated: 2026-11-22T14:55:20Z
author: gerchowl
author_url: https://github.com/gerchowl
url: https://github.com/vig-os/stepv/issues/204
comments: 0
labels: chore
assignees: none
milestone: Hardening: graph core
projects: none
parent: none
children: none
synced: 2026-11-23T10:32:54.047Z
---

# [Issue 204]: [chore(mesh): B-spline forms with implied knots (QUASI_UNIFORM_CURVE, UNIFORM_CURVE, BEZIER_CURVE, PIECEWISE_BEZIER_CURVE) are routed](https://github.com/vig-os/stepv/issues/204)

Found in M1b (#143). `curve::bspline` takes only `B_SPLINE_CURVE_WITH_KNOTS`, the curve ADR-0001 §2.6 lists. The ISO 10303-42 subtypes whose knots are implied (`QUASI_UNIFORM_CURVE`, `UNIFORM_CURVE`, `BEZIER_CURVE`, `PIECEWISE_BEZIER_CURVE`, simple or complex) are routed as `unsupported curve`.

- **Corpus data:** 31 edges in nist-pmi + occt-import-js, all `QUASI_UNIFORM_CURVE` (for example `dm1-id-214.stp`). The golden counts the matching OCCT edges as "routed in Rust" (12).
- **Fix:** derive the knot vector per ISO 10303-42 §4.3.4 (quasi-uniform: end multiplicity p + 1 and evenly spaced interior knots; uniform: evenly spaced; Bézier: a single span). Then feed the same `BSpline`, and add a golden fixture for each form.
- **Also worth measuring:** M1b's Ultimaker build is about 580 ms against M1a's 270 ms for the analytic edges. The vertex search (`invert`) allocates per node and is the likely share. Profile it before M4a.

Refs: #143
