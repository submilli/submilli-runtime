import { label } from "submilli:test";
import secrets from "submilli:secrets";
import { search } from "@submilli/firecrawl";

function main(): void {
    const key = secrets.get("FIRECRAWL_API_KEY");
    if (key === null || key.trim().length === 0) {
        label("skip: FIRECRAWL_API_KEY is not bound");
        return;
    }
    label("live discovery search returns sources without page extraction");
    const found = search("Firecrawl documentation", { limit: 2, includeDomains: ["docs.firecrawl.dev"] });
    assert(found.results.length > 0, "search results");
    for (const result of found.results) {
        assert(result.url.length > 0 && result.title.length > 0, "source URL and title");
        const content = result.content;
        assert(content === null || (content.markdown === null && content.html === null && content.json === null), "no requested extraction");
    }
    label("live explicit result scraping returns page content");
    const extracted = search("Firecrawl scrape documentation", { limit: 1, includeDomains: ["docs.firecrawl.dev"],
        scrapeOptions: { formats: ["markdown", "html"] } });
    assert(extracted.results.length > 0, "search with content");
    let hasContent = false;
    for (const result of extracted.results) {
        const page = result.content;
        if (page !== null && page.markdown !== null && page.markdown!.length > 0) hasContent = true;
    }
    assert(hasContent, "usable extracted markdown");
}
