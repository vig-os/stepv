---
type: issue
state: closed
created: 2026-10-04T18:39:42Z
updated: 2026-10-04T20:46:00Z
author: gerchowl
author_url: https://github.com/gerchowl
url: https://github.com/vig-os/stepv/issues/18
comments: 0
labels: bug, priority:high
assignees: none
milestone: none
projects: none
parent: none
children: none
synced: 2026-10-05T08:45:49.986Z
---

# [Issue 18]: [security: sandbox the kernel on every path (Landlock + seccomp, macOS profile), test-first](https://github.com/vig-os/stepv/issues/18)

## Description

The kernel parses **untrusted input** with OCCT's C++ STEP/IGES/BREP readers, a large legacy parser
not designed for hostile files. A crafted file that triggers a memory-corruption bug could run code
with whatever rights the kernel process has.

Today the kernel always runs isolated as a **process** (crash, hang and memory blow-up are
contained), but it is only **sandboxed** on some paths:

| Path | Sandboxed today |
| --- | --- |
| macOS Quick Look (preview + thumbnail) | **yes**: App Sandbox (read-only access to the previewed file, no network, no exec) |
| GNOME thumbnails | **yes**: GNOME runs thumbnailers under bubblewrap |
| XFCE (Tumbler), KDE (Dolphin) thumbnails | **no**: full user rights |
| `stepv` CLI / `stepv view` (both platforms) | **no**: full user rights |

On the unsandboxed paths, an exploit could read and write the user's files, use the network and
exec programs. This is a security hardening, not a feature.

## What the kernel actually needs

- **read**: the input file, and its directory (multi-file assemblies resolve external references
  as siblings);
- **write**: exactly one output file (the `--mesh` STEPVMSH);
- nothing else: no network, no exec, no writes elsewhere, no reads outside the input's directory
  (beyond its own executable and shared libraries).

## Plan: tests first

1. **Write the tests first, and see them fail** against today's kernel. A test hook in the kernel
   (env-gated, like `STEPV_OCCT_TEST_BALLOON_MB`) attempts each forbidden action after the sandbox
   is entered, and the test asserts each one is **refused**, not just that the run survives:
   - open a TCP/UDP socket;
   - `execve` a program;
   - create or write a file outside the output path (e.g. `$HOME/stepv-sandbox-probe`,
     `/tmp/...`);
   - read a file outside the input's directory (e.g. `~/.ssh/known_hosts`, a temp canary file);
   - and the **allowed** set still works: read the input, read a sibling external-reference file
     (CAx-IF multi-file assembly), write the mesh, and every `tests/data` file renders.
2. **Linux:**
   - **Landlock** (unprivileged, Linux ≥ 5.13): read-only on the input's directory and the
     loader/library paths; write on the output file only.
   - **seccomp-bpf**: deny `socket`/`connect`, `execve`/`execveat`, `ptrace`.
   - Applied in the kernel before OCCT touches the input. On kernels without Landlock, degrade to
     today's behaviour and **report it** (a summary field, e.g. `"sandbox": "none" | "landlock" |
     "landlock+seccomp" | "macos-profile"`), never silently.
3. **macOS CLI:** start the kernel under a sandbox profile (`sandbox_init` / SBPL) with the same
   rules. Quick Look is already App-Sandboxed; add a test that the extension's entitlements stay
   minimal (no network, no write).
4. **Harness:** `harness --strict` and `cli-sweep` must still pass on the corpus under the
   sandbox. Nothing legitimate may break; the external-reference assemblies are the critical case.
5. **CI:** the sandbox tests run on both Kernel lanes. Record the outcome in `plan.md` §4 / §6 and
   in the README's "contained" claim, which should only become unconditional once this lands.

## Acceptance

Every forbidden action is refused on Linux (Landlock-capable kernels) and on macOS, in tests that
were red before the change; the corpus gates are unchanged; and the summary says which sandbox was
in effect.

