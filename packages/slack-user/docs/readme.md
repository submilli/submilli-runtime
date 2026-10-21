# Slack user

Use `@submilli/slack-user` when work must be performed as the authenticated
Slack user. Visibility and attribution follow that user's Slack identity and
workspace access. It supports:

<!-- submilli:if capability="slack.com/user/search" -->
- real-time search
<!-- submilli:endif -->
<!-- submilli:if any="slack.com/user/getMessage slack.com/user/listMessages slack.com/user/getThread" -->
- message and thread retrieval
<!-- submilli:endif -->
<!-- submilli:if any="slack.com/user/getChannel slack.com/user/listChannels" -->
- reading channels
<!-- submilli:endif -->
<!-- submilli:if any="slack.com/user/getUser slack.com/user/listUsers slack.com/user/findUserByEmail" -->
- reading users
<!-- submilli:endif -->
<!-- submilli:if capability="slack.com/user/getFile" -->
- reading file details
<!-- submilli:endif -->
<!-- submilli:if all="slack.com/user/downloadFile fs.write" -->
- downloading files
<!-- submilli:endif -->
<!-- submilli:if any="slack.com/user/sendMessage slack.com/user/sendDirectMessage slack.com/user/sendGroupDirectMessage" -->
- sending messages
<!-- submilli:endif -->
<!-- submilli:if capability="slack.com/user/addReaction" -->
- adding reactions
<!-- submilli:endif -->
<!-- submilli:if capability="slack.com/user/getIdentity" -->
- reading the authenticated user's identity
<!-- submilli:endif -->

<!-- submilli:if capability="slack.com/user/search" -->

Use `search()` for Slack's semantic/keyword Real-time Search API and cursor through `nextCursor`.

<!-- submilli:endif -->

<!-- submilli:if all="slack.com/user/downloadFile fs.write" -->

`downloadFile()` streams a private Slack file directly to the VFS and only accepts download URLs on `files.slack.com`.

<!-- submilli:endif -->

<!-- submilli:if any="slack.com/user/getMessage slack.com/user/addReaction" -->

Message references use a
channel ID plus Slack timestamp; include `threadTs` when reading a specific
reply.

<!-- submilli:endif -->

<!-- submilli:if capability="slack.com/user/sendDirectMessage" -->

Use `sendDirectMessage(userId, text)` for a one-step 1:1 DM. It opens or
resumes the DM and posts as the authenticated user.

<!-- submilli:endif -->

<!-- submilli:if capability="slack.com/user/sendGroupDirectMessage" -->

Use
`sendGroupDirectMessage(userIds, text)` for an MPIM with 2–8 recipients; do not
include the authenticated user.

<!-- submilli:endif -->

Each user operation has its own `slack.com/user/<operation>` capability, so a
permission denial means the operation is forbidden, not mis-called.

Credentials are supplied internally. Never request, accept, or pass a Slack
token in package calls.

<!-- submilli:if any="slack.com/user/sendDirectMessage slack.com/user/sendGroupDirectMessage" -->

A direct-message recipient is one Slack user ID such
as `U012ABCDEF`; anything else throws `SlackError` with code
`invalid_user_id`.

<!-- submilli:endif -->

Failures throw `SlackError` with Slack's
machine-readable `code` and HTTP `status`. The package does not sleep or retry
on rate limits; retry only when the surrounding operation remains timely and
safe.

<!-- submilli:if capability="slack.com/user/search" -->

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

<!-- submilli:endif -->

<!-- submilli:if all="slack.com/user/downloadFile fs.write" -->

Downloads also require the caller's `fs.write { path, max_bytes }` permission.
The default download limit is 20 MB.

<!-- submilli:endif -->
