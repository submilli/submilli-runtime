// Real network integration: hits r.jina.ai keyless (no token needed) against a
// stable target. Requires outbound network; `build test` runs it every time.
// Search is intentionally not covered here — Jina's keyless Search tier is
// unreliable, so it would flake without a token.

import { label } from "submilli:test";
import { read, readJson } from "@submilli/jina";

const TARGET = "https://example.com";

function main(): void {
    label("reader fetches a real page as markdown");
    // Fetch the origin: Jina's shared cache may contain an unrelated snapshot.
    const md = read(TARGET, { noCache: true });
    assert(md.length > 0, "markdown body is non-empty");
    assert(md.includes("Example Domain"), "markdown carries the page heading");

    label("reader returns a structured result");
    // In JSON mode Jina splits the page: the heading is the title, the body is
    // the content. Assert on stable invariants — the body text itself drifts.
    const result = readJson(TARGET, { noCache: true });
    assert(result.title === "Example Domain", "title is the page heading");
    assert(result.url.includes("example.com"), "result echoes the source url");
    assert(result.content.length > 0, "structured content is non-empty");
    assert(result.tokens > 0, "usage token count is parsed from the envelope");
}
