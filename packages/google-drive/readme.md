# @submilli/google-drive

Synchronous Google Drive helpers for files, folders, Shared Drives, resumable
uploads, downloads, organization, trash/restore, and permissions.

## Google and blueprint setup

Enable the Google Drive API for the OAuth client. For the full package surface,
request:

```text
https://www.googleapis.com/auth/drive
```

Use `https://www.googleapis.com/auth/drive.readonly` for reads. The narrower
`https://www.googleapis.com/auth/drive.file` scope limits access to files the
app created or that the user explicitly opened with the app.

Declare the package and harness-bound access token:

```yaml
packages:
  - "@submilli/google-drive"

secrets:
  GOOGLE_ACCESS_TOKEN:
    harness:
      required: true
```

Bind the OAuth access token as `GOOGLE_ACCESS_TOKEN` when creating the harness
session. The package does not refresh tokens, so the harness must replace
expired tokens. It reads the token through `submilli:secrets`; no `auth_proxy`
rule is needed.

Run `submilli blueprint add-package @submilli/google-drive` to scaffold the
package's permissions, then review the generated operation grants.

## Development

Run:

```bash
submilli build test -p @submilli/google-drive
```

Live reads run when `.env` contains `GOOGLE_ACCESS_TOKEN`. The temporary upload
test also requires `GOOGLE_LIVE_MUTATIONS=true` and trashes its uploaded file
during cleanup.
