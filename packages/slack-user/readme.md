# @submilli/slack-user

A Slack Web API client that searches, reads, and acts as the authenticated
workspace user. This package is intended for per-user agent sessions.

## Blueprint and harness setup

```yaml
packages:
  - "@submilli/slack-user"

secrets:
  SLACK_USER_TOKEN:
    harness:
      required: true
```

Bind the user's OAuth token as `SLACK_USER_TOKEN` when creating the harness
session. The package reads it through `submilli:secrets`; no `auth_proxy` rule
is needed.

Run `submilli blueprint add-package @submilli/slack-user` to add the package
and scaffold its permissions. Review the generated `slack.com/user/*` grants
and allow only the operations the program needs.

The Slack app needs user-token scopes matching the selected operations. A broad
installation commonly includes search, conversation history/read, users,
files, chat, reactions, direct-message, and group-direct-message scopes.
`findUserByEmail` specifically needs `users:read.email`; direct messages need
`im:write` and `chat:write`; group direct messages need `mpim:write` and
`chat:write`.

## Development

Run:

```bash
submilli build test -p @submilli/slack-user
```

Put `SLACK_USER_TOKEN` in `.env` to enable live read tests. Set
`SLACK_TEST_CHANNEL` to an approved channel name to enable the channel
integration test.

## Policy tests

The scripts in `tests/policy/` run as a real `main` caller under restricted
blueprints of the same name, without a token or network:

- `user-ids.ts` shows that a blocked user ID is denied alone and inside a
  list, and that one argument holding a comma, a space, or a line break is
  rejected with `invalid_user_id` before the check.

`cargo test -p submilli --test package_policy` runs them.

## Download destination policy

Downloads check the caller's `fs.write { path, max_bytes }` before credentials
or remote requests. Grant `main` a write rule for the intended VFS folder; this
check normalizes relative paths and `..` segments. The package's download
capability keeps its original `path` field for existing filters.

Slack downloads pass a 20 MB limit to the transfer.

`tests/policy/download-path.ts` verifies caller attribution and normalized paths
without credentials or network, under its matching blueprint.

Offline transfer and size-boundary contract checks run with Node 24's native
base64 feature enabled:

```sh
node --js-base-64 --test packages/slack-user/scripts/*.test.mjs
```
