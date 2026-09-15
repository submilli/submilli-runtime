# Google Drive

Use `@submilli/google-drive` to find, inspect, read, download, upload, organize,
and share files in My Drive or Shared Drives.

The API covers structured search, recent files, text reads, streaming downloads,
8 MiB-chunk resumable VFS uploads, folders, copies, moves, trash/restore, and
permissions. Shared Drive flags are applied throughout. Native Google files require
an explicit export MIME type for downloads.

The read surface is `getFile`, `searchFiles`, `listRecentFiles`, `readText`,
`downloadFile`, and `listPermissions`. File changes use `uploadFile`,
`createFolder`, `copyFile`, `renameFile`, `moveFile`, `trashFile`, and
`restoreFile`; sharing uses `shareFile` and `removePermission`.

There is intentionally no permanent-delete operation: `trashFile` and `restoreFile`
are the supported lifecycle actions.

Prefer structured search fields over a raw Drive query. Use a Shared Drive ID
when the task targets a Shared Drive. `readText` reads ordinary UTF-8 text files
and exports Google Docs as text; `downloadFile` requires an explicit export MIME
type for other native Google files.

Sharing creates one user, group, domain, or anyone permission at a time. Confirm
the principal and role before calling it. Credentials are supplied internally;
never request, accept, or pass an access token in package calls. Failures throw
`DriveError`; do not blindly retry mutations when the outcome is uncertain.

## Example

Find files by name and report when they last changed.

```ts
import drive from "@submilli/google-drive";

function main(): string {
    const page = drive.searchFiles({ nameContains: "quarterly report", limit: 5 });
    if (page.items.length === 0) return "No matching files.";
    const lines: string[] = [];
    for (const file of page.items) {
        lines.push(file.name + "  (" + file.mimeType + ", modified " + file.modifiedTime + ")");
    }
    return lines.join("\n");
}
```
