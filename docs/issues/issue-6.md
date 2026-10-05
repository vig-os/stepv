---
type: issue
state: closed
created: 2026-10-04T14:47:14Z
updated: 2026-10-04T17:04:58Z
author: gerchowl
author_url: https://github.com/gerchowl
url: https://github.com/vig-os/stepv/issues/6
comments: 0
labels: feature
assignees: none
milestone: none
projects: none
parent: none
children: none
synced: 2026-10-05T08:45:53.688Z
---

# [Issue 6]: [S3: limits and failure modes through the CLI](https://github.com/vig-os/stepv/issues/6)

## Description

Step **S3** of [`plan.md`](../blob/main/plan.md) §5: prove the limits work end to end through the
CLI, on the files S1 found that need them.

## Scope

- **Memory cap fires** (#4) on the two known offenders: ABC `00000046` (perforated
  plate, 7.2 GB peak in BRepMesh) and the 745 MB stress file (5.5 GB; `kernel/stress-gen` with 200
  copies). Native 64-bit has no 4 GB arena, so the cliff is the host's RAM (S1).
- **Timeout fires** at the default 20 s and the CLI exits 4. The 221 MB stress fixture takes
  20.2 s, so it is the natural probe.
- **Exit 3 still prints valid metadata**, for every failure stage the kernel reports (`read`,
  `transfer`, `walk`, `mesh`, `extract`).
- **Corrupt or truncated files fail cleanly**: the `malformed` fixture set already passes in the
  harness; repeat it through the CLI.
- **Decide:** when a thumbnail times out or hits the cap, retry at `Deflection::THUMBNAIL` or a
  coarser angle, or show box + header? S1 measured that angular deflection, not linear, drives the
  perforated-plate cost (14.8 s at 20°, 7.7 s at 30°). Record the decision in plan.md.

## Acceptance

None of the above can hang or OOM the host; every outcome maps to a documented exit code.

## Depends on

S2 (#5) and the memory cap (#4).

