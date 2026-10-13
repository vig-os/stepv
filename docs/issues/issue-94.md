---
type: issue
state: open
created: 2026-10-13T00:33:59Z
updated: 2026-10-13T00:33:59Z
author: gerchowl
author_url: https://github.com/gerchowl
url: https://github.com/vig-os/stepv/issues/94
comments: 0
labels: bug, priority:low
assignees: none
milestone: none
projects: none
parent: none
children: none
synced: 2026-10-13T09:11:27.609Z
---

# [Issue 94]: [just view-test and the CI size check hardcode target/release, ignoring CARGO_TARGET_DIR](https://github.com/vig-os/stepv/issues/94)

Found in the review of #93: `justfile.project:213-214` (the last step of `just view-test`) and `kernel.yml:122` (the size check) read `target/release`. With `CARGO_TARGET_DIR` set, as agents and the local gates do, they run or measure a **stale** binary. CI is unaffected today: no workflow sets `CARGO_TARGET_DIR` on that path, and Linux uses `/build/target` explicitly. Resolve the target directory with `cargo metadata --format-version 1 | jq -r .target_directory`, or `${CARGO_TARGET_DIR:-target}`.

Refs: #85, #93
