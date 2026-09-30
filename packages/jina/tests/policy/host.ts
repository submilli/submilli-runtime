// Run through `submilli run --blueprint host.yaml`, not the allow-all build test runner.
import { read, readJson, downloadRead } from "@submilli/jina";

function deniedAt(capability: string, caller: string, action: () => void): void {
    let caught = false;
    try { action(); } catch (error) {
        caught = error.name === "PermissionDeniedError" && error.message.includes(capability) && error.message.includes(caller);
    }
    assert(caught, "expected " + caller + " denial at " + capability);
}
function reachesCredentialBoundary(action: () => void): void {
    // This denial proves the host check passed, without using credentials or HTTP.
    deniedAt("secrets.get", "@submilli/jina", action);
}
function main(): string {
    // A fully qualified host is the same host, so a rule naming it without the dot still applies.
    const spellings = ["https://evil.test/", "https://evil.test./", "https://evil.test../", "https://EVIL.test./a"];
    for (const url of spellings) {
        deniedAt("jina.ai/read", "main", () => { read(url); });
        deniedAt("jina.ai/read", "main", () => { readJson(url); });
        deniedAt("jina.ai/read", "main", () => { downloadRead(url, "/page.md"); });
    }
    reachesCredentialBoundary(() => { read("https://example.com./"); });
    reachesCredentialBoundary(() => { readJson("https://example.com./"); });
    reachesCredentialBoundary(() => { downloadRead("https://example.com./", "/page.md"); });
    return "host capability checks passed";
}
