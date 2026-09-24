import { label, expectException } from "submilli:test";
import { buildSearchBody, normalizeSearchJson, FirecrawlError } from "@submilli/firecrawl";

function rejects(code: string, action: () => void): void {
    const error = expectException(action, "FirecrawlError");
    assert(error instanceof FirecrawlError && error.code === code, "error code");
}
function main(): void {
    label("search defaults are discovery only");
    assert(buildSearchBody("test") === '{"query":"test","limit":10,"sources":["web"],"highlights":false,"domainTools":false}', "minimal web search");
    assert(buildSearchBody("test", {}) === buildSearchBody("test"), "empty options do not enable scraping");
    const escaped = JSON.parse(buildSearchBody('a "quoted" query\nnext')) as { query: string };
    assert(escaped.query === 'a "quoted" query\nnext', "query escaping");

    label("supported filters and opt-in extraction map to v2");
    const filtered = buildSearchBody("query", { limit: 100, includeDomains: ["EXAMPLE.com"], tbs: "sbd:1,qdr:w",
        country: "uk", location: "London,England,United Kingdom", safe: true, timeout: 10000 });
    assert(filtered.includes('"includeDomains":["example.com"]') && filtered.includes('"country":"UK"'), "normalized domains and country");
    assert(filtered.includes('"tbs":"sbd:1,qdr:w"') && filtered.includes('"limit":100') && filtered.includes('"safe":true'), "filters");
    assert(buildSearchBody("q", { excludeDomains: ["example.com"], safe: false }).includes('"excludeDomains":["example.com"]'), "exclusions");
    assert(buildSearchBody("q", { scrapeOptions: {} }).includes('"scrapeOptions":{"formats":["markdown"]}'), "explicit empty scrape options enable markdown");
    const content = buildSearchBody("q", { scrapeOptions: { formats: ["markdown", "html"], maxAge: 0, includeTags: ["article"] } });
    assert(content.includes('"scrapeOptions":{"formats":["markdown","html"],"includeTags":["article"],"maxAge":0}'), "reuse scrape options");

    label("search validates request limits and mutually exclusive filters");
    rejects("invalid_argument", () => { buildSearchBody(" "); });
    rejects("invalid_argument", () => { buildSearchBody("q".repeat(501)); });
    for (const limit of [0, 101, 1.5, NaN, Infinity]) rejects("invalid_argument", () => { buildSearchBody("q", { limit: limit }); });
    rejects("invalid_argument", () => { buildSearchBody("q", { includeDomains: [], excludeDomains: [] }); });
    for (const host of ["https://example.com", "example.com/path", "*.example.com", "example.com:443", "foo bar", "-example.com", "example..com"]) {
        rejects("invalid_argument", () => { buildSearchBody("q", { includeDomains: [host] }); });
    }
    rejects("invalid_argument", () => { buildSearchBody("q", { country: "USA" }); });
    rejects("invalid_argument", () => { buildSearchBody("q", { scrapeOptions: { formats: [] } }); });

    label("discovery and optional content preserve attribution and partial failures");
    const bare = normalizeSearchJson('{"success":true,"data":{"web":[{"url":"https://example.com","title":"Example","description":"Snippet"}]},"creditsUsed":2}');
    assert(bare.results[0].content === null && bare.results[0].description === "Snippet" && bare.creditsUsed === 2, "bare result");
    const result = normalizeSearchJson('{"success":true,"data":{"web":[{"url":"https://example.com/a","title":"Search title","description":"Snippet","markdown":"full text","html":"<p>full text</p>","json":{"x":1},"metadata":{"sourceURL":"https://example.com/a","url":"https://example.org/final","custom":42}},{"url":"https://example.com/b","metadata":{"error":"timeout","statusCode":408}},{"url":"https://example.com/c","error":"scrape failed"}]},"warning":"partial","warnings":[{"code":"partial"}]}');
    assert(result.results.length === 3 && result.results[0].title === "Search title", "all search hits preserved");
    const page = result.results[0].content!;
    assert(page.markdown === "full text" && page.url === "https://example.org/final" && JSON.stringify(page.metadata).includes('"custom":42'), "content and raw metadata");
    assert(result.results[1].content!.error === "timeout" && result.results[2].error === "scrape failed", "partial failures");
    assert(result.warning === "partial" && JSON.stringify(result.warnings).includes("partial"), "warnings preserved");
    assert(normalizeSearchJson('{"success":true,"data":{"web":[]}}').results.length === 0, "empty valid results");
    assert(normalizeSearchJson('{"success":true,"data":{"web":[{"url":"https://example.com","markdown":"text"}]}}').results[0].content!.markdown === "text", "metadata optional");

    label("malformed search envelopes fail instead of appearing empty");
    for (const body of ["null", "not json", "{}", '{"success":false,"data":{"web":[]}}', '{"success":true,"data":[]}', '{"success":true,"data":{}}', '{"success":true,"data":{"web":[{"url":3}]}}', '{"success":true,"data":{"web":[{"url":"u","markdown":4}]}}']) {
        rejects("invalid_response", () => { normalizeSearchJson(body); });
    }
}
