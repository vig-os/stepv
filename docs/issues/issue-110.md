---
type: issue
state: open
created: 2026-10-13T09:38:53Z
updated: 2026-10-13T09:38:53Z
author: gerchowl
author_url: https://github.com/gerchowl
url: https://github.com/vig-os/stepv/issues/110
comments: 0
labels: chore, priority:medium
assignees: none
milestone: none
projects: none
parent: none
children: none
synced: 2026-10-14T07:21:26.448Z
---

# [Issue 110]: [ci: pin the cachix CLI that cachix-action installs (the action is SHA-pinned, the binary isn't)](https://github.com/vig-os/stepv/issues/110)

`cachix/cachix-action` is pinned by commit SHA (v17, `38b08261…`), but at run time it installs the `cachix` binary unpinned, from cachix.org or nixpkgs depending on the action's defaults. A compromised or changed `cachix` release could therefore run with the push token on main and release jobs. Found by the review of #81.

## Scope
- [ ] Pin the `cachix` CLI that the action uses. Either:
  - point `installCommand` at a nix-pinned `cachix` from this flake's locked nixpkgs (for example `nix profile install .#cachix`, or `nix run` from the flake.lock);
  - or use the action's version input, if it has one.
  
  Document the choice in plan.md "Binary cache".
- [ ] Check every workflow that uses cachix-action: kernel.yml, release-extension.yml, and devkit's ci.yml, which is scaffold-owned. If devkit's needs the change, file it in devkit.

## Acceptance
The CI log shows the store path of the `cachix` that was installed, and it matches the flake.lock's nixpkgs.

Refs #74, #81

