---
title: "Add an MCP server"
description: "How to make an MCP server importable as a package: declare it, allow the tools the task needs, give it a credential or log in, register it on a server, and handle a server that can't be reached."
slug: next/blueprints/add-an-mcp-server
pagefind: false
sidebar:
  order: 6
  hidden: true
---

The curated packages cover common services, and you can write a package
for your own. For a service that has neither yet, or whose package lacks
a feature you need, there is often an MCP server. Declared in a
blueprint, that server becomes a package: Submilli
reads its tools and their JSON schemas and turns them into a TypeScript
library, one typed function per tool, that a program imports and calls
like any other package, under the same rules. The blueprint says which of
its tools a program may call, and the credential stays outside the
program.

This guide shows you how to make an MCP server importable as a package:
declare it, allow the tools the task needs, give it a credential or log
in, register the blueprint on a server, and handle a server that can't be
reached. The examples are Playwright's server, which needs no account,
and Linear's, which takes an API key or an OAuth login; substitute your
server's URL and tools.

To follow the Playwright example, start its server first:

```sh
npx @playwright/mcp@latest --port 8931 --headless --isolated
```

## Declare it

```sh
submilli blueprint init browse
submilli blueprint add-mcp playwright http://localhost:8931/mcp
```

```text
✓ added mcp server 'playwright' (no auth) to blueprint.yaml
  Gated by `mcp.playwright` (deny by default) — set its action to `allow` (optionally `filter: tool == "..."`) to use it.
```

```yaml title="blueprint.yaml (fragment)"
permissions:
  main:
  - capability: mcp.playwright
    action: deny
mcp:
  playwright:
    url: http://localhost:8931/mcp
```

The name you give becomes the key in the `mcp` block, the package name
`@mcp/playwright`, and the capability `mcp.playwright`. The server must
speak MCP over HTTP; if yours speaks it over standard input and output,
put it behind an HTTP endpoint first, as `--port` does for Playwright's.
Refer to [MCP servers](/docs/next/reference/mcp-servers) for the block's
fields.

## Allow its tools

The deny rule that `add-mcp` wrote says nothing `default: deny` doesn't,
so drop it first; `capability remove` takes every rule for a capability,
which is why it goes before the grant. One capability covers the whole
server; pick tools with the `tool` field of a filter:

```sh
submilli blueprint capability remove mcp.playwright
submilli blueprint capability add mcp.playwright \
  --filter 'tool == "browser_navigate" or tool == "browser_snapshot" or tool == "browser_click"'
```

```text
✓ removed 1 rule(s) for 'mcp.playwright' from caller 'main' in blueprint.yaml
✓ added allow mcp.playwright (filter: tool == "browser_navigate" or tool == "browser_snapshot" or tool == "browser_click") to caller 'main' in blueprint.yaml
```

```yaml title="blueprint.yaml (fragment)"
permissions:
  main:
  - capability: mcp.playwright
    filter: tool == "browser_navigate" or tool == "browser_snapshot" or tool == "browser_click"
    action: allow
```

The three tools are allowed and every other tool meets the default. If
you want every tool whose name starts the same way, use `glob`:
`tool glob "list_*"`. Don't write the tool into the capability name,
`mcp.playwright/browser_click`; the blueprint is refused.

Choose tools by what their arguments can do, not only by their names. A
rule sees the tool's name and nothing else, and `browser_navigate` asked
for a `javascript:` address runs script in the page as `browser_evaluate`
would. If an allowed tool is that broad, restrict it where the server runs
(Playwright's takes `--allowed-origins`), or put a package in front of it
that checks the arguments and grant the package instead.

## Call it

```typescript title="link.ts"
import playwright from "@mcp/playwright";

function main(): string {
    playwright.browser_navigate({ url: "https://example.com" });
    return playwright.browser_snapshot({}) as string;
}
```

```sh
submilli run --blueprint blueprint.yaml link.ts
```

````text
### Page
- Page URL: https://example.com/
- Page Title: Example Domain
### Snapshot
```yaml
- generic [ref=e2]:
  - heading "Example Domain" [level=1] [ref=e3]
  - paragraph [ref=e4]: This domain is for use in documentation examples without needing permission. Avoid use in operations.
  - paragraph [ref=e5]:
    - link "Learn more" [ref=e6] [cursor=pointer]:
      - /url: https://iana.org/domains/example
```
````

A program's calls to one server share one MCP session, closed when the
program ends, so with a server that keeps state, do a whole task in one
program. `submilli docs @mcp/playwright` lists the server's tools as
declarations; a tool without an output schema returns `unknown`, so cast
the result to the type you expect, as above.

## Give it a credential

Linear's server takes an API key as a bearer token. Declare the secret,
store the value, and declare the server with it; `--authorization-bearer`
writes the header for you:

```sh
submilli blueprint secret add LINEAR_API_KEY --store linear_api_key
submilli secret put linear_api_key
submilli blueprint add-mcp linear https://mcp.linear.app/mcp --authorization-bearer LINEAR_API_KEY
```

```text
✓ declared secret 'LINEAR_API_KEY' (store: linear_api_key) in blueprint.yaml
Value for 'linear_api_key': [hidden]
Stored secret 'linear_api_key'
✓ added mcp server 'linear' (static header auth) to blueprint.yaml
  Gated by `mcp.linear` (deny by default) — set its action to `allow` (optionally `filter: tool == "..."`) to use it.
```

```yaml title="blueprint.yaml (fragment)"
mcp:
  linear:
    url: https://mcp.linear.app/mcp
    headers:
      Authorization: Bearer ${secrets.LINEAR_API_KEY}
```

The credential is added outside the program, and a tool's arguments never
carry it. If users must reach the server as themselves, declare the
secret with `--harness` and write the header yourself with
`--header 'Authorization: Bearer ${secrets.LINEAR_API_KEY}'`; the value
is then the one your application supplied when it opened the session.

Allow two of its tools, and read their declarations; `docs` connects to
the server with the key to get them:

```sh
submilli blueprint capability remove mcp.linear
submilli blueprint capability add mcp.linear --filter 'tool == "list_teams" or tool == "get_issue"'
submilli docs @mcp/linear --blueprint blueprint.yaml
```

```text
✓ removed 1 rule(s) for 'mcp.linear' from caller 'main' in blueprint.yaml
✓ added allow mcp.linear (filter: tool == "list_teams" or tool == "get_issue") to caller 'main' in blueprint.yaml
warning: @mcp/linear: 59 tool(s) return unknown: result schemas are unavailable or unrepresentable; consult package docs for signatures
@mcp/linear — MCP server 'linear' (59 tools)
…
/**
 * Retrieve detailed information about an issue by ID, including attachments, git branch name, and active Triage Intelligence suggestions when the issue is in triage
 * Returns `unknown`; this MCP server did not publish an outputSchema — cast to a declared type (`as T`) after checking the shape.
 */
function get_issue(args: { id: string; includeCustomerNeeds?: boolean; includeRelations?: boolean; includeReleases?: boolean }): unknown;
…
```

Linear publishes no output schemas, so every tool returns `unknown`, and
a program declares the shape it expects and casts:

```typescript title="teams.ts"
import linear from "@mcp/linear";

interface Teams {
    teams: { id: string; name: string }[];
}

function main(): string {
    const result = linear.list_teams({}) as Teams;
    return result.teams.map((team) => team.name).join(", ");
}
```

```sh
submilli run --blueprint blueprint.yaml teams.ts
```

```text
warning: @mcp/linear: 59 tool(s) return unknown: result schemas are unavailable or unrepresentable; consult package docs for signatures
Submilli
```

A program that calls `list_users`, which the filter leaves out, is refused
before any request leaves:

```text
error: PermissionDeniedError: permission denied: caller=main capability=mcp.linear: policy denied mcp.linear for main. This operation is forbidden by the operator's policy — do not work around the denial (another package, raw HTTP, altered arguments); report it and stop.
```

## Log in with OAuth

Linear also takes an OAuth login, which spares you a key. In a blueprint
that doesn't declare `linear` yet, give `add-mcp` no credential flag: it
asks the server whether it requires OAuth and writes `auth: type: oauth2`
if it does:

```sh
submilli blueprint add-mcp linear https://mcp.linear.app/mcp
```

```text
✓ added mcp server 'linear' (oauth) to blueprint.yaml
  Gated by `mcp.linear` (deny by default) — set its action to `allow` (optionally `filter: tool == "..."`) to use it.
  Authenticate locally: submilli mcp authenticate linear --blueprint blueprint.yaml
  After applying to a server: submilli server mcp authenticate browse linear
```

```yaml title="blueprint.yaml (fragment)"
mcp:
  linear:
    url: https://mcp.linear.app/mcp
    auth:
      type: oauth2
```

Until someone logs in, the blueprint is `PENDING`: it still runs programs,
without that server:

```sh
submilli mcp auth-status --blueprint blueprint.yaml
```

```text
browse: PENDING
  linear                   oauth — NOT AUTHENTICATED
```

Log in once:

```sh
submilli mcp authenticate linear --blueprint blueprint.yaml
```

```text
Open this URL in your browser to authorize:

  https://mcp.linear.app/authorize?response_type=code&client_id=…

Waiting for the redirect on http://127.0.0.1:8765/callback …
✓ authenticated 'linear' on blueprint 'browse' — blueprint 'browse' is ACTIVE
```

```sh
submilli mcp auth-status --blueprint blueprint.yaml
```

```text
browse: ACTIVE
  linear                   oauth — authenticated
```

The same `teams.ts` runs under the login and answers the same. Discovery
found 68 tools this time where the key saw 59: what a credential may see
is the server's decision.

The credential lands in the local secret store, and `deauthenticate`
forgets it. One login serves every program run under the blueprint, and
on a server every user's session. Log in as an account that may do what
you are willing to let any user's agent do, and narrow it with the `tool`
filter; if users must act as themselves, use a per-user token in a header
instead.

If the service requires a registered application, as GitHub does,
configure a provider for its login host before authenticating:

```sh
submilli mcp provider add --match github.com --client-id Iv1.example \
  --client-secret '${secrets.GITHUB_CLIENT_SECRET}' --scope repo
```

If a service refuses a login's refresh token, programs get
`McpAuthExpiredError`; when the refusal lasts, log in again.

## Register it on a server

Registered on `submilli-server`, the same blueprint (here with Linear
declared for OAuth) gives every session the same packages, but three
things are now the server's: the network it connects from, the store its
logins are kept in, and the list of tools it has read. Register it and
check its logins:

```sh
submilli server blueprint apply blueprint.yaml
submilli server mcp auth-status browse
```

```text
Added blueprint 'browse'
browse: PENDING
  linear                   oauth — NOT AUTHENTICATED
  playwright               n/a (no auth)
```

A program that imports a server with no login yet doesn't compile, and
`run-code` says which login is missing:

```sh
submilli server run-code teams.ts --blueprint browse
```

```text
warning: @mcp/linear: server unavailable: not authenticated — run `submilli server mcp authenticate`
error: MCP server `linear` is unavailable — `@mcp/linear` is absent from the discovered catalog; check the blueprint's `mcp:` block and discovery warnings
```

An MCP server is an outbound destination like any other, so the server's
block on private addresses covers it. Started without flags, the server
refuses the Playwright example on `localhost`, and `link.ts` fails the
same way:

```text
warning: @mcp/playwright: server unavailable: blocked by network policy: localhost resolves only to private/loopback IP space; allow-list it on the server with --allow-ip / --allow-localhost / --allow-private
```

Start it with `--allow-localhost` for the example, or `--allow-ip` for an
MCP server inside your network. Then log in to Linear on the server. The
command runs on your machine, so the browser it opens is yours, and the
login lands in the server's secret store, separate from the local one:

```sh
submilli server mcp authenticate browse linear
```

```text
Open this URL in your browser to authorize:

  https://mcp.linear.app/authorize?response_type=code&client_id=…

Waiting for the redirect on http://127.0.0.1:8765/callback …
✓ authenticated 'linear' on blueprint 'browse' — blueprint 'browse' is ACTIVE
```

```sh
submilli server mcp auth-status browse
submilli server run-code teams.ts --blueprint browse
```

```text
browse: ACTIVE
  linear                   oauth — authenticated
  playwright               n/a (no auth)
warning: @mcp/linear: 68 tool(s) return unknown: result schemas are unavailable or unrepresentable; consult package docs for signatures
Submilli
```

`submilli server mcp deauthenticate browse linear` removes the login. A
provider for a service such as GitHub goes under `mcp_oauth` in the
server's config file, not the local provider file:

```yaml title="server.yaml (fragment)"
mcp_oauth:
  providers:
  - match: github.com
    client_id: Iv1.example
    client_secret: ${secrets.GITHUB_CLIENT_SECRET}
```

The server reads an MCP server's tools the first time a program or a
search needs them, and keeps the list until the blueprint is applied
again, a login changes, or the server restarts. After an MCP server gains
or loses a tool, apply the blueprint again.

## If the server can't be reached

When discovery can't reach an MCP server within ten seconds, the network
rules block it, or it has no login yet, the blueprint still works without
that package. Locally, a run warns `warning: @mcp/playwright: server
unavailable:` with the reason; on a server, the same line is in
`run-code`'s output, in the HTTP response's `discovery_warnings` list,
and as a `WARN` line in the server's log. A program that imports the
missing package doesn't compile:

```text
error: MCP server `playwright` is unavailable — `@mcp/playwright` is absent from the discovered catalog; check the blueprint's `mcp:` block and discovery warnings
```

Each tool call has sixty seconds, including login and connection. Results
come back as text; images are dropped, and nothing streams.
