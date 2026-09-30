---
title: "MCP servers on the server"
description: "Operating the MCP servers a blueprint declares once it is registered on submilli-server: checking what programs can use, logging in, OAuth providers, the network rules, and discovery."
slug: server-mcp
sidebar:
  order: 11
---

[Using MCP servers](/docs/mcp-servers) declared an MCP server in a blueprint
and called it from a program on your machine. Registered on
`submilli-server`, the same blueprint gives every session the same packages,
but three things are now the server's: the network it connects from, the
store its logins are kept in, and the list of tools it has read. This chapter
is about operating those.

The examples use a blueprint named `browse` that declares both of that
chapter's MCP servers: Playwright's, on `localhost`, and Linear's, which
needs a login.

## Register the blueprint and check it

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

A blueprint is `PENDING` while any of its OAuth servers lacks a login, and
`ACTIVE` otherwise. A pending blueprint still runs programs; the servers
without a login are left out of it.

`auth-status` reports logins only. To see whether the server can reach an MCP
server and what it found there, ask for its tools:

```sh
submilli server docs @mcp/playwright --blueprint browse
```

```text
MCP server 'playwright' (25 tools)
…
```

Then run a program the way an application would:

```sh
submilli server run-code link.ts --blueprint browse
```

````text
warning: @mcp/playwright: 25 tool(s) return unknown: result schemas are unavailable or unrepresentable; consult package docs for signatures
### Page
- Page URL: https://example.com/
- Page Title: Example Domain
…
````

The blueprint's rules apply as they do locally. A program that calls a tool
the filter leaves out gets `PermissionDeniedError` before any request leaves
the server.

## The network rules apply

An MCP server is an outbound destination like any other, for discovery, for
calls, and for the OAuth exchange, so the server's block on private addresses
covers it. Started without flags, the server refuses the Playwright example,
which runs on `localhost`:

```text
warning: @mcp/playwright: server unavailable: blocked by network policy: localhost resolves only to private/loopback IP space; allow-list it on the server with --allow-ip / --allow-localhost / --allow-private
```

Start the server with `--allow-localhost` to run the example. For an MCP
server inside your network in production, allow its one address with
`--allow-ip`; [outbound network](/docs/server#outbound-network) has the
choices.

## Log in to an OAuth server

```sh
submilli server mcp authenticate browse linear
```

```text
Open this URL in your browser to authorize:

  https://mcp.linear.app/authorize?response_type=code&client_id=…

Waiting for the redirect on http://127.0.0.1:8765/callback …
✓ authenticated 'linear' on blueprint 'browse' — blueprint 'browse' is ACTIVE
```

The command runs on your machine, not on the server, so the browser it needs
is yours. It waits for the browser to come back to port 8765, which
`SUBMILLI_OAUTH_REDIRECT_PORT` changes, and hands the result to the server.
The server stores the login in its [secret store](/docs/server#secrets), so
it needs one.

The login is kept per blueprint and MCP server, and
[every session shares it](/docs/mcp-servers#one-login-shared): every user's
programs reach Linear as the person who logged in. It is separate from any
login made with the local CLI. `submilli server mcp deauthenticate browse
linear` removes it.

A login survives a restart when the secret store does. Count it among the
secrets to back up in [where the server keeps state](/docs/server#where-it-keeps-state).

## OAuth providers

A service that needs an application registered with it, as GitHub does, takes
a [provider](/docs/mcp-servers#which-client-submilli-logs-in-as). The local
CLI keeps providers in a file of its own; a server takes them under
`mcp_oauth` in its config file:

```yaml title="server.yaml (fragment)"
mcp_oauth:
  providers:
  - match: github.com
    client_id: Iv1.example
    client_secret: ${secrets.GITHUB_CLIENT_SECRET}
```

## Discovery, and servers that are left out

The server reads an MCP server's tools the first time a program or a search
needs them. It keeps the result until the blueprint is applied again, a login
changes, or the server restarts, so after an MCP server gains or loses a
tool, apply the blueprint again.

An MCP server that can't be reached in ten seconds, is blocked by the network
rules, or has no login is left out, and the rest of the blueprint works. You
see it in three places:

| Where | What |
| --- | --- |
| `submilli server run-code` | A line beginning `warning: @mcp/<name>: server unavailable:` |
| The HTTP API | The same text in the response's `discovery_warnings` list |
| The server's log | A `WARN` line such as ``MCP server omitted: not authenticated — run `submilli server mcp authenticate browse linear` `` |

A program that imports a package that was left out doesn't compile, and the
error names the MCP server.

## Local and server commands

| Task | Local, with a blueprint file | On a server, with a registered blueprint |
| --- | --- | --- |
| Run a program | `submilli run --blueprint blueprint.yaml link.ts` | `submilli server run-code link.ts --blueprint browse` |
| List a server's tools | `submilli docs @mcp/<server> --blueprint blueprint.yaml` | `submilli server docs @mcp/<server> --blueprint <blueprint>` |
| Log in | `submilli mcp authenticate --blueprint blueprint.yaml <server>` | `submilli server mcp authenticate <blueprint> <server>` |
| Check logins | `submilli mcp auth-status --blueprint blueprint.yaml` | `submilli server mcp auth-status <blueprint>` |
| Log out | `submilli mcp deauthenticate --blueprint blueprint.yaml <server>` | `submilli server mcp deauthenticate <blueprint> <server>` |
| Providers | `submilli mcp provider add\|list\|remove` | `mcp_oauth` in the config file |
| Logins kept in | The local secret store, in plain files | The server's secret store, encrypted |
| Private addresses | Allowed | Refused unless the server allows them |

## With a coding agent

A coding agent with the [Submilli skill](/docs/skill) registers the blueprint
and tests what it gives programs. This prompt was run with Claude Code in a
project holding the `browse` blueprint, against a local server started on port 18128 with
a secret store and `--allow-localhost`, with Playwright's server running.

```text
Put this blueprint on my local Submilli server at http://127.0.0.1:18128 and tell me which of its MCP servers programs can use.
```

The agent checks the blueprint with `submilli blueprint lint` and looks at
what the server already holds before it applies anything, so it can say that
nothing was replaced. It then lists each MCP server's tools with `submilli
server docs` and runs three programs with `run-code`. A program that calls
`browser_snapshot` returns a snapshot. One that calls `browser_evaluate` is
refused with `PermissionDeniedError`. One that imports `@mcp/linear` doesn't
compile: `server unavailable: not authenticated`.

Its report is a table: Playwright is usable, for the two tools the filter
allows out of the 25 the server has; Linear isn't, for two reasons. The
server has no login for it, and the blueprint denies `mcp.linear`. It gives
the `authenticate` command for you to run, since the login opens a browser,
and leaves the rule to you as a decision about policy. It adds that one login
serves every session, so every program would act in Linear as whoever logged
in. It also says what it didn't test: `browser_navigate` is allowed by the
same rule as `browser_snapshot`, but no program called it.

Next: [connecting to your harness](/docs/harness), where an application or an
agent framework opens sessions against a registered blueprint.
