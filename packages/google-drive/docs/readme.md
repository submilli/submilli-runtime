# Google Drive

Use `@submilli/google-drive` to work with files in My Drive or Shared Drives:

<!-- submilli:if capability="submilli/google-drive.searchFiles" -->
- find files with structured search
<!-- submilli:endif -->
<!-- submilli:if capability="submilli/google-drive.listRecentFiles" -->
- list recent files
<!-- submilli:endif -->
<!-- submilli:if capability="submilli/google-drive.getFile" -->
- inspect files
<!-- submilli:endif -->
<!-- submilli:if capability="submilli/google-drive.readText" -->
- read text
<!-- submilli:endif -->
<!-- submilli:if all="submilli/google-drive.downloadFile fs.write" -->
- stream downloads to the VFS
<!-- submilli:endif -->
<!-- submilli:if capability="submilli/google-drive.uploadFile" -->
- upload VFS files with 8 MiB-chunk resumable uploads
<!-- submilli:endif -->
<!-- submilli:if capability="submilli/google-drive.createFolder" -->
- create folders
<!-- submilli:endif -->
<!-- submilli:if capability="submilli/google-drive.copyFile" -->
- copy files
<!-- submilli:endif -->
<!-- submilli:if capability="submilli/google-drive.renameFile" -->
- rename files
<!-- submilli:endif -->
<!-- submilli:if capability="submilli/google-drive.moveFile" -->
- move files
<!-- submilli:endif -->
<!-- submilli:if capability="submilli/google-drive.trashFile" -->
- trash files
<!-- submilli:endif -->
<!-- submilli:if capability="submilli/google-drive.restoreFile" -->
- restore files
<!-- submilli:endif -->
<!-- submilli:if capability="submilli/google-drive.listPermissions" -->
- list permissions
<!-- submilli:endif -->
<!-- submilli:if capability="submilli/google-drive.shareFile" -->
- share files
<!-- submilli:endif -->
<!-- submilli:if capability="submilli/google-drive.removePermission" -->
- remove permissions
<!-- submilli:endif -->

Shared Drive flags are applied throughout.

<!-- submilli:if all="submilli/google-drive.downloadFile fs.write" -->

Native Google files require an explicit export MIME type for downloads.

<!-- submilli:endif -->

<!-- submilli:if any="submilli/google-drive.trashFile submilli/google-drive.restoreFile" -->

There is intentionally no permanent-delete operation. The supported lifecycle
actions are:

<!-- submilli:if capability="submilli/google-drive.trashFile" -->
- `trashFile`
<!-- submilli:endif -->
<!-- submilli:if capability="submilli/google-drive.restoreFile" -->
- `restoreFile`
<!-- submilli:endif -->

<!-- submilli:endif -->

<!-- submilli:if capability="submilli/google-drive.searchFiles" -->

Prefer structured search fields over a raw Drive query. Use a Shared Drive ID
when the task targets a Shared Drive.

<!-- submilli:endif -->

<!-- submilli:if capability="submilli/google-drive.readText" -->

`readText` reads ordinary UTF-8 text files and exports Google Docs as text.

<!-- submilli:endif -->

<!-- submilli:if capability="submilli/google-drive.shareFile" -->

Sharing creates one user, group, domain, or anyone permission at a time. Set
only the fields the type uses: `emailAddress` for a user or group, `domain` for
a domain, neither for anyone. Any other combination throws `DriveError` with
code `invalid_permission_principal`. An email address or domain is sent in
lowercase. Confirm the principal and role before calling it.

<!-- submilli:endif -->

<!-- submilli:if any="submilli/google-drive.uploadFile submilli/google-drive.createFolder submilli/google-drive.copyFile submilli/google-drive.moveFile" -->

A parent folder is one Drive ID.

<!-- submilli:endif -->

Credentials are supplied internally; never request, accept, or pass an access
token in package calls. Failures throw `DriveError`.

<!-- submilli:if any="submilli/google-drive.uploadFile submilli/google-drive.createFolder submilli/google-drive.copyFile submilli/google-drive.renameFile submilli/google-drive.moveFile submilli/google-drive.trashFile submilli/google-drive.restoreFile submilli/google-drive.shareFile submilli/google-drive.removePermission" -->

Do not blindly retry mutations when the outcome is uncertain.

<!-- submilli:endif -->

<!-- submilli:if capability="submilli/google-drive.searchFiles" -->

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

<!-- submilli:endif -->

<!-- submilli:if all="submilli/google-drive.downloadFile fs.write" -->

Downloads also require the caller's `fs.write { path, max_bytes }` permission.
The default download limit is 20 MB (override with `FileDownloadOptions.maxBytes`).

<!-- submilli:endif -->
