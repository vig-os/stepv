---
type: issue
state: open
created: 2026-10-04T13:48:11Z
updated: 2026-10-04T19:00:37Z
author: gerchowl
author_url: https://github.com/gerchowl
url: https://github.com/vig-os/stepv/issues/3
comments: 0
labels: priority:blocking, needs-human
assignees: none
milestone: none
projects: none
parent: none
children: none
synced: 2026-10-05T08:45:54.714Z
---

# [Issue 3]: [Workflow credentials will not arrive from org-config#317 — six manual PUTs needed](https://github.com/vig-os/stepv/issues/3)

## Summary

`plan.md` §7 currently says stepv's devkit workflows have no credentials "until that PR applies".
That is **wrong, and optimistic**. Merging and applying
[vig-os/org-config#317](https://github.com/vig-os/org-config/pull/317) will land the rulesets, the
repo settings and the custom property — but it will **not** grant the credentials.

## Why

`otterdog apply` does not reconcile `selected_repositories` for org secrets whose declared value is
a `'********'` dummy, which is nine of the ten. The Otterdog plan on #317 is `4 to add, 11 to
change, 0 to delete` — four rulesets and eleven settings, **zero secret actions**.

Proven independently of stepv: `revkit` was added to the same six lists in `63420ca`, merged as
org-config#312 on 2026-09-29. `apply-engine.yml` ran successfully for that exact commit, and twice
more since. `revkit` is still absent from all six live lists today.

Tracked upstream as [vig-os/org-config#318](https://github.com/vig-os/org-config/issues/318).

## Consequence for stepv

`sync-issues.yml`, `devkit-upgrade.yml` and the whole release train will fail with an **empty
credential and no error message** — not a bug in those workflows, and not something a stepv-side
change can fix.

## Remediation (needs org-owner credentials)

Six PUTs, after #317 applies:

```sh
for s in COMMIT_APP_CLIENT_ID COMMIT_APP_PRIVATE_KEY \
         DEVKIT_UPGRADE_APP_CLIENT_ID DEVKIT_UPGRADE_APP_PRIVATE_KEY \
         RELEASE_APP_CLIENT_ID RELEASE_APP_PRIVATE_KEY; do
  # add stepv to the existing list — read it first, PUT the union, never the
  # single name, or every other repo loses the grant
  gh api "/orgs/vig-os/actions/secrets/$s/repositories" --jq '[.repositories[].name]'
done
```

The endpoint replaces the whole list, so read-modify-write. Verify with the same GET afterwards.

## Action here

Correct `plan.md` §7 so the next reader is not told the credentials arrive automatically. No code
change. Low priority relative to S2 — nothing in S1–S3 needs these workflows.
