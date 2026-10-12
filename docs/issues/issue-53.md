---
type: issue
state: open
created: 2026-10-11T15:23:33Z
updated: 2026-10-11T15:23:33Z
author: gerchowl
author_url: https://github.com/gerchowl
url: https://github.com/vig-os/stepv/issues/53
comments: 0
labels: chore, priority:low, needs-human
assignees: none
milestone: none
projects: none
parent: none
children: none
synced: 2026-10-12T08:55:38.529Z
---

# [Issue 53]: [Corpus: the full CAx-IF test rounds (members-only)](https://github.com/vig-os/stepv/issues/53)

## What's left of #12

The full **CAx-IF test rounds**. #52 filled everything else from public sources. The round models aren't public: CAx-IF (now MBx-IF, run with PROSTEP) shares them only in its implementor group's members area, by invitation, mostly for CAD vendors and large users. No public page links them (checked 2026-10-11).

Public coverage today:
- the as1/dm1/io1/s1/sg1 subset, via `occt-import-js`;
- MBx-IF's one open AP242 model, via `mbx-if-public`.

## What a human would do

1. Ask MBx-IF / PROSTEP (https://www.mbx-if.org) whether vig-os can get access to the test-round models.
2. If access is granted, read its terms. The manifest says "fetch, never commit, never mirror". A members-only file can't be fetched in CI, so it stays hand-collected in `tests/fixtures/cax-if/`.
3. Run `just fixtures && just harness` and add the `cax-if` row to plan.md §5 "Corpus fill (#12)".

## Acceptance

`cax-if` is reported present by `just fixtures`, and plan.md §5 is re-recorded with it. Or the request is refused, and that's recorded in the manifest and this issue closed as `wontfix`.

Refs: #12

