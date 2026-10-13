---
type: issue
state: closed
created: 2026-10-12T22:10:48Z
updated: 2026-10-12T22:24:00Z
author: gerchowl
author_url: https://github.com/gerchowl
url: https://github.com/vig-os/stepv/issues/86
comments: 1
labels: bug, priority:high
assignees: none
milestone: none
projects: none
parent: none
children: none
synced: 2026-10-13T09:11:31.224Z
---

# [Issue 86]: [viewer: a test-launched `stepv view` likely reached ~38 GB within ~90 s and wedged the host](https://github.com/vig-os/stepv/issues/86)

## What happened (2026-10-12, about 23:27–23:28 local, on sage)
A stepv process reached about **38 GB** and wedged sage, and the owner killed it. The most likely culprit, per the delegate that started it (stepv-63, working on #63):
- **What it was:** a `stepv view` test run against **main**, with `STEPV_OCCT_TEST_STALL_MS`, a real window (no `STEPV_VIEW_SCREENSHOT`), and no `--memory-mb` given. On main the viewer's load isn't streamed, so the stall hook never fired and the window stayed open.
- **When:** it started at about 23:26–23:27. It died of a signal the delegate didn't send at about 23:28 (the test saw exit -1 after 92.8 s), which matches the owner's kill.
- **What wasn't seen:** nobody saw its memory. It is not proven which process (viewer or kernel) held the memory.

## Suspects
- The viewer process itself. A code read found no unbounded path in `src/view/*`: the scene is uploaded once (`app.rs:218`), the CPU copy is dropped (`app.rs:1255`), and repaints are input-driven. But an idle or never-presented wgpu window on macOS (drawables or swapchain) hasn't been ruled out.
- The kernel `view` started: is it under the CLI's `--memory-mb` cap on the `view` path?

## Fix path
1. **Reproduce under a hard watchdog:** sample the RSS of the viewer and of the kernel every 100 ms, and SIGKILL both at 4 GB. Never run it unbounded.
2. **Find the allocation:** heap or `vmmap`/`footprint` on macOS. Fix the root cause.
3. **Guard tests:** every test that starts `view` sets `STEPV_VIEW_SCREENSHOT` and a memory cap, and fails rather than hangs (#83, #84).

Refs: #83, #63
---

# [Comment #1]() by [gerchowl]()

_Posted on October 12, 2026 at 10:23 PM_

**Not reproduced; the evidence points away from stepv.**
- **Repro:** the same `stepv view` command and env as window A, on main 00f13ba and on window A's exact binary, plus variants with `STEPV_VIEW_SCREENSHOT` and a streamed load. Each ran 120 s under a 4 GB watchdog. Memory peaks at about 900 MB for about 1.3 s at GPU start, then stays flat at about 70 MB (RSS 106 MB). CSVs and `vmmap` output are in the architect's scratch/86.
- **Jetsam record:** JetsamEvent-2026-10-12-232702 was written **2 min before** window A started (pid 15157, 23:29:03–23:30:20). It lists no stepv process among 1187. The largest were python3.13 (MLX, pid 5679) at 109 GB (lifetime max 140 GB), `com.sage.mlx-server-main` at 32 GB within 30 s of its spawn, ollama at about 45 GB, and VMs at about 58 GB. Window A's Metal shader compile was delayed by 49 s, so the host was already wedged when it started.
- **The kernel on `view`** is capped at 4 GiB (`Limits::DEFAULT`, src/main.rs:119; enforced at src/occt.rs:435).

The 38 GB 'step…' process the owner saw remains unidentified. Closing this as not reproduced. The guard (a `view` memory test that fails above 1 GB, or above 10% growth after warm-up, plus `STEPV_VIEW_SCREENSHOT` in every `view` test) moves to #83.

