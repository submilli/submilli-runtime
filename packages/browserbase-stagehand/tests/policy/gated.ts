// Run through `submilli run --blueprint gated.yaml`, not the allow-all build test runner.
import { StagehandSession } from "@submilli/browserbase-stagehand";

function deniedAt(capability: string, caller: string, action: () => void): void {
    let caught = false;
    try { action(); } catch (error) {
        caught = error.name === "PermissionDeniedError" && error.message.includes(capability) && error.message.includes(caller);
    }
    assert(caught, "expected " + caller + " denial at " + capability);
}

function reachesCredentialBoundary(action: () => void): void {
    deniedAt("secrets.get", "@submilli/browserbase-stagehand", action);
}

function main(): string {
    const spellings = ["https://evil.test/", "https://evil.test./", "https://evil.test../", "https://EVIL.test./a"];
    for (const url of spellings) {
        deniedAt("browserbase.com/navigate", "main", () => { new StagehandSession("sess_1").goto(url); });
    }
    deniedAt("browserbase.com/navigate", "main", () => { new StagehandSession("sess_1").goto("https://example.com/secret"); });
    reachesCredentialBoundary(() => { StagehandSession.open(); });
    reachesCredentialBoundary(() => { new StagehandSession("sess_1").goto("https://example.com./"); });
    return "gated capability checks passed";
}
