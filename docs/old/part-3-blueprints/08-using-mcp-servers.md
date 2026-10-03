---
title: "Using MCP servers"
description: "Reference for the blueprint's mcp block: declaring an MCP server, allowing its tools, how tools become typed functions, output schemas, credentials, and OAuth."
slug: old/mcp-servers
pagefind: false
sidebar:
  hidden: true
  order: 8
---

An MCP server is a program that offers tools to an agent over MCP, the
protocol most agent frameworks use to call tools. Many services publish one,
and you may run some of your own. When Submilli has no package for a service,
its MCP server is the way in: declare it in the blueprint and it becomes a
package that programs import, with each tool a function the blueprint can
allow or deny.

MCP appears in Submilli in two directions. This chapter is about the servers
Submilli calls. [Connecting to your harness](/docs/old/harness) is about the other
direction, where `submilli-server` is itself an MCP server for your agent.
Everything here runs on your machine with a blueprint file;
[MCP servers on the server](/docs/old/server-mcp) covers the same blueprint
registered on a server.

The examples use two servers. Playwright's drives a browser and needs no
account; Linear's needs a login. Start Playwright's on your machine with:

```sh
npx @playwright/mcp@latest --port 8931 --headless --isolated
```

## Declare a server

```sh
submilli blueprint init browse
submilli blueprint add-mcp playwright http://localhost:8931/mcp
```

```text
✓ added mcp server 'playwright' (no auth) to blueprint.yaml
  Gated by `mcp.playwright` (deny by default) — set its action to `allow` (optionally `filter: tool == "..."`) to use it.
```

The name you give is yours to choose. It becomes the key in the `mcp` block,
the package name `@mcp/playwright`, and the capability `mcp.playwright`.

```yaml
permissions:
  main:
  - capability: mcp.playwright
    action: deny
mcp:
  playwright:
    url: http://localhost:8931/mcp
```

| Field | Meaning |
| --- | --- |
| `url` | The server's MCP endpoint. Required. |
| `transport` | `streamable_http`, the default. `sse` is accepted as another name for it. `stdio` is refused: Submilli doesn't start MCP servers as child processes. |
| `headers` | Headers sent with every request. A value may hold `${secrets.NAME}`. |
| `auth` | `type: oauth2`, with optional `client_id`, `scopes`, `authorization_endpoint`, `token_endpoint`. Can't be combined with `headers`. |

A package named `@mcp/...` can't be listed under `packages`; the `mcp` block
is the only way to declare one.

## Allow its tools

One capability covers the whole server, and the `tool` field of a filter
picks tools within it:

```sh
submilli blueprint capability add mcp.playwright \
  --filter 'tool == "browser_navigate" or tool == "browser_snapshot" or tool == "browser_click"'
```

```yaml
permissions:
  main:
  - capability: mcp.playwright
    filter: tool == "browser_navigate" or tool == "browser_snapshot" or tool == "browser_click"
    action: allow
  - capability: mcp.playwright
    action: deny
```

Rules are read top to bottom and the first match decides. The command puts
your rule ahead of the deny rule that `add-mcp` wrote, so the three tools are
allowed and every other tool still meets the deny.

The filter has two fields: `tool`, the tool's name, and `transport`, which is
always `streamable_http`. `glob` works on `tool`, so `tool glob "list_*"`
allows every tool whose name begins that way. A capability written with the
tool in its name, `mcp.playwright/browser_click`, is refused when the
blueprint is read; the tool belongs in the filter.

Choosing tools matters more than it may seem. Playwright's server has a tool
named `browser_evaluate`, which runs any JavaScript in the page. Under the
filter above, a model that reached for it got this, before any request left
Submilli:

```text
error: PermissionDeniedError: permission denied: caller=main capability=mcp.playwright: policy denied mcp.playwright for main. …
```

A rule sees the tool's name and nothing else. It can't see what a tool is
asked to do, and some tools' arguments carry as much power as the tools you
left out. Asked to navigate to a `javascript:` address, `browser_navigate`
runs that script in the page, as `browser_evaluate` would. When an allowed
tool is that broad, restrict it where the MCP server runs (Playwright's takes
`--allowed-origins`), or put a package in front of it that checks the
arguments and grant the package instead.

In a blueprint without a `permissions` block, `add-mcp` writes no rule, and
the server is denied by `default: deny`.

## Call it from a program

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

Each tool is a function that takes one object of arguments and returns when
the tool has answered. There is no `await`: calls are synchronous, like the
rest of the standard library.

A program's calls to one server share one MCP session, opened at the first
call and closed when the program ends. That is why the snapshot above sees
the page the call before it opened. The next program gets a new session, so
with a server that keeps state, do a whole task in one program.

## See what a server offers

`submilli docs` lists an MCP server's tools as TypeScript declarations. It
reads `blueprint.yaml` in the current directory, or the file `--blueprint`
names, and connects to the MCP server itself:

```sh
submilli docs @mcp/playwright
```

```text
warning: @mcp/playwright: 25 tool(s) return unknown: result schemas are unavailable or unrepresentable; consult package docs for signatures
@mcp/playwright — MCP server 'playwright' (25 tools)

/**
 * Perform click on a web page
 * Returns `unknown`; this MCP server did not publish an outputSchema — cast to a declared type (`as T`) after checking the shape.
 */
function browser_click(args: { button?: "left" | "right" | "middle"; doubleClick?: boolean; element?: string; modifiers?: ("Alt" | "Control" | "ControlOrMeta" | "Meta" | "Shift")[]; target: string }): unknown;
…
```

This is also what the model reads, through the package docs tool, before it
writes a program. The listing shows every tool the MCP server has, whether or
not the blueprint allows it.

## How tools become functions

Submilli reads the server's tool list and builds the package from it. This is
**discovery**. It happens the first time a program or a search needs the
server.

| In the tool | In the package |
| --- | --- |
| Name | The function's name, unchanged |
| Input schema | The type of the single `args` parameter. A property that isn't required becomes optional; a tool with no required property can be called with no argument. |
| `integer` | `number` |
| A string `enum` | A union of string literals |
| `anyOf` or `oneOf` of simple types, or a type list such as `["string", "null"]` | A union |
| Output schema | The return type |

A part of an input schema that the type system can't express becomes
`unknown`: `allOf`, `$ref`, `not`, an object with no declared properties, or
nesting deeper than twelve levels. The tool stays callable, and the MCP
server checks what the program passes for that part.

A tool is **dropped** when its name can't be a function name, or when another
tool of the same server already has it. A dropped tool doesn't exist for
programs, and discovery warns about it.

## Output schemas

What a function returns depends on whether Submilli has a schema for the
tool's result. It looks in two places, in this order.

1. **The server publishes one.** MCP lets a tool declare an `outputSchema`.
   When it does, and the schema can be expressed, the function returns that
   type and the program uses the result directly.
2. **Submilli knows the server.** Submilli carries schemas for one server
   today: GitHub's hosted MCP server at `api.githubcopilot.com`, for 18 of its
   tools. The match is on the host of the `url`, so a copy of that server
   hosted elsewhere doesn't get them.

With neither, the function returns `unknown`, and the program casts the
result to a type it declares. That is the case for most public MCP servers,
Playwright's and Linear's among them.

A cast is checked when the program runs. This program declares what it
expects from Linear's `list_teams`:

```typescript
import linear from "@mcp/linear";

interface Team {
    id: string;
    name: string;
}

function main(): string {
    const found = linear.list_teams({}) as { teams: Team[] };
    const names: string[] = [];
    for (const team of found.teams) names.push(team.name);
    return names.join(", ");
}
```

A field the type doesn't mention is ignored. A field it does mention must be
there with that type. An earlier version of `Team` declared
`key: string | null`, a field Linear doesn't send, and the program stopped at
the cast:

```text
error: TypeError: type mismatch: expected { teams: Team[] }, got object at $["teams"][0]["key"]
```

Declare only the fields the program uses.

## What a program gets back, and what it can catch

A result with structured content comes back as that value. A text result
that parses as JSON comes back parsed, and otherwise as a string. Several
text parts are joined with newlines. Images and other content that isn't text
are left out.

Failures are ordinary errors that a program can catch with `try`:

| Failure | Message begins |
| --- | --- |
| The blueprint denies the tool | `PermissionDeniedError: permission denied: caller=main capability=mcp.<name>` |
| The tool reports an error | `@mcp/<name>.<tool>:` and the tool's own text |
| The server can't be reached, or takes more than 60 seconds | `@mcp/<name>.<tool>: transport error:` |
| The server answers with an HTTP error | `@mcp/<name>.<tool>: server returned HTTP <status>:` |
| The OAuth login is refused | `McpAuthExpiredError: @mcp/<name>.<tool>:` |

## Credentials

| The server wants | Declare it with | The value comes from |
| --- | --- | --- |
| Nothing | `add-mcp <name> <url>` | |
| An API key | `add-mcp <name> <url> --authorization-bearer <SECRET>` | The secret store |
| Each user's own token | `add-mcp <name> <url> --header 'Authorization: Bearer ${secrets.<SECRET>}'` with a `harness` secret | Your application, per session |
| An OAuth login | `add-mcp <name> <url> --oauth` | A login you perform once |

`--authorization-bearer` needs the secret declared first, and writes the
header for you:

```sh
submilli blueprint secret add CRM_API_KEY --store crm_api_key
submilli blueprint add-mcp crm https://mcp.acme.example/mcp --authorization-bearer CRM_API_KEY
```

```yaml
mcp:
  crm:
    url: https://mcp.acme.example/mcp
    headers:
      Authorization: Bearer ${secrets.CRM_API_KEY}
```

`--header 'Name: value'` writes any other header and may be repeated. When
the secret behind it has a `harness` source, the value is the one your
application supplied when it opened the session, so each user reaches the MCP
server as themselves. [Connecting to your harness](/docs/old/harness#credentials-that-belong-to-the-session)
shows how the application supplies it.

The credential is added by Submilli outside the program. A program can't read
it, and a tool's arguments never carry it.

## OAuth

Given no credential flag, `add-mcp` asks the server whether it requires
OAuth and writes `auth: type: oauth2` if it does. `--no-probe` skips the
question; `--oauth` answers it yourself.

```sh
submilli blueprint add-mcp linear https://mcp.linear.app/mcp
```

```text
✓ added mcp server 'linear' (oauth) to blueprint.yaml
  Gated by `mcp.linear` (deny by default) — set its action to `allow` (optionally `filter: tool == "..."`) to use it.
  Authenticate locally: submilli mcp authenticate linear --blueprint blueprint.yaml
  After applying to a server: submilli server mcp authenticate tracker linear
```

```yaml
mcp:
  linear:
    url: https://mcp.linear.app/mcp
    auth:
      type: oauth2
```

### Log in

Log in once:

```sh
submilli mcp authenticate linear --blueprint blueprint.yaml
```

```text
Open this URL in your browser to authorize:

  https://mcp.linear.app/authorize?response_type=code&client_id=…

Waiting for the redirect on http://127.0.0.1:8765/callback …
✓ authenticated 'linear' on blueprint 'tracker' — blueprint 'tracker' is ACTIVE
```

The command prints the address to open, waits for the browser to come back
to port 8765, and stores the credential in the local secret store.
`SUBMILLI_OAUTH_REDIRECT_PORT` changes the port.

```sh
submilli mcp auth-status --blueprint blueprint.yaml
```

```text
tracker: ACTIVE
  linear                   oauth — authenticated
```

A blueprint is `PENDING` while any of its OAuth servers lacks a credential,
and `ACTIVE` otherwise. A pending blueprint still runs programs; the servers
without a credential are left out of it.

### One login, shared

The credential is stored per blueprint and MCP server, so every program run
under the blueprint calls the MCP server as the person who logged in. On
[a server](/docs/old/server-mcp#log-in-to-an-oauth-server) that means every
session of the blueprint, for every user.
Log in as an account that may do what you are willing to let any user's
agent do, and narrow it further with the `tool` filter. When users must act
as themselves, use a per-user token in a header, as the table above shows.

### Which client Submilli logs in as

An OAuth login needs a client id, the identity of the application that is
asking. Submilli takes the first of these that it finds:

1. `client_id` in the blueprint's `auth` block, written by
   `add-mcp --client-id`.
2. A **provider** you configured for the login host.
3. A client it registers on the spot, when the server supports dynamic
   client registration. Linear's does, which is why the example needed
   nothing.

A provider holds the client id, and the client secret if there is one, of an
application you registered with the service. GitHub requires one. It is
matched on the host of the service's token endpoint, which is not always the
host of the MCP server:

```sh
submilli mcp provider add --match github.com --client-id Iv1.example \
  --client-secret '${secrets.GITHUB_CLIENT_SECRET}' --scope repo
```

```yaml title="~/.submilli/mcp_oauth.yaml"
providers:
- match: github.com
  client_id: Iv1.example
  client_secret: ${secrets.GITHUB_CLIENT_SECRET}
  scopes:
  - repo
```

`submilli mcp provider list` and `remove` manage the entries.

The scopes requested are the blueprint's `scopes` if it has any, then the
provider's, then the ones the server advertises.

### Tokens

Submilli stores the refresh token and exchanges it for an access token when
one is needed. Access tokens are kept in memory only and replaced a minute
before they expire. When the service issues a new refresh token, Submilli
stores it in place of the old one.

A service may refuse a refresh token, sometimes only for a moment after
issuing it. Submilli tries again after one second and after two. If the
service still refuses, a program that calls the server gets
`McpAuthExpiredError`, and the credential stays in the store, so a later run
can succeed without a new login. When the refusal lasts, log in again.
`deauthenticate` removes a credential; until then the blueprint stays
`ACTIVE`.

## When a server can't be used

When discovery can't reach an MCP server, or the server has no login yet, the
blueprint still works, minus that package. A run reports it with a warning
that begins `warning: @mcp/playwright: server unavailable:` and gives the
reason. A program that imports the missing package doesn't compile:

```text
error: MCP server `playwright` is unavailable — `@mcp/playwright` is absent from the discovered catalog; check the blueprint's `mcp:` block and discovery warnings
```

Discovery gives a server ten seconds to answer.

## Limits

- **Tools only.** MCP resources, prompts, and sampling aren't used.
- **HTTP only.** A server that speaks MCP over standard input and output has
  to be put behind an HTTP endpoint first, as `--port` does for Playwright's.
- **Text results only.** Images and other content in a result are dropped.
- **No streaming.** A program gets a tool's result when the tool is done.
  Progress messages are ignored.
- **Sixty seconds per call.** A call that takes longer, counting login and
  connection, fails with a transport error. The limit isn't configurable.

## With a coding agent

A coding agent with the [Submilli skill](/docs/old/skill) declares an MCP server
and chooses its tools for you. This prompt was run with Claude Code in a
project where `submilli blueprint init browse` had just run, with the skill
installed and Playwright's server started as above.

```text
Add Playwright's MCP server, running at http://localhost:8931/mcp, so programs can open and read web pages but can't run JavaScript in them.
```

The agent runs `add-mcp`, lists the server's 25 tools with
`submilli docs @mcp/playwright`, and allows the six that open and read
pages:

```yaml
permissions:
  main:
  - capability: mcp.playwright
    filter: tool == "browser_navigate" or tool == "browser_navigate_back" or tool == "browser_snapshot" or tool == "browser_find" or tool == "browser_wait_for" or tool == "browser_close"
    action: allow
  - capability: mcp.playwright
    action: deny
```

It tests the rules with `submilli run`. Opening example.com and taking a
snapshot returns the page. `browser_evaluate`, `browser_run_code_unsafe`,
and `browser_click` are each refused before the call reaches Playwright.
With the filter removed, `browser_evaluate` runs, which shows the filter is
what refuses it.

Then its review looked past the tool names, and the report says the request
isn't fully met. `browser_navigate` to a `javascript:` or `data:` address
runs script, the limit described in [allow its tools](#allow-its-tools), and
`browser_snapshot` takes a `filename` that writes a file on Playwright's
machine. No blueprint rule can close either. The agent proposes restricting
Playwright's server, or putting a package in front of it that accepts only
`http` and `https` addresses. It asks whether "can't run JavaScript"
includes the pages' own scripts, since that decides which fix fits.

Next: [permissions](/docs/old/permissions), the reference for the rules a
blueprint grants and denies calls with.
