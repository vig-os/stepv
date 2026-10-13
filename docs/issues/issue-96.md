---
type: issue
state: open
created: 2026-10-13T00:42:59Z
updated: 2026-10-13T00:42:59Z
author: gerchowl
author_url: https://github.com/gerchowl
url: https://github.com/vig-os/stepv/issues/96
comments: 0
labels: bug, priority:low
assignees: none
milestone: none
projects: none
parent: none
children: none
synced: 2026-10-13T09:11:27.088Z
---

# [Issue 96]: [kernel fd input: follow-ups from #95's review](https://github.com/vig-os/stepv/issues/96)

Nits from the review of #95 (#91) at f505fda:
- `src/occt.rs:550`: the rustdoc says the caller finds the offset moved 'where the offset is shared (macOS)'. The inherited descriptor shares the offset on Linux too, so say 'always'.
- `kernel/stepv-occt-core.cpp:1199`: a seekable descriptor that isn't a regular file (a directory, `/dev/zero`) passes the check and then fails as `input`. An `fstat` + `S_ISREG` check makes it `environment`.
- **A test for #92's limit:** a multi-file assembly by fd counts its parts in `external_missing`.
- `stepv-occt-core.cpp:1305` is over the line length; C++ isn't formatted by any tool. Consider clang-format in the hooks.

Refs: #91, #95, #92
