---
type: issue
state: closed
created: 2026-10-04T14:47:11Z
updated: 2026-10-04T17:04:58Z
author: gerchowl
author_url: https://github.com/gerchowl
url: https://github.com/vig-os/stepv/issues/4
comments: 0
labels: feature
assignees: none
milestone: none
projects: none
parent: none
children: none
synced: 2026-10-05T08:45:54.422Z
---

# [Issue 4]: [Memory cap for the kernel subprocess](https://github.com/vig-os/stepv/issues/4)

## Description

The kernel subprocess has a wall-clock cap but **no memory cap**. S1 measured why it needs one
(plan.md §5 "S1 result", §4):

- ABC `00000046`, a 6 MB perforated plate: **7.2 GB** peak, all of it BRepMesh on two planar faces
  with ~1,250 holes each.
- A 745 MB synthetic assembly: **5.5 GB** peak.

Native 64-bit OCCT has no 4 GB arena, so without a cap the limit is the host's RAM. A Quick Look
extension or a `.thumbnailer` that takes the machine to swap is a worse bug than one that shows an
icon (plan.md §4).

## Proposed solution

Per platform, because `RLIMIT_AS` is not enforced on macOS:

- **Linux:** `setrlimit(RLIMIT_AS)` in the child before OCCT allocates. The kernel already catches
  `std::bad_alloc` and reports a clean exit-3 summary.
- **macOS:** the parent samples the child's RSS (`proc_pid_rusage`) in the `occt::run` wait loop it
  already has, and kills it over the cap. Report it as a new `Outcome::MemoryCap`, not as `Crashed`.

Add a `--memory-mb` CLI option with a conservative default, and a matching harness verdict.

## Acceptance

Both offenders above end in the cap outcome, not in swap, on both platforms. Verified in S3.

