# GHCR: fix the push, then retire the old versions

The workflow is fine — it already has `packages: write` and login succeeded. The
`submilli` package is still linked to `submilli-private`, so `submilli-public`
isn't on its access list. (`read_package` is GHCR's confusing way of saying that.)

Package settings are all in one place:
<https://github.com/orgs/submilli/packages/container/package/submilli> →
**Package settings**.

## 1. Fix the push

1. **Manage Actions access** → **Add repository** → `submilli/submilli-public`
2. Set the role to **Write** — it defaults to Read, which fails the same way
3. Re-run: `gh run rerun 34978437490 --repo submilli/submilli-public --failed`

That unblocks CI. Package stays private.

## 2. Fix the source label

`Dockerfile:39` hardcodes `image.source` as `github.com/submilli/submilli` —
that's someone else's repo (`SubMilliTech/SubMilliTech`). Change it to
`submilli-public` so the next release is the first image with a correct label.

## 3. Decide the version number ⚠️

`Cargo.toml` is at `0.1.4` and `0.1.4` is already in GHCR, but the only tag is
`v0.1.0`. So the release publishes an older number over a newer one — and
`latest` moves to it, because `latest=auto` tags whatever the current build is.

The chart also can't go below `0.1.4`: `Chart.yaml:12` says that's the first
release with `--blueprint-seed-dir`, which `configmap-blueprints.yaml` needs.
Below it the server boots fine and silently ignores seeded blueprints.

**Suggest releasing `v0.1.5`** so every number moves forward. Steps 4–6 assume that.

## 4. Release `v0.1.5`

Publishes `0.1.5` + `0.1`, and takes `latest`.

## 5. Point the chart at it

`Chart.yaml` → `appVersion: "0.1.5"` (`values.yaml` has `tag: ""`, so it follows
appVersion automatically). Update the floor comment at `Chart.yaml:12` too.

Let chart-ci go green — that's the proof nothing still needs `0.1.4`.

## 6. Delete the old versions

**Manage versions** → **⋯** → *Delete version*. Delete the tagged ones first;
the untagged entries are their per-arch manifests.

| Version ID | Tags |
|---|---|
| 1128183608 | `0.1.4` |
| 1128183594 | *(arch child)* |
| 1128183573 | *(arch child)* |
| 1106722974 | `0.0.0-dockertest.1` |
| 1106722957 | *(arch child)* |
| 1106722940 | *(arch child)* |

Or via CLI — needs a scope refresh first, there's no `gh package` command:

```sh
gh auth refresh -h github.com -s delete:packages
for id in 1128183608 1128183594 1128183573 1106722974 1106722957 1106722940; do
  gh api --method DELETE "/orgs/submilli/packages/container/submilli/versions/$id"
done
```

Do this **after** step 5, not before — `0.1.4` is what the chart and chart-ci
currently pull.

This also clears the old images' stale labels, which point at
`submilli-ai` (redirects to `submilli-private`) and a commit that isn't in public
history.

## 7. Going public (when you're ready)

**Change visibility → Public** on the same settings page. Until then the push
works but nobody outside the org can pull. `values.yaml:23` and
`charts/submilli/README.md:13` have "private until launch" notes to update.
