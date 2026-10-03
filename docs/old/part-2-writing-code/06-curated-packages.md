---
title: "Curated packages"
description: "The maintained service packages: choose one, install it, configure credentials and permissions, and call it from a program."
slug: old/curated-packages
pagefind: false
sidebar:
  hidden: true
  order: 6
---

An agent that reads a GitHub issue or searches Slack needs more than an HTTP
request: it needs the service's request format, authentication, pagination,
and response types. Submilli's curated packages provide typed functions for
these tasks, with operations you can allow or deny in a blueprint.

These are maintained TypeScript packages in the
[runtime repository](https://github.com/submilli/submilli-runtime/tree/main/packages).
Unlike the built-in `submilli:` modules, they must be installed and listed in
the blueprint before a program can import them. Calls are synchronous, like
standard-library calls: the program gets a value back without `await`.

## Choose a package

The repository currently contains fourteen packages. Each name links to its setup
instructions, including the credentials and service permissions it needs.

| Package | Use it for |
| --- | --- |
| [`@submilli/brave-search`](https://github.com/submilli/submilli-runtime/tree/main/packages/brave-search) | Web search with pagination and extracted LLM context with source URLs |
| [`@submilli/exa`](https://github.com/submilli/submilli-runtime/tree/main/packages/exa) | Web search with highlights and known-URL extraction with per-URL outcomes |
| [`@submilli/firecrawl`](https://github.com/submilli/submilli-runtime/tree/main/packages/firecrawl) | Web search, scrape, map, and explicit batch/crawl jobs with paginated results and per-page failures |
| [`@submilli/github`](https://github.com/submilli/submilli-runtime/tree/main/packages/github) | Repositories, files, commits, issues, pull requests, teams, releases, and search |
| [`@submilli/gmail`](https://github.com/submilli/submilli-runtime/tree/main/packages/gmail) | Search and triage mail, read messages and threads, manage drafts and labels, send mail, and download attachments |
| [`@submilli/google-calendar`](https://github.com/submilli/submilli-runtime/tree/main/packages/google-calendar) | Calendars, events, agendas, free/busy queries, and bounded free-time searches |
| [`@submilli/google-drive`](https://github.com/submilli/submilli-runtime/tree/main/packages/google-drive) | Files, folders, Shared Drives, uploads, downloads, and permissions |
| [`@submilli/jina`](https://github.com/submilli/submilli-runtime/tree/main/packages/jina) | Turn web pages and search results into Markdown or structured data |
| [`@submilli/linear`](https://github.com/submilli/submilli-runtime/tree/main/packages/linear) | Issues, comments, teams, projects, and users |
| [`@submilli/notion`](https://github.com/submilli/submilli-runtime/tree/main/packages/notion) | Search, read and edit pages as Markdown, work with databases, data sources, blocks, comments, and uploads |
| [`@submilli/sentry`](https://github.com/submilli/submilli-runtime/tree/main/packages/sentry) | Sentry Cloud organizations, projects, issues, events, and issue triage |
| [`@submilli/slack-bot`](https://github.com/submilli/submilli-runtime/tree/main/packages/slack-bot) | Bot-owned messages, conversations, users, direct messages, and reactions |
| [`@submilli/slack-user`](https://github.com/submilli/submilli-runtime/tree/main/packages/slack-user) | Search, read, and act as the authenticated Slack user |
| [`@submilli/typesafe`](https://github.com/submilli/submilli-runtime/tree/main/packages/typesafe) | Focused semantic judgments for routing, ranking, selecting known values, and verifying supplied evidence |

The two Slack packages represent different identities. Use `slack-bot` when
the agent acts as your app's bot, and `slack-user` when it acts on behalf of a
particular workspace user. The service still decides what that identity can
access. A blueprint grant cannot give a token permissions it does not have.

## Install and inspect it

With the CLI installed as in the [quickstart](/docs/old/quickstart), install Jina:

```sh
submilli install submilli/submilli-runtime @submilli/jina
submilli docs @submilli/jina
```

`install` fetches the repository, builds the named package, and puts it in the
local package store. It records the resolved commit; to select a particular
revision, append `@<ref>` to the repository name, replacing `<ref>` with a tag,
branch, or commit. Use `--upgrade` when replacing an already installed revision.
Omitting the package name installs all packages declared by that repository.

If you already have this repository checked out, you can instead build from
its root and install the local source:

```sh
submilli build publish-local -p @submilli/jina
```

Both routes make the package available to `submilli docs` and `submilli search`.
Neither grants a program permission to use it. That comes from the blueprint.

## Read one website

Jina's Reader turns a page into Markdown. Its implementation can send a
request without an API key, so this example leaves the optional key unbound.
It needs outbound access to Jina and depends on that service accepting the
request; service errors and rate limits can still prevent a read.

In a new working directory, create a blueprint and add the package:

```sh
submilli blueprint init web-reader
submilli blueprint secret add JINA_API_KEY --harness
submilli blueprint add-package @submilli/jina --no-capabilities
submilli blueprint capability add jina.ai/read --filter 'host == "example.com"'
```

The secret declaration without `--required` permits an absent key. The package
receives `null` in that case and sends no authorization header. Supplying a key
is covered below.

`add-package` lists Jina as an import and writes its own permissions for HTTP,
downloads, and reading `JINA_API_KEY`. `--no-capabilities` leaves the program's
permissions empty. The final command grants the program one operation:
`jina.ai/read`, restricted to the requested URL's host `example.com`.

The distinction matters. The program asks to read `example.com`; the package
contacts `r.jina.ai` to perform that read. The package checks `jina.ai/read`
against the calling program's policy before making the request, and the
runtime separately checks the HTTP call against the package's policy. The
generated package rules cover its full declared requirements, including
search and downloads; review them when configuring a deployment.

Save this program beside `blueprint.yaml`:

```typescript title="read-page.ts"
import { read } from "@submilli/jina";

function main(): string {
  return read("https://example.com");
}
```

```sh
submilli run --blueprint blueprint.yaml read-page.ts
```

On a successful request, the command prints Jina's Markdown response, including
the source URL and extracted page content. The text may vary with the page and
Jina's cache. Only the string returned from `main` would reach the agent in an
agent session.

Change the URL to `https://example.org` and run it again. The program now fails
with `PermissionDeniedError` for caller `main` and capability `jina.ai/read`,
before the package sends the request. Installing the package did not make all
its operations available: search remains denied too, because there is no
`jina.ai/search` rule.

## Search with Brave

Use `@submilli/brave-search` to discover web pages or retrieve extracted passages
with source URLs. `search` returns one page of titles, descriptions, and snippets;
`context` returns passages for grounding an answer. Use Jina's `read` or
`readJson` when you already have a specific URL to read.

Brave requires an API key. In an existing blueprint directory, install and
configure the package:

```sh
submilli install submilli/submilli-runtime @submilli/brave-search
submilli blueprint add-package @submilli/brave-search --no-capabilities
submilli blueprint secret add BRAVE_SEARCH_API_KEY --store brave_search_api_key
submilli secret put brave_search_api_key
submilli blueprint capability add brave.com/search
submilli blueprint capability add brave.com/context
```

Enter the key at the hidden `secret put` prompt. The package reads it internally;
programs pass only queries and options. Grant only the operations your program
needs. `add-package` configures the package's underlying HTTP and secret access.

```typescript title="brave-search.ts"
import { search, context } from "@submilli/brave-search";

function main(): string {
  const page = search("WasmGC garbage collection", { count: 5 });
  const passages = context("WasmGC garbage collection", { maxTokens: 2048 });
  return JSON.stringify({ results: page.items, sources: passages.items });
}
```

Search results include a nullable `nextOffset`: pass it as `offset` with the same
query and count to fetch another page. Offsets count pages, not individual
results, and pages may overlap. Context token budgets are approximate
provider-side limits. Keep source URLs with extracted passages for attribution.
Both operations default to moderate safe search and make no automatic retries.

## Retrieve sources with Exa

Use `@submilli/exa` for semantic web search with highlights or batch extraction
of known URLs. It returns sources for the agent's own reasoning. Brave offers
web search and query-based context; Jina reads individual pages as Markdown.

In an existing blueprint directory, install the package and bind an Exa key:

```sh
submilli install submilli/submilli-runtime @submilli/exa
submilli blueprint add-package @submilli/exa --no-capabilities
submilli blueprint secret add EXA_API_KEY --store exa_api_key
submilli secret put exa_api_key
submilli blueprint capability add exa.ai/search
submilli blueprint capability add exa.ai/contents --filter 'host == "exa.ai"'
```

The key is entered at the hidden prompt and read only inside the package.
The contents grant above permits requested URLs on `exa.ai`; each URL in a
batch is checked before the request is sent. Adjust it for the hosts the task
needs. The search grant permits web discovery and does not restrict result hosts.

```typescript title="exa-search.ts"
import { search, getContents } from "@submilli/exa";

function main(): string {
  const found = search("WebAssembly garbage collection design");
  const pages = getContents(["https://exa.ai/docs"]);
  return JSON.stringify({ results: found.results, pages: pages });
}
```

Both calls default to highlights. Request `mode: "text"` for extraction only
when the task needs broad page context. Additional count, domain, publication,
character-budget, and cache-freshness controls are opt-in. Always inspect
`getContents().statuses`: a successful HTTP response can include failed URLs.
Keep source URLs with passages for attribution.

## Firecrawl: search, retrieval and explicit crawl jobs

`@submilli/firecrawl` supports Search, Scrape, Map, Batch Scrape and Crawl using the v2
HTTP API. It reads `FIRECRAWL_API_KEY` internally. Scrape returns Markdown,
HTML, or optional structured JSON with source URLs and metadata. Job submission
returns an ID immediately; callers explicitly retrieve status/results pages,
inspect per-page failures, download large envelopes to VFS, or cancel.

Search defaults to discovery only. Explicit `scrapeOptions` adds page content,
latency, and credits. Discovery requires `firecrawl.dev/search`; native result
scraping also requires `firecrawl.dev/search.scrape` and
`firecrawl.dev/delegatedFetch`, which authorize unknown result hosts. To enforce
submitted-host checks, search first and scrape selected URLs separately. Search
domain filters are not a security boundary.

Submitted-host checks do not constrain Firecrawl's remote redirects or crawl
expansion. Scrape, Map, Batch and Crawl submission additionally require the
`firecrawl.dev/delegatedFetch` capability. Job reads and cancellation use
separate `{ kind, jobId }` grants; an unfiltered grant covers existing jobs
accessible to the bound credential. Use a different retrieval boundary when
strict downstream-domain isolation is required.

See the [package setup](https://github.com/submilli/submilli-runtime/tree/main/packages/firecrawl)
and [agent API guide](https://github.com/submilli/submilli-runtime/blob/main/packages/firecrawl/docs/readme.md)
for bounded crawl examples, explicit pagination, and the full capability table.

## Semantic judgments with TypeSafe

For a bounded language judgment, use the installed `@submilli/typesafe` package.
TypeSafe's Jev model evaluates supplied text or application state and returns
typed answers with probabilities. Use it to route requests, rank retrieved
passages, select a known candidate, or check a claim against source evidence.
Keep exact lookups, calculations, policy checks, and actions in ordinary code;
use `submilli:llm` when the work needs generated text or reasoning explanations.
This follows TypeSafe's [System One guidance](https://docs.typesafe.ai/concepts/system-one).

| Judgment | Primitive | Result |
| --- | --- | --- |
| Select one known option | `choice` | Winning option, probabilities for every option, and confidence |
| Decide whether a condition holds | `noul` | Probability of yes; near 0.5 means uncertainty, not medium intensity |
| Rate one dimension against described levels | `score` | Fractional position along the levels, their probabilities, and confidence |

For one judgment, `choice`, `noul`, and `score` each make one HTTP call and return
a concrete answer type. For several judgments, create definitions with
`choiceQuestion`, `noulQuestion`, and `scoreQuestion`, then pass them to `batch`
to evaluate independent questions against shared state in one request.
Supply the relevant evidence and complete instructions: it cannot fetch missing
records or see the agent's conversation. Include a no-match option when no
candidate may fit, and describe Score levels as concrete situations. Questions
in a batch cannot see each other's answers. Code selects which results matter
and makes another call when new evidence or options depend on an earlier answer.

Confidence describes how concentrated the answer's probabilities are; it does
not guarantee correctness or authorize an action. Evaluate thresholds on your
own examples and route uncertain cases to more evidence, a reasoning model, or
human review. Keep the returned probabilities available when combining results.

In an existing blueprint directory:

```sh
submilli install submilli/submilli-runtime @submilli/typesafe
submilli blueprint add-package @submilli/typesafe --no-capabilities
submilli blueprint secret add TYPESAFE_AI_KEY --store typesafe_ai_key
submilli secret put typesafe_ai_key
submilli blueprint capability add typesafe.ai/systemone
submilli docs @submilli/typesafe
```

Enter the key at the hidden prompt. The package reads `TYPESAFE_AI_KEY`
internally and sends requests only to `POST api.typesafe.ai/v1/systemone`.
`batch` defaults to `jev-latest`; its `model` option permits pinning a version.
It returns the answering model and token usage alongside the judgments. Calls
consume TypeSafe credits and do not retry automatically. They use the package's
HTTP permissions, not the `llm.call` model configuration or reserved token budget.

The [agent guide](https://github.com/submilli/submilli-runtime/blob/main/packages/typesafe/docs/readme.md)
is also included in `submilli docs` output. It covers when to use the package,
state preparation, question design, independent batching, uncertainty, and
errors, with a complete example using all three primitives.

## Supply credentials outside the program

Most packages need a service credential. Their setup instructions name the
secret to bind: `GITHUB_TOKEN` for GitHub, `LINEAR_API_KEY` for Linear, and
`GOOGLE_ACCESS_TOKEN` for the three Google packages, for example. The program
passes task arguments to package functions; it does not pass credentials.

To use a Jina key with the local example, replace the optional harness binding
with a local secret-store binding and enter the key at the hidden prompt:

```sh
submilli blueprint secret remove JINA_API_KEY
submilli blueprint secret add JINA_API_KEY --store jina_api_key
submilli secret put jina_api_key
```

The same `read-page.ts` now makes an authenticated request. The blueprint holds
the store key's name, not the credential value. The package reads the value
through `submilli:secrets`; generated programs cannot call that module, even
if a blueprint grants them `secrets.get`. These packages attach their own
credentials, so they do not need an `auth_proxy` rule.

For a service acting on behalf of each user, declare a required harness secret
instead. This is a blueprint fragment for a Google package:

```yaml title="blueprint.yaml (fragment)"
secrets:
  GOOGLE_ACCESS_TOKEN:
    harness:
      required: true
```

The application supplies that user's token when opening the session. The Google
packages do not refresh tokens; the application must replace expired ones.
The package's service scopes and resource access still apply alongside the
blueprint's rules. See [connecting to your harness](/docs/old/harness) for session
setup and [crafting a blueprint](/docs/old/blueprints) for other secret sources.

## Find the next operation

Look up the installed package's declarations and capability filter fields:

```sh
submilli search jina
submilli docs @submilli/jina
submilli blueprint capability list @submilli/jina
submilli blueprint capability list @submilli/jina --unconfigured
```

The declarations show function signatures and descriptions; the capability
list shows what the blueprint can restrict. `--unconfigured` narrows it to the
operations `main` has no rule for. In this example, search is one of them.
That is intentional: the blueprint permits reading one host, not searching the
web, and `default: deny` refuses the rest.

The agent can retrieve package documentation through `packages.search` and
`packages.docs`, the lookup tools introduced in
[the standard library](/docs/old/standard-library#looking-things-up). You choose
the packages, credentials, and permissions; the agent discovers their APIs
and composes calls into a program.

Next: [crafting a blueprint](/docs/old/blueprints), where these grants become a
policy for an agent's whole session.
