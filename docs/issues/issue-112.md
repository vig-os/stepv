---
type: issue
state: open
created: 2026-10-13T10:15:12Z
updated: 2026-10-13T10:15:12Z
author: gerchowl
author_url: https://github.com/gerchowl
url: https://github.com/vig-os/stepv/issues/112
comments: 0
labels: chore, priority:high, needs-human
assignees: none
milestone: none
projects: none
parent: none
children: none
synced: 2026-10-14T07:21:25.890Z
---

# [Issue 112]: [ci: put the Cachix push token in a main-only GitHub Environment, with a write-only token](https://github.com/vig-os/stepv/issues/112)

`CACHIX_AUTH_TOKEN` is a repository secret. In a `pull_request` run the workflow file comes from the PR head, so anyone who can push a branch to vig-os/stepv can edit a workflow, or add a push-triggered one on their branch, and read the token. This is despite #81's PR-side boundary: `skipPush`, no authToken, and a guard step. Fork PRs get no secrets, so the exposure is limited to people with write access. Found by #81's re-review.

## Scope
- [ ] Move `CACHIX_AUTH_TOKEN` into a GitHub **Environment** (for example `cache-push`) whose deployment branches are limited to `main`, with required reviewers if wanted. The jobs that push (`kernel.yml` on push to main, and `release-extension.yml` via release) declare `environment: cache-push`. PR jobs don't, so the secret is unreachable from them even when the workflow is edited.
- [ ] Delete the repo-level secret. The owner sets the new secret (`gh secret set CACHIX_AUTH_TOKEN --env cache-push`); its value never goes through an agent.
- [ ] Use a **write-only** Cachix token for CI. The current one is Admin-scoped.
- [ ] Document the boundary in plan.md "Binary cache".
- [ ] The same applies to devkit's managed `ci.yml`: see vig-os/devkit#1881.

## Acceptance
A same-repo PR that edits a workflow to print `${{ secrets.CACHIX_AUTH_TOKEN }}` gets an empty value. Verify by inspection of the Environment rules; don't actually print a secret.

Refs #74, #81

