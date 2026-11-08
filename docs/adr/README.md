# Architecture decision records

An ADR records one decision that shapes the code: the context, what was decided, what it costs,
and what was rejected. `plan.md` stays the narrative of what was built and measured; an ADR is the
binding reference for a decision that later work implements against.

- An ADR is numbered in order and never renumbered.
- A decision that changes is recorded in a new ADR that supersedes the old one. The old one keeps
  its text and gains a "Superseded by" line.
- Status is one of: Proposed, Accepted, Superseded.

| ADR | Title | Status |
| --- | --- | --- |
| [0001](0001-graph-core.md) | The graph core: a Rust model of the STEP file, with conservative boxes and a Rust mesher for analytic products | Proposed |
