# @submilli/firecrawl

A typed Firecrawl v2 client using Submilli HTTP directly: Search, Scrape, Map, Batch
Scrape and Crawl in one package. Jobs are explicit: submit, inspect one result
page, retrieve errors, download a page to VFS, or cancel. Nothing polls or
retries automatically. See [agent documentation](docs/readme.md) for the API,
compiled examples, result accounting, and authorization boundaries.

## Install and configure

```sh
submilli install submilli/submilli-runtime @submilli/firecrawl
submilli blueprint add-package @submilli/firecrawl --no-capabilities
submilli blueprint secret add FIRECRAWL_API_KEY --store firecrawl_api_key
submilli secret put firecrawl_api_key
submilli blueprint capability add firecrawl.dev/scrape --filter 'host == "example.com"'
submilli blueprint capability add firecrawl.dev/delegatedFetch
```

Run beside an existing blueprint. Enter the key at the hidden `secret put`
prompt. Application-supplied keys can instead use a required harness binding:

```yaml
secrets:
  FIRECRAWL_API_KEY:
    harness:
      required: true
```

`add-package` grants the package its derived HTTP/secret permissions. Caller
operation grants remain separate. Never put credentials in generated programs.
The runtime/build test runner reads the repository-root `.env` internally for
tests; a production blueprint still needs its own secret binding.

## Search

`search(query)` defaults to web discovery: URLs, titles and descriptions. To
include page content explicitly, pass `scrapeOptions`, for example
`search(query, { limit: 2, scrapeOptions: { formats: ["markdown"] } })`.
Scraping adds latency and credits. Search supports result limits of 1–100,
domain inclusion/exclusion, provider time filters, location/country and
SafeSearch. The current v2 endpoint has no documented pagination parameter.
See the [agent guide](docs/readme.md) for compiled examples and result types.

Grant `firecrawl.dev/search` for discovery. Native search with content also
requires `firecrawl.dev/search.scrape` and `firecrawl.dev/delegatedFetch`:
these explicitly authorize provider scraping of unknown result hosts. An
ordinary host-filtered scrape grant does not authorize this mode. For per-host
checks, discover first and use `scrape` on selected result URLs instead. Search
domain filters are not an authorization boundary.

## Authority

Submitted-host grants authorize inputs, **not all downstream fetching**.
Firecrawl controls redirects, subresources, discovery and crawling remotely.
The additional `firecrawl.dev/delegatedFetch` grant explicitly accepts that
boundary. Do not enable it for a role requiring strict downstream host isolation.
This applies to Map and single-page Scrape as well as Crawl and Batch Scrape.
Every batch URL's host is checked before submission.

Crawl grants can constrain the page ceiling and expansion flags. Job read and
cancel grants use `{ kind, jobId }` independently of submission: no caller can
claim a host to authorize an existing job. Broad job grants intentionally
permit jobs accessible to the credential, including pre-existing jobs. For
narrow roles have the harness grant only approved returned IDs.

A bounded crawl grant can use:

```sh
submilli blueprint capability add firecrawl.dev/crawl.start --filter 'host == "example.com" and limit <= 5 and allowSubdomains == false and allowExternalLinks == false and crawlEntireDomain == false'
submilli blueprint capability add firecrawl.dev/jobs.read --filter 'kind == "crawl" and jobId == "66666666-6666-4666-8666-666666666666"'
```

Cancellation needs `firecrawl.dev/jobs.cancel`. VFS downloads additionally
check caller `fs.write` with path and byte cap. The package validates pagination
against the exact HTTPS API origin, job family and job ID, and reconstructs the
request URL. It does not download from arbitrary provider-supplied URLs.
The standard transport removes Authorization on cross-origin redirects.

## Verification

```sh
cargo run -p submilli -- build test -p @submilli/firecrawl
```

Offline package tests cover mappings, optional extraction, full metadata,
partial failures, malformed envelopes, pagination, lifecycle shapes, and
cancellation. Documentation fences are compiled by the package test runner.

The TypeScript policy tests in `tests/policy/` run as a real `main` caller under
restricted blueprints. They verify every batch host, submitted-host and scope
filters, delegation, separate job read/cancel grants, and VFS path/byte grants.
Allowed cases must reach a deliberately denied credential boundary; denied
cases must stop at the named business capability before reading credentials.
These tests need no key or network. Run from the repository root in an isolated
local package store:

```sh
firecrawl_test_home=$(mktemp -d)
SUBMILLI_HOME="$firecrawl_test_home" cargo run -p submilli -- build publish-local -p @submilli/firecrawl
SUBMILLI_HOME="$firecrawl_test_home" cargo run -p submilli -- run packages/firecrawl/tests/policy/scoped.ts --blueprint packages/firecrawl/tests/policy/scoped.yaml
SUBMILLI_HOME="$firecrawl_test_home" cargo run -p submilli -- run packages/firecrawl/tests/policy/no-delegation.ts --blueprint packages/firecrawl/tests/policy/no-delegation.yaml
SUBMILLI_HOME="$firecrawl_test_home" cargo run -p submilli -- run packages/firecrawl/tests/policy/search-content.ts --blueprint packages/firecrawl/tests/policy/search-content.yaml
```

These are separate commands because `build test` uses an unrestricted policy;
its ordinary unit tests cannot prove a `main` caller is constrained.

Live retrieval and search tests (discovery and explicit page scraping) skip without `FIRECRAWL_API_KEY`. For disposable live
job tests, also set `FIRECRAWL_TEST_URL` to an approved public test page in the
root `.env` or environment. These tests create a one-URL batch and a one-page
crawl, explicitly inspect results/errors, and cancel only their own jobs. They
make no polling loop or waits, and cannot require completion before the first
inspection. Live tests consume provider quota. Never commit `.env`.

Official v2 references researched for this implementation:
[Search](https://docs.firecrawl.dev/api-reference/endpoint/search),
[Scrape](https://docs.firecrawl.dev/api-reference/endpoint/scrape),
[Map](https://docs.firecrawl.dev/api-reference/endpoint/map),
[Batch Scrape](https://docs.firecrawl.dev/api-reference/endpoint/batch-scrape),
[Batch status](https://docs.firecrawl.dev/api-reference/endpoint/batch-scrape-get),
[Batch cancellation](https://docs.firecrawl.dev/api-reference/endpoint/batch-scrape-delete),
[Crawl](https://docs.firecrawl.dev/api-reference/endpoint/crawl-post),
[Crawl status](https://docs.firecrawl.dev/api-reference/endpoint/crawl-get),
[Crawl errors](https://docs.firecrawl.dev/api-reference/endpoint/crawl-get-errors),
[Crawl cancellation](https://docs.firecrawl.dev/api-reference/endpoint/crawl-delete).
