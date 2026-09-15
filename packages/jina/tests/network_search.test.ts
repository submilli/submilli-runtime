// Real network integration for Search. Unlike Reader, Jina's Search tier needs
// a token (keyless requests 401), so this runs only when a JINA_API_KEY is
// available. `submilli build test` bridges environment variables to
// `secrets.get`, so `JINA_API_KEY=jina_… submilli build test` exercises it;
// without the key the test skips (main returns before asserting) and passes.

import { label } from "submilli:test";
import secrets from "submilli:secrets";
import { searchJson } from "@submilli/jina";

function main(): void {
    if (secrets.get("JINA_API_KEY") === null) {
        return;
    }

    label("search returns structured results");
    const results = searchJson("Jina AI Reader API");
    assert(results.length > 0, "search returns at least one result");

    const top = results[0];
    assert(top.url.length > 0, "first result has a url");
    assert(top.title.length > 0, "first result has a title");
    assert(top.content.length > 0, "first result has readable content");
}
