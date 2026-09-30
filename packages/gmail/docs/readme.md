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
| `sendEmail`, `createDraft` | `to`, `cc`, and `bcc` |
| `reply`, `createReplyDraft` | The resolved To and Cc |
| `sendDraft` | The To, Cc, and Bcc headers of the stored draft |

A reply's To is the original Reply-To, or From when there is none. Reply-all
adds the original To, and its Cc holds the original Cc addresses that are not
already in To. Replies are addressed to bare addresses, without display names.

`reply`, `createReplyDraft`, and `sendDraft` read the source message or draft
from Gmail before the check. They throw `invalid_recipient` when one of its
address headers is not a plain list of `address` or `Name <address>`, and
`not_found` when it does not exist. Because that read comes first, a program
the policy denies can still tell whether the message or draft exists, and a
draft edited between the read and the send goes out as edited.

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
