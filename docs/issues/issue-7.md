---
type: issue
state: closed
created: 2026-10-04T14:47:15Z
updated: 2026-10-04T17:04:59Z
author: gerchowl
author_url: https://github.com/gerchowl
url: https://github.com/vig-os/stepv/issues/7
comments: 0
labels: feature
assignees: none
milestone: none
projects: none
parent: none
children: none
synced: 2026-10-05T08:45:53.364Z
---

# [Issue 7]: [S4: Linux front-ends (.thumbnailer, MIME, KF6 ThumbnailCreator)](https://github.com/vig-os/stepv/issues/7)

## Description

Step **S4** of [`plan.md`](../blob/main/plan.md) §5: the Linux front-ends. They're the cheapest
win: once the `.thumbnailer` exists, the project is useful.

## Scope, in order

1. A **`.thumbnailer`** file for GNOME/XFCE pointing at `stepv --png`.
2. **MIME registration**: `model/step` (`shared-mime-info` carries it for `.step`/`.stp`); add
   IGES and BREP, which the kernel already reads.
3. A **KF6 `ThumbnailCreator`** for Dolphin that shells out to the same binary.
4. The standalone viewer binary with its MIME association (plan.md §1).

## Depends on

S2 (#5, `--png`) and S3 (#6, limits): a thumbnailer is invoked unattended on every file in a folder.

