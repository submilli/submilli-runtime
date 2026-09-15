// Covers the deterministic option->header encoding. Authorization is NOT set
// here — the token is read from a secret in the request path, which (along with
// the `read`/`search` network calls) is not exercised by these unit tests.

import { label } from "submilli:test";
import {
    readerHeaders,
    searchHeaders,
} from "@submilli/jina";

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
    assert(h.get("Authorization") === null, "no token value is ever placed in headers here");

    label("unset reader options are omitted");
    const sparse = readerHeaders({ engine: "curl" });
    assert(sparse.get("x-with-links-summary") === null, "false flag is omitted, not \"false\"");
    assert(sparse.size === 1, "only the one set option is present");

    label("no options yields no headers");
    const bare = readerHeaders();
    assert(bare.size === 0, "no options means an empty header map");

    label("search options become headers");
    const s = searchHeaders({ site: "example.com", noCache: true });
    assert(s.get("x-site") === "example.com", "site maps to x-site");
    assert(s.get("x-no-cache") === "true", "search no-cache flag renders");
    assert(s.get("x-engine") === null, "unset search option omitted");
}
