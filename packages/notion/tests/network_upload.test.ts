import { label } from "submilli:test";
import { writeText } from "submilli:fs";
import secrets from "submilli:secrets";
import { uploadFile } from "@submilli/notion";

function main(): void {
    label("live single-part upload when explicitly enabled");
    if (secrets.get("NOTION_ACCESS_TOKEN") === undefined) return;
    if (secrets.get("NOTION_LIVE_UPLOADS") !== "true") return;
    const path = "/notion-upload-test.txt";
    writeText(path, "Temporary @submilli/notion upload test.\n");
    const upload = uploadFile(path, {
        filename: "submilli-notion-upload-test.txt",
        contentType: "text/plain",
    });
    assert(upload.id.length > 0, "upload has an ID");
    assert(upload.status === "uploaded", "upload is finalized");
}
