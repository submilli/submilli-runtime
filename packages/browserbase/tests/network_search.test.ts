import { label } from "submilli:test";
import secrets from "submilli:secrets";
import { search } from "@submilli/browserbase";

function main(): void {
    const key = secrets.get("BROWSERBASE_API_KEY");
    if (key === undefined || key.trim().length === 0) {
        label("skip: BROWSERBASE_API_KEY is not bound");
        return;
    }
    label("live search returns source URLs");
    const found = search("example domain", { numResults: 3, provider: "parallel", mode: "fast" });
    assert(found.results.length > 0, "search results");
    for (const result of found.results) {
        assert(result.url.startsWith("https://") || result.url.startsWith("http://"), "source URL");
        assert(result.title.length > 0, "title");
    }
}
