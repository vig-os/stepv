---
type: issue
state: open
created: 2026-10-04T11:16:29Z
updated: 2026-10-08T04:16:19Z
author: renovate[bot]
author_url: https://github.com/renovate[bot]
url: https://github.com/vig-os/stepv/issues/2
comments: 0
labels: none
assignees: none
milestone: none
projects: none
parent: none
children: none
synced: 2026-10-08T08:40:54.316Z
---

# [Issue 2]: [Dependency Dashboard](https://github.com/vig-os/stepv/issues/2)

This issue lists Renovate updates and detected dependencies. Read the [Dependency Dashboard](https://docs.renovatebot.com/key-concepts/dashboard/) docs to learn more.<br>[View this repository on the Mend.io Web Portal](https://developer.mend.io/github/vig-os/stepv).

## Awaiting Schedule

The following updates are awaiting their schedule. To get an update now, click on a checkbox below.

 - [ ] <!-- unschedule-branch=renovate/github-actions-(minor-and-patch) -->ci(actions): update actions/upload-artifact action to v7.0.2
 - [ ] <!-- unschedule-branch=renovate/macos-26.x -->ci(actions): update dependency macos to v26
 - [ ] <!-- create-all-awaiting-schedule-prs -->🔐 **Create all awaiting schedule PRs at once** 🔐

## Detected Dependencies

<details><summary>github-actions (16)</summary>
<blockquote>

<details><summary>.github/actions/setup-devkit-toolchain/action.yml</summary>


</details>

<details><summary>.github/workflows/abandon-release.yml</summary>


</details>

<details><summary>.github/workflows/ci.yml</summary>


</details>

<details><summary>.github/workflows/codeql.yml</summary>


</details>

<details><summary>.github/workflows/devkit-upgrade.yml</summary>


</details>

<details><summary>.github/workflows/kernel.yml (4)</summary>

 - `actions/checkout v7.0.1@3d3c42e5aac5ba805825da76410c181273ba90b1`
 - `cachix/install-nix-action v31.11.1@13d8dd58da0234aa297dedd986986ccb8e7f3e24`
 - `actions/checkout v7.0.1@3d3c42e5aac5ba805825da76410c181273ba90b1`
 - `ubuntu 26.04`

</details>

<details><summary>.github/workflows/prepare-release-extension.yml (4)</summary>

 - `actions/create-github-app-token v3@bcd2ba49218906704ab6c1aa796996da409d3eb1`
 - `actions/checkout v7.0.1@3d3c42e5aac5ba805825da76410c181273ba90b1`
 - `vig-os/commit-action v0.3.3@7ba7e1ae17547813e708d4cc6a771a09f978f724`
 - `ubuntu 26.04`

</details>

<details><summary>.github/workflows/prepare-release.yml</summary>


</details>

<details><summary>.github/workflows/promote-release.yml</summary>


</details>

<details><summary>.github/workflows/release-assets.yml (1)</summary>

 - `ubuntu 26.04`

</details>

<details><summary>.github/workflows/release-core.yml</summary>


</details>

<details><summary>.github/workflows/release-extension.yml (14)</summary>

 - `actions/checkout v7.0.1@3d3c42e5aac5ba805825da76410c181273ba90b1`
 - `cachix/install-nix-action v31.11.1@13d8dd58da0234aa297dedd986986ccb8e7f3e24`
 - `actions/attest-build-provenance v4.2.2@4d101475d8b20a2381f78447822ac1eab6504dd8`
 - `actions/upload-artifact v7.0.1@043fb46d1a93c77aae656e7c1c64a875d1fc6a0a` → [Updates: `v7.0.2`]
 - `actions/checkout v7.0.1@3d3c42e5aac5ba805825da76410c181273ba90b1`
 - `cachix/install-nix-action v31.11.1@13d8dd58da0234aa297dedd986986ccb8e7f3e24`
 - `actions/attest-build-provenance v4.2.2@4d101475d8b20a2381f78447822ac1eab6504dd8`
 - `actions/upload-artifact v7.0.1@043fb46d1a93c77aae656e7c1c64a875d1fc6a0a` → [Updates: `v7.0.2`]
 - `actions/checkout v7.0.1@3d3c42e5aac5ba805825da76410c181273ba90b1`
 - `cachix/install-nix-action v31.11.1@13d8dd58da0234aa297dedd986986ccb8e7f3e24`
 - `rust-lang/crates-io-auth-action v1.0.5@c6f97d42243bad5fab37ca0427f495c86d5b1a18`
 - `ubuntu 26.04`
 - `macos 15` → [Updates: `26`]
 - `ubuntu 26.04`

</details>

<details><summary>.github/workflows/release-publish.yml</summary>


</details>

<details><summary>.github/workflows/release.yml</summary>


</details>

<details><summary>.github/workflows/scorecard.yml</summary>


</details>

<details><summary>.github/workflows/sync-issues.yml</summary>


</details>

</blockquote>
</details>

---

- [ ] <!-- manual job -->Check this box to trigger a request for Renovate to run again on this repository


