---
type: issue
state: open
created: 2026-11-09T08:16:52Z
updated: 2026-11-09T09:15:17Z
author: gerchowl
author_url: https://github.com/gerchowl
url: https://github.com/vig-os/stepv/issues/187
comments: 2
labels: chore
assignees: none
milestone: none
projects: none
parent: none
children: none
synced: 2026-11-09T09:29:23.459Z
---

# [Issue 187]: [chore(graph): a work budget for `stepv graph query`](https://github.com/vig-os/stepv/issues/187)

Found while landing G5 (#137, ADR-0001 §2.4). MVP first: filed for later, not fixed there.

`stepv graph query` runs the filter to completion. `--limit` caps how many outputs are kept, but nothing caps the work done between outputs:

- `recurse(refs(.)[])` on a hostile file with a reference cycle never ends (Part 21 doesn't forbid cycles, and the graph keeps them);
- `last(range(1e12))`, `repeat(.) | empty` and similar run until killed;
- the `tree` filter materialises the whole outline, up to the expansion cap of 1,000,000 occurrences, before the filter reads any of it.

For #124 (stepv as an agent tool) the filter comes from an agent, which file text may steer, so this is a denial-of-service path.

**Proposal:** a wall-clock or step budget on the run (jaq has no fuel, so a deadline checked in the natives and between outputs), a refusal naming it, and a `tree` filter that pages like `stepv graph tree`.

Refs: #137, #124
---

# [Comment #1]() by [gerchowl]()

_Posted on November 9, 2026 at 09:11 AM_

Scope update from the architect's review of PR #188 (round 1, cdba004):

**Crash isolation is in scope here, alongside the work budget.**
- jaq 3.1.2 has no recursion guard: `def f: f+1; f` overflows the stack and aborts the process (see jaq's own note at `jaq-core-3.1.2/src/compile.rs:939`).
- A reference cycle in the file walked with `recurse(refs(.)[])` may also run unbounded. Round 2 of #188 measures this.

**This must land before any in-process use of `query`**, e.g. the MCP server (#124). An abort there would take down the server, not only one CLI call.

Design options to weigh:
1. Run each query in a subprocess, with `stepv-cap` memory and time limits (reusing the kernel's sandbox/cap path).
2. A depth/work guard in our native filters, plus a large-stack worker thread. This doesn't cover user-defined recursion in jaq itself.
3. Upstream a recursion limit to jaq.

Option 1 is the only one that covers both cases. Pick it in a short ADR note.

Also from the review: query output and errors can carry unwrapped file text when the filter asks for it (`.untrusted`, `error`, computed keys). Re-wrapping or screening for agent consumers belongs with #124.

---

# [Comment #2]() by [gerchowl]()

_Posted on November 9, 2026 at 09:15 AM_

Measured in G5 round 2 (PR #188, release build under `stepv-cap --memory-mb 4096`, 30 s timeout):
- `def f: f+1; f` overflows jaq's stack and the process aborts: SIGABRT, exit status 134, Rust's "stack overflow" on stderr, nothing on stdout. Pinned by `tests/graph_cli.rs::a_filter_s_work_is_unbounded_as_documented`.
- On a file with a reference cycle (`#1=X(#2); #2=X(#1);`), `1 | recurse(refs(.)[])` streams forever. `--limit` stops it (exit 0, `"more": true`, which is tested). Collected, as `[1 | recurse(refs(.)[])] | length`, it never ends and only the timeout killed it (exit 124). That case has no test, because the test would hang.

So crash isolation (a child process) belongs here as well as the work budget. ADR §2.4, `--help` and the CHANGELOG now state both limits.

