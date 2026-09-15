# Gmail

Use `@submilli/gmail` to inspect and triage mail, work with threads and drafts,
send or reply to messages, manage labels, and download attachments.

It supports profiles, bounded thread search and triage, normalized nested MIME
messages, drafts, sending and replying, labels, and attachment downloads to the VFS.
Outgoing bodies are capped at 1 MiB and combined attachment contents at 10 MiB.

The read surface is `getProfile`, `searchThreads`, `triage`, `getThread`,
`getMessage`, `listDrafts`, `getDraft`, and `listLabels`. Draft and send operations
are `createDraft`, `createReplyDraft`, `deleteDraft`, `sendEmail`, `reply`, and
`sendDraft`. Label and attachment workflows use `createLabel`,
`modifyThreadLabels`, `modifyMessageLabels`, and `downloadAttachment`.

`getThread` and `getMessage` normalize nested MIME parts into text, HTML,
headers, and attachment metadata. Use `downloadAttachment` only with a message
and attachment ID returned by Gmail, and choose a deliberate VFS destination.

Prefer creating a draft when human review is appropriate. `reply` and
`createReplyDraft` resolve threading headers and reply recipients from the
source message; inspect the declared recipient fields before sending. Label
changes add and remove only the explicitly listed label IDs.

Credentials are supplied internally. Never request, accept, or pass an access
token in package calls. Failures throw `GmailError`; do not blindly retry sends
when the outcome is uncertain.

## Example

Summarize unread threads: search returns lightweight refs; fetch the full
thread only for the ones you need.

```ts
import gmail from "@submilli/gmail";

function main(): string {
    const threads = gmail.searchThreads("is:unread newer_than:2d", { limit: 5 });
    if (threads.items.length === 0) return "No unread mail.";
    const lines: string[] = [];
    for (const ref of threads.items) {
        const thread = gmail.getThread(ref.id);
        if (thread === null) continue;
        const count = thread.messages.length;
        lines.push(ref.snippet + " (" + count.toString() + " messages)");
    }
    return lines.join("\n");
}
```
