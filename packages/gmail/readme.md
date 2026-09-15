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

## Development

Run:

```bash
submilli build test -p @submilli/gmail
```

Live reads run when `.env` contains `GOOGLE_ACCESS_TOKEN`. The send test also
requires `GOOGLE_LIVE_MUTATIONS=true` and an explicitly approved
`GOOGLE_TEST_EMAIL_RECIPIENT`.
