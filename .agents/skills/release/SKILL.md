---
name: release
description: Prepare and publish a Submilli runtime release with its independently versioned Helm chart, including verification, a direct push to main with its tag, and delivery checks. Use when asked to cut a runtime release; use chart-release for chart-only releases.
---

# Release

Cut a runtime release directly on `main`, bypassing a pull request. An explicit
request to release authorizes the commit, tag, direct push, and publication;
do not ask for the same permission again. Editing this skill or preparing a
release without publishing does not authorize publication.

Read `AGENTS.md` and `.github/workflows/release.yml` first. The current workflow
starts when a GitHub release is **published**, not when a tag is pushed. It builds
binaries, validates and publishes the container and Helm chart, then attaches
release assets. Read `.github/workflows/chart-release.yml` and
`charts/submilli/PUBLISHING.md` for the chart publication and public-access gates.
Treat the workflow as the authority if its behavior changes. Do not invoke
`open-pr` or `ship`: this skill owns the release sequence.

For a chart-only release, use [chart-release](../chart-release/SKILL.md).
Runtime releases already call that shared workflow; do not dispatch a second
chart publication separately.

## 1. Establish the candidate

- Inspect the working tree, index, branch, remotes, releases, and tags. Verify the
  canonical repository `submilli/submilli-runtime`, normally remote `upstream`.
- Fetch canonical `main` and tags. Start from its latest commit, preserving user
  changes; use a separate worktree if needed. Include pending work only when the
  user explicitly includes it. Do not sweep unrelated staged files or branch
  commits into the release.
- Use the requested version, or infer the next patch for an ordinary stable
  release when no compatibility change warrants a different bump. State the
  selected version before editing; clarify material ambiguity. Cargo uses
  `X.Y.Z`; tags use `vX.Y.Z`. Check local and remote tags/releases for collisions.
  An existing tag is a resume case, never permission to overwrite it.
- Find the previous published runtime release on the candidate's ancestry,
  excluding `skill-v*` releases. Inspect the entire change range for notes,
  compatibility, documentation, and verification needs. For a first release,
  inspect the candidate's contents instead.

Default to stable releases. Prereleases and older maintenance lines require
checking container aliases and GitHub prerelease/latest flags; do not move stable
aliases back to an older release. This workflow does not publish crates,
TypeScript packages or `skill-v*` releases unless
separately requested.

## 2. Prepare versions and documentation

- Update root `Cargo.toml` `[workspace.package].version`. Member crates inherit
  it; inspect their manifests for exceptions and internal version requirements.
  Do not bump independent dependencies such as `submilli-wasm` or package versions
  just because the runtime changes.
- Run `cargo build --release --bin submilli --bin submilli-server` to build and
  refresh `Cargo.lock`. Inspect the lock diff for expected workspace changes;
  avoid unrelated dependency upgrades and blanket `cargo update`. Repeat the
  build with `--locked` to verify the committed dependency resolution.
- Set `charts/submilli/Chart.yaml` `appVersion` to the runtime version and bump
  its independent chart `version` beyond the latest published chart version,
  including any intervening chart-only releases. Default to the next patch for
  an image-only update; use a larger bump when chart compatibility requires it.
  Do not derive the chart version from the runtime version. Review compatibility
  comments and `charts/submilli/README.md`. Every runtime release needs a new
  chart version, even without template changes. Chart-only releases may bump
  `version` independently while keeping `appVersion` unchanged; see
  `charts/submilli/PUBLISHING.md`.
  Update chart install pins and runtime mappings in `charts/submilli/README.md`,
  `charts/submilli/PUBLISHING.md`, and the Kubernetes deployment guide. The
  workflow rejects an `appVersion` that differs from the runtime release tag.
- Update relevant behavior, configuration, installation, and migration docs.
  Search for old version pins and assess each use; preserve historical references
  and unrelated versions.
- Always update `docs/part-4-server/06-deploy-with-compose.md`: the raw GitHub
  `compose.yaml` tag URL, `SUBMILLI_IMAGE`, and `docker compose ps` image must match
  the new release. Use a real compatible predecessor and the new release for the
  upgrade example, explaining its starting state. Do not present future tags as
  downloadable. Without a compatible predecessor, describe the procedure without
  inventing a working previous-to-current example.
- Always update `docs/part-1-start-here/02-install.md`: the `submilli --version`
  output must show the new release, from the released binary.
- Check `docs/part-4-server/05-deploy-on-linux.md` and
  `docs/part-4-server/07-deploy-on-kubernetes.md` for relevant deployment changes.
  The workflow does not attach `compose.yaml`; retain its raw tag URL unless an
  explicitly scoped workflow change adds that asset.
- Regenerate the reference: build `submilli` and `submilli-server` in release
  mode, then run `npm run reference` in `docs-site` with `SUBMILLI_BIN` and
  `SUBMILLI_SERVER_BIN` naming them, and commit the changed pages.

## 3. Write release notes

Use the previous-release-to-candidate diff, commits, and merged PRs as evidence.
Explain user-visible features, fixes, breaking changes, migration steps, and
known limitations. Include a comparison link and relevant issue/PR links. Do not
use a raw commit dump or claim unshipped behavior.

Include both the chart version and runtime version, with the pinned OCI install
command and chart upgrade notes when applicable.

Follow an existing changelog/release-note convention if present. Otherwise write
a UTF-8 Markdown file outside the checkout, retain it through publication, and
use `--notes-file`; a new committed changelog is not required. Generated GitHub
notes can supplement the curated notes. Update notes if the candidate changes.

## 4. Verify the candidate

Select checks using `AGENTS.md` and the entire release range, including preparation
changes. Version-only preparation does not erase runtime changes since the prior
release. Conformance is mandatory for every runtime release, regardless of the
change range; ordinary development and PR checks do not satisfy this gate.

- Run Rust formatting and workspace clippy for manifest/build changes. Check both
  release binaries with `--version` against the selected version.
- Run workspace/package verification with `SUBMILLI_SKIP_HTTP_TESTS=1`,
  `SUBMILLI_FULL_TEST=1`, `SUBMILLI_CONFORMANCE_TEST=0`, and package
  `--skip-network` as documented in `AGENTS.md`. Reuse completed results for the
  same candidate and environment. Enable required affected HTTP tests and
  explicitly supply credentials for live package tests. Report skipped coverage
  separately.
- Run both complete conformance suites locally on the operator machine, from
  the final release candidate checkout, before tagging, pushing, or publishing.
  Do not dispatch a GitHub Actions conformance run for release verification;
  the conformance workflow is for nightly checks only. This is the explicit pre-release exception
  to keeping conformance disabled during development and PR verification.
  `SUBMILLI_FULL_TEST` does not enable conformance. Clear inherited filters and
  baseline-update/output settings and opt in with the dedicated flag:

  ```sh
  env -u CONFORMANCE_FILTER -u UPDATE_TYPESCRIPT_EXPECTED \
    -u TYPESCRIPT_PORTED_CASES -u TYPESCRIPT_CHECKS_OUT \
    SUBMILLI_SKIP_HTTP_TESTS=1 SUBMILLI_CONFORMANCE_TEST=1 \
    cargo test --locked -p conformance --test conformance --test typescript -- --nocapture
  ```

  Require successful results for both the ECMA-262 and TypeScript suite bodies;
  a skipped suite, filtered run, or harness-only check is not a pass. Do not
  regenerate expected baselines to make release verification green. Record the
  candidate revision, any preparation diff, command, and both suite results.
  Failures or unavailable checks block release publication. Nightly results for
  any revision do not substitute for this local pre-release verification.
- Check/build the documentation site for book changes. Run
  `helm unittest charts/submilli` for chart changes and relevant additional checks
  from `.github/workflows/chart-ci.yml` for behavioral chart changes.
- Inspect the final diff, lockfile, notes, and version agreement. Required failed
  or blocked checks prevent publication. Run `graphify update .` if source code
  changed, as required by repository guidance.

## 5. Commit, tag, and push main

Stage only intended release files, inspect the staged diff, and commit with
`Release vX.Y.Z` (include relevant issue IDs if applicable). Fetch main again.
If it advanced, integrate the new commits before tagging, reassess the range and
notes, and rerun affected verification plus both complete conformance suites on
the integrated candidate. Any candidate changes after conformance verification
invalidate that gate; rerun both suites before tagging. Confirm the candidate fast-forwards remote
main and the release worktree is clean.

Set `release_remote`, `release_tag`, and `release_commit` to the verified canonical
remote, tag, and full commit SHA; set `release_repo` to the GitHub repository and
`release_notes` to the notes file path.

```sh
git tag -a "$release_tag" "$release_commit" -m "Release $release_tag"
git push --atomic "$release_remote" "$release_commit:refs/heads/main" "refs/tags/$release_tag:refs/tags/$release_tag"
```

Push only main and this tag. Never force-push main or rewrite a published tag.
Atomic push updates both refs or neither. After rejection inspect remote state:
concurrent main changes require renewed integration and verification, recreating
only a verified unpublished local tag. If repository protection denies a direct
push, report the block without changing protections or silently substituting a
PR. Do not split a rejected atomic push into a tag-only push.

Verify remote main and the tag's peeled commit against the release commit. Wait
for applicable main CI checks for that commit before publishing. A push alone
does not start the current runtime release workflow.

## 6. Publish and verify delivery

For a new stable release:

```sh
gh release create "$release_tag" --repo "$release_repo" --verify-tag --title "Submilli $release_tag" --notes-file "$release_notes" --latest
```

The workflow starts on `release.published`, or explicit `workflow_dispatch` for
an already published release. A draft does not build assets. Publication exposes
the release before assets finish uploading; stay with the task until delivery is
verified. Use credentials that trigger the event: Actions' own `GITHUB_TOKEN`
does not trigger another workflow through a release event.

- Find the `release.yml` run for this tag and commit; watch it with
  `gh run watch <run-id> --exit-status`. Check all jobs. Use bounded polling;
  report queued or stalled runs as pending, not successful.
- Derive expected assets from the workflow upload step. Currently it uploads both
  binaries for macOS arm64/x86_64, Linux x86_64 musl, and Windows x86_64, plus
  `install.sh`, `install.ps1`, and `SHA256SUMS`. Linux arm64 binaries are built for
  the container but are not currently uploaded as release assets.
- Verify assets are present and nonempty. Download them to a temporary directory
  and check their SHA-256 values against `SHA256SUMS`. Smoke-test host-compatible
  binaries with `--version`; do not overwrite the user's installation.
- Verify GHCR full-version, major.minor, and stable `latest` aliases, with
  linux/amd64 and linux/arm64 manifests and source revision matching the release
  commit. The workflow runs container smoke/conformance checks before publishing.
- Verify the release body, stable/latest designation, and tag-pinned Compose URL.
  Verify the chart version from the release commit is anonymously downloadable
  from `oci://ghcr.io/submilli/charts/submilli` and the workflow's clean-cluster
  OCI install and `helm test` jobs passed. Both chart and image must be public;
  first publication may need package administrator setup as documented in
  `charts/submilli/PUBLISHING.md`. Never overwrite an existing chart version.
  Report the release URL, commit, tag, checks, and skipped/blocked coverage.
  Completion requires successful delivery checks.

## Resume and failures

Before retrying mutations, inspect remote main, the tag, release, assets, and
workflow runs. Reuse matching state; stop on tag/commit mismatches. If the tag
exists but no release does, require recorded successful conformance results for
that exact candidate from the operator machine, or run both suites locally on
its tagged source before publication.
Then continue at creation after the remaining verification. Inspect and
publish an existing draft rather than creating another release. Do not recreate
a published release.

If no run triggered or an infrastructure failure needs another run, use
`gh workflow run release.yml --repo "$release_repo" -f "tag=$release_tag"` only
after verifying the published release and source commit. Check the workflow on
the default branch remains appropriate for that source. Do not overlap runs or
retry indefinitely. Uploads use `--clobber` and image aliases can move, so inspect
partial results first; immutable-release settings may prevent replacement.
Source fixes after a public tag need a new version. Report partial publication;
do not automatically delete releases, roll back main, or change repo settings.

## Command references

- [Git atomic push](https://git-scm.com/docs/git-push)
- [GitHub release creation and tag verification](https://cli.github.com/manual/gh_release_create)
- [GitHub workflow dispatch](https://cli.github.com/manual/gh_workflow_run)
- [Events triggered with GITHUB_TOKEN](https://docs.github.com/en/actions/how-tos/write-workflows/choose-when-workflows-run/trigger-a-workflow)
