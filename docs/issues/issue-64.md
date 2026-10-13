---
type: issue
state: open
created: 2026-10-12T14:56:44Z
updated: 2026-10-13T08:14:16Z
author: gerchowl
author_url: https://github.com/gerchowl
url: https://github.com/vig-os/stepv/issues/64
comments: 4
labels: feature, priority:medium
assignees: none
milestone: none
projects: none
parent: none
children: none
synced: 2026-10-13T09:11:39.930Z
---

# [Issue 64]: [macOS: the kernel as a child of an XPC supervisor service per Quick Look extension (#54)](https://github.com/vig-os/stepv/issues/64)

## Motivation
#54, the macOS part. In-process cancel is correct, but it leaves the extension about 0.9 GB fatter after each cancelled heavy load, and jetsam kills extensions under memory pressure. The kernel must run in a process that can be killed, and that frees its memory at once.

## Decision (owner, 2026-10-13; GOAL.v4)
Two designs fell, one after the other:
- **XPC with the kernel inside, killed with `invalidate()`:** `invalidate()` doesn't kill a busy service, and launchd throttles restarts by about 10 s (pre-mortem, comment 6131624733).
- **The appex spawns the kernel directly:** QL appexes can't `posix_spawn` (EPERM, 22 of 22, #79 round 1).

**The decided design: an XPC supervisor plus a child.** It is proven by the automated Quick Look harness in #79 round 2 (`ql-e2e`, ALL PASS, re-run independently by the architect):
- Each appex embeds a small, long-lived XPC service (`Contents/XPCServices/`).
- The appex hands the service the previewed file as a **FileHandle (fd)**. A URL carries no permission, and the service can't open the file by path.
- The service `posix_spawn`s the kernel (`stepv-occt --stream`) with the fd inherited, then streams TREE, PROGRESS and DONE back to the appex.
- **Cancel and deadline:** the service SIGKILLs its child and reaps it in about 30 ms. The next request takes about 8 ms, with no throttle. Concurrent requests are independent. The extension runs its own deadline, because QL gives no cancel callback.
- `handler(nil)` once the header and tree are on screen. With no tree (IGES, BREP, an outline that failed or was too big), on the first mesh or on failure.
- **Libraries:** the OCCT dylib closure is bundled in `Contents/Frameworks` with `@rpath`.
- **Signing:** **Developer ID, one team**; hardened runtime and library validation stay on (owner, GOAL.v3).

## Order
1. **: the kernel reads from an inherited fd, with `--format`.**
2. #64 itself: the supervisor service in both appexes; bundling and signing; swapping in both extensions; the `ql-e2e` harness moved into the repo (`scripts/`), run in CI's macOS GUI session if possible, else as a documented local gate.

## Known limits
- Multi-file assemblies: their sibling part files can't be reached (https://github.com/vig-os/stepv/issues/92).
- Containment: the kernel's own Seatbelt can't nest inside the app sandbox (`macos-outer`).

## Acceptance
- `ql-e2e` covers preview and thumbnail: load, a deadline kill with no survivor, a next request with no throttle, and concurrent requests.
- The CI macOS build produces both appexes, each with its supervisor and the Frameworks closure. `codesign --verify --deep --strict` passes.
- Nothing in the appex links the kernel in-process.

Refs: #54, #79
---

# [Comment #1]() by [gerchowl]()

_Posted on October 12, 2026 at 07:58 PM_

## Pre-mortem (2026-10-12): the decided XPC mechanism doesn't hold

A code-falsifier built a sandboxed client app with a sandboxed `.xpc` service in `Contents/XPCServices` (CLT swiftc, ad-hoc signing). The service ran a C stand-in that holds 900 MB in a loop that never checks for cancel, behind a mutex, like the kernel.

**Confirmed:**
1. **`invalidate()` doesn't kill a busy service.** The service stayed alive at 929 MB. The next connection reached the same pid and waited 9.66 s for the old load to finish. A sandboxed client also can't `kill()` the service (EPERM). Only a message that makes the service `exit()` gives a fresh process.
2. **launchd throttles restarts by about 10 s.** After a service exits, the next load took 9.2 s when the old one had run 1 s, 5.2 s at 5 s, and 0.18 s at 12 s or more. A folder of thumbnails after a cancel therefore stalls. One instance also serves every request from that extension, so killing it kills all concurrent loads.
3. **Handing over a FileHandle breaks the kernel.** On macOS `realpath("/dev/fd/3")` gives `/dev/fd/<name>`, so the real libstepvocct returns `code=3 STEP read failed`. The format is also picked from the extension. Using an fd needs a C-API change.
4. **A plain file URL carries no permission across XPC:** `startAccessingSecurityScopedResource` returns false, and the read fails with EPERM.
5. **Hardened runtime with ad-hoc signing won't load the library** ("different Team IDs"). CI never exercises the Developer ID path.
6. **Nothing in the Quick Look SDK signals a cancelled preview** (`QLPreviewingController.h` and `QLThumbnailProvider.h`).
7. **The `handler(nil)` rule has no answer when there's no tree** (IGES, BREP, or an outline that failed or was too big).

**Plausible:**
- Thumbnails have a watchdog of about 20 s, against Ultimaker 3's 42 s.
- A long-lived service just moves the 0.9 GB plateau elsewhere.
- The mesh comes back as one large XPC message.

**What happens now:** #64 isn't dispatched. Its design goes back to a decision. Next is a probe of the alternative: the appex spawns the bundled sandboxed `stepv-occt` CLI as a child process, using the existing streaming protocol, so `kill()` on its own child works with no launchd throttle. That probe is followed by an owner decision.

---

# [Comment #2]() by [gerchowl]()

_Posted on October 12, 2026 at 08:04 PM_

## Probe: the appex spawns `stepv-occt` as a child process (2026-10-12)

Tested on sage (macOS 26.4) with a sandboxed client app, ad-hoc signed. It couldn't run inside a real appex, because qlmanage hangs when driven from an agent.

**Confirmed:**
- **Spawn:** a bundled helper (`Contents/Helpers` or `Contents/MacOS`) signed with `app-sandbox` plus `inherit` spawns, sandboxed by inheritance. Without `inherit` it traps with signal 5.
- **Kill:**
  - `kill(child, SIGKILL)` works, reaped in about 18 ms, and 0.9 GB is back at once;
  - the next spawn is ready in about 60 ms, with no launchd throttle;
  - on the real kernel, killing it at the TREE frame worked 3 times in 3.
- **File access:** the child inherits the consumed sandbox extension, so it reads the granted path by path and `realpath` works. Without a grant, both the client and the child get EPERM.
- **Libraries:** `stepv-occt` links 25 OCCT dylibs from `/nix/store`, which the sandbox blocks. Bundled into `Contents/Frameworks` (31 dylibs, 51 MB, `@rpath`, signed), they load. Hardened runtime with ad-hoc signing fails with "different Team IDs"; it needs one team's Developer ID, or `disable-library-validation`.
- **Streaming:** the real kernel sends TREE, PROGRESS and DONE under the inherited sandbox. Its own Seatbelt profile can't nest (`sandbox_init` returns EPERM), so it reports `macos-outer`.

**Gaps:**
- Spawning from a **real QL appex**, not yet verified;
- the sibling files of multi-file assemblies, which the one-file grant doesn't cover;
- weaker containment than the kernel's own Seatbelt profile;
- the thumbnail watchdog, for which the extension needs its own deadline.

Recommendation: the subprocess design, but only after a real appex spawns the helper, which a human checks in Finder.

---

# [Comment #3]() by [gerchowl]()

_Posted on October 12, 2026 at 11:58 PM_

#79's result: a real QL appex **can't** spawn a bundled helper (EPERM, 22 of 22, preview and thumbnail), so the subprocess design is falsified. **The owner decided (2026-10-13) on an XPC supervisor plus a child:** a long-lived XPC service per appex spawns and kills the kernel child. It is gated on a probe in #79 (a spawn from an XPC service inside an appex, and fd passing). #64's body will be rewritten once the probe passes.

---

# [Comment #4]() by [gerchowl]()

_Posted on October 13, 2026 at 08:14 AM_

Reopening: the merge of #98 (PR 1, bca808e) closed this issue automatically, but #64 isn't complete until #107 (PR 2: the extensions move onto the supervisor and the in-process kernel is removed) merges. #107 closes it.

