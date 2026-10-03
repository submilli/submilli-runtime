# Brave Search

Use `@submilli/brave-search` for web discovery and extracted research context.
Use `search` for one page of titles, URLs, descriptions, and alternative snippets;
use `context` for passages grouped by source URL. Use `@submilli/jina` to read a
specific URL after discovering it.

Credentials are supplied internally through `BRAVE_SEARCH_API_KEY`. Never ask
for or pass an API key in package calls. The caller needs `brave.com/search` or
`brave.com/context`, respectively.

```ts
import brave from "@submilli/brave-search";

function main(): string {
    const page = brave.search("WasmGC garbage collection", { count: 5 });
    return JSON.stringify(page);
}
```

```ts
import { context } from "@submilli/brave-search";

function main(): string {
    const result = context("WasmGC garbage collection", {
        maxUrls: 5,
        maxTokens: 2048,
        maxTokensPerUrl: 1024,
    });
    return JSON.stringify(result.items);
}
```

`search` defaults to 10 results with extra snippets and spellchecking enabled.
`nextOffset` is a page number, not a result index. Pass it as `offset` with the
same query and count; stop when it is `null`. Pages may overlap. The package
never fetches another page automatically. `alteredQuery` records spellchecking.

Both operations accept `country`, `searchLanguage`, `freshness`, and `safeSearch`.
Safe search defaults to `moderate`. Freshness accepts `pd`, `pw`, `pm`, `py`, or
`YYYY-MM-DDtoYYYY-MM-DD`; it refers to Brave's best available page date.
Unspecified locale and freshness use provider defaults.

`context` defaults to 20 search candidates, 10 source URLs, 4096 total tokens,
and 2048 tokens per URL. These are approximate provider-side budgets, not exact
output sizes. Context returns extracted passages, not generated answers, and
local/map results are disabled. Keep URLs with passages for attribution.

Failures throw `BraveSearchError` with `code`, `status`, and nullable `retryAfter`
(the raw HTTP Retry-After value, which may be seconds or a date). Local failures
have status 0. Codes include `invalid_argument`, `missing_credentials`,
`unauthorized`, `forbidden`, `rate_limited`, `http_error`, and `invalid_response`.
Runtime permission and transport errors propagate unchanged. No automatic retries.
HTTP error messages include Brave's response `type` and `error.detail` when
available, including validation failures with status 422. Empty or malformed
error bodies fall back to the HTTP error message.
