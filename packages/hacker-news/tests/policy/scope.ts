// Run through `submilli run --blueprint scope.yaml`, not the allow-all build test runner.
import { search, getThread } from "@submilli/hacker-news";

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

function main(): string {
    // The allowed thread passes the business check and stops at the package's HTTP gate.
    deniedAt("http.get", "@submilli/hacker-news", () => { getThread(49980140); });
    deniedAt("news.ycombinator.com/items.get", "main", () => { getThread(1); });
    // Search is a separate capability and is not granted.
    deniedAt("news.ycombinator.com/search", "main", () => { search({ query: "submilli" }); });
    return "item scope checks passed";
}
