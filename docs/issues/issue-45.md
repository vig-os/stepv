---
type: issue
state: open
created: 2026-10-05T18:57:57Z
updated: 2026-10-05T18:57:57Z
author: gerchowl
author_url: https://github.com/gerchowl
url: https://github.com/vig-os/stepv/issues/45
comments: 0
labels: bug
assignees: none
milestone: none
projects: none
parent: none
children: none
synced: 2026-10-06T08:48:23.348Z
---

# [Issue 45]: [Linux viewer: `cargo test --lib view::` segfaulted once on lavapipe](https://github.com/vig-os/stepv/issues/45)

## Motivation
`just linux-view-test` (Debian trixie, Mesa lavapipe, `CARGO_BUILD_JOBS=2`) crashed once in the headless GPU unit tests, while working on #38 (which doesn't touch the viewer):

```
running 80 tests
.................error: test failed, to rerun pass `--lib`
  process didn't exit successfully: `/build/target/release/deps/stepv-… 'view::' --quiet` (signal: 11, SIGSEGV: invalid memory reference)
```

The immediate rerun passed every step. The macOS host was busy with `just macos-test` at the same time. A segfault only gets more likely with load, never less, so CI's `Viewer (Linux, lavapipe)` job can hit it.

## Suspects
- Several `Headless` wgpu devices created in parallel on lavapipe (the tests run multi-threaded). Mesa's lavapipe has had races in device creation and teardown.
- A device dropped while a mapped buffer or a `poll` is still in flight.

## Scope
- [ ] Reproduce: loop `cargo test --release --lib view::` in the container under load (e.g. 50 runs), and catch a core dump (`ulimit -c unlimited`, gdb `bt`).
- [ ] If it's the parallel device creation: share one device across tests (a `OnceLock<Headless>`), or run the GPU tests with `--test-threads=1` on lavapipe, and say why.

## Pitfalls
- `--test-threads=1` hides the race instead of fixing it. A real viewer has only one device, so serialising the tests may well be the honest fix, but write down the evidence first.

## Acceptance
- [ ] 50 consecutive container runs of `view::` without a crash, or a root cause with a fix.

