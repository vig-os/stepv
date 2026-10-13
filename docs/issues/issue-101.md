---
type: issue
state: open
created: 2026-10-13T01:47:27Z
updated: 2026-10-13T01:47:27Z
author: gerchowl
author_url: https://github.com/gerchowl
url: https://github.com/vig-os/stepv/issues/101
comments: 0
labels: feature, priority:low
assignees: none
milestone: none
projects: none
parent: none
children: none
synced: 2026-10-13T09:11:25.460Z
---

# [Issue 101]: [macOS: three copies of the OCCT closure (+117 MB) because each XPC service's sandbox can't read the app's Frameworks](https://github.com/vig-os/stepv/issues/101)

From #98's review (#64 PR 1). Each Quick Look supervisor service carries its own copy of `stepv-occt` plus its 31 OCCT dylibs, because the service's sandbox can't read `<app>/Contents/Frameworks` (dyld aborts with signal 6). That takes the app from 57 MB to 174 MB.

Options, verified by the reviewer:
- **`com.apple.security.temporary-exception.files.absolute-path.read-only`** on `<app>/Contents/Frameworks/`: works (the kernel runs from the shared copy, exit 0). But it ties the app to its install path, and temporary exceptions are discouraged, and refused on the App Store.
- **Link `stepv-occt` statically against OCCT:** no dylibs at all, one self-contained binary per service. This is probably far smaller than 31 dylibs each, and it also removes the `@rpath` and library-validation surface for #97. It needs OCCT's static build in the flake. **Recommended to evaluate.**
- Keep the duplication: correct, but large.

Decide with numbers (the static binary size, and build time) before #97 and the release (#9 and #10).

Refs: #64, #98, #97
