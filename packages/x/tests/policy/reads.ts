// Run with --blueprint reads.yaml; no credentials or network are used.
import { getTweet, searchRecent, searchAll, listReplies } from "@submilli/x";

function deniedAt(capability: string, caller: string, action: () => void): void {
    let matched = false;
    try { action(); } catch (error) {
        matched = error.name === "PermissionDeniedError" && error.message.includes(capability) && error.message.includes(caller);
    }
    assert(matched, "expected " + caller + " denial at " + capability);
}

function main(): string {
    deniedAt("x.com/getTweet", "main", () => { getTweet("456"); });
    deniedAt("secrets.get", "@submilli/x", () => { getTweet("https://x.com/user/status/123?s=20"); });
    deniedAt("secrets.get", "@submilli/x", () => { searchRecent("q"); });
    deniedAt("x.com/searchAll", "main", () => { searchAll("q"); });
    deniedAt("secrets.get", "@submilli/x", () => { listReplies("123"); });
    deniedAt("x.com/searchAll", "main", () => { listReplies("123", { archive: true }); });
    return "X read capability checks passed";
}
