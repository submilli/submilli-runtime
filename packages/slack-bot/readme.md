# @submilli/slack-bot

A Slack Web API client for bot-owned messages, conversations, users, direct
messages, and reactions. Inbound events and OAuth installation remain the
responsibility of the surrounding service.

## Blueprint and harness setup

```yaml
packages:
  - "@submilli/slack-bot"

secrets:
  SLACK_BOT_TOKEN:
    harness:
      required: true
```

Bind the bot OAuth token as `SLACK_BOT_TOKEN` when creating the harness
session. The package reads it through `submilli:secrets`; no `auth_proxy` rule
is needed.

Run `submilli blueprint add-package @submilli/slack-bot` to add the package and
scaffold its permissions. Review the generated `slack.com/bot/*` grants and
allow only the operations the program needs. The namespace already identifies
the bot context, so capability payloads contain only fields operators can
meaningfully filter, currently conversation and user identifiers.

Give the Slack app only the bot-token scopes required by those operations.
Typical scopes include conversation read/history scopes, `users:read`,
`chat:write`, `reactions:write`, `im:write`, and `mpim:write`. The bot must be a
member of private conversations and generally must join channels it reads or
writes.

## Message metadata

New posts can carry Slack message metadata from the session variable
`SLACK_BOT_MESSAGE_METADATA`. The package reads it through `submilli:variables`.
`submilli blueprint add-package` scaffolds `variables.get` for that name.
Existing blueprints must add the grant too: without it, every new post fails
with `PermissionDeniedError`, even when the variable is unset. When the grant
is present and the variable is unset, posts omit metadata and match the
previous request.

The value is JSON in Slack's generic shape, and nothing else:

```json
{"event_type":"task_created","event_payload":{"id":"11223","title":"Redesign Homepage"}}
```

`event_type` is a non-empty string of letters, digits, underscores, and hyphens.
`event_payload` is a JSON object. The package does not interpret the payload.
An empty or differently shaped value throws `SlackError` with code
`invalid_metadata` before any Slack request. `updateMessage` does not send
metadata, so Slack keeps whatever is already stored on the message.

```yaml
variables:
  SLACK_BOT_MESSAGE_METADATA: {}

permissions:
  '@submilli/slack-bot':
    - capability: variables.get
      filter: name == "SLACK_BOT_MESSAGE_METADATA"
      action: allow
```

## Development

Run:

```bash
submilli build test -p @submilli/slack-bot
```

Put `SLACK_BOT_TOKEN` in `.env` to enable live read tests.
`SLACK_TEST_CHANNEL` enables the message-lifecycle test in an approved channel.
`SLACK_TEST_DM_USER_ID` enables the 1:1 DM test, and the comma-separated
`SLACK_TEST_GROUP_DM_USER_IDS` enables the group-DM test.

## Policy tests

The scripts in `tests/policy/` run as a real `main` caller under restricted
blueprints of the same name, without a token or network:

- `user-ids.ts` shows that a blocked user ID is denied alone and inside a
  list, and that one argument holding a comma, a space, or a line break is
  rejected with `invalid_user_id` before the check.
- `metadata-malformed.ts` shows that a malformed `SLACK_BOT_MESSAGE_METADATA`
  is rejected on every send path, including a thread reply, before a token is
  read, and that `updateMessage` still reaches `secrets.get`.
- `metadata-valid.ts` and `metadata-absent.ts` show that a valid value and an
  unset variable both reach `secrets.get` on every send path.
- `metadata-denied.ts` shows that a missing `variables.get` grant denies every
  send path even when the variable is set, and that `updateMessage` does not
  read it.

`cargo test -p submilli --test package_policy` runs them.
