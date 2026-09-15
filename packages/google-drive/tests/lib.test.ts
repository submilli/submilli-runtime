import { label } from "submilli:test";
import {
    DriveError,
    SearchFilesOptions,
    FileUploadOptions,
    FileDownloadOptions,
    ShareFileInput,
} from "@submilli/google-drive";

function main(): void {
    label("DriveError preserves stable fields");
    const error = new DriveError("storageQuotaExceeded", "quota", 403);
    assert(error.code === "storageQuotaExceeded", "code is preserved");
    assert(error.status === 403, "status is preserved");

    label("public inputs model Shared Drives and VFS workflows");
    const search: SearchFilesOptions = {
        nameContains: "report",
        parentId: "folder1",
        driveId: "shared1",
        limit: 20,
    };
    const upload: FileUploadOptions = {
        name: "report.pdf",
        mimeType: "application/pdf",
        parentId: "folder1",
        driveId: "shared1",
    };
    const download: FileDownloadOptions = { overwrite: false, maxBytes: 1048576 };
    const share: ShareFileInput = { type: "user", role: "reader", emailAddress: "person@example.com" };
    assert(search.driveId === "shared1", "Shared Drive search is first-class");
    assert(upload.parentId === "folder1", "upload placement is explicit");
    assert(download.overwrite === false, "overwrite behavior is explicit");
    assert(share.emailAddress === "person@example.com", "one sharing principal is represented");
}
