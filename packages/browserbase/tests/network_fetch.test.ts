import { label } from "submilli:test";
import secrets from "submilli:secrets";
import { fetchPage } from "@submilli/browserbase";

function main(): void {
    const key = secrets.get("BROWSERBASE_API_KEY");
    if (key === undefined || key.trim().length === 0) {
        label("skip: BROWSERBASE_API_KEY is not bound");
        return;
    }
    label("live fetch returns markdown for example.com");
    const page = fetchPage("https://example.com");
    assert(page.statusCode === 200, "origin status");
    assert(page.content.length > 0, "page text");
    assert(page.content.toLowerCase().includes("example"), "example.com content");
}
