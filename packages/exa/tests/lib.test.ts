import { label } from "submilli:test";
import { ExaError, buildSearchBody, buildContentsBody, contentsHosts,
    normalizeSearchJson, normalizeContentsJson, exaHttpError } from "@submilli/exa";

function expectCode(code: string, action: () => void): void {
    let matched = false;
    try { action(); } catch (cause) {
        if (cause instanceof ExaError) matched = cause.code === code && cause.status === 0;
    }
    assert(matched, "expected " + code);
}

function main(): void {
    label("default requests follow Exa guidance without extra controls");
    assert(buildSearchBody("test") === '{"query":"test","contents":{"highlights":true}}', "minimal search");
    assert(buildContentsBody(["https://example.com"]) === '{"urls":["https://example.com"],"highlights":true}', "top-level extraction");
    assert(buildSearchBody("test", {}) === buildSearchBody("test"), "empty options keep defaults");
    const quoted = buildSearchBody('a "quoted" query\nwith a newline');
    const parsed = JSON.parse(quoted) as { query: string };
    assert(parsed.query === 'a "quoted" query\nwith a newline', "JSON escaping round-trips");

    label("explicit extraction overrides have correct shape");
    const search = buildSearchBody("test", { numResults: 3, contents: { mode: "text", maxCharacters: 5000, maxAgeHours: 0, livecrawlTimeout: 5000 } });
    assert(search.includes('"contents":{"text":{"maxCharacters":5000},"maxAgeHours":0,"livecrawlTimeout":5000}'), "nested search options");
    assert(!search.includes('"highlights"') && search.includes('"numResults":3'), "one extraction mode");
    const contents = buildContentsBody(["https://example.com"], { maxCharacters: 1000, maxAgeHours: -1 });
    assert(contents.includes('"highlights":{"maxCharacters":1000},"maxAgeHours":-1'), "explicit highlight budget and cache policy");
    assert(!contents.includes('"contents"'), "no nested contents on contents endpoint");

    label("hard filters are opt-in and escaped");
    const filtered = buildSearchBody("test", { includeDomains: ["example.com/docs", "*.example.org"], excludeDomains: ["other.example"],
        startPublishedDate: "2026-01-01T00:00:00Z", endPublishedDate: "2026-02-01T00:00:00Z" });
    assert(filtered.includes('"includeDomains":["example.com/docs","*.example.org"]'), "domain filters preserve caller intent");
    assert(filtered.includes('"excludeDomains":["other.example"]'), "blocklist");
    expectCode("invalid_argument", () => { buildSearchBody("q", { includeDomains: [" "] }); });
    expectCode("invalid_argument", () => { buildSearchBody("q", { startPublishedDate: "yesterday" }); });
    expectCode("invalid_argument", () => { buildSearchBody("q", { startPublishedDate: "2026-02-01T00:00:00Z", endPublishedDate: "2026-01-01T00:00:00Z" }); });

    label("invalid inputs and bounds reject before network access");
    expectCode("invalid_argument", () => { buildSearchBody(" \n"); });
    expectCode("invalid_argument", () => { buildSearchBody("q", { numResults: 0 }); });
    expectCode("invalid_argument", () => { buildSearchBody("q", { numResults: 101 }); });
    expectCode("invalid_argument", () => { buildSearchBody("q", { numResults: 1.5 }); });
    expectCode("invalid_argument", () => { buildSearchBody("q", { numResults: NaN }); });
    expectCode("invalid_argument", () => { buildContentsBody(["https://example.com"], { maxCharacters: 0 }); });
    expectCode("invalid_argument", () => { buildContentsBody(["https://example.com"], { maxCharacters: Infinity }); });
    expectCode("invalid_argument", () => { buildContentsBody(["https://example.com"], { maxAgeHours: -2 }); });
    expectCode("invalid_argument", () => { buildContentsBody(["https://example.com"], { maxAgeHours: 721 }); });
    expectCode("invalid_argument", () => { buildContentsBody(["https://example.com"], { livecrawlTimeout: 90001 }); });
    expectCode("invalid_argument", () => { buildContentsBody(["https://example.com"], { livecrawlTimeout: 0 }); });
    assert(buildSearchBody("q", { numResults: 100 }).includes('"numResults":100'), "maximum result count");
    assert(buildContentsBody(["https://example.com"], { maxAgeHours: 720, livecrawlTimeout: 90000 }).length > 0, "upper crawl bounds");

    label("URL batch validation and host normalization");
    const hosts = contentsHosts(["https://EXAMPLE.com/a", "http://other.example:8080/b"]);
    assert(hosts[0] === "example.com" && hosts[1] === "other.example", "normalized permission hosts");
    const dotted = contentsHosts(["https://evil.test./a", "https://evil.test../b", "https://EVIL.test./c"]);
    assert(dotted[0] === "evil.test" && dotted[1] === "evil.test" && dotted[2] === "evil.test", "trailing dots are not part of the permission host");
    expectCode("invalid_argument", () => { contentsHosts(["https://./"]); });
    expectCode("invalid_argument", () => { buildContentsBody([]); });
    expectCode("invalid_argument", () => { buildContentsBody(["relative/path"]); });
    expectCode("invalid_argument", () => { buildContentsBody(["file:///etc/passwd"]); });
    expectCode("invalid_argument", () => { buildContentsBody(["https://example.com", "not a URL"]); });
    expectCode("invalid_argument", () => { buildContentsBody(["https://example.com "]); });
    expectCode("invalid_argument", () => { buildContentsBody(["https://example.com/" + "a".repeat(2048)]); });
    const many: string[] = [];
    for (let i = 0; i < 100; i += 1) many.push("https://example.com/" + i.toString());
    assert(contentsHosts(many).length === 100, "maximum batch");
    many.push("https://example.com/extra");
    expectCode("invalid_argument", () => { buildContentsBody(many); });

    label("results preserve sources and normalize optional metadata");
    const result = normalizeSearchJson('{"requestId":"r1","costDollars":{"total":0.007},"results":[{"id":"doc1","url":"https://example.com","highlights":["passage"]}]}');
    assert(result.requestId === "r1" && result.costDollars === 0.007, "request metadata");
    assert(result.results[0].id === "doc1" && result.results[0].highlights[0] === "passage", "source association");
    assert(result.results[0].title === "" && result.results[0].author === null && result.results[0].text === "", "sparse metadata");
    assert(normalizeSearchJson('{"results":[]}').costDollars === null, "empty search is valid");
    expectCode("invalid_response", () => { normalizeSearchJson("{}"); });
    expectCode("invalid_response", () => { normalizeSearchJson('{"results":[{"url":3}]}'); });
    expectCode("invalid_response", () => { normalizeSearchJson('{"results":[{"title":"missing URL"}]}'); });
    expectCode("invalid_response", () => { normalizeSearchJson('<html>bad gateway</html>'); });

    label("HTTP 200 partial and total crawl failures remain visible");
    const partial = normalizeContentsJson('{"results":[{"id":"ok","url":"https://ok.example","text":"page"}],"statuses":[{"id":"ok","status":"success","source":"cached"},{"id":"bad","status":"error","error":{"tag":"CRAWL_TIMEOUT","httpStatusCode":504}}]}');
    assert(partial.results.length === 1 && partial.results[0].text === "page", "successful page");
    assert(partial.statuses[0].source === "cached" && partial.statuses[0].errorTag === null, "provenance");
    assert(partial.statuses[1].errorTag === "CRAWL_TIMEOUT" && partial.statuses[1].httpStatusCode === 504, "failure details");
    const failed = normalizeContentsJson('{"results":[],"statuses":[{"id":"bad","status":"error"}]}');
    assert(failed.results.length === 0 && failed.statuses[0].status === "error", "total crawl failure is not hidden");
    expectCode("invalid_response", () => { normalizeContentsJson('{"results":[]}'); });
    expectCode("invalid_response", () => { normalizeContentsJson('{"results":[],"statuses":[{"id":"x"}]}'); });

    label("HTTP failure mapping preserves retry metadata");
    assert(exaHttpError(400).code === "invalid_request", "invalid request");
    assert(exaHttpError(401).code === "unauthorized", "invalid key");
    assert(exaHttpError(402).code === "quota_exceeded", "credits exhausted");
    assert(exaHttpError(403).code === "forbidden", "access denied");
    const rate = exaHttpError(429, "10");
    assert(rate.code === "rate_limited" && rate.status === 429 && rate.retryAfter === "10", "rate limit");
    assert(exaHttpError(503).code === "http_error", "server error");
}
