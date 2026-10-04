# Publishing the chart

The runtime `Release` workflow publishes the chart from the same commit as the
runtime binaries and image. After the image is published, it calls `Chart Release`,
which calls `Chart CI` to validate the chart under Helm 3 and 4, install it into kind, check upgrades
and run its test hooks. Those installs use the public runtime image with no
pull secret. Only then does the release publish the chart to GHCR.

Chart versions are independent of runtime versions. Every runtime release bumps
`Chart.yaml`'s `version` and sets `appVersion` to the new runtime version.
Between runtime releases, bump only `version` for chart fixes and keep
`appVersion` pointing at the existing runtime. Publication stops if that image
cannot be read anonymously.

To publish a chart independently, merge the version bump and chart changes to
`main`, then dispatch the same chart release workflow:

```sh
gh workflow run chart-release.yml --repo submilli/submilli-runtime --ref main
```

This publishes the chart from main without creating a runtime tag or rebuilding
the runtime. Main pushes validate chart changes but do not publish them.

The publication job serializes writes to `ghcr.io/submilli/charts/submilli`.
An existing version with identical packaged files is reused. Different files
under the same version fail the job, and require a version bump. Authentication
and registry outages fail the job without being treated as an absent version.
Do not manually overwrite chart tags.

After publication, a separate runner installs the exact chart version from OCI
and runs `helm test`. That runner has no registry login or image pull secret.
A green publication job alone does not establish that the install passed.

## First public publication

The workflow cannot make a new GHCR package public. GitHub creates packages
private by default, and visibility is configured separately from repository
visibility. These steps need a package administrator:

1. Publish the runtime selected by `appVersion` using the runtime release
   workflow. In the organization's **Packages**, open `submilli-runtime`, then
   **Package settings**, and change its visibility to **Public**.
2. Enable `Chart CI` if it is disabled. After merging these workflow changes,
   publish the runtime release through the release workflow. To resume an
   already published runtime release, set `release_tag` to that published tag and dispatch it:

   ```sh
   gh workflow enable chart-ci.yml --repo submilli/submilli-runtime
   gh workflow run release.yml --repo submilli/submilli-runtime -f "tag=$release_tag"
   ```

   Use a tag containing the chart publication scripts. Do not move an existing
   runtime tag to add them. For a runtime released before these changes,
   use the standalone chart release command above after merging to main.

3. The first upload may fail its anonymous download check because the new
   `charts/submilli` container package is private. Open that package's settings
   and change it to **Public**. Ensure this repository has Actions write access
   to the package. Rerun the failed release workflow using the same source. It
   compares the existing chart, leaves identical content unchanged, then checks
   anonymous installation.
4. Require successful `install published chart without registry credentials`
   results. Keep the chart and image public for subsequent versions.

GitHub's [package visibility instructions](https://docs.github.com/en/packages/learn-github-packages/configuring-a-packages-access-control-and-visibility)
describe the administrator controls. A public container package supports
anonymous pulls. Once public, a package cannot be made private again.

## Launch checks

On a machine with no registry credentials and a Kubernetes cluster whose CNI
enforces NetworkPolicy, run:

```sh
helm install submilli oci://ghcr.io/submilli/charts/submilli --version 0.3.4 --wait
helm test submilli --logs
```

The chart's API hook checks authenticated execution and its network hook checks
that a pod outside the allowlist cannot reach the server. A successful download
does not replace either check.

Also replay the public book's [Compose setup](https://submilli.ai/docs/server/deploy-with-compose)
in a scratch directory. Download `compose.yaml` from the published runtime tag,
set `SUBMILLI_SERVER_TOKEN`, and run `docker compose up -d --wait` with
`SUBMILLI_IMAGE` unset to exercise the default public image. Keep registry
credentials out of that Docker configuration. Run `docker compose down -v`
only against this disposable test project when finished.

Record the chart version, runtime tag, workflow run and actual command results
in SUB-959 before declaring launch verification complete. Never substitute
authenticated pulls or a locally loaded image for anonymous delivery checks.
