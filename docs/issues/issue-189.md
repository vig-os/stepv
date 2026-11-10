---
type: issue
state: open
created: 2026-11-09T09:11:31Z
updated: 2026-11-10T00:12:33Z
author: gerchowl
author_url: https://github.com/gerchowl
url: https://github.com/vig-os/stepv/issues/189
comments: 1
labels: chore
assignees: none
milestone: Hardening: graph core
projects: none
parent: none
children: none
synced: 2026-11-10T09:31:59.262Z
---

# [Issue 189]: [chore(graph): G5 review nits (tree --depth expands everything, heap-test cross-check, loud skip)](https://github.com/vig-os/stepv/issues/189)

These nits are from the fresh review of PR #188 (G5, #137) at cdba004. Under MVP-first they are deferred as a chore.

1. `src/graph/json.rs:260-264`: `--depth` only filters rows; the whole outline is still expanded. So `tree --depth 0` on an outline over the caps exits 3 even though only the roots were asked for. The depth should limit the expansion itself.
2. `tests/graph_cli.rs:259-360` heap test:
   - on the stress file it only asserts `faces > 0`; cross-check it against `faces_of`;
   - with `STEPV_CORPUS` unset, the test reports "ok" rather than ignored. Prefer `#[ignore]` with a reason, or the repo's skip convention.
3. The injection test injects only FILE_NAME's name field. Also inject author, org and description directly (the golden files show they're wrapped, but no test proves it).

Refs: #137
---

# [Comment #1]() by [gerchowl]()

_Posted on November 9, 2026 at 09:48 AM_

These nits are from the re-review of round 2 (f2b02ff, now merged):

4. `tests/graph_cli.rs` `stepv_within` (~line 251) has two latent problems:
   - it pipes stdout and stderr but doesn't read them while it polls, so a child that writes more than ~64 KiB blocks and fails as a misleading "still running";
   - it doesn't `wait()` after `kill()`.
5. The recursion test relies on a finite stack. Under `ulimit -s unlimited` it would grow without a memory cap, against the repo's memory-bounds rule. Run it under `stepv-cap`.
6. `src/graph_cmd.rs:153-156`: no test covers the stdout write-error exit code (EPIPE), and the two match arms duplicate each other.
7. The injection test's stderr check spawns each command a second time instead of reusing the first run's output.

