# Jina

Use `@submilli/jina` to turn web pages and search results into text suitable for
research, summarization, extraction, and grounding. Use `submilli:http` instead
when raw response bytes, headers, or general HTTP behavior are required.

- `read` returns one URL as LLM-friendly markdown; `readJson` returns structured
  metadata and markdown content.
- `search` concatenates search results as markdown; `searchJson` returns
  structured result objects.
- `downloadRead` and `downloadSearch` stream large results directly to the VFS.
  Prefer them when bringing the whole response into Wasm memory is unnecessary.
  The file is written for the caller, so the blueprint's `fs.write` rule for
  `main` must allow the path. A response over 20 MB is refused.

Set only the options needed for the task. Reader options can control selectors,
links, images, generated alt text, and cache behavior. Search options can bound
the result count and restrict sites.

Credentials, when available, are supplied internally. Never request, accept, or
pass an API key in package calls.

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
