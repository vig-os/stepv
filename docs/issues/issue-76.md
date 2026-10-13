---
type: issue
state: open
created: 2026-10-12T19:52:45Z
updated: 2026-10-12T20:44:13Z
author: gerchowl
author_url: https://github.com/gerchowl
url: https://github.com/vig-os/stepv/issues/76
comments: 0
labels: none
assignees: none
milestone: none
projects: none
parent: none
children: none
synced: 2026-10-13T09:11:34.883Z
---

# [Issue 76]: [artifact: the outline's cost to a streamed run, no artifact for IGES/BREP or on a cache hit (#62)](https://github.com/vig-os/stepv/issues/76)

## Limits of the outline artifact (#62, B's PR)

- [ ] **A streamed run pays for the outline inside its timeout.** Cached CLI runs are now streamed (`occt::run_streaming`) so they can write the artifact on `Event::Tree`. The outline costs the run its parse again, about 5% of a large assembly's run (#54 step 2), so a run near its `--timeout` can time out streamed where it would not run plain. Options: reuse the outline's parse for the transfer, or leave the outline's time out of the cap.
- [ ] **IGES and BREP files leave no artifact.** The kernel streams an outline only for STEP, so a killed IGES/BREP run leaves nothing a preview can show. Decide whether those formats get an outline (header and a flat tree) or are documented as out of scope.
- [ ] **A cache hit writes no artifact.** A run whose output is already cached returns before `tessellate`, so it never streams and never writes the outline artifact. A file cached before #78 gets no outline until its entry misses (a new deflection, size or output, or the file changing). Options: on a hit with no artifact, run `occt::outline` in the background, or write the artifact on the first miss of any output for the file.

Not here: `view` writing the artifact is #63's; per-part boxes are #66.

Refs: #62



