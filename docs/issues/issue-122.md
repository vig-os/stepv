---
type: issue
state: open
created: 2026-11-08T18:17:59Z
updated: 2026-11-08T18:17:59Z
author: gerchowl
author_url: https://github.com/gerchowl
url: https://github.com/vig-os/stepv/issues/122
comments: 0
labels: none
assignees: none
milestone: none
projects: none
parent: none
children: none
synced: 2026-11-09T09:29:49.621Z
---

# [Issue 122]: [macOS preview: a preview closed before it is ready keeps its kernel until it becomes ready (≤ 20 s)](https://github.com/vig-os/stepv/issues/122)

Found in #107 (round 6), measured on sage, macOS 26.4.

Quick Look keeps the preview extension's view controller alive while its `preparePreviewOfFile` completion handler is outstanding. So a preview closed **before** it is ready, i.e. before the tree, the first mesh or a failure, is not released at the close. Its `deinit` cancel runs only once the handler is called.

Measured: the host closed at 0.3 s, the 4000-part file's `ready (tree)` came at 4.97 s, and the kernel was cancelled 32 ms after that, not at the close. The bound is `StepvKernel.previewHandlerAfter` (20 s): the ready timer calls the handler at the latest then. Until that point, the kernel of a preview nobody can see keeps running, within its 4096 MB cap.

ql-e2e pins this behaviour ("closed before ready": the cancel follows the file's ready line). The ready-preview close cases signal the host once the file is ready.

Design question for the owner: is ≤ 20 s acceptable, or should `handler(nil)` come sooner? For example, as soon as the header is on screen. That would change #64's ready rule (ready once header and tree are up), which is why it is not changed here.

Refs #64
