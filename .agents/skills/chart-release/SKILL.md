---
name: chart-release
description: Prepare and publish an independent Submilli Helm chart release while retaining its existing runtime appVersion. Use for chart-only releases between runtime releases; use release when publishing a new runtime too.
---

# Chart release

Publish the Helm chart to `oci://ghcr.io/submilli/charts/submilli` without
rebuilding the runtime. An explicit request to release the chart authorizes
the release commit, direct push to canonical main, and workflow dispatch.
Editing this skill or preparing a candidate alone does not authorize publication.
Preserve narrower user instructions such as preparing without publishing.

Read `AGENTS.md`, `.github/workflows/chart-release.yml`,
`.github/workflows/chart-ci.yml`, `scripts/publish_chart.py`, and
`charts/submilli/PUBLISHING.md`. These define the checks, immutable publication,
and first-publication setup. Use [release](../release/SKILL.md) when the request
includes a new runtime; that workflow already publishes its chart.

## Prepare the candidate

- Inspect status, staging, remotes, and ongoing release runs. Fetch canonical
  `submilli/submilli-runtime` main, normally remote `upstream`, and start from
  its latest commit. Preserve unrelated work with a separate worktree if needed.
  Include pending changes only when the user includes them in the release.
- Inspect the current chart and published registry versions. Use the requested
  chart version, or select the next patch for compatible fixes; account for
  feature and breaking changes. The version must advance beyond published chart
  versions, including charts shipped by runtime releases. An existing version
  is a resume case, never permission to replace its contents.
- Update `charts/submilli/Chart.yaml` `version`. Keep `appVersion` unchanged and
  verify its default runtime image is already published and anonymously readable.
  If main points at an unreleased runtime, report that prerequisite; do not
  silently downgrade `appVersion` or publish a runtime as part of this task.
- Keep Cargo versions and runtime tags unchanged. Update chart install pins and
  compatibility notes in `charts/submilli/README.md`,
  `charts/submilli/PUBLISHING.md`, and
  `docs/part-4-server/07-deploy-on-kubernetes.md`. Read `docs-site/WRITING.md`
  before book edits. Preserve historical examples and unrelated version pins.

## Verify and push

Run the chart checks defined in Chart CI: publisher regression tests, Helm lint,
render/schema checks, unit tests, and package-boundary checks. Use its supported
Helm versions. For behavioral chart changes, include the affected install,
upgrade, and hook checks. Reuse valid results for the exact candidate. Check
changed workflows and build the documentation site when the book changes.
Chart-only preparation does not require Rust/package/conformance suites; assess
any broader included changes using `AGENTS.md` rather than ignoring them.
Required failed or unavailable pre-publication checks block publication.

Stage only intended release files, inspect the staged diff, and commit with
`Release chart X.Y.Z` and relevant issue IDs. Fetch canonical main again. If it
advanced, integrate it, reassess chart/runtime versions and rerun affected
checks. Require a clean release checkout and a fast-forward push.

Set `release_remote` to the verified canonical remote and `release_commit` to
the verified full candidate SHA, then push only that commit to main:

```sh
git push "$release_remote" "$release_commit:refs/heads/main"
```

Do not create a Git tag or GitHub release: chart versions are OCI tags. If branch
protection rejects the push, report the block without changing protections or
force-pushing. Confirm remote main matches the candidate and its applicable CI
checks passed before dispatching publication.

## Publish and verify delivery

Dispatch the shared chart workflow from main:

```sh
gh workflow run chart-release.yml --repo submilli/submilli-runtime --ref main
```

The standalone workflow resolves main in its `source` job and pins that commit
for subsequent jobs. It has no manual source-ref input: `--ref main` selects
the workflow ref, not an immutable chart candidate. Inspect the resolved source
SHA and chart metadata in the run. If main advanced, do not claim the earlier
candidate was published; inspect and verify the actual candidate. Cancel before
publication when unverified chart changes are detected, then reassess.

Follow the matching run through validation, publication, and anonymous OCI
installation. Require the published chart's version and `appVersion` to match
the candidate, and require a successful clean-cluster install and `helm test`
without registry credentials or image pull secrets. A successful push alone is
not delivery verification. Report the OCI reference, chart/runtime versions,
resolved commit, workflow URL, checks, and any blocked coverage.

## Resume and failures

Inspect the registry and existing runs before retrying. The publisher reuses an
existing chart version only when its packaged contents match; changed contents
require a new version. Authentication errors and outages are failures, not proof
that a version is absent. Never bypass the publisher with an overwrite.

For partial publication, prefer rerunning failed jobs of the original run,
retaining its completed `source` job and SHA. A fresh dispatch or rerunning the
source job resolves main again: inspect it and reverify any changes first. Do
not overlap releases or retry indefinitely; stop and report a repeated blocker.

If a new package is private, follow `charts/submilli/PUBLISHING.md` for the
administrator's visibility setup and rerun after it is public. Report disabled
workflows, missing package permissions, and unavailable public image access as
blockers. Do not automatically change repository/package settings or claim
anonymous delivery from authenticated checks.
