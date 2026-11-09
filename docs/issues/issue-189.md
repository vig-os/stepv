---
type: issue
state: open
created: 2026-11-09T09:11:31Z
updated: 2026-11-09T09:11:31Z
author: gerchowl
author_url: https://github.com/gerchowl
url: https://github.com/vig-os/stepv/issues/189
comments: 0
labels: chore
assignees: none
milestone: MVP: graph core
projects: none
parent: none
children: none
synced: 2026-11-09T09:29:22.911Z
---

# [Issue 189]: [chore(graph): G5 review nits (tree --depth expands everything, heap-test cross-check, loud skip)](https://github.com/vig-os/stepv/issues/189)

These nits are from the fresh review of PR #188 (G5, #137) at cdba004. Under MVP-first they are deferred as a chore.

1. `src/graph/json.rs:260-264`: `--depth` only filters rows; the whole outline is still expanded. So `tree --depth 0` on an outline over the caps exits 3 even though only the roots were asked for. The depth should limit the expansion itself.
2. `tests/graph_cli.rs:259-360` heap test:
   - on the stress file it only asserts `faces > 0`; cross-check it against `faces_of`;
   - with `STEPV_CORPUS` unset, the test reports "ok" rather than ignored. Prefer `#[ignore]` with a reason, or the repo's skip convention.
3. The injection test injects only FILE_NAME's name field. Also inject author, org and description directly (the golden files show they're wrapped, but no test proves it).

Refs: #137
