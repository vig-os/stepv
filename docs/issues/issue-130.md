---
type: issue
state: open
created: 2026-11-08T20:53:01Z
updated: 2026-11-08T20:53:01Z
author: gerchowl
author_url: https://github.com/gerchowl
url: https://github.com/vig-os/stepv/issues/130
comments: 0
labels: chore
assignees: none
milestone: none
projects: none
parent: none
children: none
synced: 2026-11-09T09:29:46.473Z
---

# [Issue 130]: [stepv-cap: signal forwarding keeps one signal, and has no test](https://github.com/vig-os/stepv/issues/130)

Review finding on #126. `src/bin/stepv-cap.rs` forwards SIGINT, SIGTERM, SIGHUP and SIGQUIT to the command's group through a single `AtomicI32` slot (`FORWARD`). Two different signals arriving within one loop iteration (1 ms) keep only the last, so the first is never forwarded. No test sends a signal to the wrapper.

Fix:
- Keep a set of pending signals (a bitmask), and forward each one.
- Define the wrapper's exit after a forwarded signal (128 + n of the first one received), and document it.
- A test that sends SIGINT then SIGTERM to the wrapper and checks the group received both, plus one checking the exit code.

Refs: #83
