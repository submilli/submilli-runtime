import { label } from "submilli:test";
import secrets from "submilli:secrets";
import brave from "@submilli/brave-search";

function main(): void {
    const key = secrets.get("BRAVE_SEARCH_API_KEY");
    if (key === undefined || key.trim().length === 0) {
        label("skip: BRAVE_SEARCH_API_KEY is not bound");
        return;
    }
    label("live web search returns attributed results");
    const page = brave.search("WebAssembly garbage collection", { count: 2 });
    assert(page.items.length > 0 && page.items.length <= 2, "bounded results");
    for (const item of page.items) {
        assert(item.url.startsWith("https://") || item.url.startsWith("http://"), "source URL");
        assert(item.title.length > 0, "title");
    }
}
