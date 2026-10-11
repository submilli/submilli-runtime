import { label } from "submilli:test";
import secrets from "submilli:secrets";
import { scrape, map } from "@submilli/firecrawl";

function main(): void {
    const key = secrets.get("FIRECRAWL_API_KEY");
    if (key === undefined || key.trim().length === 0) {
        label("skip: FIRECRAWL_API_KEY is not bound");
        return;
    }
    label("live scrape and map preserve content and attribution");
    const page = scrape("https://example.com", { formats: ["markdown", "html"] });
    assert(page.sourceURL !== null && page.markdown !== null && page.html !== null, "content and metadata");
    const links = map("https://example.com", { limit: 5, includeSubdomains: false });
    assert(links.links.length > 0 && links.limit === 5, "bounded site discovery");
}
