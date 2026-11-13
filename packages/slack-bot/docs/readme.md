# Slack bot

Use `@submilli/slack-bot` for bot-owned Slack work:

<!-- submilli:if any="slack.com/bot/sendMessage slack.com/bot/sendDirectMessage slack.com/bot/sendGroupDirectMessage" -->
- send bot messages
<!-- submilli:endif -->
<!-- submilli:if capability="slack.com/bot/updateMessage" -->
- update bot messages
<!-- submilli:endif -->
<!-- submilli:if capability="slack.com/bot/deleteMessage" -->
- delete bot messages
<!-- submilli:endif -->
<!-- submilli:if any="slack.com/bot/listMessages slack.com/bot/getMessage slack.com/bot/getThread" -->
- read conversation history and threads
<!-- submilli:endif -->
<!-- submilli:if any="slack.com/bot/listConversations slack.com/bot/getConversation slack.com/bot/listMembers" -->
- list conversations and members
<!-- submilli:endif -->
<!-- submilli:if any="slack.com/bot/openDirectMessage slack.com/bot/openGroupDirectMessage" -->
- open direct messages
<!-- submilli:endif -->
<!-- submilli:if any="slack.com/bot/getUser slack.com/bot/listUsers" -->
- read users
<!-- submilli:endif -->
<!-- submilli:if capability="slack.com/bot/addReaction" -->
- add reactions
<!-- submilli:endif -->
<!-- submilli:if capability="slack.com/bot/removeReaction" -->
- remove reactions
<!-- submilli:endif -->
<!-- submilli:if capability="slack.com/bot/getIdentity" -->
- read the bot's own identity
<!-- submilli:endif -->

Each bot operation has its own `slack.com/bot/<operation>` capability, so a
permission denial means the operation is forbidden, not mis-called.

<!-- submilli:if capability="slack.com/bot/sendDirectMessage" -->

Use `sendDirectMessage(userId, text)` for a one-step 1:1 DM. It opens or
resumes the DM and posts the message.

<!-- submilli:endif -->

<!-- submilli:if capability="slack.com/bot/openDirectMessage" -->

Use `openDirectMessage(userId)` when the
caller only needs the 1:1 conversation ID.

<!-- submilli:endif -->

<!-- submilli:if capability="slack.com/bot/sendGroupDirectMessage" -->

Use `sendGroupDirectMessage(userIds, text)` to open or resume an MPIM with 2–8
recipients and send in one step.

<!-- submilli:endif -->

<!-- submilli:if capability="slack.com/bot/openGroupDirectMessage" -->

`openGroupDirectMessage(userIds)` opens the same MPIM without sending.

<!-- submilli:endif -->

<!-- submilli:if any="slack.com/bot/updateMessage slack.com/bot/deleteMessage" -->

Only update or delete messages authored by the bot.

<!-- submilli:endif -->

The package has no
inbound-event loop, scheduled-message API, views, file upload, or admin API.

Credentials are supplied internally. Never request, accept, or pass a Slack
token in package calls.

<!-- submilli:if any="slack.com/bot/sendDirectMessage slack.com/bot/openDirectMessage slack.com/bot/sendGroupDirectMessage slack.com/bot/openGroupDirectMessage" -->

A direct-message recipient is one Slack user ID such
as `U012ABCDEF`; anything else throws `SlackError` with code
`invalid_user_id`.

<!-- submilli:endif -->

Failures throw `SlackError` with Slack's
machine-readable `code` and HTTP `status`. The package never sleeps or retries
automatically.

<!-- submilli:if capability="slack.com/bot/sendMessage" -->

## Example

`sendMessage` accepts optional `blocks: Record<string, unknown>[]` for Block Kit
content. Keep `text` as fallback text for notifications and accessibility.
Blocks pass through unchanged; Slack validates their contents. Absent or empty
arrays omit the field. More than 50 blocks throws `SlackError` with code
`invalid_blocks` before sending.

Post a status update and reply in its thread.

```ts
import slack from "@submilli/slack-bot";

function main(): string {
    const root = slack.sendMessage({
        channelId: "C0123456789",
        text: "Deploy finished.",
        blocks: [
            { type: "header", text: { type: "plain_text", text: "Deploy finished" } },
            { type: "section", text: { type: "mrkdwn", text: "All 214 checks green." } },
        ],
    });
    slack.sendMessage({
        channelId: "C0123456789",
        text: "All 214 checks green.",
        threadTs: root.ts,
    });
    return "Posted thread at " + root.ts;
}
```

<!-- submilli:endif -->
