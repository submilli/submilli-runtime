# @submilli/linear

A synchronous, typed Linear GraphQL client for issues, comments, teams,
projects, and users.

## Blueprint setup

Add the installed package and declare the token as a required harness secret:

```yaml
packages:
  - "@submilli/linear"

secrets:
  LINEAR_API_KEY:
    harness:
      required: true
```

`submilli blueprint add-package @submilli/linear` adds the package and
scaffolds its permissions. Review them so the main program can call only the
`linear.app/*` operations it needs and the package can reach Linear and read
`LINEAR_API_KEY`.

Create a personal API key in Linear's API settings and bind it as
`LINEAR_API_KEY` when creating the harness session. The package reads the
secret directly; no `auth_proxy` rule is needed.

## Development

Run:

```bash
submilli build test -p @submilli/linear
```

Put `LINEAR_API_KEY` in the repository `.env` to enable the live read test.

## Agent interaction tests

The package also supports agent sessions, activities, plans, external links, and
threaded comments. Use an **app OAuth access token** (authorized with `actor=app`)
as `LINEAR_API_KEY` for agent operations; the package adds the Bearer prefix.
The harness can bind its managed `linear` OAuth grant directly to this secret.
Personal API keys continue to work for ordinary issue and comment operations.

Offline HTTP-contract tests (Node 22.13+ or Node 24) exercise the real wrappers
with mocked host functions, checking capabilities, variables, responses, failures,
and authentication:

```sh
node --test packages/linear/scripts/contract.test.mjs
```

To additionally validate every new GraphQL operation against Linear's published
schema, download the schema linked from the developer documentation and install
`graphql` into a temporary directory, then set `LINEAR_SCHEMA_PATH` to that SDL
file and `GRAPHQL_MODULE_PATH` to the absolute path of `graphql/index.js` when
running the same command. This validates query shapes; it does not exercise live
permissions or webhook delivery.

The native package tests compile all operations and documentation examples. A live
mutation smoke test is opt-in. Bind these through the test environment or `.env`:

- `LINEAR_API_KEY`: an app OAuth access token for a test workspace.
- `LINEAR_LIVE_MUTATIONS=true`.
- `LINEAR_TEST_ISSUE_ID`: a disposable issue UUID visible to the app.

Then run `cargo run -p submilli -- build test -p @submilli/linear`. The smoke test
creates sessions on the issue and on a comment, posts a threaded reply, records
activities, and reads back session state, links, plan, comments, and activities.
It leaves the comments and completed sessions in Linear for inspection. Session
creation may also reach your subscribed webhook handler; use a test app without
an active worker when testing the package in isolation. Testing real `created`,
`prompted`, and `stop` delivery requires the webhook integration separately.

API references: [agent interactions](https://linear.app/developers/agent-interaction)
and [signals](https://linear.app/developers/agent-signals).
