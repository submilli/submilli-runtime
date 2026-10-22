# Exa

Use `@submilli/exa` to retrieve web sources for the calling agent's own reasoning.

<!-- submilli:if capability="exa.ai/search" -->

`search(query)` discovers pages with token-efficient highlights.
Callers need `exa.ai/search` for search.

<!-- submilli:endif -->

<!-- submilli:if capability="exa.ai/contents" -->

`getContents(urls)` extracts known URLs and returns per-URL statuses as well as
successful results. Callers need `exa.ai/contents` for every requested URL's
host. All hosts are checked before sending a batch.

<!-- submilli:endif -->

Keep source URLs with all extracted text for attribution.

Credentials are supplied internally. Never request or pass a key in package
calls.

<!-- submilli:if capability="exa.ai/search" -->

```ts
import exa from "@submilli/exa";

function main(): string {
    const response = exa.search("latest developments in WebAssembly garbage collection");
    return JSON.stringify(response.results);
}
```

<!-- submilli:endif -->

<!-- submilli:if capability="exa.ai/contents" -->

For a task that explicitly needs broad page context:

```ts
import { getContents } from "@submilli/exa";

function main(): string {
    const response = getContents(["https://exa.ai/docs"], { mode: "text" });
    return JSON.stringify({ sources: response.results, statuses: response.statuses });
}
```

<!-- submilli:endif -->

<!-- submilli:if any="exa.ai/search exa.ai/contents" -->

## Choose options deliberately

<!-- submilli:if capability="exa.ai/search" -->

- Default to `search(query)` and bare highlights. The provider's default search
  mode and result count apply. There is no pagination cursor.
- Use `numResults` only for an explicit result-count requirement (1–100).
- Use `includeDomains` or `excludeDomains` only for a requested hard allowlist or
  blocklist. They support domain paths and wildcard subdomains. Ordinary source
  preferences belong in the query.
- `startPublishedDate` and `endPublishedDate` enforce ISO 8601 publication windows
  and can drop undated pages. For just "latest" or "recent", phrase it in the query.
- Search extraction controls go in `options.contents`.

<!-- submilli:endif -->

<!-- submilli:if capability="exa.ai/contents" -->

- `getContents` accepts extraction controls directly.

<!-- submilli:endif -->

- `mode` selects highlights (default) or text, never both.
- `maxCharacters` is a per-page extraction budget, not a token limit. Set it only
  for a task with an explicit budget; highlights below about 400 characters tend
  to lose useful context.
- `maxAgeHours` controls cached-content age, not publication recency. `-1` uses
  cache only; `0` requests a live crawl; positive values cap cache age. If live
  crawling matters, supply `livecrawlTimeout` in milliseconds. The HTTP transport
  has its own timeout, which may finish the request sooner.

<!-- submilli:endif -->

<!-- submilli:if capability="exa.ai/contents" -->

Always inspect `getContents().statuses`: HTTP 200 can contain partial or total
crawl failures. Match statuses and results by ID/URL, not array index. Status
records retain provider status, cache/crawl provenance, error tag, and source
HTTP status when present.

<!-- submilli:endif -->

Metadata and cost estimates may be absent; costs are
provider estimates rather than invoices.

`ExaError` includes `code`, HTTP `status` (0 for local errors), and nullable raw
`retryAfter`. Codes are `invalid_argument`, `missing_credentials`,
`invalid_request`, `unauthorized`, `quota_exceeded`, `forbidden`, `rate_limited`,
`http_error`, and `invalid_response`. Runtime permission/transport errors remain
unchanged. The package never retries automatically.

This package supplies retrieval for an agent that already has an LLM. It does
not expose Exa-generated answers/summaries, structured synthesis, async Agent
runs, Monitors, Websets, snapshots, or the deprecated findSimilar endpoint.
