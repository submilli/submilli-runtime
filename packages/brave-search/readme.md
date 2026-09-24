# @submilli/brave-search

A synchronous, typed client for Brave web search and LLM context. Requires a
Brave Search API key with access to both endpoints.

## Install and configure

```sh
submilli install submilli/submilli-runtime @submilli/brave-search
submilli blueprint add-package @submilli/brave-search --no-capabilities
submilli blueprint secret add BRAVE_SEARCH_API_KEY --store brave_search_api_key
submilli secret put brave_search_api_key
submilli blueprint capability add brave.com/search
submilli blueprint capability add brave.com/context
```

Run these blueprint commands beside an existing blueprint. `secret put` prompts
for the value; never put it in generated programs. For application-supplied keys,
bind a required harness secret instead:

```yaml
secrets:
  BRAVE_SEARCH_API_KEY:
    harness:
      required: true
```

`add-package` configures the package's HTTP and secret permissions. Caller grants
are separate: grant only the operations needed. Requests target the fixed
`https://api.search.brave.com/res/v1/web/search` and `/res/v1/llm/context` paths.

## Use

```ts
import { search, context } from "@submilli/brave-search";

function main(): string {
    const page = search("WasmGC garbage collection", { count: 5, freshness: "py" });
    const passages = context("WasmGC garbage collection", { maxTokens: 2048 });
    return JSON.stringify({ results: page.items, sources: passages.items });
}
```

See [agent documentation](docs/readme.md) for options, defaults, pagination, and
errors. Named imports and the default package namespace are supported. News,
images, local results, Goggles, generated answers, and arbitrary HTTP overrides
are outside this package's initial scope. Use Jina to read a specific page URL.

References: [Web Search](https://api-dashboard.search.brave.com/api-reference/web/search/get)
and [LLM Context](https://api-dashboard.search.brave.com/documentation/services/llm-context).

## Development

```sh
cargo run -p submilli -- build test -p @submilli/brave-search
```

Offline tests exercise request encoding, limits, response normalization, and
error handling. Live tests make one read request per endpoint and skip explicitly
when `BRAVE_SEARCH_API_KEY` is absent or blank. Bind it through the environment
or the repository-root `.env` file, which the package test runner loads internally.
Live requests consume provider quota; tests check structure rather than ranking.
