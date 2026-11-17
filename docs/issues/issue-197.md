---
type: issue
state: open
created: 2026-11-16T13:54:39Z
updated: 2026-11-16T13:54:39Z
author: gerchowl
author_url: https://github.com/gerchowl
url: https://github.com/vig-os/stepv/issues/197
comments: 0
labels: chore
assignees: none
milestone: Hardening: graph core
projects: none
parent: none
children: none
synced: 2026-11-17T10:27:39.926Z
---

# [Issue 197]: [graph colours: model XCAF's part split for a solid beside a curve set](https://github.com/vig-os/stepv/issues/197)

From G7 (#141). `graph::Colors::parts` mirrors XCAF's split of a product into parts per shape definition representation, per related representation, per solid/shell of a related representation holding several, and for a shape aspect's own solids/shells. XCAF splits further in cases not modelled, e.g. one representation listing a solid and a `GEOMETRIC_CURVE_SET` (io1-tu-203: 2 kernel parts, graph 1). On the corpus 4 leaves fall in this case; `tests/graph_colors.rs` compares every face's drawn colour there (0 mismatches), so the drawing is right, but the per-part colour isn't compared.

Done when: those leaves split as XCAF does (derived black-box from fixtures, ADR §4), and the parity test's 'split unlike the graph' count is 0 on the corpus.
