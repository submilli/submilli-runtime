import { label } from "submilli:test";
import { FirecrawlError, buildScrapeBody, buildMapBody, buildBatchBody, buildCrawlBody,
    jobPagePath, urlHost, normalizeScrapeJson, normalizeMapJson, normalizeJobJson,
    normalizeJobPageJson, normalizeJobErrorsJson, normalizeCancellationJson, firecrawlHttpError,
    getJob, downloadJobPage, cancelJob } from "@submilli/firecrawl";

function rejects(code: string, action: () => void): void {
    let caught = false;
    try { action(); } catch (error) { if (error instanceof FirecrawlError) caught = error.code === code; }
    assert(caught, "expected " + code);
}
function main(): void {
    label("minimal retrieval has no synthesis, exact fields and escaping");
    const request = buildScrapeBody("https://example.com");
    assert(request === '{"formats":["markdown"],"url":"https://example.com"}', "default markdown");
    const html = buildScrapeBody("https://example.com", { formats: ["html"], onlyMainContent: false,
        includeTags: ["article"], excludeTags: ["nav"], maxAge: 0, storeInCache: false, timeout: 10000 });
    assert(html.includes('"formats":["html"]') && html.includes('"maxAge":0') && html.includes('"onlyMainContent":false'), "explicit controls");
    const json = buildScrapeBody("https://example.com", { formats: [], json: { schema: { type: "object", properties: { title: { type: "string" } } }, prompt: 'Read "title"' } });
    assert(json.includes('"formats":[{"type":"json","schema":') && !json.includes('"markdown"'), "optional structured extraction");
    assert(buildBatchBody(["https://example.com/a", "https://other.example/b"]).includes('"ignoreInvalidURLs":false'), "no silent provider URL drops");
    assert(urlHost("https://EXAMPLE.com:443/a") === "example.com", "canonical host");
    assert(urlHost("https://example.com./a") === "example.com" && urlHost("https://EXAMPLE.com../a") === "example.com", "trailing dots are not part of the permission host");
    assert(buildMapBody("https://example.com").includes('"limit":100,"includeSubdomains":false'), "bounded map default");
    const crawl = buildCrawlBody("https://example.com/docs", { limit: 5, maxDiscoveryDepth: 1, sitemap: "skip",
        includePaths: ["docs/.*"], excludePaths: ["docs/archive/.*"], scrapeOptions: { formats: ["markdown", "html"] } });
    assert(crawl.includes('"allowExternalLinks":false') && crawl.includes('"allowSubdomains":false') && crawl.includes('"limit":5'), "explicit crawl limits");
    assert(crawl.includes('"scrapeOptions":{"formats":["markdown","html"]}'), "nested scrape options");

    label("invalid input is rejected locally");
    for (const url of ["relative", "file:///tmp/a", "https://user:pass@example.com", " https://example.com", "https://example.com\\@evil.example"]) {
        rejects("invalid_argument", () => { buildScrapeBody(url); });
    }
    rejects("invalid_argument", () => { buildBatchBody([]); });
    rejects("invalid_argument", () => { buildBatchBody(["https://example.com", "bad"]); });
    rejects("invalid_argument", () => { buildCrawlBody("https://example.com", { limit: 0 }); });
    rejects("invalid_argument", () => { buildCrawlBody("https://example.com", { limit: 2.5 }); });
    rejects("invalid_argument", () => { buildMapBody("https://example.com", { limit: 100001 }); });
    rejects("invalid_argument", () => { buildScrapeBody("https://example.com", { maxAge: -1 }); });
    rejects("invalid_argument", () => { buildScrapeBody("https://example.com", { formats: [] }); });
    rejects("invalid_argument", () => { buildScrapeBody("https://example.com", { json: { schema: "bad" } }); });

    label("content, source URLs, arbitrary metadata and per-page failures survive");
    const page = normalizeScrapeJson('{"success":true,"data":{"markdown":"full text","html":"<p>full text</p>","json":{"title":"Example"},"metadata":{"sourceURL":"https://example.com/start","url":"https://example.com/final","title":"Example","statusCode":200,"custom":{"value":42}},"warning":"cached"}}');
    assert(page.markdown === "full text" && page.sourceURL === "https://example.com/start" && page.url === "https://example.com/final", "attribution");
    assert(JSON.stringify(page.metadata).includes('"custom":{"value":42}') && JSON.stringify(page.json) === '{"title":"Example"}', "no lost JSON fields");
    assert(page.warning === "cached" && page.error === null, "optional metadata");
    const links = normalizeMapJson('{"success":true,"links":[{"url":"https://example.com/a","title":"A"},{"url":"https://example.com/b"}]}', 2);
    assert(links.links.length === 2 && links.limit === 2, "map does not trim");
    const job = normalizeJobJson('{"success":true,"id":"11111111-1111-4111-8111-111111111111","invalidURLs":["provider-rejected"]}');
    assert(job.id === "11111111-1111-4111-8111-111111111111" && job.invalidURLs.length === 1, "submission preserves rejected URLs");
    const partial = normalizeJobPageJson('{"status":"scraping","total":3,"completed":1,"creditsUsed":1,"next":"https://api.firecrawl.dev/v2/crawl/11111111-1111-4111-8111-111111111111?skip=1","data":[{"markdown":"ok","metadata":{"sourceURL":"https://example.com/a"}},{"metadata":{"sourceURL":"https://example.com/b","error":"timeout","statusCode":408}}]}', "crawl", "11111111-1111-4111-8111-111111111111");
    assert(partial.data.length === 2 && partial.data[1].error === "timeout" && partial.next !== null, "partial results include failed pages");
    for (const status of ["completed", "failed", "cancelled", "future_state"]) {
        const result = normalizeJobPageJson('{"status":"' + status + '","total":1,"completed":0,"data":[],"error":"detail"}', "batch", "11111111-1111-4111-8111-111111111111");
        assert(result.status === status && result.next === null && result.error === "detail", "terminal/future state preserved");
    }
    const errors = normalizeJobErrorsJson('{"errors":[{"id":"p1","timestamp":null,"url":"https://example.com/fail","error":"timeout"}],"robotsBlocked":["https://example.com/private"]}');
    assert(errors.errors[0].error === "timeout" && errors.robotsBlocked.length === 1, "separate failures endpoint");
    assert(normalizeCancellationJson('{"status":"cancelled"}').status === "cancelled", "v2 cancellation");

    label("pagination cannot redirect credentials or cross job boundaries");
    assert(jobPagePath("batch", "11111111-1111-4111-8111-111111111111") === "/batch/scrape/11111111-1111-4111-8111-111111111111", "first page");
    assert(jobPagePath("crawl", "11111111-1111-4111-8111-111111111111", "https://api.firecrawl.dev/v2/crawl/11111111-1111-4111-8111-111111111111?skip=2&limit=5") === "/crawl/11111111-1111-4111-8111-111111111111?skip=2&limit=5", "next page");
    for (const next of ["https://evil.example/v2/crawl/11111111-1111-4111-8111-111111111111?skip=1", "http://api.firecrawl.dev/v2/crawl/11111111-1111-4111-8111-111111111111?skip=1",
        "https://api.firecrawl.dev@evil.example/v2/crawl/11111111-1111-4111-8111-111111111111?skip=1", "https://api.firecrawl.dev/v2/crawl/other?skip=1",
        "https://api.firecrawl.dev/v2/batch/scrape/11111111-1111-4111-8111-111111111111?skip=1", "https://api.firecrawl.dev/v2/crawl/11111111-1111-4111-8111-111111111111?skip=1#fragment",
        "https://api.firecrawl.dev/v2/crawl/11111111-1111-4111-8111-111111111111?skip=1&url=https://evil.example", "https://api.firecrawl.dev/v2/crawl/11111111-1111-4111-8111-111111111111?skip=1&skip=2"]) {
        rejects("invalid_argument", () => { jobPagePath("crawl", "11111111-1111-4111-8111-111111111111", next); });
    }
    rejects("invalid_argument", () => { jobPagePath("crawl", "../scrape"); });

    // Reserved provider routes must never be accepted as job IDs, especially for raw downloads.
    rejects("invalid_argument", () => { jobPagePath("crawl", "active"); });
    rejects("invalid_argument", () => { downloadJobPage("crawl", "active", "/page.json"); });

    label("unsafe destinations fail through public operations before credentials or HTTP");
    rejects("invalid_argument", () => { getJob("crawl", "11111111-1111-4111-8111-111111111111", "https://evil.example"); });
    rejects("invalid_argument", () => { downloadJobPage("crawl", "11111111-1111-4111-8111-111111111111", "/page.json", "https://api.firecrawl.dev/v2/crawl/other?skip=1"); });
    rejects("invalid_argument", () => { downloadJobPage("crawl", "11111111-1111-4111-8111-111111111111", "/page.json", null, { maxBytes: 0 }); });
    rejects("invalid_argument", () => { cancelJob("batch", "../crawl/11111111-1111-4111-8111-111111111111"); });

    label("large content is preserved rather than silently shortened");
    const text = "whole page text ".repeat(10000);
    const large = normalizeScrapeJson(JSON.stringify({ success: true, data: { markdown: text, metadata: { sourceURL: "https://example.com" } } }));
    assert(large.markdown === text, "all content retained");

    label("malformed envelopes never appear as empty successful results");
    for (const body of ["null", "{}", "[]", "not json", '{"success":false}', '{"success":true,"data":{}}', '{"success":true,"data":{"metadata":42}}']) {
        rejects("invalid_response", () => { normalizeScrapeJson(body); });
    }
    rejects("invalid_response", () => { normalizeMapJson('{"success":true,"links":[{"url":5}]}', 1); });
    rejects("invalid_response", () => { normalizeJobJson('{"success":true}'); });
    rejects("invalid_response", () => { normalizeJobPageJson('{"status":"scraping","total":-1,"completed":0,"data":[]}', "crawl", "11111111-1111-4111-8111-111111111111"); });
    rejects("invalid_response", () => { normalizeJobPageJson('{"status":"scraping","total":1,"completed":0}', "crawl", "11111111-1111-4111-8111-111111111111"); });
    rejects("invalid_response", () => { normalizeJobPageJson('{"status":"completed","total":1,"completed":1,"data":[],"next":"https://evil.example"}', "crawl", "11111111-1111-4111-8111-111111111111"); });
    rejects("invalid_response", () => { normalizeJobErrorsJson('{"errors":[]}'); });
    rejects("invalid_response", () => { normalizeCancellationJson('{"success":true}'); });
    assert(firecrawlHttpError(429, "10").retryAfter === "10" && firecrawlHttpError(429).code === "rate_limited", "rate limit hints");
    assert(firecrawlHttpError(404).code === "not_found" && firecrawlHttpError(402).code === "quota_exceeded", "job expiry and credits");
}
