---
type: issue
state: open
created: 2026-10-12T19:50:45Z
updated: 2026-10-12T19:50:45Z
author: gerchowl
author_url: https://github.com/gerchowl
url: https://github.com/vig-os/stepv/issues/75
comments: 0
labels: none
assignees: none
milestone: none
projects: none
parent: none
children: none
synced: 2026-10-13T09:11:35.421Z
---

# [Issue 75]: [cache/kernel cause: follow-ups from #69's review](https://github.com/vig-os/stepv/issues/75)

Nits from the fresh review of #69 (at 6e66f2d, merged as ebdf784). None blocks; each is a small follow-up.

- [ ] **A test that skips as root:** `tests/cache.rs` `an_input_the_host_will_not_open_is_the_environment` prints a skip message and passes when run as root, so a root CI container would never run it. Either fail as root, or create an input that root can't open either (for example a directory, or a FIFO with nothing on the other end), so the test always runs.
- [ ] **Cause serialisation:** `src/main.rs:514` builds the JSON value with `format!("{c:?}").to_lowercase()`, which ties it to the `Debug` output. Derive `Serialize` on `occt::Cause` with `#[serde(rename_all = "lowercase")]` instead.
- [ ] **The test hook in production:** document the `STEPV_OCCT_TEST_THROW` hook as a known limit in `stepv_occt.h`, alongside the balloon hook. It ships in the production kernel and anyone who can set the env var can trigger it. Alternatively, put it behind a compile-time flag.
- [ ] **Undocumented policy:** document in `stepv_occt.h` that every `Standard_Failure` counts as cause `input`, including failures thrown during meshing.

Refs: #62, #69
