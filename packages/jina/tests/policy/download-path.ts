// Run through `submilli run --blueprint download-path.yaml`, not the allow-all build test runner.
import { downloadRead, downloadSearch } from "@submilli/jina";

function deniedAt(capability: string, caller: string, action: () => void): void {
    let outcome = "no error";
    try {
        action();
    } catch (error: PermissionDeniedError) {
        outcome = error.caller + " denied at " + error.capability;
    } catch (error) {
        outcome = error.name + ": " + error.message;
    }
    const expected = caller + " denied at " + capability;
    assert(outcome === expected, "expected " + expected + ", got " + outcome);
}

function reachesCredentialBoundary(action: () => void): void {
    // This denial proves the destination check passed, without reading a key or calling Jina.
    deniedAt("secrets.get", "@submilli/jina", action);
}

function main(): string {
    // The file is written for `main`, so `main`'s own `fs.write` rule decides the path.
    deniedAt("fs.write", "main", () => { downloadRead("https://example.com/", "/elsewhere/page.md"); });
    deniedAt("fs.write", "main", () => { downloadSearch("query", "/elsewhere/results.md"); });
    // The path is checked as `main`'s own write would be, so stepping out of the folder is denied too.
    deniedAt("fs.write", "main", () => { downloadRead("https://example.com/", "/downloads/../elsewhere/page.md"); });
    reachesCredentialBoundary(() => { downloadRead("https://example.com/", "/downloads/page.md"); });
    reachesCredentialBoundary(() => { downloadSearch("query", "/downloads/results.md"); });
    // The business capability is still checked first.
    deniedAt("jina.ai/read", "main", () => { downloadRead("https://evil.test/", "/downloads/page.md"); });
    return "download destination checks passed";
}
