---
type: issue
state: open
created: 2026-11-09T02:05:42Z
updated: 2026-11-09T02:05:42Z
author: gerchowl
author_url: https://github.com/gerchowl
url: https://github.com/vig-os/stepv/issues/156
comments: 0
labels: chore, priority:low
assignees: none
milestone: none
projects: none
parent: none
children: none
synced: 2026-11-09T09:29:36.163Z
---

# [Issue 156]: [graph: G2 review nits (census curve subtypes, ref refusal docs, depth invariant)](https://github.com/vig-os/stepv/issues/156)

Review nits from #154 (G2), deferred under MVP-first:

- `examples/graph_census.rs:172` `curve_3d`: matches only `SURFACE_CURVE` and `SEAM_CURVE`. A plain `INTERSECTION_CURVE` (a subtype of `SURFACE_CURVE`) is counted as out of scope, and a complex `SURFACE_CURVE` reads as "(unreadable)" through `ref_arg`. Both undercount, so the census stays conservative. **M1a (#142) must classify these correctly with `is_a`.**
- `src/graph/lex.rs:305`: an argument reference above u32, or a bare `#` with no digits, now refuses the whole file (before #154 it was skipped silently). Document it in the CHANGELOG and ADR §2.1.
- `src/graph/args.rs:364`: `depth -= 1` is safe only because `part()` checks for `(` before calling `skip()`. Make the invariant explicit (`debug_assert!` or `saturating_sub`).

Refs: #134

