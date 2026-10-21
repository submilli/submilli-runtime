// Request building, URL validation and pagination are private: these tests reach
// them through the public operations. Invalid input fails with `invalid_argument`
// before any check, credential read or HTTP. Valid input gets past validation and
// stops at the credential read with `missing_credentials`, because tests have no
// credentials by default; that segment is skipped when FIRECRAWL_API_KEY is bound,
// so this offline file never sends a request.
import { label } from "submilli:test";
import secrets from "submilli:secrets";
import { FirecrawlError, scrape, map, startBatch, startCrawl, getJob, getJobErrors, downloadJobPage, cancelJob } from "@submilli/firecrawl";

const JOB_ID = "11111111-1111-4111-8111-111111111111";

function rejects(code: string, action: () => void): void {
    let caught = false;
    try { action(); } catch (error) { if (error instanceof FirecrawlError) caught = error.code === code; }
    assert(caught, "expected " + code);
}
function main(): void {
    label("invalid input is rejected locally");
    for (const url of ["relative", "file:///tmp/a", "https://user:pass@example.com", " https://example.com", "https://example.com\\@evil.example", "https://./", "https://../a"]) {
        rejects("invalid_argument", () => { scrape(url); });
        rejects("invalid_argument", () => { map(url); });
        rejects("invalid_argument", () => { startCrawl(url, { limit: 1 }); });
    }
    rejects("invalid_argument", () => { startBatch([]); });
    rejects("invalid_argument", () => { startBatch(["https://example.com", "bad"]); });
    rejects("invalid_argument", () => { startCrawl("https://example.com", { limit: 0 }); });
    rejects("invalid_argument", () => { startCrawl("https://example.com", { limit: 2.5 }); });
    rejects("invalid_argument", () => { map("https://example.com", { limit: 100001 }); });
    rejects("invalid_argument", () => { scrape("https://example.com", { maxAge: -1 }); });
    rejects("invalid_argument", () => { scrape("https://example.com", { formats: [] }); });
    rejects("invalid_argument", () => { scrape("https://example.com", { json: { schema: "bad" } }); });
    rejects("invalid_argument", () => { startBatch(["https://example.com"], { formats: [] }); });

    label("pagination cannot redirect credentials or cross job boundaries");
    for (const next of ["https://evil.example/v2/crawl/11111111-1111-4111-8111-111111111111?skip=1", "http://api.firecrawl.dev/v2/crawl/11111111-1111-4111-8111-111111111111?skip=1",
        "https://api.firecrawl.dev@evil.example/v2/crawl/11111111-1111-4111-8111-111111111111?skip=1", "https://api.firecrawl.dev/v2/crawl/other?skip=1",
        "https://api.firecrawl.dev/v2/batch/scrape/11111111-1111-4111-8111-111111111111?skip=1", "https://api.firecrawl.dev/v2/crawl/11111111-1111-4111-8111-111111111111?skip=1#fragment",
        "https://api.firecrawl.dev/v2/crawl/11111111-1111-4111-8111-111111111111?skip=1&url=https://evil.example", "https://api.firecrawl.dev/v2/crawl/11111111-1111-4111-8111-111111111111?skip=1&skip=2"]) {
        rejects("invalid_argument", () => { getJob("crawl", JOB_ID, next); });
        rejects("invalid_argument", () => { downloadJobPage("crawl", JOB_ID, "/page.json", next); });
    }
    rejects("invalid_argument", () => { getJob("crawl", "../scrape"); });
    rejects("invalid_argument", () => { getJobErrors("crawl", "../scrape"); });

    // Reserved provider routes must never be accepted as job IDs, especially for raw downloads.
    rejects("invalid_argument", () => { getJob("crawl", "active"); });
    rejects("invalid_argument", () => { downloadJobPage("crawl", "active", "/page.json"); });

    label("unsafe destinations fail through public operations before credentials or HTTP");
    rejects("invalid_argument", () => { getJob("crawl", JOB_ID, "https://evil.example"); });
    rejects("invalid_argument", () => { downloadJobPage("crawl", JOB_ID, "/page.json", null, { maxBytes: 0 }); });
    rejects("invalid_argument", () => { downloadJobPage("crawl", JOB_ID, "/page.json", undefined, { maxBytes: 0 }); });
    rejects("invalid_argument", () => { cancelJob("batch", "../crawl/11111111-1111-4111-8111-111111111111"); });

    const key = secrets.get("FIRECRAWL_API_KEY");
    if (key !== undefined && key.trim().length > 0) {
        label("skip: valid input reaches the credential read only when FIRECRAWL_API_KEY is not bound");
        return;
    }
    label("valid input passes local validation and stops at the credential read");
    rejects("missing_credentials", () => { scrape("https://example.com"); });
    rejects("missing_credentials", () => { scrape("https://example.com./a"); });
    rejects("missing_credentials", () => { scrape("https://example.com", { formats: ["html"], onlyMainContent: false,
        includeTags: ["article"], excludeTags: ["nav"], maxAge: 0, storeInCache: false, timeout: 10000 }); });
    rejects("missing_credentials", () => { scrape("https://example.com", { formats: [], json: { schema: { type: "object", properties: { title: { type: "string" } } }, prompt: 'Read "title"' } }); });
    rejects("missing_credentials", () => { scrape("https://example.com", { formats: [], json: { schema: undefined, prompt: "Read title" } }); });
    rejects("missing_credentials", () => { startBatch(["https://example.com/a", "https://other.example/b"]); });
    rejects("missing_credentials", () => { map("https://example.com"); });
    rejects("missing_credentials", () => { map("https://example.com", { limit: undefined, sitemap: undefined }); });
    rejects("missing_credentials", () => { startCrawl("https://example.com/docs", { limit: 5, maxDiscoveryDepth: 1, sitemap: "skip",
        includePaths: ["docs/.*"], excludePaths: ["docs/archive/.*"], scrapeOptions: { formats: ["markdown", "html"] } }); });
    rejects("missing_credentials", () => { getJob("batch", JOB_ID); });
    rejects("missing_credentials", () => { getJob("crawl", JOB_ID, "https://api.firecrawl.dev/v2/crawl/11111111-1111-4111-8111-111111111111?skip=2&limit=5"); });
    rejects("missing_credentials", () => { getJobErrors("crawl", JOB_ID); });
    rejects("missing_credentials", () => { cancelJob("batch", JOB_ID); });
}
