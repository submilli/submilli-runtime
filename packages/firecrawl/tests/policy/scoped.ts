// Run through `submilli run --blueprint scoped.yaml`, not the allow-all build test runner.
import { search, scrape, map, startBatch, startCrawl, getJob, getJobErrors, cancelJob, downloadJobPage } from "@submilli/firecrawl";

function deniedAt(capability: string, caller: string, action: () => void): void {
    let caught = false;
    try { action(); } catch (error) {
        caught = error.name === "PermissionDeniedError" && error.message.includes(capability) && error.message.includes(caller);
    }
    assert(caught, "expected " + caller + " denial at " + capability);
}
function reachesCredentialBoundary(action: () => void): void {
    // This denial proves all business gates passed, without using credentials or HTTP.
    deniedAt("secrets.get", "@submilli/firecrawl", action);
}
function main(): string {
    deniedAt("firecrawl.dev/search", "main", () => { search("q", { limit: 3 }); });
    reachesCredentialBoundary(() => { search("q", { limit: 2 }); });
    // An allowed domain filter and an ordinary host-scoped scrape grant do not authorize native search scraping.
    deniedAt("firecrawl.dev/search.scrape", "main", () => { search("q", { limit: 2, includeDomains: ["example.com"], scrapeOptions: {} }); });
    deniedAt("firecrawl.dev/scrape", "main", () => { scrape("https://blocked.example"); });
    reachesCredentialBoundary(() => { scrape("https://EXAMPLE.com/a"); });
    // A fully qualified host is the same host, so the same grant and the same denial apply.
    reachesCredentialBoundary(() => { scrape("https://example.com./a"); });
    deniedAt("firecrawl.dev/scrape", "main", () => { scrape("https://blocked.example./"); });

    deniedAt("firecrawl.dev/map", "main", () => { map("https://blocked.example", { limit: 5 }); });
    deniedAt("firecrawl.dev/map", "main", () => { map("https://example.com", { limit: 6 }); });
    deniedAt("firecrawl.dev/map", "main", () => { map("https://example.com", { limit: 5, includeSubdomains: true }); });
    reachesCredentialBoundary(() => { map("https://example.com", { limit: 5 }); });
    reachesCredentialBoundary(() => { map("https://example.com.", { limit: 5 }); });
    deniedAt("firecrawl.dev/map", "main", () => { map("https://blocked.example.", { limit: 5 }); });

    // Checking just the first URL, or reading a credential between host checks, fails this assertion.
    deniedAt("firecrawl.dev/batch.start", "main", () => { startBatch(["https://example.com/a", "https://blocked.example/b"]); });
    deniedAt("firecrawl.dev/batch.start", "main", () => { startBatch(["https://example.com/a", "https://example.com/b", "https://example.com/c"]); });
    reachesCredentialBoundary(() => { startBatch(["https://example.com/a", "https://EXAMPLE.com/b"]); });
    reachesCredentialBoundary(() => { startBatch(["https://example.com./a", "https://EXAMPLE.com../b"]); });
    deniedAt("firecrawl.dev/batch.start", "main", () => { startBatch(["https://example.com/a", "https://blocked.example./b"]); });

    deniedAt("firecrawl.dev/crawl.start", "main", () => { startCrawl("https://blocked.example", { limit: 2 }); });
    deniedAt("firecrawl.dev/crawl.start", "main", () => { startCrawl("https://example.com", { limit: 3 }); });
    deniedAt("firecrawl.dev/crawl.start", "main", () => { startCrawl("https://example.com", { limit: 2, allowExternalLinks: true }); });
    deniedAt("firecrawl.dev/crawl.start", "main", () => { startCrawl("https://example.com", { limit: 2, allowSubdomains: true }); });
    deniedAt("firecrawl.dev/crawl.start", "main", () => { startCrawl("https://example.com", { limit: 2, crawlEntireDomain: true }); });
    reachesCredentialBoundary(() => { startCrawl("https://example.com", { limit: 2 }); });
    reachesCredentialBoundary(() => { startCrawl("https://example.com.", { limit: 2 }); });
    deniedAt("firecrawl.dev/crawl.start", "main", () => { startCrawl("https://blocked.example.", { limit: 2 }); });

    deniedAt("firecrawl.dev/jobs.read", "main", () => { getJob("crawl", "44444444-4444-4444-8444-444444444444"); });
    deniedAt("firecrawl.dev/jobs.read", "main", () => { getJob("batch", "22222222-2222-4222-8222-222222222222"); });
    deniedAt("firecrawl.dev/jobs.read", "main", () => { getJobErrors("crawl", "44444444-4444-4444-8444-444444444444"); });
    deniedAt("firecrawl.dev/jobs.read", "main", () => { getJob("batch", "33333333-3333-4333-8333-333333333333"); });
    reachesCredentialBoundary(() => { getJob("crawl", "22222222-2222-4222-8222-222222222222"); });
    reachesCredentialBoundary(() => { getJob("crawl", "22222222-2222-4222-8222-222222222222", "https://api.firecrawl.dev/v2/crawl/22222222-2222-4222-8222-222222222222?skip=1"); });
    reachesCredentialBoundary(() => { getJobErrors("crawl", "22222222-2222-4222-8222-222222222222"); });
    deniedAt("firecrawl.dev/jobs.cancel", "main", () => { cancelJob("crawl", "22222222-2222-4222-8222-222222222222"); });
    deniedAt("firecrawl.dev/jobs.cancel", "main", () => { cancelJob("batch", "44444444-4444-4444-8444-444444444444"); });
    reachesCredentialBoundary(() => { cancelJob("batch", "33333333-3333-4333-8333-333333333333"); });

    deniedAt("firecrawl.dev/jobs.read", "main", () => { downloadJobPage("crawl", "44444444-4444-4444-8444-444444444444", "/approved.json", null, { maxBytes: 1000 }); });
    deniedAt("fs.write", "main", () => { downloadJobPage("crawl", "22222222-2222-4222-8222-222222222222", "/denied.json", null, { maxBytes: 1000 }); });
    deniedAt("fs.write", "main", () => { downloadJobPage("crawl", "22222222-2222-4222-8222-222222222222", "/approved.json", null, { maxBytes: 1001 }); });
    reachesCredentialBoundary(() => { downloadJobPage("crawl", "22222222-2222-4222-8222-222222222222", "/approved.json", null, { maxBytes: 1000 }); });
    return "scoped capability checks passed";
}
