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

## Policy tests

The scripts in `tests/policy/` run as a real `main` caller under restricted
blueprints of the same name, without a token or network:

- `sharing.ts` shows that `shareFile` is held to rules on `type`, `role`,
  `principal`, `sendNotificationEmail`, and `allowFileDiscovery`; that a second
  principal beside the checked one is rejected; that `emailAddress` is read
  once; and that `uploadFile`, `copyFile`, and `moveFile` are held to a rule on
  `parentId`.

`cargo test -p submilli --test package_policy` runs them.

## Upload destination policy

`uploadFile` resolves its parent folder before checking `path`, `parentId` and
`driveId`. The check's `driveId` is the folder's actual shared-drive ID, or null for
My Drive. An explicit `driveId` must match the resolved destination; it no longer
serves only as a shared-drive support hint. Omitted parents and `root` resolve My
Drive root. Missing/non-folder parents and mismatched drives fail before starting
a resumable upload. The metadata lookup requests only folder MIME type and drive ID using native
package HTTP/secret grants;
caller upload denial prevents the session request and transfer.

```sh
node --test packages/google-drive/scripts/contract.test.mjs
```
