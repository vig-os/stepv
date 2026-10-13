---
type: issue
state: closed
created: 2026-10-13T02:04:56Z
updated: 2026-10-13T05:43:07Z
author: gerchowl
author_url: https://github.com/gerchowl
url: https://github.com/vig-os/stepv/issues/102
comments: 0
labels: bug, priority:high
assignees: none
milestone: none
projects: none
parent: none
children: none
synced: 2026-10-13T09:11:24.968Z
---

# [Issue 102]: [kernel fd input: re-opening /dev/fd/N is denied by the inherited sandbox on macOS 15 — read the descriptor directly](https://github.com/vig-os/stepv/issues/102)

Found by #98's CI (#64 PR 1) on the `macos-15-arm64` runner. The kernel runs as a child of the Quick Look XPC supervisor and inherits its sandbox. There, `open("/dev/fd/3")` fails with **`cannot open input: Operation not permitted`**. On macOS 26.4 (sage, #79) the same re-open is allowed. So #91's design (pass `/dev/fd/N` as a path, and let OCCT's path-based readers re-open it) doesn't hold on macOS 15. The supported macOS versions need it to.

**Root cause:** the path re-open goes through the sandbox's path checks. Reading the **already-open descriptor** does not.

**Fix: the kernel reads the inherited descriptor itself, never re-opening it by path.**
- **(A, preferred where OCCT supports it):** wrap the fd in a `std::istream` (a small `streambuf` over `read(2)`, with `lseek` for seekable files), and use OCCT's stream readers: `STEPControl_Reader::ReadStream` for STEP, and `BRepTools::Read(TopoDS_Shape&, std::istream&, …)` for BREP. IGES: check whether OCCT 7.9.3 has a stream reader.
- **(B):** copy the fd's contents into a private temporary file in the kernel's own writable tmp, then read that by path. This works for every format. It costs one copy, and the file must be unlinked on every exit path (#98's lifeline).

Pick A for every format OCCT can stream, and B only where it can't, documented per format. Or B uniformly, if A doesn't cover STEP. Justify the choice with a probe on macOS 15 (CI) and 26.

**Acceptance:**
- The #91 fd tests pass on CI's `macos-15` and `ubuntu` runners. The kernel never calls `open()` on `/dev/fd/N`. Add a test that asserts this, for example a Seatbelt profile denying file reads of `/dev/fd/*`, or an fd whose path doesn't exist.
- The corpus shows no differences for path inputs.

Refs: #91, #95, #64, #98
