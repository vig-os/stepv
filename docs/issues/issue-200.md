---
type: issue
state: open
created: 2026-11-17T16:11:10Z
updated: 2026-11-17T16:11:10Z
author: gerchowl
author_url: https://github.com/gerchowl
url: https://github.com/vig-os/stepv/issues/200
comments: 0
labels: chore
assignees: none
milestone: Hardening: graph core
projects: none
parent: none
children: none
synced: 2026-11-18T09:57:19.478Z
---

# [Issue 200]: [chore(graph): G7 review nits + corpus gates don't count 'not compared' files](https://github.com/vig-os/stepv/issues/200)

These nits are from the fresh review of PR #199 (G7, #141) at 2a8780a:

1. **Gate strength, affecting G4, G6c and G7.** `tests/graph_colors.rs:207-212`: a file whose `check` fails is only printed as "not compared". That covers "leaf has no graph path" and "mesh and topology parts differ", not just kernel exit 86. Such files aren't counted in `Tally`, so a regression passes silently as long as one leaf is compared. The `s1-c5-214`/`s1_pe_214` skip isn't counted either. The G4/G6c gates copy the same pattern. Count every not-compared file by reason, and fail on any reason outside a documented allow-list.
2. `tests/graph_colors.rs:51-63`: the colour comparison isn't exact. It allows 1e-6 absolute, and the colour-set fallback rounds to 1e-5, with no reason given. Use bit equality, or document XCAF's conversion error.
3. `tests/graph_colors.rs:124-127`: the set fallback (111 corpus parts) can hide a swap: {red, None} vs {None, red} compares equal. It's counted, but only exact face ids (M5) will close it.
4. No fixture has an assembly. "A leaf's colour is its product's" and "styles on an instance's representation colour nothing" (`src/graph/colors.rs:34-36`) rest only on the corpus.
5. `src/graph/bounds.rs:786-789`: the `mapped_rep` doc line was separated from its function and now opens `related_reps`'s doc.
6. `src/graph/structure.rs:242-245`: `structure()` always makes an extra pass over the whole file to index styles, even for callers that never read colours. Make it lazy.
7. `tests/graph_colors.rs:425`: the fixture assumes the shell's id is the surface model's id − 1.

Refs: #141
