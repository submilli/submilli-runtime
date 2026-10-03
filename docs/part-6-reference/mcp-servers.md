---
title: "MCP servers"
description: "The blueprint's mcp block, discovery, how tools become functions, output schemas, results, failure messages, OAuth, limits, and the local and server commands."
slug: reference/mcp-servers
sidebar:
  order: 11
---

An entry in a blueprint's `mcp` block declares an outbound MCP server, which
programs import as the package `@mcp/<name>`. This page describes the block,
how Submilli turns the server's tools into functions, what calls return and
throw, OAuth logins, limits, and the commands that operate the servers
locally and on `submilli-server`.

## The mcp block

A map from a server name to a server. The name is chosen by the blueprint
and becomes three things:

| Name | Example, for `linear` |
| --- | --- |
| The key in `mcp` | `linear` |
| The package | `@mcp/linear` |
| The capability | `mcp.linear` |

| Field | Type | Required | Default | Constraints |
| --- | --- | --- | --- | --- |
| `url` | string | yes | | Non-empty. The server's MCP endpoint |
| `transport` | `streamable_http`, `sse` | no | `streamable_http` | `sse` is another name for `streamable_http`. `stdio` is refused |
| `headers` | map of name to string | no | none | Sent with every request. Values may hold `${secrets.NAME}`. Not with `auth` |
| `auth` | map | no | none | `type: oauth2`, the only type. Not with `headers` |

| `auth` field | Type | Required | Default |
| --- | --- | --- | --- |
| `type` | `oauth2` | yes | |
| `client_id` | string | no | See [Client id](#client-id) |
| `authorization_endpoint` | string | no | Discovered from the server |
| `token_endpoint` | string | no | Discovered from the server |
| `scopes` | list of strings | no | See [Scopes](#scopes) |

Each `auth` value may hold `${secrets.NAME}`. Every `${secrets.NAME}` in the
block must name a secret the blueprint declares. A `harness` secret in a
header takes the value the application supplied for the session.

```yaml title="blueprint.yaml (fragment)"
secrets:
  CRM_API_KEY:
    store: crm_api_key
mcp:
  crm:
    url: https://mcp.acme.example/mcp
    headers:
      Authorization: Bearer ${secrets.CRM_API_KEY}
  linear:
    url: https://mcp.linear.app/mcp
    auth:
      type: oauth2
```

`submilli blueprint add-mcp <name> <url>` writes an entry and a
`mcp.<name>` deny rule under `main`, when the blueprint has a `permissions`
block.

| `add-mcp` option | Writes |
| --- | --- |
| none | `auth: {type: oauth2}` if a probe of the server finds it requires OAuth; otherwise no auth |
| `--authorization-bearer <SECRET>` | `headers: {Authorization: Bearer ${secrets.<SECRET>}}`; the secret must be declared |
| `--header 'Name: value'` (repeatable) | That header |
| `--oauth` | `auth: {type: oauth2}` |
| `--client-id <ID>` | `auth.client_id`; implies `--oauth` |
| `--scope <SCOPE>` (repeatable) | `auth.scopes`; implies `--oauth` |
| `--no-probe` | Skips the probe |

### Errors

```text
error: case.yaml: blueprint parse error: mcp.a: missing field `url` at line 4 column 5
```

```text
error: case.yaml: invalid mcp config: mcp server 'a': stdio transport is deferred for v1; use 'streamable_http'
```

```text
error: case.yaml: invalid mcp config: mcp server 'a': unknown transport 'websocket' (use 'streamable_http' or its 'sse' alias)
```

```text
error: case.yaml: invalid mcp config: mcp server 'a' sets both 'headers' and 'auth'; use one (static-key auth or OAuth)
```

```text
error: case.yaml: invalid mcp config: mcp server 'a' references undeclared secret 'K'
```

```text
error: case.yaml: blueprint parse error: mcp.a.auth.type: unknown variant `apikey`, expected `oauth2` at line 5 column 18
```

```text
error: case.yaml: blueprint parse error: mcp.a: unknown field `command`, expected one of `transport`, `url`, `headers`, `auth` at line 5 column 5
```

`@mcp/<name>` can't be listed under `packages`:

```text
error: case.yaml: invalid packages config: `@mcp/linear` is an MCP virtual package; declare the server in `mcp:` instead
```

## The capability

Every call to a tool is checked against the capability `mcp.<name>` before
any request is sent. One capability covers all of a server's tools. Its
filter fields are:

| Field | Type | Value |
| --- | --- | --- |
| `tool` | string | The tool's name |
| `transport` | string | Always `streamable_http` |

```yaml title="blueprint.yaml (fragment)"
permissions:
  main:
  - capability: mcp.playwright
    filter: tool == "browser_navigate" or tool == "browser_snapshot"
    action: allow
```

A rule for `mcp.<name>` must name a declared server, and may not put a tool
in the capability name:

```text
error: case.yaml: invalid mcp config: permission rule 'mcp.b' references undeclared mcp server 'b'
```

```text
error: case.yaml: invalid mcp config: permission rule 'mcp.a/click': use capability 'mcp.a' with a filter such as 'tool == "name"' instead of '/tool'
```

A filter on any other field is reported by `submilli blueprint lint` and
refused at registration:

```text
error: case.yaml: `permissions.main` rule 1 for `mcp.a` tests `name`, which the operation doesn't report, so a condition on it is false for every call, and true under `not`; its fields are: tool, transport
```

A denied call throws:

```text
PermissionDeniedError: permission denied: caller=main capability=mcp.helpdesk: policy denied mcp.helpdesk for main. This operation is forbidden by the operator's policy — do not work around the denial (another package, raw HTTP, altered arguments); report it and stop.
```

Rule matching is in [Permissions](/docs/reference/permissions), the filter
grammar in [Filter language](/docs/reference/filter-language).

## Discovery

Discovery connects to a server, calls `tools/list`, and builds
`@mcp/<name>` from the result. It sends the entry's headers or OAuth token,
and gives the server 10 seconds.

| Where | Servers discovered | When |
| --- | --- | --- |
| `submilli run --blueprint` | Every server in the block | At each run |
| `submilli docs @mcp/<name>` | The named server | At each call |
| `submilli-server`, running a program | The servers the program imports | At first use; then cached |
| `submilli-server`, package docs and search | Every server in the block | At first use; then cached |

The server keeps a blueprint's discovered tools until the blueprint is
applied again or removed, a login for it is stored or removed, or the server
restarts. A session with `harness` secrets bound is discovered for that
session and not cached.

### A server that can't be used

A server is left out of the package catalog, and the rest of the blueprint
works, when:

- it has `auth: oauth2` and no login is stored for it;
- the connection or `tools/list` fails;
- it doesn't answer within 10 seconds;
- on `submilli-server`, the network rules refuse its address.

Each is a warning that begins `warning: @mcp/<name>: server unavailable:`
and gives the reason:

```text
warning: @mcp/tracker: server unavailable: not authenticated — run `submilli server mcp authenticate`
```

```text
warning: @mcp/local: server unavailable: blocked by network policy: 127.0.0.1 is private/loopback IP space; allow-list it on the server with --allow-ip / --allow-localhost / --allow-private
```

On `submilli-server`, the same text is in `submilli server run-code`'s
output, in the `discovery_warnings` list of the HTTP response, and in the
server's log as a `WARN` line:

```text
2026-10-03T12:59:22.636375Z  WARN submilli_shared::mcp::discovery: MCP server omitted: not authenticated — run `submilli server mcp authenticate browse tracker` server="tracker"
```

A program that imports a package that was left out doesn't compile:

```text
error: MCP server `local` is unavailable — `@mcp/local` is absent from the discovered catalog; check the blueprint's `mcp:` block and discovery warnings
 --> link.ts:1:19
  |
1 | import local from "@mcp/local";
  |                   ^^^^^^^^^^^^
2 | 
  |
help: no MCP servers are currently available; declared servers may need authentication or may have failed discovery
```

### PENDING and ACTIVE

A blueprint is `PENDING` while any of its `auth: oauth2` servers has no
stored login, and `ACTIVE` otherwise. A `PENDING` blueprint runs programs;
its servers without a login are left out. `auth-status` prints the state and
one line per server:

```text
browse: PENDING
  local                    n/a (no auth)
  tracker                  oauth — NOT AUTHENTICATED
```

| Server line | Server |
| --- | --- |
| `oauth — authenticated` | `auth: oauth2`, login stored |
| `oauth — NOT AUTHENTICATED` | `auth: oauth2`, no login |
| `n/a (static API key)` | `headers` |
| `n/a (no auth)` | Neither |

## Tools as functions

Each tool becomes an exported function of `@mcp/<name>`, named as the tool
is. Calls are synchronous.

| In the tool | In the function |
| --- | --- |
| `name` | The function's name |
| `description` | The start of its documentation |
| `inputSchema` with properties | One parameter, `args`, an object type |
| `inputSchema` with properties, none required | `args?`, which may be omitted |
| `inputSchema` without properties | No parameter |
| A property not in `required` | An optional field |
| `outputSchema` | The return type; see [Output schemas](#output-schemas) |

| JSON Schema | Type |
| --- | --- |
| `string` | `string` |
| `number`, `integer` | `number` |
| `boolean` | `boolean` |
| `null` | `null` |
| `array` with `items` | An array of the item type |
| `object` with `properties`, or `properties` without `type` | An object type |
| A string `enum` of two or more values | A union of string literals |
| A string `enum` of one value | `string` |
| `anyOf` or `oneOf` of scalar types | A union |
| A type list of scalar types, such as `["string", "null"]` | A union |

An input property whose schema has no type here becomes `unknown`, and the
MCP server validates the value: `allOf`, `$ref`, `not`, an object without
properties, a non-string `enum`, `anyOf` or `oneOf` with an object or array
member, or nesting deeper than 12 levels.

A tool is dropped, with a warning, when its name isn't a TypeScript
identifier or is one of `await`, `delete`, `with`, `debugger`, `yield`,
`eval`, `arguments`, `implements`, `interface`, `package`, `private`,
`protected`, `public`, `static`, `let`; and when two tools have the same
name, both are dropped. A dropped tool isn't callable.

A server whose `tools/list` has these tools:

```text
warning: @mcp/helpdesk: tool `bad-name` dropped: tool name must be a TypeScript function identifier
warning: @mcp/helpdesk: tool `dup` dropped: duplicate tool name in tools/list
warning: @mcp/helpdesk: tool `dup` dropped: duplicate tool name in tools/list
warning: @mcp/helpdesk: 3 tool(s) return unknown: result schemas are unavailable or unrepresentable; consult package docs for signatures
@mcp/helpdesk — MCP server 'helpdesk' (4 tools)

/**
 * Always fails.
 * Returns `unknown`; this MCP server did not publish an outputSchema — cast to a declared type (`as T`) after checking the shape.
 */
function fail(): unknown;

/**
 * Fetch one support ticket by id.
 * Typed — use the result directly (narrow optional `foo?` fields first); no cast needed.
 */
function get_ticket(args: { extra?: unknown; id: string; limit?: number; note?: string | null; priority?: "low" | "high" }): { id: string; status: string };

/**
 * List open tickets.
 * Returns `unknown`; this MCP server did not publish an outputSchema — cast to a declared type (`as T`) after checking the shape.
 */
function list_tickets(args?: { query?: string }): unknown;

/**
 * Two text parts.
 * Returns `unknown`; this MCP server did not publish an outputSchema — cast to a declared type (`as T`) after checking the shape.
 */
function two_texts(): unknown;
```

`get_ticket`'s `extra` property is an `allOf`; `limit` is an `integer`;
`note` is `["string", "null"]`. `submilli docs @mcp/<name>` and the package
docs tool the agent reads list every discovered tool, whether or not the
blueprint allows it.

## Output schemas

A function's return type comes from the first of:

1. The tool's own `outputSchema`, when every part of it maps to a type in
   the table above.
2. A schema Submilli carries for the tool. Submilli carries schemas for 18
   tool names of GitHub's MCP server, for a `url` whose host is
   `api.githubcopilot.com`; a server at any other host gets none.
3. Otherwise, `unknown`.

The documentation line says which:

| Return | Documentation line |
| --- | --- |
| From 1 or 2 | ``Typed — use the result directly (narrow optional `foo?` fields first); no cast needed.`` |
| `unknown`, no `outputSchema` | ``Returns `unknown`; this MCP server did not publish an outputSchema — cast to a declared type (`as T`) after checking the shape.`` |
| `unknown`, `outputSchema` not representable | ``Returns `unknown` (the server's outputSchema is not representable) — cast to a declared type (`as T`) after checking the shape.`` |

Discovery warns once per server with the count of tools that return
`unknown`. A cast from `unknown` is checked when it runs: a field the type
declares must be present with that type, and fields it doesn't declare are
ignored.

```typescript title="cast.ts"
import helpdesk from "@mcp/helpdesk";

interface Tickets {
    tickets: { id: string; subject: string; owner: string }[];
}

function main(): number {
    const open = helpdesk.list_tickets() as Tickets;
    return open.tickets.length;
}
```

```text
error: TypeError: type mismatch: expected Tickets, got object at $["tickets"][0]["owner"]
```

## Results

| The tool's result | The function returns |
| --- | --- |
| `isError: true` | Nothing; it throws (see [Failures](#failures)) |
| `structuredContent` | That value |
| One text part | The text parsed as JSON, or the text as a string when it isn't JSON |
| Several text parts | The texts joined with newlines, as a string |
| No text part | `null` |

Content that isn't text, such as images, is ignored.

## Failures

A failed call throws an error the program can catch with `try`. The message
begins with the function's name:

| Failure | Message |
| --- | --- |
| The blueprint denies the call | `PermissionDeniedError: permission denied: caller=main capability=mcp.<name>: …` |
| The tool returns `isError: true` | `@mcp/<name>.<tool>: ` and the tool's text, or `MCP tool reported an error` when it has none |
| The connection, the HTTP exchange, or the protocol fails | `@mcp/<name>.<tool>: transport error: ` and the cause |
| The call takes more than 60 seconds | `@mcp/<name>.<tool>: transport error: MCP tool '<name>/<tool>' timed out after 60 seconds` |
| The OAuth token endpoint refuses the refresh token after retries | `McpAuthExpiredError: @mcp/<name>.<tool>: OAuth authentication failed; retry later or authenticate the MCP server again` |
| The OAuth token endpoint answers with another HTTP error | `@mcp/<name>.<tool>: server returned HTTP <status>: <body>` |

A tool error, caught:

```text
Error: @mcp/helpdesk.fail: ticket store is read-only today
```

## MCP sessions

A program's calls to one server share one MCP session, opened by the first
call and closed when the program ends. A call that fails or times out closes the session, and the next call
opens a new one. Each program run gets its own sessions.

## OAuth

A server with `auth: type: oauth2` needs a login, made once per blueprint
and server with `submilli mcp authenticate` locally or
`submilli server mcp authenticate` for a registered blueprint. Both run on
the machine where the command is typed: they print an authorization URL and
wait for the browser's redirect on `http://127.0.0.1:8765/callback`.
`SUBMILLI_OAUTH_REDIRECT_PORT` changes the port. The flow uses PKCE.

Endpoints the blueprint doesn't set are discovered from the server's OAuth
metadata.

### Client id

The first of:

1. `auth.client_id` in the blueprint.
2. The `client_id` of a configured [provider](#providers) for the OAuth
   host.
3. A client registered at login through the server's dynamic client
   registration endpoint, when it advertises one.

Without any of them, the login fails and names the two fixes: setting
`auth.client_id`, or configuring a provider.

### Scopes

The first non-empty list of: `auth.scopes`, the provider's `scopes`, and
the `scopes_supported` the server advertises.

### Providers

A provider holds an OAuth application registered with a service: its client
id, and a client secret for a confidential client.

| Field | Type | Required | Value |
| --- | --- | --- | --- |
| `match` | string | yes | The OAuth host, such as `github.com`; not the MCP server's host |
| `client_id` | string | yes | A literal, `${secrets.NAME}`, or `${env.VAR}` |
| `client_secret` | string | no | `${secrets.NAME}` or `${env.VAR}`; absent for a public client |
| `scopes` | list of strings | no | Scopes to request |

| | Local | `submilli-server` |
| --- | --- | --- |
| Where | `$SUBMILLI_HOME/mcp_oauth.yaml` (default `~/.submilli/mcp_oauth.yaml`), under `providers` | The config file, under `mcp_oauth.providers`; see [Server settings](/docs/reference/server-settings) |
| Edited with | `submilli mcp provider add`, `list`, `remove` | The config file |
| `match` compared with | The token endpoint's host | The authorization endpoint's host when choosing the client id; the token endpoint's host for the token exchange |
| `${secrets.NAME}` from | The local secret store | The server's secret store |

```yaml title="~/.submilli/mcp_oauth.yaml"
providers:
- match: github.com
  client_id: Iv1.example
  client_secret: ${secrets.GITHUB_CLIENT_SECRET}
  scopes:
  - repo
```

### Credentials and tokens

A login is stored in the secret store under
`mcp_oauth/<blueprint>/<server>/credential`: the local store for
`submilli mcp authenticate`, the server's store for
`submilli server mcp authenticate`. The two are separate. The server's
login needs the server to have a secret store. One login serves every
program run under the blueprint, and on a server every session of it.

| Event | Behavior |
| --- | --- |
| The service issues a refresh token | Only the refresh token is stored |
| The service issues only an access token | The access token is stored |
| A call needs an access token | Obtained with the refresh token and kept in memory, never stored; replaced 60 seconds before it expires, or after 5 minutes when the service gives no lifetime |
| The service issues a new refresh token | It replaces the stored one |
| The service refuses the refresh token (`invalid_grant`) | Retried after 1 and 2 more seconds, then `McpAuthExpiredError`; the stored login is kept |
| A call fails | The token is refreshed and the call retried once |
| `deauthenticate` | The login is removed; the blueprint is `PENDING` |

Running `authenticate` again replaces the stored login.

## Limits

| Limit | Value |
| --- | --- |
| Transport | HTTP only (`streamable_http`). A server that speaks MCP over standard input and output needs an HTTP endpoint in front of it |
| MCP features | Tools only; resources, prompts, and sampling aren't used |
| Discovery | 10 seconds per server |
| One call | 60 seconds, including obtaining a token and connecting; not configurable |
| Results | Text parts and structured content; other content is dropped |
| Streaming | None; a call returns when the tool finishes, and progress messages are ignored |
| Network | Locally, any address. On `submilli-server`, the server's outbound network rules apply to discovery, calls, and the OAuth exchange; private and loopback addresses are refused unless the server allows them (see [Server settings](/docs/reference/server-settings)) |

## Local and server commands

| Task | Local, with a blueprint file | On `submilli-server`, with a registered blueprint |
| --- | --- | --- |
| Declare a server | `submilli blueprint add-mcp <name> <url>` | `submilli server blueprint apply <file>` after declaring it |
| Run a program | `submilli run --blueprint <file> <script>` | `submilli server run-code <script> --blueprint <blueprint>` |
| List a server's tools | `submilli docs @mcp/<name> [--blueprint <file>]` (default `blueprint.yaml`) | `submilli server docs @mcp/<name> --blueprint <blueprint>` |
| Log in | `submilli mcp authenticate --blueprint <file> <name>` | `submilli server mcp authenticate <blueprint> <name>` |
| Check logins | `submilli mcp auth-status --blueprint <file>` | `submilli server mcp auth-status <blueprint>` |
| Log out | `submilli mcp deauthenticate --blueprint <file> <name>` | `submilli server mcp deauthenticate <blueprint> <name>` |
| Providers | `submilli mcp provider add\|list\|remove` | `mcp_oauth.providers` in the config file |
| Logins kept in | The local secret store, in plain files | The server's secret store, encrypted |
| Private and loopback addresses | Allowed | Refused unless the server allows them |
| Tool list refreshed | On every run | When the blueprint is applied, a login changes, or the server restarts |

The steps for declaring a server, choosing tools, and logging in are in
[Add an MCP server](/docs/blueprints/add-an-mcp-server).
