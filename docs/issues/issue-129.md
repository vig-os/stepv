---
type: issue
state: open
created: 2026-11-08T20:53:00Z
updated: 2026-11-08T20:53:00Z
author: gerchowl
author_url: https://github.com/gerchowl
url: https://github.com/vig-os/stepv/issues/129
comments: 0
labels: chore
assignees: none
milestone: none
projects: none
parent: none
children: none
synced: 2026-11-09T09:29:46.913Z
---

# [Issue 129]: [stepv::cap: Linux footprint is statm RSS x 4096; use smaps_rollup Pss_Anon + Pss_Shmem](https://github.com/vig-os/stepv/issues/129)

Review finding on #126. On Linux `stepv::cap::footprint` (`src/cap.rs`) reads `/proc/<pid>/statm` resident pages × a hard-coded 4096:
- RSS counts shared libraries and copy-on-write pages. Summed over several processes, it counts OCCT's libraries once per process, which would falsely kill a suite-wide cap (83a (ii)).
- 4096 under-counts on 16K and 64K page kernels.

Fix: the proportional set size from `/proc/<pid>/smaps_rollup`, `Pss_Anon` + `Pss_Shmem`. It is the closest match to macOS `ri_phys_footprint` and sums correctly across processes. If `smaps_rollup` is unavailable, fail closed with a clear error, with no fallback to statm. Any remaining page-based reading uses `sysconf(_SC_PAGESIZE)`. Document the choice and the residual difference from macOS in rustdoc. Mind the cost: `smaps_rollup` walks the page tables, so measure it at the 5 ms poll on a multi-GB kernel.

Refs: #83
