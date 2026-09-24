import { label } from "submilli:test";
import { decodeComponent } from "submilli:url";
import {
    BraveSearchError, buildSearchQuery, buildContextQuery,
    normalizeSearchJson, normalizeContextJson, braveHttpError,
} from "@submilli/brave-search";

function expectError(code: string, action: () => void): void {
    let matched = false;
    try { action(); } catch (cause) {
        if (cause instanceof BraveSearchError) matched = cause.code === code && cause.status === 0;
    }
    assert(matched, "expected " + code);
}

function main(): void {
    label("web defaults and query encoding");
    const defaults = buildSearchQuery("a & b/path");
    assert(defaults.includes("q=a%20%26%20b%2F"), "query cannot inject parameters");
    assert(defaults.includes("count=10&offset=0&extra_snippets=true&spellcheck=true"), "web defaults");
    assert(defaults.includes("safesearch=moderate"), "safe default");
    assert(defaults.includes("result_filter=web&text_decorations=false"), "web only without decoration");
    const selected = buildSearchQuery("test", { count: 20, offset: 9, extraSnippets: false,
        spellcheck: false, country: "GB", searchLanguage: "en", freshness: "2026-01-01to2026-02-01", safeSearch: "strict" });
    assert(decodeComponent(selected).includes("country=GB&search_lang=en&freshness=2026-01-01to2026-02-01"), "shared options");
    assert(selected.includes("extra_snippets=false&spellcheck=false"), "explicit false preserved");

    label("query and numeric boundaries reject before HTTP");
    expectError("invalid_argument", () => { buildSearchQuery(" \t\n"); });
    expectError("invalid_argument", () => { buildSearchQuery("a".repeat(601)); });
    expectError("invalid_argument", () => { buildSearchQuery("word ".repeat(76)); });
    assert(buildSearchQuery("word\t".repeat(75)).length > 0, "75 whitespace-separated words accepted");
    assert(buildSearchQuery("a".repeat(600), { count: 1 }).length > 0, "600 characters accepted");
    expectError("invalid_argument", () => { buildSearchQuery("q", { count: 0 }); });
    expectError("invalid_argument", () => { buildSearchQuery("q", { count: 21 }); });
    expectError("invalid_argument", () => { buildSearchQuery("q", { offset: -1 }); });
    expectError("invalid_argument", () => { buildSearchQuery("q", { offset: 10 }); });
    expectError("invalid_argument", () => { buildSearchQuery("q", { count: 1.5 }); });
    expectError("invalid_argument", () => { buildSearchQuery("q", { count: Number("nan") }); });

    label("context budgets and bounds");
    const budgets = buildContextQuery("q");
    assert(budgets.includes("count=20&maximum_number_of_urls=10&maximum_number_of_tokens=4096"), "context defaults");
    assert(budgets.includes("maximum_number_of_tokens_per_url=2048&enable_local=false"), "web context only");
    assert(buildContextQuery("q", { count: 50, maxUrls: 50, maxTokens: 32768, maxTokensPerUrl: 8192 }).length > 0, "upper bounds");
    assert(buildContextQuery("q", { count: 1, maxUrls: 1, maxTokens: 1024, maxTokensPerUrl: 512 }).length > 0, "lower bounds");
    expectError("invalid_argument", () => { buildContextQuery("q", { maxTokens: 1023 }); });
    expectError("invalid_argument", () => { buildContextQuery("q", { maxTokens: 32769 }); });
    expectError("invalid_argument", () => { buildContextQuery("q", { maxUrls: 51 }); });
    expectError("invalid_argument", () => { buildContextQuery("q", { maxUrls: 0 }); });
    expectError("invalid_argument", () => { buildContextQuery("q", { maxTokensPerUrl: 511 }); });
    expectError("invalid_argument", () => { buildContextQuery("q", { maxTokensPerUrl: 8193 }); });
    expectError("invalid_argument", () => { buildContextQuery("q", { count: 51 }); });
    expectError("invalid_argument", () => { buildContextQuery("q", { maxTokens: Infinity }); });

    label("web normalization and pagination metadata");
    const body = '{"type":"search","query":{"original":"orignal","altered":"original","more_results_available":true},"web":{"results":[{"title":"Page","url":"https://example.com","extra_snippets":["second"],"unknown":7}]}}';
    const page = normalizeSearchJson(body, "fallback", 0);
    assert(page.items.length === 1 && page.items[0].description === "", "missing optional description");
    assert(page.items[0].age === null && page.items[0].extraSnippets[0] === "second", "optional metadata");
    assert(page.originalQuery === "orignal" && page.alteredQuery === "original", "query metadata");
    assert(page.nextOffset === 1, "page increment");
    assert(normalizeSearchJson(body, "fallback", 9).nextOffset === null, "provider page cap");
    const empty = normalizeSearchJson('{"type":"search"}', "fallback", 0);
    assert(empty.items.length === 0 && empty.originalQuery === "fallback", "empty response fallback");
    assert(empty.nextOffset === null && empty.alteredQuery === null, "missing metadata ends pagination");
    const sparse = normalizeSearchJson('{"type":"search","web":{"results":[{"title":"Page","url":"https://example.com"}]}}', "q", 0);
    assert(sparse.items[0].extraSnippets.length === 0, "missing snippets normalize empty");
    assert(normalizeSearchJson('{"type":"search","query":{"more_results_available":false},"web":{"results":[]}}', "q", 0).nextOffset === null, "explicit final page");
    expectError("invalid_response", () => { normalizeSearchJson("{}", "q", 0); });
    expectError("invalid_response", () => { normalizeSearchJson('{"type":"search","web":{"results":[{"title":"x"}]}}', "q", 0); });
    expectError("invalid_response", () => { normalizeSearchJson('{"type":"search","web":{"results":[{"title":3,"url":"x"}]}}', "q", 0); });
    expectError("invalid_response", () => { normalizeSearchJson('<html>bad gateway</html>', "q", 0); });

    label("context preserves source order and passages");
    const result = normalizeContextJson('{"grounding":{"generic":[{"title":"A","url":"https://a.example","snippets":["a1","a2"]},{"title":"B","url":"https://b.example","snippets":[]}]},"sources":{}}');
    assert(result.items[0].url === "https://a.example" && result.items[0].snippets[1] === "a2", "source association");
    assert(result.items[1].title === "B", "ordering");
    assert(normalizeContextJson('{"grounding":{}}').items.length === 0, "empty grounding");
    assert(normalizeContextJson('{"grounding":{"generic":[{"title":"A","url":"https://a.example"}]}}').items[0].snippets.length === 0, "missing passages normalize empty");
    expectError("invalid_response", () => { normalizeContextJson("{}"); });
    expectError("invalid_response", () => { normalizeContextJson('{"grounding":{"generic":[{"title":"A","url":"","snippets":[]}]}}'); });
    expectError("invalid_response", () => { normalizeContextJson('{"grounding":{"generic":[{"title":"A","url":"https://a.example","snippets":[3]}]}}'); });
    expectError("invalid_response", () => { normalizeContextJson("{broken"); });

    label("HTTP error metadata stays safe and actionable");
    assert(braveHttpError(401).code === "unauthorized", "authentication mapping");
    assert(braveHttpError(403).code === "forbidden", "permission mapping");
    const rate = braveHttpError(429, "10");
    assert(rate.code === "rate_limited" && rate.status === 429 && rate.retryAfter === "10", "retry metadata");
    assert(braveHttpError(503).code === "http_error" && braveHttpError(503).retryAfter === null, "generic failure");
}
