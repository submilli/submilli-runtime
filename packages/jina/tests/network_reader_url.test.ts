// Keyless Reader integration: the target URL must not become part of the
// Reader endpoint's path syntax. No JINA_API_KEY is required.
import { label } from "submilli:test";
import { downloadRead } from "@submilli/jina";
import { readText } from "submilli:fs";

function main(): void {
    label("encoded dot segments cannot replace the checked host");
    const attack = "https://docs.python.org/%2e%2e/%2e%2e/https://example.com/";
    const result = downloadRead(attack, "/reader-host.md");
    assert(result.status === 200, "Reader returns its page envelope");
    const body = readText(result.path) ?? "";
    assert(body.includes("URL Source: https://docs.python.org/https://example.com/"),
        "Reader must fetch the checked host even when the target path normalizes");

    label("ordinary URLs still download markdown");
    const ordinary = downloadRead("https://example.com/", "/reader-ordinary.md");
    assert(ordinary.status === 200, "ordinary Reader request succeeds");
    assert(ordinary.bytesWritten > 0, "markdown is streamed to the VFS");
    assert((readText(ordinary.path) ?? "").includes("URL Source: https://example.com/"),
        "encoding preserves the ordinary target");
}
