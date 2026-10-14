---
type: issue
state: open
created: 2026-10-13T09:37:20Z
updated: 2026-10-13T12:54:05Z
author: gerchowl
author_url: https://github.com/gerchowl
url: https://github.com/vig-os/stepv/issues/109
comments: 2
labels: chore, priority:medium
assignees: none
milestone: none
projects: none
parent: none
children: none
synced: 2026-10-14T07:21:26.740Z
---

# [Issue 109]: [macOS supervisor admission: follow-ups from #108's review (timing-bound handover test, silent override parse, untested cancel race, docs)](https://github.com/vig-os/stepv/issues/109)

Follow-ups from the final review of #108 (merged as 6b9ba41, the supervisor's admission limit; part of #64). None of these is a product bug.

## Scope
- [ ] **The handover test is timing-bound** (`scripts/ql-e2e/supervisor.sh:264`, probe `handover`). It passes only if B is submitted between about 10 ms and about 3 s after A, because of a fixed 3 s hold. Replace the hold with an event: the hook waits until B's queued-deadline timer has fired, so the test is deterministic. Outside that window it fails loudly today, not silently.
- [ ] **Malformed test-override files are silently ignored** (`macos/Supervisor/main.swift:72-76`). If `admission-limit` doesn't parse (for example `1x`), the service falls back to the machine's L. An unparseable or negative `admission-handover-delay-ms` means no hold. Both should be startup errors (exit 78), like the out-of-range case.
- [ ] **The cancel-race fixes have no tests.** Moving the `withdrawQueued` install back after `submit`, or dropping the `wasCancelled` re-check, still passes every test. Add a test-build hook that cancels in the window between queue entry and the re-check.
- [ ] **Stale docs:**
  - `macos/Shared/StepvSupervisor.swift:47` still says `memoryMB` is "1 to 2^20", but it is now at most 4096 (`Admission.slotMemoryMB`).
  - `plan.md:1622` still says the test "checks two things".
  - The CHANGELOG doesn't mention that requests above 4096 MB are now refused.
- [ ] **Two unlinked 4096 constants:** the extensions' `StepvKernel.memoryMB` and the supervisor's `Admission.slotMemoryMB` aren't connected at compile time. Share one constant, or add a test that asserts they are equal.

## Acceptance
Each item is fixed. Each new test is shown to fail without its change.

Refs #64, #108

---

# [Comment #1]() by [gerchowl]()

_Posted on October 13, 2026 at 12:24 PM_

#113 (c71e629) resolves the first item here. The handover hold in the test build is no longer a timed delay: it holds each admitted run until the test creates `release-N` in the service's tmp, and logs when the run's deadline passes while held. The probe releases the run only after that line, so the admission-time check is reached past the deadline whatever the timing. If the hold releases on its own timer instead, the case fails (exit 1).

---

# [Comment #2]() by [gerchowl]()

_Posted on October 13, 2026 at 12:54 PM_

Follow-ups from the round-3 review of #113 (c71e629), not fixed there:
- **`accepted(_:)` in product runs.** The service sends the client `accepted(run)` in product builds as well, not only in tests: one extra XPC message per run, which the extensions ignore.
- **The B2 mutant is conditional.** The 'hold released by its own timer' mutant fails only because its timer is shorter than the run's 2 s deadline.
- **The 'handover' name.** The case no longer exercises a handover between two runs: it holds a single run between its admission and its start. It should be renamed.
- **A hardcoded `run #1`.** The live-sweep case (4b) awaits `run #1` of the live probe's service. That holds because each probe process starts a fresh instance, but it should come from the run's own `accepted` number.
- **`slowBeside` only at L ≥ 2.** The deadline kill beside a running run is tested only where two kernels can run at once; at L = 1 (CI) only the kill on its own is covered.

