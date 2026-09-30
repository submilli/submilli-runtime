// Run through `submilli run --blueprint host.yaml`, not the allow-all build test runner.
import { getContents } from "@submilli/exa";

function deniedAt(capability: string, caller: string, action: () => void): void {
    let caught = false;
    try { action(); } catch (error) {
        caught = error.name === "PermissionDeniedError" && error.message.includes(capability) && error.message.includes(caller);
    }
    assert(caught, "expected " + caller + " denial at " + capability);
}
function reachesCredentialBoundary(action: () => void): void {
    // This denial proves every host check passed, without using credentials or HTTP.
    deniedAt("secrets.get", "@submilli/exa", action);
}
function main(): string {
    // A fully qualified host is the same host, so a rule naming it without the dot still applies.
    const spellings = ["https://evil.test/", "https://evil.test./", "https://evil.test../", "https://EVIL.test./a"];
    for (const url of spellings) {
        deniedAt("exa.ai/contents", "main", () => { getContents([url]); });
        deniedAt("exa.ai/contents", "main", () => { getContents(["https://example.com/", url]); });
    }
    reachesCredentialBoundary(() => { getContents(["https://example.com./", "https://example.com../a"]); });
    return "host capability checks passed";
}
