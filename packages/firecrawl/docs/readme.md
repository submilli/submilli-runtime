# Firecrawl

Use `@submilli/firecrawl` for known-page retrieval, site URL discovery, and
explicit Batch Scrape or Crawl jobs. Named imports and the default package
namespace are supported. `FIRECRAWL_API_KEY` is supplied internally: never ask
for it, pass it in a call, or print it.

## Retrieve a page or discover URLs

```ts
import { scrape } from "@submilli/firecrawl";

function main(): string {
    const page = scrape("https://example.com", { formats: ["markdown"] });
    return JSON.stringify({ sourceURL: page.sourceURL, title: page.title,
        markdown: page.markdown, warning: page.warning, error: page.error });
}
```

```ts
import { map } from "@submilli/firecrawl";

function main(): string {
    const result = map("https://example.com", { limit: 20, includeSubdomains: false });
    return JSON.stringify(result);
}
```

`scrape(url, options?)` returns a `Page`: markdown, HTML and optional extracted
JSON, source and resolved URLs, title, description, language, source HTTP
status, error, warning, and the complete provider metadata. Missing optional
values are `null`. Keep URLs with the content. Page errors and warnings do not
become request exceptions.

`ScrapeOptions` supports `formats` (`markdown`, `html`; default markdown),
`onlyMainContent`, `includeTags`, `excludeTags`, `maxAge` in milliseconds
(`0` requests fresh content), `storeInCache`, and provider `timeout` in
milliseconds. Unset controls use Firecrawl's defaults. HTTP transport has its
own timeout (normally 30 seconds), which may end before the provider timeout.

Structured extraction is optional: supply `json: { schema, prompt }` to add a
v2 JSON format object. `schema` is a JSON Schema object; `prompt` is optional.
Use `formats: []` with `json` for JSON-only output. This invokes provider
synthesis and can cost more; ordinary markdown/HTML retrieval does not require
it. The returned `page.json` is `unknown`; cast it to the requested concrete
schema before accessing its fields.

`map(url, options?)` returns `{ links, limit }`; links retain provider URL,
title and description. The default limit is 100, with subdomains excluded.
Options: `limit` (1–100000), `search` (relevance ordering), `sitemap`
(`include`, `skip`, `only`), `includeSubdomains`, `ignoreQueryParameters`,
`ignoreCache`. Map has no pagination or reliable completeness indicator.
Reaching `limit` may indicate more URLs exist; even fewer results do not prove
complete coverage. The package never slices the provider response.

## Submit and explicitly inspect jobs

`startBatch(urls, scrapeOptions?)` accepts 1–1000 HTTP(S) URLs and returns
`{ id, invalidURLs }` immediately after submission. Every requested host is
checked before HTTP. It requests `ignoreInvalidURLs: false`; if the provider
nevertheless reports rejected URLs they remain in `invalidURLs`.

`startCrawl(url, options)` requires an explicit `limit` (1–10000 pages).
`allowExternalLinks`, `allowSubdomains`, and `crawlEntireDomain` default to
false and are always sent. Additional options are `maxDiscoveryDepth`,
`includePaths`, `excludePaths`, `sitemap`, `ignoreQueryParameters`, and nested
`scrapeOptions`. Path patterns use the provider's Rust regex syntax. The
provider validates pattern syntax and combined pattern limits. Sitemap entries
count as discovery depth zero; use `sitemap: "skip"` for link-depth bounds.
Limits and patterns are provider instructions, not a network sandbox.

Start a bounded crawl and return its ID without waiting:

```ts
import { startCrawl } from "@submilli/firecrawl";

function main(): string {
    const job = startCrawl("https://example.com", {
        limit: 2,
        maxDiscoveryDepth: 1,
        sitemap: "skip",
        scrapeOptions: { formats: ["markdown"] }
    });
    return JSON.stringify(job);
}
```

`getJob(kind, jobId, next?)` retrieves status and **one page** of results.
`kind` is `"batch"` or `"crawl"`; job IDs must be provider UUIDs, so reserved
account-level routes cannot be reached as jobs. The first call omits `next`; explicitly pass
the previous response's `next` to get the next page. Only exact HTTPS URLs for
the same Firecrawl job with `skip` and optional `limit` are accepted. There is
no implicit polling, retry, pagination loop or sleep. `next` can be present
while the job is still running, including on an empty page; it does not mean
new content is immediately available. Arrange later calls in the harness.

A complete example that submits and immediately inspects available results:

```ts
import { startCrawl, getJob, getJobErrors } from "@submilli/firecrawl";

function main(): string {
    const job = startCrawl("https://example.com", { limit: 2, sitemap: "skip" });
    const page = getJob("crawl", job.id);
    const failures = getJobErrors("crawl", job.id);
    return JSON.stringify({ jobId: job.id, status: page.status, next: page.next,
        pages: page.data, errors: failures.errors, robotsBlocked: failures.robotsBlocked });
}
```

For a job ID saved by your harness, explicitly fetch at most two pages per run:

```ts
import { getJob } from "@submilli/firecrawl";

function main(): string {
    // Replace with the saved provider job ID; this example only compiles in tests.
    const jobId = "55555555-5555-4555-8555-555555555555";
    const first = getJob("crawl", jobId);
    if (first.next === null) return JSON.stringify(first);
    const second = getJob("crawl", jobId, first.next);
    return JSON.stringify({ first: first, second: second });
}
```

`JobPage` preserves status (including unknown future states), `total`,
`completed`, `creditsUsed`, expiry/start/completion timestamps, duration,
job-level error, `next`, and all returned pages. Normal states include
`scraping`, `completed`, `failed`, and `cancelled`. Partial pages remain usable
in running or failed jobs. Counters are provider accounting, not evidence that
every discovered URL succeeded. Results expire; persist them before `expiresAt`.

`getJobErrors(kind, jobId)` returns `{ errors, robotsBlocked }`. Failures retain
URL, error, and optional ID/timestamp. Always inspect this endpoint as well as
page errors: failed pages can be absent from results and counters. Firecrawl
does not guarantee its error list covers every failure class.

`cancelJob(kind, jobId)` returns `{ status: "cancelled" }`. Cancellation is a
separate permission from job reads and submission. It does not imply that
already-consumed credits are refunded or that in-flight work vanished.

## Large results

`downloadJobPage(kind, jobId, path, next?, options?)` downloads exactly one raw
JSON status/results envelope into the VFS. It uses the same validated job URL
as `getJob`; it accepts no object-storage URL argument and does not follow URLs
embedded in response bodies. The parent directory must exist. `options.maxBytes` defaults to 20000000 and `overwrite`
defaults to false. Download returns the standard `DownloadResult`: check its
HTTP `status` before treating the saved body as a successful result. A non-2xx
response is saved too, following the existing `http.download` contract.

```ts
import { downloadJobPage } from "@submilli/firecrawl";

function main(): string {
    const saved = downloadJobPage("crawl", "55555555-5555-4555-8555-555555555555", "/crawl-page.json");
    return JSON.stringify({ status: saved.status, path: saved.path, bytes: saved.bytesWritten });
}
```

Read the file in bounded VFS chunks or through the harness. Inspect its `next`
and make an explicit subsequent download to a different path. Downloads have
a separate transport timeout (normally 60 seconds). Byte/memory limits fail
rather than silently truncate. Scrape/Map responses remain in memory; for a
large known page, submit a one-URL batch and download the result envelope.
Provider format/content selection and page limits still apply.

## Permissions and limits of authorization

| Operation | Caller capability and truthful fields |
| --- | --- |
| Scrape | `firecrawl.dev/scrape { host }` |
| Map | `firecrawl.dev/map { host, limit, includeSubdomains }` |
| Batch submission | `firecrawl.dev/batch.start { host, count }`, checked for every URL |
| Crawl submission | `firecrawl.dev/crawl.start { host, limit, allowSubdomains, allowExternalLinks, crawlEntireDomain }` |
| All four above | Also `firecrawl.dev/delegatedFetch {}` |
| Read status/results/errors or download | `firecrawl.dev/jobs.read { kind, jobId }` |
| Cancel | `firecrawl.dev/jobs.cancel { kind, jobId }` |
| Download destination | Also caller `fs.write { op: "download", path, max_bytes }` |

A submitted-host check cannot restrict Firecrawl's downstream requests.
`delegatedFetch` explicitly authorizes provider-controlled redirects,
subresources, sitemap/discovery fetching and crawl expansion beyond submitted
hosts. Crawl scope options narrow provider behavior; they are not enforced by
Submilli on Firecrawl's servers. Do not grant this capability in a role that
requires a strict downstream-domain allowlist.

Job reads/cancellation do not accept a caller-claimed original host. The status
API does not supply a trustworthy original-host ownership boundary. Instead,
the operator grants specific `{ kind, jobId }` values, or deliberately grants
account-wide job access. Creating a job does not automatically grant access to
it. Unfiltered job permissions cover existing jobs accessible to the bound
credential, including jobs created outside this package. A harness can grant
the returned ID for later runs. A denial is an authorization decision, not an
invitation to retry with different arguments.

All authenticated package requests are constructed under the fixed
`https://api.firecrawl.dev/v2` origin. Provider submission URLs are ignored;
pagination URLs must match the same job and accepted query shape. The standard
runtime HTTP transport strips Authorization on cross-origin redirects;
embedders supplying another transport must preserve this guarantee. Target-page
redirects happen inside Firecrawl and are covered by `delegatedFetch`.

`FirecrawlError` has `code`, HTTP `status` (0 for local errors), and nullable
`retryAfter`. Codes: `invalid_argument`, `missing_credentials`,
`invalid_response`, `invalid_request`, `unauthorized`, `quota_exceeded`,
`forbidden`, `not_found`, `rate_limited`, `http_error`. HTTP errors never expose
raw response bodies or credentials. Runtime permission/transport/resource-limit
errors pass through. A transport timeout after submission may leave a running
job; never blindly retry a charged submission. Pure exported `build*`,
`normalize*`, `urlHost`, `jobPagePath`, and `firecrawlHttpError` helpers perform
no protected effects and are intended for testing.

Search, Agent, legacy Extract, Parse, Monitor, browser sessions, actions,
webhooks, arbitrary HTTP overrides and automatic retries are outside this
package. No LLM-generated extraction is requested unless `json` is supplied.
