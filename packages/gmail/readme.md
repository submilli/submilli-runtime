# @submilli/gmail

Synchronous Gmail helpers for profiles, search, triage, messages, threads,
drafts, sending, labels, and VFS attachment downloads.

## Google and blueprint setup

Enable the Gmail API for the OAuth client. For the full package surface,
request:

```text
https://www.googleapis.com/auth/gmail.modify
```

Use `https://www.googleapis.com/auth/gmail.readonly` for reads,
`https://www.googleapis.com/auth/gmail.compose` for drafts and sending, or
`https://www.googleapis.com/auth/gmail.send` for send-only deployments.

Declare the package and harness-bound access token:

```yaml
packages:
  - "@submilli/gmail"

secrets:
  GOOGLE_ACCESS_TOKEN:
    harness:
      required: true
```

Bind the OAuth access token as `GOOGLE_ACCESS_TOKEN` when creating the harness
session. The package does not refresh tokens, so the harness must replace
expired tokens. It reads the token through `submilli:secrets`; no `auth_proxy`
rule is needed.

Run `submilli blueprint add-package @submilli/gmail` to scaffold the package's
permissions, then review the generated operation grants.

## Recipient policy

`sendEmail`, `createDraft`, `reply`, `createReplyDraft`, and `sendDraft` each
report one field, `recipients`: every address the message goes to, across To,
Cc, and Bcc, with each address once.

| Operation | `recipients` |
| --- | --- |
| `sendEmail`, `createDraft` | The `to`, `cc`, and `bcc` inputs |
| `reply`, `createReplyDraft` | The To and Cc resolved from the original message |
| `sendDraft` | The To, Cc, and Bcc headers of the stored draft, read before the check |

```yaml
permissions:
  main:
    - capability: submilli/gmail.sendEmail
      filter: not (recipients contains "legal@example.com")
      action: allow
```

Addresses are bare. `to`, `cc`, and `bcc` take one address per entry; an entry
with a display name, angle brackets, a comma, a line break, or other address
syntax is rejected with `invalid_recipient` before the check. Spaces and tabs
around an entry are removed; any other whitespace, inside or around the
address, is rejected the same way.
An address holds printable ASCII characters only, so an internationalized
address, in an entry or in a message or draft header, is rejected the same way.
Replies take the bare addresses from the original message's headers and drop
display names. The message headers are written from the same lists the check
reported.

A filter compares each address exactly as the message carries it, including
letter case: `Legal@example.com` does not equal `legal@example.com`, although
most mail systems deliver both to one mailbox.

`sendDraft` sends the draft Gmail stores, so its check reads that draft's
headers. A draft whose To, Cc, or Bcc header is not a plain list of `address`
or `Name <address>` is refused with `invalid_recipient` and is not sent.

## Development

Run:

```bash
submilli build test -p @submilli/gmail
```

Live reads run when `.env` contains `GOOGLE_ACCESS_TOKEN`. The send test also
requires `GOOGLE_LIVE_MUTATIONS=true` and an explicitly approved
`GOOGLE_TEST_EMAIL_RECIPIENT`.

## Policy tests

The TypeScript policy tests in `tests/policy/` run as a real `main` caller under
restricted blueprints. Each script has a blueprint of the same name:

- `recipients.ts` shows that a blocked address is denied in To, in Cc, and in
  Bcc; that an allowed message reaches a deliberately denied credential
  boundary; that an entry holding a comma, a line break, or a display name is
  rejected; and that `to` is read once even when it answers differently on each
  read.
- `request-values.ts` shows that the message the package builds goes to the
  recipients the policy approved. The package holds a placeholder token from
  `fake-token.txt`, and the blueprint tells the message to the allowed list
  from a longer one by the size of the request body.

These tests need no real token or network. Run from the repository root in an
isolated local package store:

```sh
gmail_test_home=$(mktemp -d)
SUBMILLI_HOME="$gmail_test_home" cargo run -p submilli -- build publish-local -p @submilli/gmail
SUBMILLI_HOME="$gmail_test_home" cargo run -p submilli -- run packages/gmail/tests/policy/recipients.ts --blueprint packages/gmail/tests/policy/recipients.yaml
SUBMILLI_HOME="$gmail_test_home" cargo run -p submilli -- run packages/gmail/tests/policy/request-values.ts --blueprint packages/gmail/tests/policy/request-values.yaml
```

These are separate commands because `build test` uses an unrestricted policy;
its ordinary unit tests cannot prove a `main` caller is constrained.
`cargo test -p submilli --test package_policy` runs them all.
