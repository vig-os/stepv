---
type: issue
state: open
created: 2026-11-09T02:29:29Z
updated: 2026-11-09T02:29:29Z
author: gerchowl
author_url: https://github.com/gerchowl
url: https://github.com/vig-os/stepv/issues/163
comments: 0
labels: chore, priority:low
assignees: none
milestone: none
projects: none
parent: none
children: none
synced: 2026-11-09T09:29:33.051Z
---

# [Issue 163]: [graph: recover from malformed records instead of refusing the file](https://github.com/vig-os/stepv/issues/163)

Today a malformed record refuses the whole file with a byte offset. Skip the record, record it in a diagnostics list, and continue where resynchronisation at `;` is safe. Acceptance: the malformed set yields partial graphs where OCCT also reads them.

Deferred from ADR-0001 §5 ("Later", hardening); not in the MVP milestone.

Refs: #115
