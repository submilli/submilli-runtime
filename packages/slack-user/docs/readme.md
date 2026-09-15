# Slack user

Use `@submilli/slack-user` when work must be performed as the authenticated
Slack user. It supports real-time search, message and thread retrieval,
channels, users, files, sending messages, and adding reactions. Visibility and
attribution follow that user's Slack identity and workspace access.

Use `search()` for Slack's semantic/keyword Real-time Search API and cursor through `nextCursor`. `downloadFile()` streams a private Slack file directly to the VFS and only accepts download URLs on `files.slack.com`.

The read surface is `getIdentity`, `search`, `getMessage`, `listMessages`,
`getThread`, `getChannel`, `listChannels`, `getUser`, `listUsers`,
`findUserByEmail`, `getFile`, and `downloadFile`. Message references use a
channel ID plus Slack timestamp; include `threadTs` when reading a specific
reply.

Use `sendDirectMessage(userId, text)` for a one-step 1:1 DM. It opens or
resumes the DM and posts as the authenticated user. Use
`sendGroupDirectMessage(userIds, text)` for an MPIM with 2–8 recipients; do not
include the authenticated user.

User operations use the `slack.com/user/*` semantic capability namespace. The
namespace already identifies the user context, so capability payloads contain
only fields operators can meaningfully filter, such as channel, user, file, and
search identifiers. The write surface is `sendMessage()`,
`sendDirectMessage()`, `sendGroupDirectMessage()`, and `addReaction()`.

Credentials are supplied internally. Never request, accept, or pass a Slack
token in package calls. Failures throw `SlackError` with Slack's
machine-readable `code` and HTTP `status`. The package does not sleep or retry
on rate limits; retry only when the surrounding operation remains timely and
safe.

## Example

Search messages and report where the matches were said.

```ts
import slack from "@submilli/slack-user";

function main(): string {
    const page = slack.search("budget review after:2026-08-01", { limit: 5 });
    if (page.messages.length === 0) return "No matches.";
    const lines: string[] = [];
    for (const message of page.messages) {
        lines.push("#" + message.channelName + ": " + message.content);
    }
    return lines.join("\n");
}
```
