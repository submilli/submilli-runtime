---
title: "Curated packages"
description: "The packages Submilli maintains for common services: each package, what it is for, the secret it reads, the hosts it reaches, its readme, and the commands that install it and grant one of its capabilities."
slug: reference/curated-packages
sidebar:
  order: 7
---

This page lists the curated packages, the `@submilli/*` packages maintained
in the `submilli/submilli-runtime` repository, under `packages/<name>`. All
are at version `0.1.0`. How to add a package to a blueprint and bind its
secret is in [Packages](/docs/packages) and
[HTTP and credentials](/docs/blueprints/http-and-credentials).

## The packages

| Package | For | Secret | Hosts |
| --- | --- | --- | --- |
| [`@submilli/brave-search`](#submillibrave-search) | Web search, one page of results at a time, and extracted passages grouped by source URL. | `BRAVE_SEARCH_API_KEY` | `api.search.brave.com` |
| [`@submilli/exa`](#submilliexa) | Web search with highlights, and extraction of known URLs with a status for each URL. | `EXA_API_KEY` | `api.exa.ai` |
| [`@submilli/firecrawl`](#submillifirecrawl) | Firecrawl v2 web search, page scraping, site URL discovery, and explicit Batch Scrape and Crawl jobs. | `FIRECRAWL_API_KEY` | `api.firecrawl.dev` |
| [`@submilli/github`](#submilligithub) | GitHub.com repositories, file contents, commits, branches, issues, pull requests, teams, releases, and search. | `GITHUB_TOKEN` | `api.github.com` |
| [`@submilli/gmail`](#submilligmail) | Gmail profiles, thread search and triage, messages, drafts, sending and replying, labels, and attachment downloads. | `GOOGLE_ACCESS_TOKEN` | `gmail.googleapis.com` |
| [`@submilli/google-calendar`](#submilligoogle-calendar) | Google Calendar calendars, events, agendas, free/busy queries, and bounded free-time searches. | `GOOGLE_ACCESS_TOKEN` | `www.googleapis.com` |
| [`@submilli/google-drive`](#submilligoogle-drive) | Google Drive files and folders in My Drive and Shared Drives: search, read, download, upload, organize, and share. | `GOOGLE_ACCESS_TOKEN` | `www.googleapis.com` |
| [`@submilli/jina`](#submillijina) | Web pages and search results as Markdown or structured data, through Jina Reader and Search. | `JINA_API_KEY` | `r.jina.ai`, `s.jina.ai` |
| [`@submilli/linear`](#submillilinear) | Linear issues, comments, teams, projects, users, and agent sessions, through Linear's GraphQL API. | `LINEAR_API_KEY` | `api.linear.app` |
| [`@submilli/notion`](#submillinotion) | Notion search, pages as enhanced Markdown, databases, data sources, views, comments, users, blocks, and file uploads. | `NOTION_ACCESS_TOKEN` | `api.notion.com` |
| [`@submilli/sentry`](#submillisentry) | Sentry Cloud organizations, projects, issues, and events, and issue triage. | `SENTRY_AUTH_TOKEN` | `sentry.io` |
| [`@submilli/slack-bot`](#submillislack-bot) | Slack as the app's bot: bot messages, conversation history and threads, conversations and members, direct messages, users, and reactions. | `SLACK_BOT_TOKEN` | `slack.com` |
| [`@submilli/slack-user`](#submillislack-user) | Slack as the authenticated user: search, messages and threads, channels, users, files, sending messages, and reactions. | `SLACK_USER_TOKEN` | `files.slack.com`, `slack.com` |
| [`@submilli/typesafe`](#submillitypesafe) | Semantic judgments by TypeSafe's Jev model: choice, yes/no, and rubric-scored questions with typed answers and probabilities. | `TYPESAFE_AI_KEY` | `api.typesafe.ai` |

## Package entries

Each entry below has the same rows, taken from the package's
`capabilities.yaml` and source:

| Row | Holds |
| --- | --- |
| Secret | The name the package reads with `secrets.get`. Its `requires` entry is `secrets.get` with `name == "<secret>"` |
| Credential | The value the secret must hold |
| Without the secret | What a call does when `secrets.get` returns `null` for the secret |
| HTTP | Each host the package requires, with the `http.<method>` capabilities for it. `download` is `http.download`. A path is the `path ==` term of a requirement's filter. |
| Filesystem | The `fs.*` capabilities the package requires, all without a filter |
| Readme | The package's readme on GitHub, with its setup and development notes |

Each entry ends with the same four commands, run in the directory of a
blueprint:

| Command | Does |
| --- | --- |
| `submilli install submilli/submilli-runtime @submilli/<name>` | Fetches the repository, builds the package, and installs it in the local package store. `@<ref>` after the repository name pins a branch, tag, or commit. |
| `submilli blueprint add-package @submilli/<name> --no-capabilities` | Lists the package in `blueprint.yaml` and writes the rules the package needs for its own calls, and grants the program nothing |
| `submilli blueprint capability list @submilli/<name>` | Prints the package's capabilities, their fields, and the rules for them |
| `submilli blueprint capability add <capability>` | Allows the program one capability, here one that reads. `--filter` narrows it |

`submilli docs @submilli/<name>` prints an installed package's
declarations.

## @submilli/brave-search

Web search, one page of results at a time, and extracted passages grouped by source URL.

| | |
| --- | --- |
| Secret | `BRAVE_SEARCH_API_KEY` |
| Credential | A Brave Search API key with access to the web search and LLM context endpoints |
| Without the secret | Throws `BraveSearchError` with code `missing_credentials` |
| HTTP | `api.search.brave.com`: GET |
| Filesystem | None |
| Readme | [`packages/brave-search/readme.md`](https://github.com/submilli/submilli-runtime/blob/main/packages/brave-search/readme.md) |

```sh
submilli install submilli/submilli-runtime @submilli/brave-search
submilli blueprint add-package @submilli/brave-search --no-capabilities
submilli blueprint capability list @submilli/brave-search
submilli blueprint capability add brave.com/search
```

## @submilli/exa

Web search with highlights, and extraction of known URLs with a status for each URL.

| | |
| --- | --- |
| Secret | `EXA_API_KEY` |
| Credential | An Exa API key |
| Without the secret | Throws `ExaError` with code `missing_credentials` |
| HTTP | `api.exa.ai`: POST `/contents`, POST `/search` |
| Filesystem | None |
| Readme | [`packages/exa/readme.md`](https://github.com/submilli/submilli-runtime/blob/main/packages/exa/readme.md) |

```sh
submilli install submilli/submilli-runtime @submilli/exa
submilli blueprint add-package @submilli/exa --no-capabilities
submilli blueprint capability list @submilli/exa
submilli blueprint capability add exa.ai/search
```

## @submilli/firecrawl

Firecrawl v2 web search, page scraping, site URL discovery, and explicit Batch Scrape and Crawl jobs.

| | |
| --- | --- |
| Secret | `FIRECRAWL_API_KEY` |
| Credential | A Firecrawl API key |
| Without the secret | Throws `FirecrawlError` with code `missing_credentials` |
| HTTP | `api.firecrawl.dev`: DELETE, download, GET, POST `/v2/batch/scrape`, POST `/v2/crawl`, POST `/v2/map`, POST `/v2/scrape`, POST `/v2/search` |
| Filesystem | `fs.write` |
| Readme | [`packages/firecrawl/readme.md`](https://github.com/submilli/submilli-runtime/blob/main/packages/firecrawl/readme.md) |

```sh
submilli install submilli/submilli-runtime @submilli/firecrawl
submilli blueprint add-package @submilli/firecrawl --no-capabilities
submilli blueprint capability list @submilli/firecrawl
submilli blueprint capability add firecrawl.dev/scrape
```

## @submilli/github

GitHub.com repositories, file contents, commits, branches, issues, pull requests, teams, releases, and search.

| | |
| --- | --- |
| Secret | `GITHUB_TOKEN` |
| Credential | A GitHub personal access token, OAuth token, or GitHub App installation token |
| Without the secret | Throws `GitHubError` with code `missing_token` |
| HTTP | `api.github.com`: DELETE, GET, PATCH, POST, PUT |
| Filesystem | None |
| Readme | [`packages/github/readme.md`](https://github.com/submilli/submilli-runtime/blob/main/packages/github/readme.md) |

```sh
submilli install submilli/submilli-runtime @submilli/github
submilli blueprint add-package @submilli/github --no-capabilities
submilli blueprint capability list @submilli/github
submilli blueprint capability add github.com/issues.list
```

## @submilli/gmail

Gmail profiles, thread search and triage, messages, drafts, sending and replying, labels, and attachment downloads.

| | |
| --- | --- |
| Secret | `GOOGLE_ACCESS_TOKEN` |
| Credential | A Google OAuth access token with a Gmail scope (`gmail.modify` covers every operation). The package doesn't refresh it |
| Without the secret | Throws `GmailError` with code `missing_token` |
| HTTP | `gmail.googleapis.com`: DELETE, GET, POST |
| Filesystem | `fs.read`, `fs.stat`, `fs.write` |
| Readme | [`packages/gmail/readme.md`](https://github.com/submilli/submilli-runtime/blob/main/packages/gmail/readme.md) |

```sh
submilli install submilli/submilli-runtime @submilli/gmail
submilli blueprint add-package @submilli/gmail --no-capabilities
submilli blueprint capability list @submilli/gmail
submilli blueprint capability add submilli/gmail.searchThreads
```

## @submilli/google-calendar

Google Calendar calendars, events, agendas, free/busy queries, and bounded free-time searches.

| | |
| --- | --- |
| Secret | `GOOGLE_ACCESS_TOKEN` |
| Credential | A Google OAuth access token with a Calendar scope (`calendar` covers every operation). The package doesn't refresh it |
| Without the secret | Throws `CalendarError` with code `missing_token` |
| HTTP | `www.googleapis.com`: DELETE, GET, PATCH, POST |
| Filesystem | None |
| Readme | [`packages/google-calendar/readme.md`](https://github.com/submilli/submilli-runtime/blob/main/packages/google-calendar/readme.md) |

```sh
submilli install submilli/submilli-runtime @submilli/google-calendar
submilli blueprint add-package @submilli/google-calendar --no-capabilities
submilli blueprint capability list @submilli/google-calendar
submilli blueprint capability add submilli/google-calendar.listEvents
```

## @submilli/google-drive

Google Drive files and folders in My Drive and Shared Drives: search, read, download, upload, organize, and share.

| | |
| --- | --- |
| Secret | `GOOGLE_ACCESS_TOKEN` |
| Credential | A Google OAuth access token with a Drive scope (`drive` covers every operation). The package doesn't refresh it |
| Without the secret | Throws `DriveError` with code `missing_token` |
| HTTP | `www.googleapis.com`: DELETE, download, GET, PATCH, POST, PUT |
| Filesystem | `fs.read`, `fs.stat`, `fs.write` |
| Readme | [`packages/google-drive/readme.md`](https://github.com/submilli/submilli-runtime/blob/main/packages/google-drive/readme.md) |

```sh
submilli install submilli/submilli-runtime @submilli/google-drive
submilli blueprint add-package @submilli/google-drive --no-capabilities
submilli blueprint capability list @submilli/google-drive
submilli blueprint capability add submilli/google-drive.searchFiles
```

## @submilli/jina

Web pages and search results as Markdown or structured data, through Jina Reader and Search.

| | |
| --- | --- |
| Secret | `JINA_API_KEY` |
| Credential | An optional Jina API key |
| Without the secret | Sends the request without an `Authorization` header |
| HTTP | `r.jina.ai`: download, POST `/`; `s.jina.ai`: download, POST `/` |
| Filesystem | `fs.write` |
| Readme | [`packages/jina/readme.md`](https://github.com/submilli/submilli-runtime/blob/main/packages/jina/readme.md) |

```sh
submilli install submilli/submilli-runtime @submilli/jina
submilli blueprint add-package @submilli/jina --no-capabilities
submilli blueprint capability list @submilli/jina
submilli blueprint capability add jina.ai/read
```

## @submilli/linear

Linear issues, comments, teams, projects, users, and agent sessions, through Linear's GraphQL API.

| | |
| --- | --- |
| Secret | `LINEAR_API_KEY` |
| Credential | A Linear personal API key, or an OAuth access token |
| Without the secret | Sends the request without an `Authorization` header |
| HTTP | `api.linear.app`: POST `/graphql` |
| Filesystem | None |
| Readme | [`packages/linear/readme.md`](https://github.com/submilli/submilli-runtime/blob/main/packages/linear/readme.md) |

```sh
submilli install submilli/submilli-runtime @submilli/linear
submilli blueprint add-package @submilli/linear --no-capabilities
submilli blueprint capability list @submilli/linear
submilli blueprint capability add linear.app/listIssues
```

## @submilli/notion

Notion search, pages as enhanced Markdown, databases, data sources, views, comments, users, blocks, and file uploads.

| | |
| --- | --- |
| Secret | `NOTION_ACCESS_TOKEN` |
| Credential | The installation access token of a Notion connection that the pages and databases are shared with |
| Without the secret | Throws `NotionError` with code `missing_token` |
| HTTP | `api.notion.com`: GET, PATCH, POST |
| Filesystem | `fs.read`, `fs.stat` |
| Readme | [`packages/notion/readme.md`](https://github.com/submilli/submilli-runtime/blob/main/packages/notion/readme.md) |

```sh
submilli install submilli/submilli-runtime @submilli/notion
submilli blueprint add-package @submilli/notion --no-capabilities
submilli blueprint capability list @submilli/notion
submilli blueprint capability add submilli/notion.search
```

## @submilli/sentry

Sentry Cloud organizations, projects, issues, and events, and issue triage.

| | |
| --- | --- |
| Secret | `SENTRY_AUTH_TOKEN` |
| Credential | A Sentry user auth token with the scopes the operations need: `org:read`, `project:read`, `event:read`, `event:write` |
| Without the secret | Throws `SentryError` with code `missing_token` |
| HTTP | `sentry.io`: GET, PUT |
| Filesystem | None |
| Readme | [`packages/sentry/readme.md`](https://github.com/submilli/submilli-runtime/blob/main/packages/sentry/readme.md) |

```sh
submilli install submilli/submilli-runtime @submilli/sentry
submilli blueprint add-package @submilli/sentry --no-capabilities
submilli blueprint capability list @submilli/sentry
submilli blueprint capability add sentry.io/issues.list
```

## @submilli/slack-bot

Slack as the app's bot: bot messages, conversation history and threads, conversations and members, direct messages, users, and reactions.

| | |
| --- | --- |
| Secret | `SLACK_BOT_TOKEN` |
| Credential | A Slack bot OAuth token with the bot scopes the operations need |
| Without the secret | Throws `SlackError` with code `missing_token` |
| HTTP | `slack.com`: GET, POST |
| Filesystem | None |
| Readme | [`packages/slack-bot/readme.md`](https://github.com/submilli/submilli-runtime/blob/main/packages/slack-bot/readme.md) |

```sh
submilli install submilli/submilli-runtime @submilli/slack-bot
submilli blueprint add-package @submilli/slack-bot --no-capabilities
submilli blueprint capability list @submilli/slack-bot
submilli blueprint capability add slack.com/bot/listMessages
```

## @submilli/slack-user

Slack as the authenticated user: search, messages and threads, channels, users, files, sending messages, and reactions.

| | |
| --- | --- |
| Secret | `SLACK_USER_TOKEN` |
| Credential | A Slack user OAuth token with the user scopes the operations need |
| Without the secret | Throws `SlackError` with code `missing_token` |
| HTTP | `files.slack.com`: download; `slack.com`: GET, POST |
| Filesystem | `fs.write` |
| Readme | [`packages/slack-user/readme.md`](https://github.com/submilli/submilli-runtime/blob/main/packages/slack-user/readme.md) |

```sh
submilli install submilli/submilli-runtime @submilli/slack-user
submilli blueprint add-package @submilli/slack-user --no-capabilities
submilli blueprint capability list @submilli/slack-user
submilli blueprint capability add slack.com/user/search
```

## @submilli/typesafe

Semantic judgments by TypeSafe's Jev model: choice, yes/no, and rubric-scored questions with typed answers and probabilities.

| | |
| --- | --- |
| Secret | `TYPESAFE_AI_KEY` |
| Credential | A TypeSafe API key |
| Without the secret | Throws `TypeSafeError` with code `missing_credentials` |
| HTTP | `api.typesafe.ai`: POST `/v1/systemone` |
| Filesystem | None |
| Readme | [`packages/typesafe/readme.md`](https://github.com/submilli/submilli-runtime/blob/main/packages/typesafe/readme.md) |

```sh
submilli install submilli/submilli-runtime @submilli/typesafe
submilli blueprint add-package @submilli/typesafe --no-capabilities
submilli blueprint capability list @submilli/typesafe
submilli blueprint capability add typesafe.ai/systemone
```
