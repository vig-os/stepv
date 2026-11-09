---
type: issue
state: open
created: 2026-11-08T20:52:59Z
updated: 2026-11-08T20:52:59Z
author: gerchowl
author_url: https://github.com/gerchowl
url: https://github.com/vig-os/stepv/issues/128
comments: 0
labels: chore
assignees: none
milestone: none
projects: none
parent: none
children: none
synced: 2026-11-09T09:29:47.335Z
---

# [Issue 128]: [stepv-cap: a reparented process, or one that leaves the group, escapes the memory cap](https://github.com/vig-os/stepv/issues/128)

Review finding on #126 (`stepv-cap`, 83a (i), at b574e07). `stepv::cap::tree_footprint` (`src/cap.rs`) and `src/bin/stepv-cap.rs` measure the child and its descendants **by parent chain**. A process whose parent exits is reparented to init/launchd and drops out of the measure.

Reproduced by the reviewer with a 64 MiB cap:
- `stepv-cap --memory-mb 64 -- sh -c "(python3 -c '<200 MiB balloon>' &); sleep 4"` exits 0 and kills nothing.
- `stepv-cap --memory-mb 64 -- sh -c "<balloon> & exit 0"` exits 0 in 10 ms with `peak_bytes: 0`, and the balloon keeps running, uncapped, **after the wrapper exits**.

Also: the post-breach kill sends SIGKILL to each recorded pid (`stepv-cap.rs`, the loop after `killpg`), which leaves a pid-reuse window.

Design:
- **Account by process group.** The command already runs in its own group, so measure every member: `proc_listpids(PROC_PGRP_ONLY, pgid)` on macOS, field 5 (pgrp) of `/proc/*/stat` on Linux.
- **Kill with `killpg`** only, which removes the per-pid reuse window.
- **Reap the group on exit:** when the direct child exits, SIGTERM the rest of the group, give it a bounded grace period, then SIGKILL. stepv-cap never exits while its group lives.
- **The limit, documented:** a process that leaves the group on purpose (`setsid`/`setpgid`) still escapes. The structural fix on Linux is a cgroup v2 per run (`memory.max`, `memory.peak`, `cgroup.kill`). macOS has no equivalent: the closest is a per-run `RLIMIT`-free footprint sampler over every process spawned (an EndpointSecurity/`kqueue NOTE_FORK`-based tracker), so the macOS cap stays best effort against deliberate escapes.
- **Tests:** both repros above, failing at b574e07.

Refs: #83
