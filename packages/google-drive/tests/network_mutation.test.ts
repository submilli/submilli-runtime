import { label } from "submilli:test";
import secrets from "submilli:secrets";
import { writeText } from "submilli:fs";
import { uploadFile, trashFile } from "@submilli/google-drive";

function main(): void {
    label("live Drive resumable upload and trash cleanup");
    if (secrets.get("GOOGLE_ACCESS_TOKEN") === undefined) return;
    if (secrets.get("GOOGLE_LIVE_MUTATIONS") !== "true") return;

    const stamp = Temporal.Now.instant().toString();
    const sourcePath = "/google-drive-integration-test.txt";
    writeText(sourcePath, "Temporary @submilli/google-drive upload test.\n" + stamp + "\n");
    let fileId: string | undefined;
    try {
        const file = uploadFile(sourcePath, {
            name: "submilli-google-drive-test-" + Temporal.Now.instant().epochMilliseconds.toString() + ".txt",
            mimeType: "text/plain",
        });
        fileId = file.id;
        assert(file.id.length > 0, "uploaded file has an ID");
        assert(file.mimeType === "text/plain", "uploaded MIME type is preserved");
    } finally {
        if (fileId !== undefined) {
            const trashed = trashFile(fileId);
            assert(trashed.trashed, "temporary uploaded file is moved to trash");
        }
    }
}
