# Gmail

Use `@submilli/gmail` to:

<!-- submilli:if any="submilli/gmail.getProfile submilli/gmail.searchThreads submilli/gmail.triage submilli/gmail.getThread submilli/gmail.getMessage" -->
- inspect and triage mail
<!-- submilli:endif -->
<!-- submilli:if any="submilli/gmail.listDrafts submilli/gmail.getDraft submilli/gmail.createDraft submilli/gmail.createReplyDraft submilli/gmail.deleteDraft" -->
- work with drafts
<!-- submilli:endif -->
<!-- submilli:if any="submilli/gmail.sendEmail submilli/gmail.reply submilli/gmail.sendDraft" -->
- send or reply to messages
<!-- submilli:endif -->
<!-- submilli:if any="submilli/gmail.listLabels submilli/gmail.createLabel submilli/gmail.modifyThreadLabels submilli/gmail.modifyMessageLabels" -->
- manage labels
<!-- submilli:endif -->
<!-- submilli:if all="submilli/gmail.downloadAttachment fs.write" -->
- download attachments to the VFS
<!-- submilli:endif -->

<!-- submilli:if any="submilli/gmail.createDraft submilli/gmail.createReplyDraft submilli/gmail.sendEmail submilli/gmail.reply" -->

Outgoing bodies are capped at 1 MiB and combined attachment contents at 10 MiB.

<!-- submilli:endif -->

<!-- submilli:if any="submilli/gmail.getThread submilli/gmail.getMessage" -->

These operations normalize nested MIME parts into text, HTML, headers, and
attachment metadata:

<!-- submilli:if capability="submilli/gmail.getThread" -->
- `getThread`
<!-- submilli:endif -->
<!-- submilli:if capability="submilli/gmail.getMessage" -->
- `getMessage`
<!-- submilli:endif -->

<!-- submilli:endif -->

<!-- submilli:if all="submilli/gmail.downloadAttachment fs.write" -->

Use `downloadAttachment` only with a message and attachment ID returned by
Gmail, and choose a deliberate VFS destination.

<!-- submilli:endif -->

<!-- submilli:if any="submilli/gmail.createDraft submilli/gmail.createReplyDraft" -->

Prefer creating a draft when human review is appropriate.

<!-- submilli:endif -->

<!-- submilli:if any="submilli/gmail.reply submilli/gmail.createReplyDraft" -->

These operations resolve threading headers and reply recipients from the
source message:

<!-- submilli:if capability="submilli/gmail.reply" -->
- `reply`
<!-- submilli:endif -->
<!-- submilli:if capability="submilli/gmail.createReplyDraft" -->
- `createReplyDraft`
<!-- submilli:endif -->

Inspect the declared recipient fields before sending.

<!-- submilli:endif -->

<!-- submilli:if any="submilli/gmail.modifyThreadLabels submilli/gmail.modifyMessageLabels" -->

Label changes add and remove only the explicitly listed label IDs.

<!-- submilli:endif -->

Credentials are supplied internally. Never request, accept, or pass an access
token in package calls. Failures throw `GmailError`.

<!-- submilli:if any="submilli/gmail.sendEmail submilli/gmail.reply submilli/gmail.sendDraft" -->

Do not blindly retry sends when the outcome is uncertain.

<!-- submilli:endif -->

<!-- submilli:if any="submilli/gmail.createDraft submilli/gmail.createReplyDraft submilli/gmail.sendEmail submilli/gmail.reply submilli/gmail.sendDraft" -->

## Recipients

Pass one bare address per entry in `to`, `cc`, and `bcc`: `"dana@example.com"`,
never `"Dana <dana@example.com>"` and never several addresses in one string.
Spaces and tabs around an entry are removed, and no other whitespace. An
address holds printable ASCII characters only, so an internationalized address,
in a message or draft header as well, is refused with `invalid_recipient`. Any
other entry throws `GmailError` with code `invalid_recipient` before anything
is checked or sent, and the message names the field. Addresses are checked and
sent in lowercase.

`from`, when set, is one bare address too; anything else throws
`invalid_sender`. Policy sees it as `from`, which is null when unset. An
attachment `filename` cannot contain a double quote or a backslash.

Policy sees every address a message goes to as one `recipients` list, in the
order To, Cc, Bcc, with each address once:

| Operation | `recipients` |
| --- | --- |
<!-- submilli:if capability="submilli/gmail.sendEmail" -->
| `sendEmail` | `to`, `cc`, and `bcc` |
<!-- submilli:endif -->
<!-- submilli:if capability="submilli/gmail.createDraft" -->
| `createDraft` | `to`, `cc`, and `bcc` |
<!-- submilli:endif -->
<!-- submilli:if capability="submilli/gmail.reply" -->
| `reply` | The resolved To and Cc |
<!-- submilli:endif -->
<!-- submilli:if capability="submilli/gmail.createReplyDraft" -->
| `createReplyDraft` | The resolved To and Cc |
<!-- submilli:endif -->
<!-- submilli:if capability="submilli/gmail.sendDraft" -->
| `sendDraft` | The To, Cc, and Bcc headers of the stored draft |
<!-- submilli:endif -->

<!-- submilli:if any="submilli/gmail.reply submilli/gmail.createReplyDraft" -->

A reply's To is the original Reply-To, or From when there is none. Reply-all
adds the original To, and its Cc holds the original Cc addresses that are not
already in To. Replies are addressed to bare addresses, without display names.

<!-- submilli:endif -->

<!-- submilli:if any="submilli/gmail.reply submilli/gmail.createReplyDraft submilli/gmail.sendDraft" -->

These operations read the source message or draft from Gmail before the
check:

<!-- submilli:if capability="submilli/gmail.reply" -->
- `reply`
<!-- submilli:endif -->
<!-- submilli:if capability="submilli/gmail.createReplyDraft" -->
- `createReplyDraft`
<!-- submilli:endif -->
<!-- submilli:if capability="submilli/gmail.sendDraft" -->
- `sendDraft`
<!-- submilli:endif -->

They throw `invalid_recipient` when one of its
address headers is not a plain list of `address` or `Name <address>`, and
`not_found` when it does not exist. Because that read comes first, a program
the policy denies can still tell whether the message or draft exists, and a
draft edited between the read and the send goes out as edited.

<!-- submilli:endif -->

<!-- submilli:endif -->

<!-- submilli:if all="submilli/gmail.searchThreads submilli/gmail.getThread" -->

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

<!-- submilli:endif -->

<!-- submilli:if all="submilli/gmail.downloadAttachment fs.write" -->

Downloads also require the caller's `fs.write { path, max_bytes }` permission.
The default download limit is 20 MB of decoded attachment bytes.

<!-- submilli:endif -->
