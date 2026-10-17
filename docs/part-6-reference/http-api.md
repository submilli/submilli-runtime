---
title: "HTTP API"
description: "The endpoints a harness calls to run programs over HTTP: sessions and their execute, rebind, last-run, and delete; the execute result; application tools and their runs; the prompt, Package, and built-in descriptions; and one-off runs."
slug: reference/http-api
sidebar:
  order: 14
authorship:
  label: ai-assisted
  confirmed: true
  contentHash: "dfa2b77df768a11b77cfb80063f815edec090861ee9e5efc3d8c8cb36ffaa0ca"
  confirmedAt: "2026-10-15T18:31:05.000Z"
---

This page describes the endpoints of `submilli-server` a harness calls to run
an agent's programs over HTTP: sessions, the execute result, the
descriptions a harness builds its tools from, and one-off runs.
[Use the HTTP API](/docs/tutorials/use-the-http-api) walks through them. A
harness that speaks MCP connects to `/mcp/<blueprint>` instead, as
[Connect a harness](/docs/tutorials/connect-a-harness) shows. The server's
other endpoints serve the `submilli server` commands. For the settings named
here, see [Server settings](/docs/reference/server-settings).

`DELETE /mcp/<blueprint>` with an `MCP-Session-Id` header ends the MCP
session and returns `204`. An unknown or already-ended session returns `404`.

## Conventions

The server listens on `http://127.0.0.1:8128` by default. Paths below are
relative to that address.

### Authentication

Every endpoint on this page requires `Authorization: Bearer <token>`, with a
token of either role, `user` or `admin`. `SUBMILLI_SERVER_TOKEN` is an
`admin` token. Tokens declared under `api_tokens` in the config file carry
the role given there. A server started with `--allow-unauthenticated`
checks no token.

A missing or unknown token answers `401` with a `WWW-Authenticate: Bearer`
header:

```json
{"error":"unauthorized","message":"missing or unrecognised API token; send `Authorization: Bearer <token>` with a token this server was started with"}
```

### Request bodies

A request body is JSON and must be sent with `Content-Type: application/json`.
The JSON body of a `/v1` endpoint is limited to 2 MiB. Malformed requests are
refused before the endpoint runs, with a plain-text body:

| Status | Cause | Body (example) |
| --- | --- | --- |
| `400` | Body is not JSON | `Failed to parse the request body as JSON: key must be a string at line 1 column 2` |
| `400` | Required query parameter missing | ``Failed to deserialize query string: missing field `name` `` |
| `413` | Body over 2 MiB | `Failed to buffer the request body: length limit exceeded` |
| `415` | No `Content-Type: application/json` | ``Expected request with `Content-Type: application/json` `` |
| `422` | JSON of the wrong shape | ``Failed to deserialize the JSON body into the target type: missing field `code` at line 1 column 20`` |

### Error bodies

Most errors answer `{ "error": <code>, "message": <text> }`, where `error` is
a stable code and `message` is for people. Some endpoints add fields to it,
and the session endpoints use other shapes. Each endpoint's section lists
its own. A program that fails is not an HTTP error. `POST /v1/execute` and
`POST /v1/sessions/{session_id}/execute` answer `200` with the failure in the
[execute result](#the-execute-result).

### Endpoints

| Method | Path |
| --- | --- |
| `POST` | [`/v1/sessions`](#post-v1sessions) |
| `POST` | [`/v1/sessions/{session_id}/execute`](#post-v1sessionssession_idexecute) |
| `POST` | [`/v1/sessions/{session_id}/rebind`](#post-v1sessionssession_idrebind) |
| `GET` | [`/v1/sessions/{session_id}/last-run`](#get-v1sessionssession_idlast-run) |
| `DELETE` | [`/v1/sessions/{session_id}`](#delete-v1sessionssession_id) |
| `POST` | [`/v1/runs/{run_id}/resume`](#post-v1runsrun_idresume) |
| `GET` | [`/v1/runs/{run_id}`](#get-v1runsrun_id) |
| `GET` | [`/v1/sessions/{session_id}/packages/search`](#get-v1sessionssession_idpackagessearch) |
| `GET` | [`/v1/sessions/{session_id}/packages/docs`](#get-v1sessionssession_idpackagesdocs) |
| `GET` | [`/v1/blueprints/{name}/prompt`](#get-v1blueprintsnameprompt) |
| `GET` | [`/v1/blueprints/{name}/packages/search`](#get-v1blueprintsnamepackagessearch) |
| `GET` | [`/v1/blueprints/{name}/packages/docs`](#get-v1blueprintsnamepackagesdocs) |
| `GET` | [`/v1/blueprints/{name}/builtins`](#get-v1blueprintsnamebuiltins) |
| `GET` | [`/v1/blueprints/{name}/builtins/docs`](#get-v1blueprintsnamebuiltinsdocs) |
| `POST` | [`/v1/execute`](#post-v1execute) |

## Sessions

A session binds a Blueprint, variables, and harness secrets once, and every
program run in it uses them. Its files last as long as the Blueprint's `vfs`
keeps them. A session ends on `DELETE`, when its Blueprint is removed, or
after the Blueprint's `idle_timeout` without a request. Sessions survive a
server restart. Harness secrets do not, because the server keeps them in
memory only.

### POST /v1/sessions

Role: `user`. Opens a session.

| Field | Type | Required | Value |
| --- | --- | --- | --- |
| `blueprint` | string | yes | Registered Blueprint |
| `variables` | object of strings | no | Values for the Blueprint's `variables` |
| `secrets` | object of strings | no | Values for the Blueprint's `harness` secrets |
| `tools` | object[] | no | The application's tools programs may call as `@tools`. See [Application tools](#application-tools) |

Answers `200` with `{"session_id": <id>}` and the id in an `mcp-session-id`
response header. When `tools` were declared, `warnings` lists what programs
will not see of them: tools dropped because their schema can't be expressed,
tools that return `unknown`, and a Blueprint that grants no `tools.call`.

| Status | Body |
| --- | --- |
| `200` | `{"session_id":"147f9327-7c8b-41bd-b0a8-3d4153d695b8"}` |
| `400` | `{"error":"invalid variables: required variable 'userId' was not supplied"}` |
| `400` | `{"error":"invalid variables: variable 'extra' is not declared in the blueprint"}` |
| `400` | `{"error":"invalid secrets: required harness secret 'USER_TOKEN' is missing or empty"}` |
| `400` | `{"error":"invalid secrets: secret 'OTHER' is not declared"}` |
| `400` | `{"error": <text>}` when a `vfs` or `git` path filled from a variable is invalid |
| `400` | `{"error":"invalid tools: a session may declare at most 256 tools"}`, or the catalog is over 1 MiB of JSON |
| `404` | `{"error":"unknown blueprint","message":"unknown blueprint: nope","name":"nope"}` |
| `500` | `{"error": <text>}` when the session could not be created |

### POST /v1/sessions/{session_id}/execute

Role: `user`. Runs one program in the session.

| Field | Type | Required | Value |
| --- | --- | --- | --- |
| `code` | string | yes | Program source, with an exported `main` |

| Header | Required | Value |
| --- | --- | --- |
| `Idempotency-Key` | no | Printable ASCII, 1 to 120 bytes, sent once. See [Idempotency](#idempotency) |

Answers `200` with the [execute result](#the-execute-result) and the session
id in an `mcp-session-id` response header. In a session with
[application tools](#application-tools), a run can also answer `suspended` at
a tool call.

| Status | Body |
| --- | --- |
| `200` | The execute result |
| `404` | `{"error":"unknown session","session_id":"nosuch"}` |
| `404` | `{"error":"blueprint no longer exists","message": <text>,"name": <blueprint>}` |
| `409` | `session_requires_secrets`, below |
| `400`, `409`, `503` | An `Idempotency-Key` refusal, below |

`409 session_requires_secrets` answers a session whose Blueprint requires a
harness secret the server no longer holds, as after a restart. `required`
lists the secrets to send to
[`rebind`](#post-v1sessionssession_idrebind):

```json
{"error":"session_requires_secrets","required":["USER_TOKEN"],"session_id":"a5994115-19ff-4de5-8040-27e6f1227e08"}
```

#### Idempotency

With an `Idempotency-Key`, the server records the outcome of the run under
the pair of session and key, on disk next to the session store. A later
request with the same key and the same `code` replays the recorded status and
body without running anything.

A recorded body is kept for 24 hours (`idempotency_retention`), and the
session's three newest are kept whatever their age (`idempotency_keep_last`).
Past both, the body is dropped and the key stays: it still refuses to run the
code again. The record of the key lasts as long as the session.

| Situation | Status | `error` |
| --- | --- | --- |
| First use of the key | `200` | The program runs and its result is recorded |
| Same key, same code, outcome recorded | `200` | The recorded body, replayed |
| Same key, same code, first request still running | — | The request waits up to 300 seconds for that outcome |
| Same key, same code, the run may wait on application tools | `200` | The run's next settled state: `suspended` with its pending call, or `finished` |
| Same key, different code | `409` | `idempotency_conflict` |
| Same key, an earlier request never recorded an outcome (the server stopped mid-run) | `409` | `idempotency_incomplete` |
| Same key, same code, the recorded body is no longer kept | `409` | `idempotency_response_expired` |
| Same key, the wait for the running request ran out | `503` | `idempotency_in_progress` |
| The ledger could not be read or written | `503` | `idempotency_unavailable` |
| Key empty, over 120 bytes, not printable ASCII, or sent twice | `400` | `idempotency_key_invalid` |

A run that ends before the program is dispatched, such as a
`package_resolution` failure, records nothing, and the key can be used
again. A refusal answers `{error, session_id, detail}`:

```json
{"detail":"this idempotency key was already used with different code","error":"idempotency_conflict","session_id":"147f9327-7c8b-41bd-b0a8-3d4153d695b8"}
```

```json
{"detail":"Idempotency-Key must be at most 120 bytes","error":"idempotency_key_invalid","session_id":"147f9327-7c8b-41bd-b0a8-3d4153d695b8"}
```

### POST /v1/sessions/{session_id}/rebind

Role: `user`. Replaces the session's harness secrets.

| Field | Type | Required | Value |
| --- | --- | --- | --- |
| `secrets` | object of strings | yes | Values for the Blueprint's `harness` secrets |

| Status | Body |
| --- | --- |
| `204` | Empty |
| `400` | `{"error":"invalid secrets: required harness secret 'USER_TOKEN' is missing or empty"}` |
| `404` | `{"error":"unknown session","session_id":"nosuch"}` |
| `404` | `{"error":"blueprint no longer exists","message": <text>,"name": <blueprint>}` |

### GET /v1/sessions/{session_id}/last-run

Role: `user`. Returns the session's most recent run, with its full console.

| Field | Type | Value |
| --- | --- | --- |
| `result` | string or null | As in the execute result |
| `console` | string[] | Every line the run logged, on success too |
| `error` | object or null | As in the execute result |

```json
{"result":"saved","console":["writing"],"error":null}
```

Answers `404` with an empty body when no run is recorded for the id,
including once the session has ended, and after a run whose end is unknown,
such as one the server was stopped in. A run over
[`max_last_run_size`](/docs/reference/server-settings) is kept without its
result and console, and answers `413`:

```json
{"error":"last_run_too_large","bytes":70254831,"limit":67108864}
```

### DELETE /v1/sessions/{session_id}

Role: `user`. Ends the session and deletes its `per_session` files. Answers
`204`, or `404` with an empty body for an unknown session.

## The execute result

`POST /v1/sessions/{session_id}/execute` and `POST /v1/execute` answer this
object.

| Field | Type | Value |
| --- | --- | --- |
| `status` | string | Always `finished`. A run that waits on [application tools](#application-tools) can also be `suspended` or `running`, see [Run states](#run-states) |
| `execution_id` | string | The run, as named in the [audit trail](/docs/reference/audit-trail#execution) |
| `session_id` | string | The session the program ran in |
| `result` | string or null | What `main` returned: a string as is, other concrete values as JSON text (including `"null"` for a null return), `null` for an undefined return, no value, or failure |
| `console` | string[] | Lines the program logged. Empty after a successful session run. On failure, what it logged before it stopped |
| `error` | object or null | `null` on success |
| `error.kind` | string | One of the kinds below |
| `error.message` | string | Rendered message, with source excerpts for compile and runtime errors |
| `error.caller`, `error.capability`, `error.source` | strings | Present on `permission_denied` only: the Package that was refused, the capability it asked for, and who refused: `policy` (the Blueprint), `invariant` (the runtime, ahead of any policy), `read_only` (a write to a read-only volume), or `harness` (the application refused a `@tools` call) |
| `error.diagnostics` | object[] | Present on compile errors only: `severity`, `line`, `column`, `message`, and `notes` (each `line`, `column`, `message`) when there are any |
| `discovery_warnings` | string[] | Present only when non-empty. Tools of an imported `@mcp/<server>` or `@tools` Package that were dropped or degraded at discovery |

`GET /v1/sessions/{session_id}/last-run` returns the console of a
successful session run. `POST /v1/execute` returns it in `console`.

| `error.kind` | Cause |
| --- | --- |
| `compile_error` | The program does not compile |
| `runtime_error` | The program threw, or the run failed for another reason |
| `permission_denied` | A gated call was denied and the denial escaped the program uncaught. Only a denial the runtime threw counts: a `PermissionDeniedError` the program constructs is a `runtime_error` |
| `timeout` | The run passed `max_execution_time` |
| `fuel_exhausted` | The run burned `max_execution_fuel` |
| `memory_exhausted` | The run passed `max_execution_memory` |
| `stack_exhausted` | The run passed `max_execution_stack` |
| `package_resolution` | An imported Package could not be prepared |
| `blueprint_not_found` | The Blueprint is not registered or not usable (`POST /v1/execute` only) |
| `invalid_request` | Variables, secrets, or the `vfs` or `git` paths they fill are invalid (`POST /v1/execute` only) |

Examples, from real runs:

```json
{"status":"finished","execution_id":"f0fbcebf-ce95-4df5-ba4b-ba645a09b470","session_id":"3abfead8-1e3c-4be9-b8c7-622f27938d5a","result":"2","console":[],"error":null}
```

```json
{"status":"finished","execution_id":"30ae2b06-c066-43c1-a5be-e3e37ebbeb09","session_id":"876321aa-da28-4d9b-99d9-91c20508d12b","result":"{\"label\":\"two\",\"total\":2}","console":[],"error":null}
```

```json
{"status":"finished","execution_id":"f3aa2f85-cc80-4725-b603-b321c05ab2c5","session_id":"b7f51b45-6ba9-45f8-ae34-fbfb461dfa06","result":null,"console":["before"],"error":{"kind":"runtime_error","message":"error: Error: boom\n  at main (<execute>:1:73)  [thrown here]\n1 | export function main(): number { console.log(\"before\"); throw new Error(\"boom\"); }\n  |                                                                         ^\n"}}
```

```json
{"status":"finished","execution_id":"704a5868-7459-4c1f-8f5a-8a6d09a6312f","session_id":"3a0f75d1-2d5e-4c3a-a5d8-3ecfbab7015c","result":null,"console":[],"error":{"kind":"compile_error","message":"error: expected `number`, got `string`\n --> <execute>:1:52\n  |\n1 | export function main(): number { const x: number = \"a\"; return x; }\n  |                                                    ^^^\n","diagnostics":[{"severity":"error","line":1,"column":52,"message":"expected `number`, got `string`"}]}}
```

## Application tools

A harness can let programs call the application's own tools, such as a
LangChain `@tool` that queries the application's database. It declares them
when it opens the session, and programs import them as the `@tools` Package.
Each call stops the run and hands the call to the harness, which runs the
tool through its normal path, approvals included, and resumes the run with
the result.

`tools` on [`POST /v1/sessions`](#post-v1sessions) is a list of tools, each
in the shape of an MCP `tools/list` entry:

| Field | Type | Required | Value |
| --- | --- | --- | --- |
| `name` | string | yes | A TypeScript function name |
| `description` | string | no | Shown in the Package docs |
| `input_schema` | object | yes | JSON Schema of the tool's one `args` object |
| `output_schema` | object | no | JSON Schema of the result. Without one, the tool returns `unknown` |

```json
{"blueprint":"orders","tools":[{"name":"query_orders","description":"Orders of one customer.","input_schema":{"type":"object","properties":{"customer":{"type":"string"}},"required":["customer"]},"output_schema":{"type":"object","properties":{"count":{"type":"number"}},"required":["count"]}}]}
```

The tools are mapped as `@mcp/<server>` tools are, and fixed for the
session's life. They survive a server restart with the session. Programs see
`@tools` only when the Blueprint grants `tools.call`, which is denied by
default. Its `tool` filter field names the tool. MCP sessions and
[`POST /v1/execute`](#post-v1execute) have no application tools.

```ts
import tools from "@tools";

function main(): string {
    const orders = tools.query_orders({ customer: "c_42" });
    return `${orders.count} orders`;
}
```

### Run states

Every execute answers with the run's state, as do `resume` and
`GET /v1/runs/{run_id}`. Only a run in a session with application tools can be
anything but `finished`. A run's id is its `execution_id`.

| `status` | Body |
| --- | --- |
| `suspended` | `execution_id`, `session_id`, and `calls`: the calls waiting on the harness, each `call_id`, `tool`, and `args` |
| `finished` | The [execute result](#the-execute-result) |
| `running` | `execution_id` and `session_id`. Only `GET` answers it: between a resume and the next state, or after a call timed out or the run was cancelled, until it finishes |

```json
{"status":"suspended","execution_id":"8d1c1b0e-6c0c-4a0f-9b5e-3f6f0f9b2a11","session_id":"b6cc8dc3-631f-4bc2-8ac3-921053ad88c7","calls":[{"call_id":"8d1c1b0e-6c0c-4a0f-9b5e-3f6f0f9b2a11:1","tool":"query_orders","args":{"customer":"c_42"}}]}
```

A call id is the run id and the call's sequence number, so it is stable
across retries. One suspension carries one call. A run does not depend on the
request that started it: it carries on when the client goes away. A harness
that may lose a response sends an `Idempotency-Key`, and a retry with the same
key answers with the run's current state.

| Limit | Value |
| --- | --- |
| Time | `max_execution_time` and fuel count the program's work only, not the time it waits on the harness |
| One wait | The Blueprint's `tool_call_timeout`, 10 minutes by default. The pending call then throws `TimeoutError` in the program, and so does every later `@tools` call of that run, without waiting |
| Waiting runs | At most `max_suspended_runs` server-wide, 256 by default. Past it, the call throws `QuotaExceededError` |
| Shutdown | A server shutting down cancels its suspended runs. A run that is working carries on, but a later `@tools` call cancels it. Either way its key stays indeterminate |
| Restart | A suspended run is lost on restart: `GET` answers `404` `unknown_run` |
| Session end | Deleting the session ends its runs. A session with an unfinished run is not idle |
| Finished runs | Readable with `GET` for 10 minutes, if they suspended. A run that never did is answered whole by its execute |

### POST /v1/runs/{run_id}/resume

Role: `user`. Answers the run's pending call, then waits for the run to
suspend again or finish, and answers with that state.

| Field | Type | Required | Value |
| --- | --- | --- | --- |
| `results` | object[] | yes | One answer per call, at least one, each call once |
| `results[].call_id` | string | yes | The call answered |
| `results[].ok` | boolean | yes | Whether the tool returned a value |
| `results[].value` | any | no | With `ok: true`: what the tool returned |
| `results[].error.kind` | string | with `ok: false` | `failed` or `denied` |
| `results[].error.message` | string | with `ok: false` | Shown to the program |

```json
{"results":[{"call_id":"8d1c1b0e-6c0c-4a0f-9b5e-3f6f0f9b2a11:1","ok":true,"value":{"count":3}}]}
```

| Answer | In the program |
| --- | --- |
| `ok: true` | The tool's return value, checked against its `output_schema`. A value that does not match throws `TypeError` |
| `failed` | Throws `Error` with the message |
| `denied` | Throws `PermissionDeniedError`. Uncaught, the run reports `permission_denied` with `source` `harness` |

An answer to a call already answered changes nothing, so a resume can be
retried. A value may nest at most 128 levels deep, inside the 2 MiB limit on
request bodies.

| Status | `error` |
| --- | --- |
| `200` | The run's state |
| `400` | `invalid_request`: no answer, a call answered twice, a malformed answer, or a value over the limits. Nothing is delivered |
| `400` | `unknown_call`: the run waits on another call |
| `404` | `unknown_run` |
| `409` | `run_not_suspended`: the run waits on nothing, because it finished, its call timed out, or it was cancelled |

Refusals answer `{error, execution_id, message}`.

### GET /v1/runs/{run_id}

Role: `user`. Answers the run's state now, or `404` `unknown_run`.

## Blueprint discovery

These endpoints serve what an agent needs to write programs for one
Blueprint. Each answers `404` for a Blueprint that is not registered:

```json
{"error":"not_found","message":"blueprint 'nope' is not registered","name":"nope"}
```

### GET /v1/blueprints/{name}/prompt

Role: `user`. Returns tool descriptions for a harness that builds its own
tools on this API.

| Field | Type | Value |
| --- | --- | --- |
| `name` | string | The Blueprint |
| `prompt` | string | The execute tool's description, with this Blueprint's modules, Packages, files, and rules filled in |
| `tools.search` | string | Description for a Package-search tool |
| `tools.docs` | string | Description for a Package-docs tool |
| `tools.builtins` | string | Description for one tool that both lists built-ins and returns their declarations |
| `tools.builtins_list` | string | Description for a built-ins list tool |
| `tools.builtins_docs` | string | Description for a built-ins docs tool |
| `tools.last_run` | string | Description for a last-run tool |

### GET /v1/blueprints/{name}/packages/search

Role: `user`. Query parameter `q` (optional, default empty) is a substring
matched against Package names, descriptions, and exported symbols. Searches
the standard-library modules, the Blueprint's Packages, and its
`@mcp/<server>` Packages.

```json
{"results":[{"description":"UUID v4/v7 generation and validation.","name":"submilli:uuid","source":"host"}]}
```

Each result has `name`, `description`, and `source`, which is `host` for a
standard-library module, `registry` for an installed Package, or `mcp` for an
`@mcp/<server>` Package. A search with no hits answers `results: []` with
`available_packages` (the same entries), `builtins` (a pointer to the
built-ins endpoint), and `available_packages_omitted` when the listing is
cut short.

### GET /v1/blueprints/{name}/packages/docs

Role: `user`. Query parameter `name` (required) is a Package name.

A standard-library module answers `200` with JSON `name`, `source`,
`description`, and `declarations` (TypeScript declarations):

```json
{"declarations":"/**\n * Generate a random UUID v4 (RFC 4122). Returns the canonical lowercase hyphenated form.\n */\nfunction v4(): string;\n\n/**\n * Generate a time-ordered UUID v7 (RFC 9562). Sortable / index-friendly; prefer over `v4` when the destination is a sorted store. Returns the canonical lowercase hyphenated form.\n */\nfunction v7(): string;\n\n/**\n * Returns `true` if `string` is a valid UUID (any version), `false` otherwise.\n * @param string The candidate UUID text.\n */\nfunction validate(string: string): boolean;","description":"UUID v4/v7 generation and validation.","name":"submilli:uuid","source":"host"}
```

An installed Package and an `@mcp/<server>` Package answer `200` with its
documentation followed by its declarations, as
`Content-Type: text/markdown; charset=utf-8`. A built-in name answers JSON
with `source` `builtin`. An unknown name answers `404`:

```json
{"did_you_mean":"submilli:code","error":"unknown_package","message":"unknown package: submilli:nope. Did you mean `submilli:code`?"}
```

`did_you_mean` is present only when there is a suggestion. An unknown
`@mcp/<server>` answers `404` with `error` `unknown_mcp_server`.

### GET /v1/blueprints/{name}/builtins

Role: `user`. Lists the built-ins in scope without an `import`:

```json
{"namespaces":["JSON","Math","Temporal"],"types":["Array","BigInt","Boolean","Error","Map","Number","Object","PermissionDeniedError","QuotaExceededError","RangeError","Record","RegExp","Set","String","SyntaxError","TextDecoder","TextEncoder","TimeoutError","TypeError","Uint8Array"]}
```

### GET /v1/blueprints/{name}/builtins/docs

Role: `user`. Query parameter `name`, repeated once per built-in
(`?name=Math&name=Array`). Answers `200` with `results`, one entry per name
in order. An entry is `{name, declarations}` when found, otherwise `{name, error,
message}` and, when there is a suggestion, `did_you_mean`. `error` is
`unknown_builtin`, or `not_a_builtin` for a Package name:

```json
{"results":[{"did_you_mean":"submilli:code","error":"unknown_builtin","message":"unknown built-in: Nope. Did you mean `submilli:code`?","name":"Nope"}]}
```

### GET /v1/sessions/{session_id}/packages/search

Role: `user`. The [search](#get-v1blueprintsnamepackagessearch) of the
session's Blueprint, plus the session's `@tools` Package with `source`
`tools`. A session without application tools answers exactly as its
Blueprint does. An unknown session answers `404`.

### GET /v1/sessions/{session_id}/packages/docs

Role: `user`. The [docs](#get-v1blueprintsnamepackagesdocs) of the session's
Blueprint, plus `@tools`, which answers as an `@mcp/<server>` Package does.
`@tools` answers `404` in a session without application tools.

## One-off runs

### POST /v1/execute

Role: `user`. Runs one program in a new session that ends when the program
returns, and with it the session's last run. `console` holds every line the
program logged, on success too.

| Field | Type | Required | Value |
| --- | --- | --- | --- |
| `code` | string | yes | Program source, with an exported `main` |
| `blueprint` | string | yes | Registered Blueprint to run under |
| `variables` | object of strings | no | Values for the Blueprint's `variables` |
| `secrets` | object of strings | no | Values for the Blueprint's `harness` secrets |

Answers `200` with the [execute result](#the-execute-result) and the session
id in an `mcp-session-id` response header, for every outcome, including an
unknown Blueprint (`error.kind` `blueprint_not_found`) and invalid variables
or secrets (`error.kind` `invalid_request`):

```json
{"status":"finished","execution_id":"06c8d2b3-bd65-4ec5-95c6-92426962ead9","session_id":"de5c829e-6520-4563-86be-9acfa84e900d","result":null,"console":[],"error":{"kind":"invalid_request","message":"invalid variables: required variable 'userId' was not supplied"}}
```

## Package installation configuration

| Surface | Contract |
| --- | --- |
| `POST /v1/packages/install` | [Configuration inputs and dependency errors](/docs/reference/package-configuration/#administrative-http-api) |
| `GET /v1/packages` | Installed Package metadata, including saved overrides and resolved configuration |
