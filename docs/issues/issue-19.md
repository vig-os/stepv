---
type: issue
state: closed
created: 2026-10-04T18:42:25Z
updated: 2026-10-04T21:00:47Z
author: gerchowl
author_url: https://github.com/gerchowl
url: https://github.com/vig-os/stepv/issues/19
comments: 0
labels: bug, priority:medium
assignees: none
milestone: none
projects: none
parent: none
children: none
synced: 2026-10-05T08:45:49.633Z
---

# [Issue 19]: [macOS Quick Look: multi-file assemblies show nothing (sandbox blocks sibling files)](https://github.com/vig-os/stepv/issues/19)

## Description

**Multi-file STEP assemblies show nothing in macOS Quick Look**: the parts live in sibling files the
top-level file references (CAx-IF `s1-c5-214`, and a common SolidWorks/CATIA export style).

- CLI, unsandboxed: `stepv s1-c5-214.stp --png …` → ok, **11 parts**.
- Quick Look thumbnail and preview: **no geometry**. The extension logs
  `no geometry for s1-c5-214.stp: no geometry in file`.

## Cause

The Quick Look extension's App Sandbox grants read access to **the previewed file only**. OCCT's
`STEPCAFControl_Reader` cannot open the sibling files, skips them **silently**, and the document
ends up empty. That's worse than a clear error, because the user sees "no preview" with no reason
that points at the assembly structure.

## Options (this is a security trade-off, see #18)

1. **Honest message, no wider access** (minimum, do regardless): detect unresolved external
   references (`DOCUMENT_FILE` / `APPLIED_EXTERNAL_IDENTIFICATION_ASSIGNMENT` in the header-level
   scan, or OCCT's `ExternFiles()` after transfer) and say *"assembly of N external files — Quick
   Look can't read sibling files; open with `stepv view`"* in the preview, and in the CLI's JSON as
   a distinct status.
2. **Read access to siblings**: a `com.apple.security.temporary-exception.files…read-only`
   entitlement (home-relative) or a security-scoped bookmark flow. This widens what an exploited
   parser could read, so it must be decided together with #18, not before.
3. **Show what resolved**: render the parts that did load, with a badge for the missing ones. This
   needs the kernel to report unresolved externals per component.

## Acceptance

Quick Look on a multi-file assembly never shows an unexplained blank: either the parts (if option 2
is taken) or a message that names the cause. Covered by a test using the CAx-IF `s1-c5-214` fixture.

