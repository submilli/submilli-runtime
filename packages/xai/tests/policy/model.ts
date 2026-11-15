// Run with --blueprint model.yaml; no credentials or network are used.
import { research, getTweetSummary } from "@submilli/xai";

function deniedAt(capability: string, caller: string, action: () => void): void {
    let matched = false;
    try { action(); } catch (error) {
        matched = error.name === "PermissionDeniedError" && error.message.includes(capability) && error.message.includes(caller);
    }
    assert(matched, "expected " + caller + " denial at " + capability);
}

function main(): string {
    deniedAt("x.ai/research", "main", () => { research({ question: "q", model: "grok-4.6" }); });
    deniedAt("secrets.get", "@submilli/xai", () => { research({ question: "q" }); });
    deniedAt("secrets.get", "@submilli/xai", () => { research({ question: "q", model: "grok-4.7" }); });
    deniedAt("secrets.get", "@submilli/xai", () => { getTweetSummary("https://x.com/user/status/123"); });
    return "model capability checks passed";
}
