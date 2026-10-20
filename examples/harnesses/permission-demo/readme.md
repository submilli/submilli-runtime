# Harness demos

`basic-blueprint.yaml` registers `harness-demo` with a deny-by-default policy
and no Packages. Apply it with
`submilli server blueprint apply basic-blueprint.yaml`. The basic harness check
executes `export function main(): number { return 2 + 2; }` and returns `4`.
It needs no billing setup or model call.

The docs build publishes this basic Blueprint at
`/docs/examples/permission-demo/basic-blueprint.yaml`.

## Optional billing permission demo

`blueprint.yaml` grants `acme.com/charges.list` only for `cus_northwind`.
It needs the existing Quickstart `@acme/billing` Package, version 0.1.0, which
reads a static fixture without network access or credentials.

The docs build publishes this Blueprint and `billing-source.zip` at
`/docs/examples/permission-demo/`. The archive is assembled from
`examples/quickstart/submilli.toml`, `package/src/lib.ts`, and
`package/docs/readme.md`. Extract it into a new directory and run
`submilli build publish-local`, then register the downloaded Blueprint using
`submilli server blueprint apply blueprint.yaml` against your local server.
There is no separate registry or precompiled runtime-dependent artifact.

The shared test runs the download builder and starts a loopback server with a
temporary `SUBMILLI_HOME`. It verifies basic arithmetic before installing any
Package, then installs the billing Package and executes the same operation for
Northwind (allowed) and Initech (denied). It checks structured
runtime denial fields and an unfiltered control that can read Initech. No model
calls or external services are used.

```sh
SUBMILLI=/absolute/path/to/submilli \
SUBMILLI_SERVER=/absolute/path/to/submilli-server \
node docs-site/scripts/check-permission-demo.mjs
```

Run from the repository root. The main CI workflow runs this after the Quickstart
journey. The docs build checks downloaded sources against these fixtures.

Set `SUBMILLI_DEMO_DOWNLOAD_ORIGIN=http://127.0.0.1:4321` to test the built
preview's actual HTTP downloads, including native content types, before the
same extraction and runtime checks. The builder uses `zip`; validation and
manual extraction use `unzip`. Install them on the build/test host if they are not already available.
