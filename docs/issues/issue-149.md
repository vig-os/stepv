---
type: issue
state: closed
created: 2026-11-09T00:13:03Z
updated: 2026-11-09T02:05:41Z
author: gerchowl
author_url: https://github.com/gerchowl
url: https://github.com/vig-os/stepv/issues/149
comments: 0
labels: docs
assignees: none
milestone: MVP: graph core
projects: none
parent: none
children: none
synced: 2026-11-09T09:29:38.324Z
---

# [Issue 149]: [ADR-0001 §2.1: a duplicated #id is resolved per reference, not kept once](https://github.com/vig-os/stepv/issues/149)

Found in G1 (#133). ADR-0001 §2.1 says "the graph keeps the same definition OCCT's reader keeps". OCCT 7.9.3's reader doesn't keep a single definition.

**What OCCT does** (`StepData_StepReaderData::SetEntityNumbers`, second pass): every record stays an entity. Each `#id` reference is resolved by searching back from the referring record for the nearest earlier definition of that id. If there is none, it searches forward for the nearest later one.

**Checked against `occt::outline`** on three variants of `tests/data/sketch.step` with `#7 = PRODUCT(...)` defined twice (A, then B):
- `A, #6, B` → A
- `#6, A, B` → A
- `A, B, #6` → B

"Keep first" and "keep last" both fail one of these.

**What G1 does:** `Graph::resolve(id, from)` follows OCCT's rule, and the test `graph::tests::duplicate_ids_resolve_as_occt_does` records it. `Graph::node(id)`, which has no referrer, gives the first definition.

**For G2:** the forward CSR must resolve each reference with `resolve(id, referrer)`, not `node(id)`.

**For the ADR:** amend §2.1's duplicate-id bullet to say this.

Refs: #133, #115
