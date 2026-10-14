---
type: issue
state: open
created: 2026-10-13T10:12:35Z
updated: 2026-10-13T10:12:35Z
author: gerchowl
author_url: https://github.com/gerchowl
url: https://github.com/vig-os/stepv/issues/111
comments: 0
labels: bug, priority:blocking
assignees: none
milestone: none
projects: none
parent: none
children: none
synced: 2026-10-14T07:21:26.182Z
---

# [Issue 111]: [main red: macos-15 supervisor check 'the slow run's kernel is running' fails after #108 merged](https://github.com/vig-os/stepv/issues/111)

Main is red after #108 merged (6b9ba41). In the push run [38377606897](https://github.com/vig-os/stepv/actions/runs/38377606897), Kernel (macos-15), the step `just macos-test` (supervisor checks) fails:
```
FAIL: the slow run's kernel is running
supervisor: FAILURES
FAIL: the supervisor service
error: recipe `macos-test` failed on line 225 with exit code 1
```
The same checks passed on #108's own PR head, a6f2ad4. #81's CI at cf90325, rebased onto 6b9ba41, fails the same way (reported by its author). The cause isn't diagnosed yet. It is either a timing-dependent assertion in `scripts/ql-e2e/supervisor.sh` (#109 already lists the handover test as timing-bound), or something specific to running on main.

The same main run also hit the Linux viewer X-server flake (#104), a separate issue.

## Acceptance
- [ ] Root cause identified, with evidence (logs and a local repro if possible).
- [ ] The fix is structural: the assertion waits on an event, not on time. No retry, no widened timeout.
- [ ] The macOS CI job is green on main.

Refs #64, #108, #109

