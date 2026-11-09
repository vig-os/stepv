---
type: issue
state: closed
created: 2026-11-08T16:05:35Z
updated: 2026-11-08T17:17:52Z
author: gerchowl
author_url: https://github.com/gerchowl
url: https://github.com/vig-os/stepv/issues/117
comments: 0
labels: bug
assignees: none
milestone: none
projects: none
parent: none
children: none
synced: 2026-11-09T09:29:51.361Z
---

# [Issue 117]: [Window tests on Linux: ETXTBSY race, script() writes an executable while other test threads fork](https://github.com/vig-os/stepv/issues/117)

Found while root-causing #104 (Linux viewer job). It's separate from #104's X-server reset and has a different root cause.

**What happened:** looping CI's window-test step (`scripts/test-window-tests.sh -q --release` under `xvfb-run … -noreset`, in CI's Debian trixie container on Linux x86_64) failed in 2 of 120 runs:

```
closing_the_viewer_mid_load_cancels_the_kernel --- FAILED
panicked at tests/cache.rs:992:5:
left: (3, Some("error"))   right: (0, Some("cancelled"))
{"backend":"vulkan","error":"cannot start kernel: Text file busy (os error 26)", …}
```

The other failure was in `a_viewer_with_no_cache_leaves_no_artifact` (tests/cache.rs:862, an exit-code/status mismatch; that run's output wasn't kept). It also execs a `script()` kernel.

**Root cause:** `script()` (tests/cache.rs:219) writes an executable with `std::fs::write`. The tests run in parallel, and other threads `fork` (`Command::spawn`) meanwhile. A child forked while the write descriptor is open holds it until that child execs (the descriptor is O_CLOEXEC, but fork→exec is a window). When stepv then execs the script, Linux returns ETXTBSY. This is the known Rust issue rust-lang/rust#114554. macOS has no ETXTBSY, so `just view-test` there never sees it.

**Fix direction (not a retry):** close the window, for example with a process-wide `RwLock`. `script()` writes under the write lock, and every test-side spawn holds the read lock. Or write each script in a child process, so the test process never holds a writable descriptor to an executable.

Refs: #104

