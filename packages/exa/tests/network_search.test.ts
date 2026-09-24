import { label } from "submilli:test";
import secrets from "submilli:secrets";
import exa from "@submilli/exa";

function main(): void {
    const key = secrets.get("EXA_API_KEY");
    if (key === null || key.trim().length === 0) {
        label("skip: EXA_API_KEY is not bound");
        return;
    }
    label("live search returns sources and highlights");
    const response = exa.search("WebAssembly garbage collection design");
    assert(response.results.length > 0, "search results");
    let passages = 0;
    for (const result of response.results) {
        assert(result.url.startsWith("https://") || result.url.startsWith("http://"), "source URL");
        for (const highlight of result.highlights) {
            if (highlight.length > 0) passages += 1;
        }
    }
    assert(passages > 0, "usable highlights");
}
