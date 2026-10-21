// The search request builder is private: these tests reach it through `search`.
// Invalid input fails with `invalid_argument` before the check, credential read or
// HTTP. Valid input stops at the credential read with `missing_credentials`; that
// segment is skipped when FIRECRAWL_API_KEY is bound, so no request is sent.
import { label, expectException } from "submilli:test";
import secrets from "submilli:secrets";
import { search, FirecrawlError } from "@submilli/firecrawl";

function rejects(code: string, action: () => void): void {
    const error = expectException(action, "FirecrawlError");
    assert(error instanceof FirecrawlError && error.code === code, "error code");
}
function main(): void {
    label("search validates request limits and mutually exclusive filters");
    rejects("invalid_argument", () => { search(" "); });
    rejects("invalid_argument", () => { search("q".repeat(501)); });
    for (const limit of [0, 101, 1.5, NaN, Infinity]) rejects("invalid_argument", () => { search("q", { limit: limit }); });
    rejects("invalid_argument", () => { search("q", { includeDomains: [], excludeDomains: [] }); });
    for (const host of ["https://example.com", "example.com/path", "*.example.com", "example.com:443", "foo bar", "-example.com", "example..com"]) {
        rejects("invalid_argument", () => { search("q", { includeDomains: [host] }); });
    }
    rejects("invalid_argument", () => { search("q", { country: "USA" }); });
    rejects("invalid_argument", () => { search("q", { scrapeOptions: { formats: [] } }); });
    rejects("invalid_argument", () => { search("q", { scrapeOptions: { json: { schema: "bad" } } }); });

    const key = secrets.get("FIRECRAWL_API_KEY");
    if (key !== undefined && key.trim().length > 0) {
        label("skip: valid search reaches the credential read only when FIRECRAWL_API_KEY is not bound");
        return;
    }
    label("supported filters and opt-in extraction pass local validation");
    rejects("missing_credentials", () => { search("test"); });
    rejects("missing_credentials", () => { search('a "quoted" query\nnext', {}); });
    rejects("missing_credentials", () => { search("query", { limit: 100, includeDomains: ["EXAMPLE.com"], tbs: "sbd:1,qdr:w",
        country: "uk", location: "London,England,United Kingdom", safe: true, timeout: 10000 }); });
    rejects("missing_credentials", () => { search("q", { excludeDomains: ["example.com"], safe: false }); });
    rejects("missing_credentials", () => { search("q", { scrapeOptions: {} }); });
    rejects("missing_credentials", () => { search("q", { scrapeOptions: { formats: ["markdown", "html"], maxAge: 0, includeTags: ["article"] } }); });
}
