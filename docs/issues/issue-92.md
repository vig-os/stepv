---
type: issue
state: open
created: 2026-10-13T00:09:09Z
updated: 2026-10-13T00:09:09Z
author: gerchowl
author_url: https://github.com/gerchowl
url: https://github.com/vig-os/stepv/issues/92
comments: 0
labels: feature, priority:low
assignees: none
milestone: none
projects: none
parent: none
children: none
synced: 2026-10-13T09:11:28.192Z
---

# [Issue 92]: [macOS QL: multi-file assemblies can't reach their part files from the fd-only kernel](https://github.com/vig-os/stepv/issues/92)

Known limit of #64's design (XPC supervisor plus a child kernel reading from an inherited fd, #91). Quick Look grants the extension access to the previewed file only. The supervisor service can't open sibling part files by path (EPERM, #79), so an assembly whose parts live in separate files previews without them. The run reports them as missing, as it does today for missing files.

Options to evaluate later: the extension resolves and opens the siblings itself, if its sandbox extension allows reading the directory, and passes several fds; or the host app gets user-granted folder access. Until then it is documented in plan.md and in the preview's message.

Refs: #64, #91, #79
