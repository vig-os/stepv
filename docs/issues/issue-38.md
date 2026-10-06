---
type: issue
state: closed
created: 2026-10-05T14:12:48Z
updated: 2026-10-05T19:26:01Z
author: gerchowl
author_url: https://github.com/gerchowl
url: https://github.com/vig-os/stepv/issues/38
comments: 0
labels: bug
assignees: none
milestone: none
projects: none
parent: none
children: none
synced: 2026-10-06T08:48:24.080Z
---

# [Issue 38]: [Kernel: a failed --topology fails the whole run, though the mesh is fine](https://github.com/vig-os/stepv/issues/38)

## Motivation
The kernel computes `--topology` after the mesh (`kernel/stepv-occt-core.cpp`, stage `topology`). When that stage fails, it returns `kExitFailed`, so the whole run fails even though the mesh it already wrote is good. Failures include an OCCT exception in one prototype's `prototype_json` (`kernel/topology.cpp`), or an open or write error.

Found in the review of #29 (PR #37).
- In `stepv view`, which now always asks for the topology, #37 works around it: when the failed run's stage is `topology`, it reruns without `--topology` and opens without the inspector.
- `stepv x.step --png o.png --topology t.json` still loses the PNG to a topology failure.

## Scope
- [x] A topology failure is non-fatal in the kernel. Record it in the summary (`topology_error`, say), delete the partial JSON, and exit OK if the mesh is fine.
- [x] The CLI reports it in the JSON line. When `--topology` was asked for alone, it still fails.
- [x] Drop `stepv view`'s retry, which would then cost a second kernel run for nothing.
- [x] A test: a hook that makes `prototype_json` throw, like `STEPV_OCCT_TEST_BALLOON_MB` does for memory.

## Pitfalls
- The topology stage also counts against `--timeout` and the memory cap. A file just under the limits without topology fails with it. Decide whether the topology gets its own budget.
- The output contract: front-ends parse the JSON line, so add fields rather than changing existing ones.

## Acceptance
- [x] A file whose topology throws still yields its PNG/mesh (exit 0) and a reported `topology_error`.
- [x] `stepv view` opens such a file with one kernel run.

