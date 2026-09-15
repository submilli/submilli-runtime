# Slack bot

Use `@submilli/slack-bot` for bot-owned Slack work: send, update, and delete bot
messages; read conversation history and threads; list conversations and
members; open direct messages; read users; and add or remove reactions.

Bot operations use the `slack.com/bot/*` semantic capability namespace. The
namespace already identifies the bot context, so capability payloads contain
only fields operators can meaningfully filter, currently conversation and user
identifiers.

Use `sendDirectMessage(userId, text)` for a one-step 1:1 DM. It opens or
resumes the DM and posts the message. Use `openDirectMessage(userId)` when the
caller only needs the 1:1 conversation ID.
Use `sendGroupDirectMessage(userIds, text)` to open or resume an MPIM with 2–8
recipients and send in one step.
`openGroupDirectMessage(userIds)` opens the same MPIM without sending.

Only update or delete messages authored by the bot. The package has no
inbound-event loop, scheduled-message API, views, file upload, or admin API.

Credentials are supplied internally. Never request, accept, or pass a Slack
token in package calls. Failures throw `SlackError` with Slack's
machine-readable `code` and HTTP `status`. The package never sleeps or retries
automatically.

## Example

Post a status update and reply in its thread.

```ts
import slack from "@submilli/slack-bot";

function main(): string {
    const root = slack.sendMessage({ channelId: "C0123456789", text: "Deploy finished." });
    slack.sendMessage({
        channelId: "C0123456789",
        text: "All 214 checks green.",
        threadTs: root.ts,
    });
    return "Posted thread at " + root.ts;
}
```
