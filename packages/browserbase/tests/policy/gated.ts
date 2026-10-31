// Run through `submilli run --blueprint gated.yaml`, not the allow-all build test runner.
import { fetchPage, StagehandSession } from "@submilli/browserbase";

function deniedAt(capability: string, caller: string, action: () => void): void {
    let caught = false;
    try { action(); } catch (error) {
        caught = error.name === "PermissionDeniedError" && error.message.includes(capability) && error.message.includes(caller);
    }
    assert(caught, "expected " + caller + " denial at " + capability);
}

function reachesCredentialBoundary(action: () => void): void {
    deniedAt("secrets.get", "@submilli/browserbase", action);
}

function main(): string {
    const spellings = ["https://evil.test/", "https://evil.test./", "https://evil.test../", "https://EVIL.test./a"];
    for (const url of spellings) {
        deniedAt("browserbase.com/fetch", "main", () => { fetchPage(url); });
        deniedAt("browserbase.com/stagehand.navigate", "main", () => { new StagehandSession("sess_1").goto(url); });
    }
    deniedAt("browserbase.com/fetch", "main", () => { fetchPage("https://example.com", { proxies: true }); });
    deniedAt("browserbase.com/fetch", "main", () => { fetchPage("https://example.com", { allowRedirects: true }); });
    deniedAt("browserbase.com/fetch", "main", () => { fetchPage("https://example.com", { allowInsecureSsl: true }); });
    deniedAt("browserbase.com/stagehand.start", "main", () => { StagehandSession.open({ proxies: true }); });
    deniedAt("browserbase.com/stagehand.start", "main", () => { StagehandSession.open({ verified: true }); });
    deniedAt("browserbase.com/stagehand.start", "main", () => { StagehandSession.open({ solveCaptchas: true }); });
    reachesCredentialBoundary(() => { fetchPage("https://example.com./"); });
    reachesCredentialBoundary(() => { StagehandSession.open(); });
    reachesCredentialBoundary(() => { new StagehandSession("sess_1").goto("https://example.com./"); });
    return "gated capability checks passed";
}
