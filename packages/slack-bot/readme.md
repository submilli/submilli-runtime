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
allow only the operations the program needs.

Give the Slack app only the bot-token scopes required by those operations.
Typical scopes include conversation read/history scopes, `users:read`,
`chat:write`, `reactions:write`, `im:write`, and `mpim:write`. The bot must be a
member of private conversations and generally must join channels it reads or
writes.

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

`cargo test -p submilli --test package_policy` runs them.
