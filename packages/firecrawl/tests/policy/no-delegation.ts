// Even an allowed submitted host cannot authorize provider-controlled downstream fetching.
import { search, scrape, map, startBatch, startCrawl } from "@submilli/firecrawl";

function denied(action: () => void): void {
    let caught = false;
    try { action(); } catch (error) {
        caught = error.name === "PermissionDeniedError" && error.message.includes("firecrawl.dev/delegatedFetch") && error.message.includes("main");
    }
    assert(caught, "delegated fetching needs an explicit caller grant before reading credentials");
}
function main(): string {
    let discovered = false;
    try { search("q"); } catch (error) {
        discovered = error.name === "PermissionDeniedError" && error.message.includes("secrets.get") && error.message.includes("@submilli/firecrawl");
    }
    assert(discovered, "discovery needs no delegatedFetch grant");
    denied(() => { search("q", { scrapeOptions: {} }); });
    denied(() => { scrape("https://example.com"); });
    denied(() => { map("https://example.com"); });
    denied(() => { startBatch(["https://example.com"]); });
    denied(() => { startCrawl("https://example.com", { limit: 1 }); });
    return "delegated fetching checks passed";
}
