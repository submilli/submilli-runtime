import { label } from "submilli:test";
import secrets from "submilli:secrets";
import { context } from "@submilli/brave-search";

function main(): void {
    const key = secrets.get("BRAVE_SEARCH_API_KEY");
    if (key === undefined || key.trim().length === 0) {
        label("skip: BRAVE_SEARCH_API_KEY is not bound");
        return;
    }
    label("live context returns passages with source URLs");
    const result = context("WebAssembly garbage collection", { count: 5, maxUrls: 2, maxTokens: 2048, maxTokensPerUrl: 1024 });
    assert(result.items.length > 0, "context sources");
    let passages = 0;
    for (const item of result.items) {
        assert(item.url.startsWith("https://") || item.url.startsWith("http://"), "source URL");
        for (const snippet of item.snippets) {
            if (snippet.length > 0) passages += 1;
        }
    }
    assert(passages > 0, "usable extracted text");
}
