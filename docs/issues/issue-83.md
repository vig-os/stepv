---
type: issue
state: open
created: 2026-10-12T21:34:42Z
updated: 2026-10-13T00:41:30Z
author: gerchowl
author_url: https://github.com/gerchowl
url: https://github.com/vig-os/stepv/issues/83
comments: 4
labels: feature, priority:high
assignees: none
milestone: none
projects: none
parent: none
children: none
synced: 2026-10-13T09:11:32.337Z
---

# [Issue 83]: [ci: profiling gates — peak memory and time budgets for the kernel, tests and the corpus](https://github.com/vig-os/stepv/issues/83)

## Why
On 2026-10-12 a stepv process reached **38 GB** of memory and wedged sage (32 cores), and the owner had to kill it by hand. The culprit isn't identified yet; it is being traced. Nothing in CI or in the local gate would have caught it. Memory and time regressions are found by a human watching Activity Monitor.

The CLI already caps the kernel (`--memory-mb`, default 4096; `src/measure.rs`, `src/occt.rs` `Outcome::MemoryCap`). But that cap only applies when the kernel runs through the CLI. These run `stepv-occt` or OCCT with **no cap**:
- `just harness`;
- `corpus-diff.py`;
- `occt-patch-check` (#81);
- direct kernel invocations in tests;
- the test binaries themselves.

## Proposal
1. **Memory cap everywhere.**
   - Every way the kernel runs (harness, scripts, tests, the nix checks) runs under a cap, which kills it and reports the hit.
   - The test runner gets a per-test cap. A test that exceeds it fails, naming the test, instead of wedging the host. Use nextest per-test settings, or a wrapper that samples RSS on macOS and sets `RLIMIT_AS`/cgroups on Linux.
2. **Profiling gates in CI.**
   - **Kernel harness:** record peak RSS and wall time per file, against a checked-in budget file with headroom. Fail on going over budget, and on a regression past an agreed margin. Ultimaker 3 is the heavy reference: read 4 s, transfer about 29–34 s, mesh 3.5 s; peak RSS still to be measured.
   - **Tests:** peak RSS per test binary, gated.
3. **A report on each PR:** a short table (worst files by time and RSS, deltas against main), so a regression is visible in review.
4. **Devkit:** the generic part (a per-test resource cap, the peak-RSS and time harness, the budget-file format, the PR table) comes from the org scaffold, so every repo gets it. Filed on vig-os/devkit. stepv keeps only its budget file and its kernel-specific runner.

## Open questions (decide before implementation)
- The cap mechanism on macOS, which has no enforced `RLIMIT_AS`: sample-and-kill, or another mechanism? stepv's CLI already caps the kernel; reuse it.
- Budget margins: absolute numbers, or a ratio to main measured on the same runner? CI runners are noisy.
- Where the heavy files run: Ultimaker is 143 MB and not in the repo. A nightly job, or a fetched fixture?

## Acceptance
- A test or harness run that goes over its memory budget fails, naming the culprit, on both macOS and Linux CI. Prove it with a deliberate balloon, via the existing memory-cap test hook.
- The CI kernel gate fails on an RSS or time regression past budget, and the PR shows the table.
- No uncapped kernel invocation is left in scripts, the justfile or tests (checked in CI).

Refs: #69 (memory cap and cause), #81

---

# [Comment #1]() by [gerchowl]()

_Posted on October 12, 2026 at 09:35 PM_

Devkit side: vig-os/devkit#1875.

---

# [Comment #2]() by [gerchowl]()

_Posted on October 12, 2026 at 10:09 PM_

Uncapped kernel launches found by an audit (stepv-74, 2026-10-13). The CLI's `--memory-mb` (src/occt.rs:186-213) covers only kernels that `stepv` itself starts. These have no cap:
- `corpus-diff.py:12` (only a 300 s timeout);
- `examples/harness.rs:131` (`memory: None` unless `--memory-gb` is passed);
- tests that spawn `stepv-occt` directly: `tests/heal.rs:53`, `tests/stream.rs:266`, `tests/outline.rs:131`, `tests/cli.rs:671`, `tests/sandbox.rs:138,335`.

The parent-side readers are bounded (`MAX_FRAME`, src/occt.rs:715; count checks at :889/:902).

A second lead, from stepv-63: before #84, a `view` test on main opened a real viewer window, because that load wasn't streamed and the stall hook never fired. The viewer itself (`stepv view`, which tessellates) is a candidate for the 38 GB process. It is being confirmed.

---

# [Comment #3]() by [gerchowl]()

_Posted on October 13, 2026 at 12:23 AM_

Data point (2026-10-13): during #91, a stepv delegate's own memory guard stopped `corpus-diff.py` at **9.2 GB** with its default parallelism; it re-ran with 2 workers. The corpus run is a real heavy consumer and needs the per-run cap and a parallelism budget from this issue.

---

# [Comment #4]() by [gerchowl]()

_Posted on October 13, 2026 at 12:41 AM_

Budget data from #91's corpus run (2026-10-13): corpus-diff with stock parallelism went past **8 GB twice**. Run one kernel at a time under a 4 GB per-kernel cap, two ABC files (`00000046…`, `00000048…`) hit the cap on **both** main's kernel and the branch's. They're the corpus's memory outliers and the first entries for the budget file.

