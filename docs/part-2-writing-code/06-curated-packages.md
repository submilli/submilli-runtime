---
title: "Curated packages"
description: "The maintained service packages: choose one, install it, configure credentials and permissions, and call it from a program."
slug: curated-packages
sidebar:
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

The repository currently contains ten packages. Each name links to its setup
instructions, including the credentials and service permissions it needs.

| Package | Use it for |
| --- | --- |
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

The two Slack packages represent different identities. Use `slack-bot` when
the agent acts as your app's bot, and `slack-user` when it acts on behalf of a
particular workspace user. The service still decides what that identity can
access. A blueprint grant cannot give a token permissions it does not have.

## Install and inspect it

With the CLI installed as in the [quickstart](/docs/quickstart), install Jina:

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
blueprint's rules. See [connecting to your harness](/docs/harness) for session
setup and [crafting a blueprint](/docs/blueprints) for other secret sources.

## Find the next operation

Look up the installed package's declarations and capability filter fields:

```sh
submilli search jina
submilli docs @submilli/jina
submilli blueprint capability list @submilli/jina
submilli blueprint lint blueprint.yaml
```

The declarations show function signatures and descriptions; the capability
list shows what the blueprint can restrict. In this example, lint warns that
search has no grant for `main`. That is intentional: the blueprint permits
reading one host, not searching the web.

The agent can retrieve package documentation through `packages.search` and
`packages.docs`, the lookup tools introduced in
[the standard library](/docs/standard-library#looking-things-up). You choose
the packages, credentials, and permissions; the agent discovers their APIs
and composes calls into a program.

Next: [crafting a blueprint](/docs/blueprints), where these grants become a
policy for an agent's whole session.
