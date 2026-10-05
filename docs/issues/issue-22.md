---
type: issue
state: closed
created: 2026-10-04T19:33:04Z
updated: 2026-10-04T20:46:00Z
author: gerchowl
author_url: https://github.com/gerchowl
url: https://github.com/vig-os/stepv/issues/22
comments: 0
labels: bug, priority:medium
assignees: none
milestone: none
projects: none
parent: none
children: none
synced: 2026-10-05T08:45:48.509Z
---

# [Issue 22]: [harness --strict passes a total pass-rate collapse](https://github.com/vig-os/stepv/issues/22)

## Description

`harness --strict` (the CI robustness gate) only fails on crashes, timeouts, bad meshes, memory-cap kills, and malformed files that don't fail cleanly. If **every** file fails cleanly, it exits 0.

Found while building #18: an early sandbox stopped the kernel resolving relative input paths, and every corpus file came back `clean-fail` ("cannot resolve input path: Operation not permitted"). The table showed **0.0%**, and `harness --strict` still exited **0**. On CI's subset (nist-pmi, malformed, tests/data), that regression would have gone green.

## Fix

Add a pass-rate floor, `--min-pass <percent>`, computed over the non-malformed files (the table's "all but malformed" row), and use it in the Kernel workflow's robustness gate.

## Acceptance

- `harness --strict --min-pass N` exits non-zero when the pass rate is below N.
- CI's gate carries a floor that the current corpus clears and a collapse does not.
