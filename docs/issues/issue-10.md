---
type: issue
state: open
created: 2026-10-04T14:47:19Z
updated: 2026-10-04T19:00:44Z
author: gerchowl
author_url: https://github.com/gerchowl
url: https://github.com/vig-os/stepv/issues/10
comments: 1
labels: feature, priority:high, needs-human
assignees: none
milestone: none
projects: none
parent: none
children: none
synced: 2026-10-05T08:45:52.336Z
---

# [Issue 10]: [S6: release — crates.io publish and binaries for both platforms](https://github.com/vig-os/stepv/issues/10)

## Description

Step **S6** of [`plan.md`](../blob/main/plan.md) §5: publish `stepv` to crates.io and ship binaries
for macOS and Linux through the devkit release train.

## Scope

- **`CARGO_REGISTRY_TOKEN` and a `crates-io` environment**, in `vig-os/scitadel`'s shape. Not
  declared yet on purpose (plan.md §7): don't declare a secret whose live value does not exist.
- Binaries for both platforms, **with OCCT**: Linux can depend on the distribution's package;
  macOS bundles it (S5).
- The crate on crates.io contains the Rust side only; `kernel/` is built separately. Document how
  a crates.io user gets the kernel.

## Depends on

- #3: the release train has no app credentials until the six org-secret grants are made.
- #8: the LGPL NOTICE.
- S4 (#7) and S5 (#9).

---

# [Comment #1]() by [gerchowl]()

_Posted on October 4, 2026 at 05:05 PM_

## Status: wired and tested; waiting only on credentials

Everything that can run without a credential has been built and exercised in CI on every PR (#17):
the relocatable Linux tarball, proven in debian:11, ubuntu:22.04/24.04 and fedora:41 containers
without nix; the macOS app with its headless checks; and `cargo publish --dry-run`-equivalent
packaging.

The release train (`prepare-release-extension.yml` → `release-extension.yml` →
`release-assets.yml`) needs these from a human:

- [ ] **org-config#317 apply**: approve the waiting `production` deployment (rulesets).
- [ ] **#3**: add `stepv` to the six org-secret repository lists. devkit's release workflows mint
      their tokens from these.
- [ ] **#16**: enable the dependency graph. Dependency Review is red on every PR until then.
- [ ] **Apple** (a *final* release refuses to ship without these; a candidate falls back to ad-hoc):
      `MACOS_SIGNING_P12`, `MACOS_SIGNING_P12_PASSWORD`, `MACOS_SIGNING_IDENTITY`,
      `APPLE_NOTARY_KEY_P8`, `APPLE_NOTARY_KEY_ID`, `APPLE_NOTARY_ISSUER`.
- [ ] **crates.io**: create the `crates-io` environment (a required reviewer is recommended) with a
      `CARGO_REGISTRY_TOKEN` for the **first** publish. After it, configure the trusted publisher
      on crates.io and delete the token; the workflow switches to OIDC by itself.

Then run **Prepare Release** with a version. A candidate exercises the whole train without
publishing to crates.io.


