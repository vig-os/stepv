---
type: issue
state: open
created: 2026-11-08T20:36:51Z
updated: 2026-11-08T20:36:51Z
author: gerchowl
author_url: https://github.com/gerchowl
url: https://github.com/vig-os/stepv/issues/127
comments: 0
labels: none
assignees: none
milestone: none
projects: none
parent: none
children: none
synced: 2026-11-09T09:29:47.797Z
---

# [Issue 127]: [ql-e2e: a preview's kernel is sometimes cancelled only after the host quits (closed/switched preview)](https://github.com/vig-os/stepv/issues/127)

`scripts/ql-e2e/run-all.sh` fails some macOS runs, in different scenarios, with the same symptom: the preview's kernel is cancelled only after `ql-preview-host` quits, not when the host closes or switches the preview.

- **PR #126** (run 42518342591, Kernel macos-15, step "Quick Look end to end (ql-e2e)"), **switched** preview:
  `FAIL: switched preview: 60774 cancelled only after the host quit, reaped 40.2 ms after the cancel`, then `FAIL: switched preview: box.brep was not shown` (`no log line for …/switched-to-box.brep: the extension did not run, or not this build`).
- **#64 PR 2's branch** (run 42506311007), **closed** preview:
  `FAIL: closed preview: 46642 cancelled only after the host quit, reaped 55.9 ms after the cancel`, with `0 completion(s) []`.

#126 changes only Rust (`src/cap.rs`, `src/bin/stepv-cap.rs`, `src/occt.rs`'s sampler). The extensions' path is the Swift supervisor plus `stepv-capi`, which uses only `occt::read_mesh`, `header` and `render`, so #126 doesn't reach it. The same failure class on #64's own branch shows the nondeterminism predates #126.

To find: why the extension's cancel on close or switch sometimes doesn't arrive (or isn't logged) before the host exits, and whether the switched-to file's request is then lost. The fix belongs in the extension/supervisor or the e2e's ordering, not in a wider wait.

Refs: #64
