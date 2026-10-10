// Covers the deterministic option->header encoding. Authorization is NOT set
// here — the token is read from a secret in the request path, which (along with
// the `read`/`search` network calls) is not exercised by these unit tests.

import { label } from "submilli:test";
import {
    readerHeaders,
    searchHeaders,
    normalizeReaderJson,
    normalizeSearchJson,
} from "@submilli/jina";

function throwsInvalid(action: () => void): boolean {
    try { action(); } catch (cause) {
        return cause instanceof Error && cause.message === "Jina returned an invalid response";
    }
    return false;
}

function main(): void {
    label("reader options become headers");
    const h = readerHeaders({
        engine: "browser",
        targetSelector: "main",
        withLinksSummary: true,
        noCache: true,
        timeout: 30,
        locale: "en-US",
        tokenBudget: 4000,
    });
    assert(h.get("x-engine") === "browser", "engine maps to x-engine");
    assert(h.get("x-target-selector") === "main", "target selector maps through");
    assert(h.get("x-with-links-summary") === "true", "boolean flag renders as \"true\"");
    assert(h.get("x-no-cache") === "true", "no-cache flag renders");
    assert(h.get("x-timeout") === "30", "numeric option stringifies");
    assert(h.get("x-token-budget") === "4000", "token budget stringifies");
    assert(h.get("x-locale") === "en-US", "locale maps through");
    assert(h.get("Authorization") === undefined, "no token value is ever placed in headers here");

    label("unset reader options are omitted");
    const sparse = readerHeaders({ engine: "curl" });
    assert(sparse.get("x-with-links-summary") === undefined, "false flag is omitted, not \"false\"");
    assert(sparse.size === 1, "only the one set option is present");

    label("explicit undefined optional fields are omitted");
    const explicit = readerHeaders({ engine: undefined, timeout: undefined, noCache: false });
    assert(explicit.size === 0, "undefined fields and false flags are omitted");

    label("no options yields no headers");
    const bare = readerHeaders();
    assert(bare.size === 0, "no options means an empty header map");

    label("search options become headers");
    const s = searchHeaders({ site: "example.com", noCache: true });
    assert(s.get("x-site") === "example.com", "site maps to x-site");
    assert(s.get("x-no-cache") === "true", "search no-cache flag renders");
    assert(s.get("x-engine") === undefined, "unset search option omitted");

    label("reader JSON tolerates omitted and null fields");
    const page = normalizeReaderJson('{"code":200,"data":{"title":"T","url":"https://example.com","content":"body","usage":{"tokens":12}}}');
    assert(page.title === "T" && page.description === "" && page.content === "body" && page.tokens === 12, "reader fields");
    const nulls = normalizeReaderJson('{"data":{"title":null,"description":null,"url":"https://example.com","content":null,"usage":{"tokens":null}}}');
    assert(nulls.title === "" && nulls.description === "" && nulls.content === "" && nulls.tokens === 0, "JSON nulls normalize");
    assert(normalizeReaderJson('{"data":{"url":"https://example.com","usage":null}}').tokens === 0, "null usage is zero tokens");
    assert(throwsInvalid(() => { normalizeReaderJson('{"data":null}'); }), "missing reader data is invalid");
    assert(throwsInvalid(() => { normalizeReaderJson("<html>bad gateway</html>"); }), "non-JSON reader body is invalid");

    label("search JSON tolerates omitted and null fields");
    const hits = normalizeSearchJson('{"code":200,"data":[{"title":"A","url":"https://a.example","content":"a","usage":{"tokens":3}},{"title":null,"description":null,"url":"https://b.example","content":null,"usage":null}]}');
    assert(hits.length === 2 && hits[0].title === "A" && hits[0].tokens === 3, "search fields");
    assert(hits[1].title === "" && hits[1].content === "" && hits[1].tokens === 0, "JSON null hit fields normalize");
    assert(normalizeSearchJson('{"code":200,"data":null}').length === 0, "null data means no hits");
    assert(normalizeSearchJson('{"data":[]}').length === 0, "empty data means no hits");
    assert(throwsInvalid(() => { normalizeSearchJson('{"data":[{"title":"x"}]}'); }), "a hit without a URL is invalid");
    assert(throwsInvalid(() => { normalizeSearchJson("{broken"); }), "non-JSON search body is invalid");
}
