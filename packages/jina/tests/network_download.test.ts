// Real network integration for the download-to-VFS helpers. Streams the Reader
// and Search markdown straight to disk, bypassing the in-memory string / JSON
// path entirely. Runs only when a JINA_API_KEY is available (Search 401s
// keyless); without it the test skips and passes, as with the other network
// suites here.

import { label } from "submilli:test";
import secrets from "submilli:secrets";
import { downloadRead, downloadSearch } from "@submilli/jina";
import { readText, size } from "submilli:fs";

function main(): void {
    if (secrets.get("JINA_API_KEY") === undefined) {
        return;
    }

    label("downloadRead streams reader markdown to the VFS");
    const read = downloadRead("https://example.com", "/example.md");
    assert(read.bytesWritten > 0, "downloadRead reports bytes written");
    assert(read.path === "/example.md", "result echoes the destination path");
    const body = readText("/example.md");
    assert(body !== undefined && body.length > 0, "the reader file has readable content");

    label("downloadSearch streams search markdown to the VFS");
    const found = downloadSearch("Jina AI Reader API", "/search.md");
    assert(found.bytesWritten > 0, "downloadSearch reports bytes written");
    assert(size("/search.md") > 0, "the search file is non-empty on disk");
}
