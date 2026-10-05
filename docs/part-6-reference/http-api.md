---
title: "HTTP API"
description: "The endpoints a harness calls to run programs over HTTP: sessions and their execute, rebind, last-run, and delete; the execute result; the prompt, package, and built-in descriptions; and one-off runs."
slug: reference/http-api
sidebar:
  order: 14
authorship:
  label: ai-assisted
  confirmed: true
  contentHash: "472972cbc0d08c0a43d0a295a0006c90201caaa4d8fbd0f4165e0515a0f175fe"
  confirmedAt: "2026-10-05T10:59:51.477Z"
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
| `GET` | [`/v1/blueprints/{name}/prompt`](#get-v1blueprintsnameprompt) |
| `GET` | [`/v1/blueprints/{name}/packages/search`](#get-v1blueprintsnamepackagessearch) |
| `GET` | [`/v1/blueprints/{name}/packages/docs`](#get-v1blueprintsnamepackagesdocs) |
| `GET` | [`/v1/blueprints/{name}/builtins`](#get-v1blueprintsnamebuiltins) |
| `GET` | [`/v1/blueprints/{name}/builtins/docs`](#get-v1blueprintsnamebuiltinsdocs) |
| `POST` | [`/v1/execute`](#post-v1execute) |

## Sessions

A session binds a blueprint, variables, and harness secrets once, and every
program run in it uses them. Its files last as long as the blueprint's `vfs`
keeps them. A session ends on `DELETE`, when its blueprint is removed, or
after the blueprint's `idle_timeout` without a request. Sessions survive a
server restart. Harness secrets do not, because the server keeps them in
memory only.

### POST /v1/sessions

Role: `user`. Opens a session.

| Field | Type | Required | Value |
| --- | --- | --- | --- |
| `blueprint` | string | yes | Registered blueprint |
| `variables` | object of strings | no | Values for the blueprint's `variables` |
| `secrets` | object of strings | no | Values for the blueprint's `harness` secrets |

Answers `200` with `{"session_id": <id>}` and the id in an `mcp-session-id`
response header.

| Status | Body |
| --- | --- |
| `200` | `{"session_id":"147f9327-7c8b-41bd-b0a8-3d4153d695b8"}` |
| `400` | `{"error":"invalid variables: required variable 'userId' was not supplied"}` |
| `400` | `{"error":"invalid variables: variable 'extra' is not declared in the blueprint"}` |
| `400` | `{"error":"invalid secrets: required harness secret 'USER_TOKEN' is missing or empty"}` |
| `400` | `{"error":"invalid secrets: secret 'OTHER' is not declared"}` |
| `400` | `{"error": <text>}` when a `vfs` or `git` path filled from a variable is invalid |
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
id in an `mcp-session-id` response header.

| Status | Body |
| --- | --- |
| `200` | The execute result |
| `404` | `{"error":"unknown session","session_id":"nosuch"}` |
| `404` | `{"error":"blueprint no longer exists","message": <text>,"name": <blueprint>}` |
| `409` | `session_requires_secrets`, below |
| `400`, `409`, `503` | An `Idempotency-Key` refusal, below |

`409 session_requires_secrets` answers a session whose blueprint requires a
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
body without running anything. The record lasts as long as the session.

| Situation | Status | `error` |
| --- | --- | --- |
| First use of the key | `200` | The program runs and its result is recorded |
| Same key, same code, outcome recorded | `200` | The recorded body, replayed |
| Same key, same code, first request still running | — | The request waits up to 300 seconds for that outcome |
| Same key, different code | `409` | `idempotency_conflict` |
| Same key, an earlier request never recorded an outcome (the server stopped mid-run) | `409` | `idempotency_incomplete` |
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
| `secrets` | object of strings | yes | Values for the blueprint's `harness` secrets |

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

Answers `404` with an empty body when no run is recorded for the id. Runs
are recorded in memory.

### DELETE /v1/sessions/{session_id}

Role: `user`. Ends the session and deletes its `per_session` files. Answers
`204`, or `404` with an empty body for an unknown session.

## The execute result

`POST /v1/sessions/{session_id}/execute` and `POST /v1/execute` answer this
object.

| Field | Type | Value |
| --- | --- | --- |
| `execution_id` | string | The run, as named in the [audit trail](/docs/reference/audit-trail#execution) |
| `session_id` | string | The session the program ran in |
| `result` | string or null | What `main` returned: a string as is, any other value as JSON text, `null` for no value or on failure |
| `console` | string[] | Lines the program logged. Empty after a successful run. On failure, what it logged before it stopped |
| `error` | object or null | `null` on success |
| `error.kind` | string | One of the kinds below |
| `error.message` | string | Rendered message, with source excerpts for compile and runtime errors |
| `error.caller`, `error.capability`, `error.source` | strings | Present on `permission_denied` only: the package that was refused, the capability it asked for, and who refused: `policy` (the blueprint), `invariant` (the runtime, ahead of any policy), or `read_only` (a write to a read-only volume) |
| `error.diagnostics` | object[] | Present on compile errors only: `severity`, `line`, `column`, `message`, and `notes` (each `line`, `column`, `message`) when there are any |
| `discovery_warnings` | string[] | Present only when non-empty. Tools of an imported `@mcp/<server>` package that were dropped or degraded at discovery |

`GET /v1/sessions/{session_id}/last-run` returns the console of a
successful run.

| `error.kind` | Cause |
| --- | --- |
| `compile_error` | The program does not compile |
| `runtime_error` | The program threw, or the run failed for another reason |
| `permission_denied` | A gated call was denied and the denial escaped the program uncaught. Only a denial the runtime threw counts: a `PermissionDeniedError` the program constructs is a `runtime_error` |
| `timeout` | The run passed `max_execution_time` |
| `fuel_exhausted` | The run burned `max_execution_fuel` |
| `memory_exhausted` | The run passed `max_execution_memory` |
| `stack_exhausted` | The run passed `max_execution_stack` |
| `package_resolution` | An imported package could not be prepared |
| `blueprint_not_found` | The blueprint is not registered or not usable (`POST /v1/execute` only) |
| `invalid_request` | Variables, secrets, or the `vfs` or `git` paths they fill are invalid (`POST /v1/execute` only) |

Examples, from real runs:

```json
{"execution_id":"f0fbcebf-ce95-4df5-ba4b-ba645a09b470","session_id":"3abfead8-1e3c-4be9-b8c7-622f27938d5a","result":"2","console":[],"error":null}
```

```json
{"execution_id":"30ae2b06-c066-43c1-a5be-e3e37ebbeb09","session_id":"876321aa-da28-4d9b-99d9-91c20508d12b","result":"{\"label\":\"two\",\"total\":2}","console":[],"error":null}
```

```json
{"execution_id":"f3aa2f85-cc80-4725-b603-b321c05ab2c5","session_id":"b7f51b45-6ba9-45f8-ae34-fbfb461dfa06","result":null,"console":["before"],"error":{"kind":"runtime_error","message":"error: Error: boom\n  at main (<execute>:1:73)  [thrown here]\n1 | export function main(): number { console.log(\"before\"); throw new Error(\"boom\"); }\n  |                                                                         ^\n"}}
```

```json
{"execution_id":"704a5868-7459-4c1f-8f5a-8a6d09a6312f","session_id":"3a0f75d1-2d5e-4c3a-a5d8-3ecfbab7015c","result":null,"console":[],"error":{"kind":"compile_error","message":"error: expected `number`, got `string`\n --> <execute>:1:52\n  |\n1 | export function main(): number { const x: number = \"a\"; return x; }\n  |                                                    ^^^\n","diagnostics":[{"severity":"error","line":1,"column":52,"message":"expected `number`, got `string`"}]}}
```

## Blueprint discovery

These endpoints serve what an agent needs to write programs for one
blueprint. Each answers `404` for a blueprint that is not registered:

```json
{"error":"not_found","message":"blueprint 'nope' is not registered","name":"nope"}
```

### GET /v1/blueprints/{name}/prompt

Role: `user`. Returns tool descriptions for a harness that builds its own
tools on this API.

| Field | Type | Value |
| --- | --- | --- |
| `name` | string | The blueprint |
| `prompt` | string | The execute tool's description, with this blueprint's modules, packages, files, and rules filled in |
| `tools.search` | string | Description for a package-search tool |
| `tools.docs` | string | Description for a package-docs tool |
| `tools.builtins` | string | Description for one tool that both lists built-ins and returns their declarations |
| `tools.builtins_list` | string | Description for a built-ins list tool |
| `tools.builtins_docs` | string | Description for a built-ins docs tool |
| `tools.last_run` | string | Description for a last-run tool |

### GET /v1/blueprints/{name}/packages/search

Role: `user`. Query parameter `q` (optional, default empty) is a substring
matched against package names, descriptions, and exported symbols. Searches
the standard-library modules, the blueprint's packages, and its
`@mcp/<server>` packages.

```json
{"results":[{"description":"UUID v4/v7 generation and validation.","name":"submilli:uuid","source":"host"}]}
```

Each result has `name`, `description`, and `source`, which is `host` for a
standard-library module, `registry` for an installed package, or `mcp` for an
`@mcp/<server>` package. A search with no hits answers `results: []` with
`available_packages` (the same entries), `builtins` (a pointer to the
built-ins endpoint), and `available_packages_omitted` when the listing is
cut short.

### GET /v1/blueprints/{name}/packages/docs

Role: `user`. Query parameter `name` (required) is a package name.

A standard-library module answers `200` with JSON `name`, `source`,
`description`, and `declarations` (TypeScript declarations):

```json
{"declarations":"/**\n * Generate a random UUID v4 (RFC 4122). Returns the canonical lowercase hyphenated form.\n */\nfunction v4(): string;\n\n/**\n * Generate a time-ordered UUID v7 (RFC 9562). Sortable / index-friendly; prefer over `v4` when the destination is a sorted store. Returns the canonical lowercase hyphenated form.\n */\nfunction v7(): string;\n\n/**\n * Returns `true` if `string` is a valid UUID (any version), `false` otherwise.\n * @param string The candidate UUID text.\n */\nfunction validate(string: string): boolean;","description":"UUID v4/v7 generation and validation.","name":"submilli:uuid","source":"host"}
```

An installed package and an `@mcp/<server>` package answer `200` with its
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
{"namespaces":["JSON","Math","Temporal"],"types":["Array","BigInt","Boolean","Error","Map","Number","Object","PermissionDeniedError","QuotaExceededError","RangeError","Record","RegExp","Set","String","SyntaxError","TextDecoder","TextEncoder","TypeError","Uint8Array"]}
```

### GET /v1/blueprints/{name}/builtins/docs

Role: `user`. Query parameter `name`, repeated once per built-in
(`?name=Math&name=Array`). Answers `200` with `results`, one entry per name
in order. An entry is `{name, declarations}` when found, otherwise `{name, error,
message}` and, when there is a suggestion, `did_you_mean`. `error` is
`unknown_builtin`, or `not_a_builtin` for a package name:

```json
{"results":[{"did_you_mean":"submilli:code","error":"unknown_builtin","message":"unknown built-in: Nope. Did you mean `submilli:code`?","name":"Nope"}]}
```

## One-off runs

### POST /v1/execute

Role: `user`. Runs one program in a new session that ends when the program
returns.

| Field | Type | Required | Value |
| --- | --- | --- | --- |
| `code` | string | yes | Program source, with an exported `main` |
| `blueprint` | string | yes | Registered blueprint to run under |
| `variables` | object of strings | no | Values for the blueprint's `variables` |
| `secrets` | object of strings | no | Values for the blueprint's `harness` secrets |

Answers `200` with the [execute result](#the-execute-result) and the session
id in an `mcp-session-id` response header, for every outcome, including an
unknown blueprint (`error.kind` `blueprint_not_found`) and invalid variables
or secrets (`error.kind` `invalid_request`):

```json
{"execution_id":"06c8d2b3-bd65-4ec5-95c6-92426962ead9","session_id":"de5c829e-6520-4563-86be-9acfa84e900d","result":null,"console":[],"error":{"kind":"invalid_request","message":"invalid variables: required variable 'userId' was not supplied"}}
```
