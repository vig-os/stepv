---
type: issue
state: open
created: 2026-11-09T00:59:35Z
updated: 2026-11-09T00:59:35Z
author: gerchowl
author_url: https://github.com/gerchowl
url: https://github.com/vig-os/stepv/issues/152
comments: 0
labels: chore, priority:low
assignees: none
milestone: none
projects: none
parent: none
children: none
synced: 2026-11-09T09:29:37.447Z
---

# [Issue 152]: [graph: G1 scanner review nits (test comment, depth overflow, BOM, rustdoc)](https://github.com/vig-os/stepv/issues/152)

Review nits from #151 (G1 scanner), deferred under MVP-first:

- `src/graph/tests.rs` `malformed_inputs_refuse_without_panicking`: the comment says each of the 200 bit flips is tried alone and all together, but only the all-together case is built. Either build the per-flip cases or fix the comment. The case also asserts a refusal, so editing `assembly.step` would break it; assert only "no panic" for the flip cases.
- `src/graph/lex.rs` `group`: `depth += 1` on a `u32` can overflow in debug builds (theoretical; ≥ 4 GiB of `(`). Use checked arithmetic and refuse with an offset.
- `src/graph/lex.rs` `scan`: a UTF-8 BOM at the start of a file is refused as `NotPart21`; `type_name` accepts names that start with a digit or `!`. Decide and document per ISO 10303-21.
- `src/graph/mod.rs`: `TypeName::Complex(&[Box<str>])` differs from ADR-0001 §2.4's sketch (`&[&str]`). Note it in the §2.1/§2.4 amendment that #149 needs.
- Missing rustdoc: `GraphError::kind` and the variants `UnterminatedString`, `UnterminatedComment`, `Syntax`. Consider `#![deny(missing_docs)]` on `graph`.

Refs: #133

