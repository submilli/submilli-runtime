// Even an allowed submitted host cannot authorize provider-controlled downstream fetching.
import { search, scrape, map, startBatch, startCrawl } from "@submilli/firecrawl";

function denied(action: () => void): void {
    let caught = false;
    try { action(); } catch (error) {
        caught = error.name === "PermissionDeniedError" && error.message.includes("firecrawl.dev/delegatedFetch") && error.message.includes("main");
    }
    assert(caught, "delegated fetching needs an explicit caller grant before reading credentials");
}
function reachesCredentialBoundary(action: () => void, message: string): void {
    // This denial proves every business gate passed, without credentials or HTTP.
    let reached = false;
    try { action(); } catch (error) {
        reached = error.name === "PermissionDeniedError" && error.message.includes("secrets.get") && error.message.includes("@submilli/firecrawl");
    }
    assert(reached, message);
}
function main(): string {
    reachesCredentialBoundary(() => { search("q"); }, "discovery needs no delegatedFetch grant");
    reachesCredentialBoundary(() => { search("q", { scrapeOptions: {} }); }, "search with result content needs only the search grant");
    denied(() => { scrape("https://example.com"); });
    denied(() => { map("https://example.com"); });
    denied(() => { startBatch(["https://example.com"]); });
    denied(() => { startCrawl("https://example.com", { limit: 1 }); });
    return "delegated fetching checks passed";
}
