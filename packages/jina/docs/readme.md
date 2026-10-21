# Jina

Use `@submilli/jina` to turn web pages and search results into text suitable for
research, summarization, extraction, and grounding. Use `submilli:http` instead
when raw response bytes, headers, or general HTTP behavior are required.

<!-- submilli:if capability="jina.ai/read" -->

- `read` returns one URL as LLM-friendly markdown; `readJson` returns structured
  metadata and markdown content.

<!-- submilli:endif -->

<!-- submilli:if capability="jina.ai/search" -->

- `search` concatenates search results as markdown; `searchJson` returns
  structured result objects.

<!-- submilli:endif -->

<!-- submilli:if all="jina.ai/read fs.write" -->

- `downloadRead` streams a page directly to the VFS.

<!-- submilli:endif -->

<!-- submilli:if all="jina.ai/search fs.write" -->

- `downloadSearch` streams search results directly to the VFS.

<!-- submilli:endif -->

<!-- submilli:if capability="fs.write" -->

<!-- submilli:if any="jina.ai/read jina.ai/search" -->

Prefer streaming to the VFS when bringing the whole response into Wasm memory
is unnecessary. The file is written for the caller, so the caller's `fs.write`
permission must allow the path. A response over 20 MB is refused.

<!-- submilli:endif -->

<!-- submilli:endif -->

Set only the options needed for the task.

<!-- submilli:if capability="jina.ai/read" -->

Reader options can control selectors, links, images, generated alt text, and
cache behavior.

<!-- submilli:endif -->

<!-- submilli:if capability="jina.ai/search" -->

Search options can restrict results to one site and set the locale and a
per-result timeout.

<!-- submilli:endif -->

Credentials, when available, are supplied internally. Never request, accept, or
pass an API key in package calls. Each function checks its `jina.ai/read` or
`jina.ai/search` capability. A permission denial means the operation is
forbidden, not mis-called.

<!-- submilli:if capability="jina.ai/search" -->

## Example

Search the web and keep the structured results.

```ts
import jina from "@submilli/jina";

function main(): string {
    const results = jina.searchJson("WasmGC garbage collection design");
    if (results.length === 0) return "No results.";
    const lines: string[] = [];
    for (const result of results) {
        lines.push(result.title + " — " + result.url);
    }
    return lines.join("\n");
}
```

<!-- submilli:endif -->
