---
type: issue
state: open
created: 2026-10-13T01:46:44Z
updated: 2026-10-13T01:46:44Z
author: gerchowl
author_url: https://github.com/gerchowl
url: https://github.com/vig-os/stepv/issues/100
comments: 0
labels: bug, priority:low
assignees: none
milestone: none
projects: none
parent: none
children: none
synced: 2026-10-13T09:11:25.988Z
---

# [Issue 100]: [status messages: name the limit and drop Rust Debug formatting (timeout, memory cap, crash)](https://github.com/vig-os/stepv/issues/100)

From #99's review: the failure messages the CLI and the viewer share (`fail_in` and the outcome mapping in src/main.rs) are honest but not specific:
- a timeout says 'wall-clock cap exceeded' without the limit (for example '5 s');
- a memory cap says 'memory cap exceeded' without the limit (for example '64 MiB');
- a crash says 'signal Some(11), exit None', which is Rust Debug formatting. It should read like 'killed by signal 11 (SIGSEGV)'.

Also, `failure()`'s fallbacks to `message` and to 'no reason given' (src/main.rs:545) are untested.

Refs: #63, #99
