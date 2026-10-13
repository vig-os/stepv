---
type: issue
state: open
created: 2026-10-13T01:02:48Z
updated: 2026-10-13T01:02:48Z
author: gerchowl
author_url: https://github.com/gerchowl
url: https://github.com/vig-os/stepv/issues/97
comments: 0
labels: feature, priority:medium, needs-human
assignees: none
milestone: none
projects: none
parent: none
children: none
synced: 2026-10-13T09:11:26.522Z
---

# [Issue 97]: [macOS: Developer ID signing with hardened runtime + library validation for the app, appexes, supervisor and kernel closure (#64)](https://github.com/vig-os/stepv/issues/97)

Owner decision (GOAL.v3): one team's Developer ID, with hardened runtime and library validation kept on. No `disable-library-validation`. Under ad-hoc signing with the hardened runtime, dyld refuses the bundled dylibs ("different Team IDs", #64 pre-mortem), so #64 is built and tested ad-hoc **without** the hardened runtime. This issue carries the release signing.

## Needs the owner
- A Developer ID Application certificate. sage has none today (`security find-identity -v -p codesigning` lists only `nix-tcc`).
- For CI: the certificate (.p12) and its password as repo secrets, set by the owner with `gh secret set`. Notarization credentials too, if releases are notarized.

## Scope
- [ ] `scripts/build-macos-app.sh` signs everything with one identity, inside out: the dylib closure, `stepv-occt`, the supervisor `.xpc`, the appexes, then the app. Hardened runtime on.
- [ ] Entitlements per bundle, minimal.
- [ ] A CI job signs with the secret identity, and `codesign --verify --deep --strict` plus `spctl --assess` pass. The ql-e2e harness runs against the signed build.
- [ ] Notarization, if releases ship outside the Mac App Store: decide, and document.

Refs: #64, #9, #10
