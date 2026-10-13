---
type: issue
state: open
created: 2026-10-12T20:42:48Z
updated: 2026-10-13T00:07:11Z
author: gerchowl
author_url: https://github.com/gerchowl
url: https://github.com/vig-os/stepv/issues/79
comments: 3
labels: feature
assignees: none
milestone: none
projects: none
parent: none
children: none
synced: 2026-10-13T09:11:33.845Z
---

# [Issue 79]: [spike(macos): a real Quick Look appex spawns a bundled sandboxed helper (#64 step 1)](https://github.com/vig-os/stepv/issues/79)

## Why
#64 moves the kernel into a child process that the Quick Look extension spawns and kills. A probe proved this from a sandboxed **app** (comment 6131714969 on #64). It could not prove it from a real **appex**, because `qlmanage` hangs when driven from an agent. The remaining risk is that Quick Look's extension sandbox refuses `posix_spawn` of a bundled helper. If it does, #64's design changes, so this goes first.

## Scope
- [ ] A minimal preview appex (the existing `scripts/build-macos-app.sh` pattern) whose `preparePreviewOfFile` spawns `Contents/Helpers/<helper>`. The helper is signed `app-sandbox` plus `inherit`, opens the previewed file by path, and returns its size and first bytes. The appex shows the result as text.
- [ ] The same in a thumbnail appex.
- [ ] Kill test: the helper sleeps holding 900 MB; the appex kills it at a deadline and reports the reap.
- [ ] A needs-human checklist: install, preview a file in Finder (space bar), check the result text and Activity Monitor (no leftover helper).

## Pitfalls
- Ad-hoc signing without the hardened runtime is enough for this spike. Developer ID is #64's concern.
- The helper must be the `inherit` kind, or it traps with signal 5.
- Clean up the installed test app after the check.

## Acceptance
- A human confirms in Finder that the spawn works in preview and thumbnail, or reports the exact failure. The result goes on #64, and #64's design stands or is reopened.

Refs: #64
---

# [Comment #1]() by [gerchowl]()

_Posted on October 12, 2026 at 08:46 PM_

## Spike #79, round 1: built, installed, **needs a human** (2026-10-12, sage, macOS 26.4)

**Built** (scratch `~/.local/state/architect/stepv/scratch/79/`, no PR): `stepv-spike79.app` (`ch.vigos.stepv-spike79`, ad-hoc signed, no hardened runtime). It has a preview appex and a thumbnail appex. Each one `posix_spawn`s `Contents/Helpers/probe-helper` (a C helper signed `app-sandbox` plus `inherit`) on the previewed file, and then a second copy in the appex's own `Contents/Helpers`. The helper reports the file size, the first 16 bytes and `sandbox_check`. If the file name contains `HOLD`, the helper touches 900 MB and sleeps, and the appex sends SIGKILL at 3 s and reports the reap. The preview shows this report as text, and the thumbnail draws it with a green or red bar. The appexes claim their own type, `.spike79`, because the installed stepv.app owns `.step`. The test files are byte copies of an abc-dataset `.step` fixture.

**Verified (agent, without Quick Look):**
- `./build.sh` exit 0, including `codesign --verify --deep --strict`.
- `codesign -d --entitlements` shows: helper = `app-sandbox` + `inherit`; both appexes = `app-sandbox` + `files.user-selected.read-only`.
- Spawn from a plain sandboxed client running the same Swift spawn code:
  - granted file: `SPAWN OK`, `sandboxed=yes`, `helper open OK size=11859 head=49534f…"ISO-10303-21;.HE"`, `exit=0`;
  - HOLD: `deadline 3000 ms hit…: SIGKILL`, `KILL rc=0 reaped=true in 0.4 ms, gone=true`, `signal=9`;
  - ungranted path: `helper open FAILED errno=1`;
  - afterwards `pgrep probe-helper` finds nothing (exit 1).
- New finding: a sandboxed command-line client with no embedded Info.plist traps with SIGTRAP (exit 133), even with `codesign -i`. Embedding the plist with `-sectcreate __TEXT __info_plist` fixes it.
- `./install.sh` exit 0. `pluginkit -m -v | grep spike79` lists both `+ ch.vigos.stepv-spike79.{preview,thumbnail}(0.0.79)` entries. `UTType(filenameExtension:"spike79")` resolves to `ch.vigos.stepv-spike79.probe`. `open -g` on the container failed with launchd error 125 from the agent session. That launch isn't needed, because lsregister and pluginkit already registered the extensions.

**Needs a human (nothing about Quick Look has been observed yet):** whether a real appex can spawn the helper, whether the helper inherits the file grant, and whether the kill works inside Quick Look. The steps are in `scratch/79/CHECKLIST.md`:
1. `open ~/.local/state/architect/stepv/scratch/79/testfiles`
2. Press Space on `probe.spike79`. Expect `SPAWN OK` + `helper open OK size=11859` + `exit=0`.
3. Press Space on `probe-HOLD.spike79`. After about 3 s, expect `KILL rc=0 reaped=true … gone=true` and `signal=9`.
4. Switch to icon view (Cmd-1) with large icons. Expect `THUMB` text with a green bar on both files.
5. Press Space on `control.step`. Expect the normal stepv preview.
6. Check Activity Monitor for `probe-helper`. Expect none.
7. Send screenshots and `./logs.sh`. Clean up with `./uninstall.sh`.


---

# [Comment #2]() by [gerchowl]()

_Posted on October 12, 2026 at 11:58 PM_

## Result (the owner ran it in Finder, 2026-10-13 01:49–01:52): **spawning from a Quick Look appex is denied**
- **Both appexes ran:** `Spike79Preview` (pid 20004) 7 times, and `Spike79Thumbnail` (pid 79284).
- **Every spawn failed** from both helper locations (`app/Contents/Helpers` and `appex/Contents/Helpers`), 22 of 22: `SPAWN FAILED rc=1 (Operation not permitted)`.
- **The thumbnail didn't draw** because of a separate spike bug (`Could not draw thumbnail from QLThumbnailReply because the requested context size is invalid`), so Finder showed a generic icon. This doesn't affect the spawn result.

The earlier probe succeeded from a sandboxed **app**. Quick Look extensions run under a stricter sandbox that denies `posix_spawn` of bundled helpers, so **#64's subprocess design does not hold.**

**Owner decision (2026-10-13): an XPC supervisor plus a child.** The appex talks to a small XPC service that never exits. The service spawns the kernel as its own child and kills it on cancel, so there's no launchd throttle and the memory is freed at once. A probe comes first: can a sandboxed XPC service inside an appex spawn a child, and can file access work through an fd passed over XPC (which needs a kernel `--fd` entry point)? This spike extends to that probe.

---

# [Comment #3]() by [gerchowl]()

_Posted on October 13, 2026 at 12:07 AM_

## Spike #79, round 2: an automated Quick Look e2e harness, then the XPC supervisor. **All pass** (2026-10-13, sage, macOS 26.4)

**Harness** (`scratch/79/ql-e2e/`; `./run-all.sh` runs the matrix, no human needed):
- **Thumbnail:** `ql-thumb` calls `QLThumbnailGenerator.generateBestRepresentation`, with a timeout.
- **Preview:** `ql-preview-host.app` shows a `QLPreviewView`, started with `open -n -W -a … --args <file> <secs>`, and quits itself.
- **Each run:** a fresh copy of the test file, so no Quick Look cache answers; a read-only RSS sampler on `probe-helper`; `log show --style ndjson` from the run's start time; `verdict.py` turns it into PASS or FAIL.
- **Hard timeouts:** 45 s per run, 60 s on `log show`, 20 s per XPC request.

**Round 1 reproduced first, on the unchanged spike:** preview, preview HOLD and thumbnail all gave `SPAWN FAILED rc=1 (Operation not permitted)` from both helper locations, which matches your Finder run. The direct spawn stays in as a control block: EPERM in every run.

**Two round-1 bugs fixed:**
- The thumbnail drew into a fixed 256×256, larger than `maximumSize`; it now draws at `maximumSize`. It is verified to draw at 16, 32, 64 and 512 pt. At 1024 the request fails with QL error 102 before the extension is called, and the installed stepv.app fails the same way, so that is a Quick Look limit.
- The round-1 HOLD didn't hold anything: clang optimised the 900 MB touch away. The round-1 kill numbers were therefore not under load. Fixed with `volatile`; the footprint is now 901 MB.

**Probe:** `Spike79Supervisor.xpc` in each appex's `Contents/XPCServices`, signed `app-sandbox` only. The appex sends `FileHandle(forReadingFrom:)` over NSXPC. The service `posix_spawn`s its own `Contents/Helpers/probe-helper` (`app-sandbox` + `inherit`) with that fd as fd 3 (`POSIX_SPAWN_CLOEXEC_DEFAULT`, only 0–3 inherited). `./run-all.sh` exit 0, 4 of 4 PASS:
1. **Spawn from the XPC service works**, in both preview and thumbnail: `SPAWN OK in 0.3–1.2 ms`, `sandboxed=yes`.
2. **Reading through the fd works:** `size=11859 head=49534f2d31303330332d32313b0a4845 "ISO-10303-21;.HE"`.
   - Opening the same file by path in the helper gives `Operation not permitted`, so the fd is the only way in.
   - `open("/dev/fd/3")` works too: `"ISO-10303-21;"`.
3. **HOLD:** footprint 901 MB (the sampler sees an RSS peak of 901 MB). `deadline 3000 ms hit at 3001 ms: SIGKILL`, `KILL rc=0 reaped=true in 30–31 ms, gone=true`, `signal=9`, and 0 `probe-helper` left. "Memory freed" here means the process is reaped and gone. A `vm_stat` free-page sample was too noisy on this machine to show the release, so it isn't counted as evidence.
   - **No throttle:** request C, sent right after the kill through the same service, took **7.3–7.9 ms** round trip.
4. **Service lifetime and concurrency:**
   - The thumbnail service (pid 41870) stayed alive **35 s** and served 8 requests across 5 thumbnail calls.
   - The preview service is one per preview-host session: its lifetime follows the appex process. Whether it persists across Finder previews is **unmeasured**.
   - **Concurrent:** request B ran while A held 900 MB and returned in **9–21 ms**, independent of A.

**What the kernel needs (#64):**
- An fd input: either `--fd N`, or `--input /dev/fd/N`, which works under the inherited sandbox and keeps OCCT's path-based `ReadFile`.
- **`--format step|iges|brep`:** `run()` picks the reader by file extension, and an fd has none.
- **`realpath`:** `run()` `realpath`s the input; it must accept `/dev/fd/N` or skip that step on the fd path.
- **Still open:** multi-file assemblies. The siblings aren't reachable through one fd, and opening by path is denied.


