---
type: issue
state: open
created: 2026-11-09T01:13:09Z
updated: 2026-11-09T01:13:09Z
author: gerchowl
author_url: https://github.com/gerchowl
url: https://github.com/vig-os/stepv/issues/153
comments: 0
labels: chore
assignees: none
milestone: MVP: graph core
projects: none
parent: none
children: none
synced: 2026-11-09T09:29:37.009Z
---

# [Issue 153]: [graph: re-run the G2 census per product after G3](https://github.com/vig-os/stepv/issues/153)

G2 (#134) committed the fast-path census to \`docs/adr/0001-census.md\`. ADR-0001 §2.6 asks for "the share of products all of whose faces are analytic", but the product → faces mapping is G3's \`Structure\`. So G2 counts **per shape representation**: each \`…SHAPE_REPRESENTATION\` that reaches an \`ADVANCED_FACE\` over \`refs\`, without entering another representation.

G2's numbers: 70.9% of 2,299 representations in scope (Ultimaker 55.3%, stress assembly 50.0%, \`nist-pmi\` 69.0%, ABC 75.3%).

## Do after G3
- Extend \`examples/graph_census.rs\` to count per product (\`Product::bodies\`), with shapeless products and the representations reached through \`MAPPED_ITEM\`s handled as \`structure()\` defines them.
- Re-run over the same 391 files and replace the "Representations" columns in \`docs/adr/0001-census.md\`. Edit ADR §2.6's measured line to match.
- If the per-product share comes out far below §2.6's guess, the M slices are re-cut before M1a (ADR §6).

Refs: #134, #115
