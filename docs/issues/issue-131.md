---
type: issue
state: open
created: 2026-11-08T20:53:03Z
updated: 2026-11-08T20:53:03Z
author: gerchowl
author_url: https://github.com/gerchowl
url: https://github.com/vig-os/stepv/issues/131
comments: 0
labels: chore
assignees: none
milestone: none
projects: none
parent: none
children: none
synced: 2026-11-09T09:29:46.005Z
---

# [Issue 131]: [measure.rs keeps its own memory-sampling loop instead of stepv::cap::Sampler](https://github.com/vig-os/stepv/issues/131)

Review finding on #126. `src/measure.rs`'s `wait()` samples `footprint` on every 5 ms `recv_timeout` with its own loop. The CLI's `occt::supervise` uses the shared `stepv::cap::Sampler`, so only `footprint` is shared with `measure`.

Fix: have `measure::wait` use `Sampler`, after checking that its 5 ms throttle leaves measure's results unchanged (the first sample comes 5 ms after the query is sent, as today). Then the `stepv::cap` doc can say the measurement server shares the sampler.

Refs: #83
