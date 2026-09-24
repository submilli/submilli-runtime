import { search, scrape } from "@submilli/firecrawl";

function main(): string {
    let reached = false;
    try { search("q", { limit: 2, scrapeOptions: {} }); } catch (error) {
        reached = error.name === "PermissionDeniedError" && error.message.includes("secrets.get") && error.message.includes("@submilli/firecrawl");
    }
    assert(reached, "explicit broad search scraping grants reach the credential boundary");
    let denied = false;
    try { scrape("https://example.com"); } catch (error) {
        denied = error.name === "PermissionDeniedError" && error.message.includes("firecrawl.dev/scrape") && error.message.includes("main");
    }
    assert(denied, "search scraping does not grant ordinary scrape access");
    return "search content capability checks passed";
}
