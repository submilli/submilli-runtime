---
title: "Audit trail"
description: "Every record submilli-server writes to its audit trail: the fields all records share, and the decision, execution, session, admin, auth, and server records with their events and fields."
slug: reference/audit-trail
# Records captured from a build of the audit branch (SUB-1317); every
# record type's fields re-checked against main 08a944c8. MCP login records
# are described from the code; no example was captured.
sidebar:
  order: 10
authorship:
  label: ai-assisted
  confirmed: true
  contentHash: "2f6cc84e7e593b1cf2b2721e9de7401cacf2bd1b9c39b74d2c502dc3d585e715"
  confirmedAt: "2026-10-05T10:59:51.473Z"
---

The server writes one audit record per line, in logfmt, to its log's
output or to `logging.audit.file`. [Server
settings](/docs/reference/server-settings#audit-trail) has the settings
that turn the trail on and off, choose its file, and decide how allowed
operations are recorded.

## Every record

```text
ts=2026-10-03T20:02:39.647Z level=info stream=audit target=submilli_server::audit msg=admin event=blueprint_created event_id=f3343d01-752d-479e-999e-0c5036af2b48 name=support new_hash=5a2a1e61c440e6b36c43c60b2eff7ecad0ed7989bf5dee30d486678206a480d0 old_hash=null outcome=ok principal=SUBMILLI_SERVER_TOKEN route=/v1/blueprints/support schema=submilli.audit/1 status=200 type=admin
```

| Field | Value |
| --- | --- |
| `ts` | When the record was written, UTC, to the millisecond |
| `level` | `info` |
| `stream` | `audit`, which tells a record from a log line |
| `target` | `submilli_server::audit` |
| `msg` | The record's type, as in `type` |
| `type` | `decision`, `execution`, `session`, `admin`, `auth`, or `server` |
| `event` | What happened, for every type except `decision` |
| `event_id` | A UUID unique to this record |
| `schema` | `submilli.audit/1`. A field added keeps the version; a field renamed or removed changes it |

After the first five keys the fields are in alphabetical order. A
nested value is flattened into dotted keys, and a list by position, as in
`vars.0.name=customerId vars.0.value=cus_northwind`. The selected
quick-search `context.*` text fields are cut at 512 characters, and a
URL loses its user, password, query string, and fragment. The full
JSON in `context.payload_json` has neither transformation.

## `decision`

One for each operation a program was refused, and for operations it was
allowed as the `allows` setting says.

```text
ts=2026-10-04T05:44:32.121Z level=info stream=audit target=submilli_server::audit msg=decision blueprint=test blueprint_hash=35d4367e3fb94fcad8d47c0af8c7f2a054a229a69f069928dfcca5f53d835ac5 caller=main capability=fs.write context.length=11 context.path=/denied context.payload_json="{\"length\":11,\"path\":\"/denied\"}" decision=deny event_id=b2a431fb-87ba-4602-8859-013f5f2a8e83 execution_id=375d5dae-685c-4920-a471-c350f1b33271 principal=unauthenticated reason="policy denied the capability" rule=default schema=submilli.audit/1 source=policy type=decision
```

| Field | Value |
| --- | --- |
| `execution_id` | The run that asked. Absent for the MCP file tools, which read a session's files outside a run |
| `session_id` | The session, when there is one |
| `blueprint`, `blueprint_hash` | The blueprint, and a SHA-256 of the version in force |
| `principal` | The name of the API token the request carried |
| `caller` | `main`, or the package that made the call |
| `capability` | The operation, such as `fs.write` or `acme.com/credits.apply` |
| `context.*` | The operation's fields, as below, including `payload_json` |
| `decision` | `allow` or `deny` |
| `source` | What decided: `policy`, the blueprint's rules; `invariant`, a refusal no rule can change, such as `secrets.get` from `main`; `read_only`, a write into a read-only volume; `egress_guard`, the block on private addresses; `quota`, a model-token or session-state budget |
| `rule` | The rule that decided, by its position in the caller's list from `0`, or `default` |
| `reason` | Why it was refused, on refusals only |

`context.payload_json` is the complete JSON context passed to the policy
check, serialized as one logfmt text value. Parse that value as JSON to
recover objects, arrays, numbers, booleans, and nulls. It is written for
package `check()` calls and built-in operations alike, including refusals.
It is not redacted or shortened, so package authors and operators should treat
the audit destination as a store of the data passed to permission checks.
An encoded audit record over 1 MiB is rejected in full, reported to
standard error, and does not stop the program.

For quick searches, `context` also keeps these fields of the operation,
and only when their value is text, a number, or a boolean: `host`, `path`, `url_path`,
`vfs_path`, `from`, `to`, `method`, `tool`, `tool_name`, `name`,
`secret`, `model`, `key`, `prefix`, `body_size`, `timeout_ms`,
`max_bytes`, `overwrite`, `decompress`, `prompt_count`, `op`, `length`,
`recursive`, `remote`, `remoteName`, `branch`, and `transport`.

This refused `acme.com/credits.apply` call came from a package run with
`check("acme.com/credits.apply", { customerId: "cus_northwind", amount: 42,
customerClass: "business" })`:

```text
ts=2026-10-04T05:37:48.530Z level=info stream=audit target=submilli_server::audit msg=decision blueprint=test blueprint_hash=35d4367e3fb94fcad8d47c0af8c7f2a054a229a69f069928dfcca5f53d835ac5 caller=main capability=acme.com/credits.apply context.payload_json="{\"amount\":42,\"customerClass\":\"business\",\"customerId\":\"cus_northwind\"}" decision=deny event_id=0991217e-e863-4f0d-b4f5-cd85e61aea85 execution_id=3506cc41-9d9d-4c2e-b76a-2f11d6fb433e principal=unauthenticated reason="policy denied the capability" rule=default schema=submilli.audit/1 source=policy type=decision
```

Under `allows: summary`, a run's allowed operations are written when the
run finishes, one record for each caller, capability, and rule, with the
number of operations and up to ten distinct contexts:

```text
ts=2026-10-04T05:44:32.155Z level=info stream=audit target=submilli_server::audit msg=decision blueprint=test blueprint_hash=587d2f32ff8c32f8808d17e4ff4a41b3250b70628c067500c527b726a193e6a9 caller=main capability=acme.com/credits.apply contexts.0.payload_json="{\"amount\":42,\"customerClass\":\"business\",\"customerId\":\"cus_northwind\"}" count=1 decision=allow event_id=555cbab7-33e1-4c34-9d9b-bcbe23b5b5a6 execution_id=7adecd7f-273e-41a7-ad4d-8e606315b102 principal=unauthenticated rule=0 schema=submilli.audit/1 source=policy type=decision
```

| Field | Value |
| --- | --- |
| `count` | How many operations the record stands for |
| `contexts.*` | Distinct contexts numbered from `0`, each including `payload_json` |
| `summary_overflow` | `true` when the summary count, context count, or encoded size limit is reached. Further distinct operations are written one by one |

## `execution`

Two for each run: `event=started` when it begins and `event=finished`
when it ends. A request that is refused before the program runs, such
as one naming a blueprint the server doesn't hold, still gets both.

```text
ts=2026-10-03T20:02:39.695Z level=info stream=audit target=submilli_server::audit msg=execution blueprint=support blueprint_hash=5a2a1e61c440e6b36c43c60b2eff7ecad0ed7989bf5dee30d486678206a480d0 entry_point=http event=finished event_id=9b94bd43-647f-4b60-a740-0dee88e0d048 execution_id=0dfa846a-4483-4b8a-b271-45339daecbaf fuel=2264 host_fuel=2211 memory_peak=65536 model_tokens=0 outcome=ok principal=SUBMILLI_SERVER_TOKEN schema=submilli.audit/1 source_hash=9869da7b5d729a383bbfb33edf1a8ab68ab23fcc3fd3939a675b91bd906179cc source_size=265 type=execution vars.0.name=customerId vars.0.value=cus_northwind wall_ms=38 wasm_fuel=53
```

| Field | Value |
| --- | --- |
| `execution_id` | The run. The execute response carries the same ID |
| `entry_point` | `http`, a one-off `POST /v1/execute`; `session`, a session's execute endpoint; `mcp`, the MCP execute tool |
| `session_id` | The session, when there is one |
| `principal` | The name of the API token |
| `blueprint`, `blueprint_hash` | The blueprint, and a SHA-256 of the version in force |
| `vars.*` | The variables bound for the run, each a `name` and a `value`. A value is `[redacted]` when the name contains `secret`, `token`, `password`, `credential`, `authorization`, or `api_key` |
| `source_hash`, `source_size` | A SHA-256 of the program's source and its length in bytes, never the source |

`event=finished` adds:

| Field | Value |
| --- | --- |
| `outcome` | `ok`, `error`, `fuel_exhausted`, `timeout`, `memory_exhausted`, `stack_exhausted`, or `cancelled` |
| `error_class` | The error's `kind`, when the run failed. A permission denial that no `catch` handled is `permission_denied` |
| `fuel`, `wasm_fuel`, `host_fuel` | Fuel consumed, as in [Set limits](/docs/server/set-limits#measure-a-program) |
| `memory_peak` | The most memory the run held, in bytes |
| `model_tokens` | Model tokens the run spent |
| `wall_ms` | Elapsed milliseconds |

A request repeated with the same `Idempotency-Key` returns the first
run's result and `execution_id`, and writes no new records.

## `session`

```text
ts=2026-10-03T20:02:39.674Z level=info stream=audit target=submilli_server::audit msg=session blueprint=support event=created event_id=1b32fa8e-1dfe-43de-ae7d-c2483f5f3972 file_area_mode=per_session principal=SUBMILLI_SERVER_TOKEN schema=submilli.audit/1 session_id=3b0e881b-753f-491c-8259-257016f5c448 type=session vars.0.name=customerId vars.0.value=cus_northwind
```

Every `session` record has `session_id` and `principal`.

| `event` | When | Other fields |
| --- | --- | --- |
| `created` | A session was opened | `blueprint`, `vars.*`, `file_area_mode` |
| `rebound` | Its variables or harness secrets were bound again | `vars.*`, `old_vars.*` |
| `deleted` | It was closed | `reason=disconnect` |
| `evicted` | Its blueprint was removed | `reason=blueprint_deleted`, `blueprint` |
| `expired` | It was idle past its blueprint's `idle_timeout` | `reason=idle_timeout` |
| `found` | It was reloaded when the server started | `blueprint` |
| `lost` | At startup, its files were missing, so it was dropped | `reason=workspace_missing` |

Variables are redacted as in `execution`.

## `admin`

One for each request that changes the server, written when the request
completes.

```text
ts=2026-10-03T20:11:15.828Z level=info stream=audit target=submilli_server::audit msg=admin event=secret_put event_id=cb6549c4-c63e-4056-9ab0-f68221120177 key=billing_api_key outcome=ok principal=admin route=/v1/secrets schema=submilli.audit/1 status=200 type=admin
```

Every `admin` record has:

| Field | Value |
| --- | --- |
| `principal` | The name of the API token |
| `route` | The request's path |
| `status` | The HTTP status of the response |
| `outcome` | `ok` for a 2xx status, `error` for any other, `cancelled` when the request was abandoned |

| `event` | Request | Other fields |
| --- | --- | --- |
| `blueprint_created` | `POST /v1/blueprints`, or `PUT /v1/blueprints/{name}` for a name the server didn't hold | `name`, `new_hash`, `old_hash` |
| `blueprint_replaced` | `PUT /v1/blueprints/{name}` for a name it held | `name`, `new_hash`, `old_hash` |
| `blueprint_deleted` | `DELETE /v1/blueprints/{name}` | `name`, `old_hash` |
| `secret_put` | `POST /v1/secrets` | `key`, never the value |
| `secret_deleted` | `DELETE /v1/secrets/{key}` | `key` |
| `package_installed` | `POST /v1/packages/install` | `packages.*`, each a `name`, `version`, and `digest` |
| `package_removed` | `DELETE /v1/packages/{name}` | `name`, `version`, `digest` |
| `oauth_refresh_token_set` | `POST /v1/mcp/{blueprint}/{server}/refresh-token` | `blueprint`, `server` |
| `oauth_refresh_token_deleted` | `DELETE /v1/mcp/{blueprint}/{server}/refresh-token` | `blueprint`, `server` |
| `oauth_code_exchanged` | `POST /v1/mcp/{blueprint}/{server}/oauth/exchange` | `blueprint`, `server` |
| `shutdown_requested` | `POST /v1/shutdown` | |

`new_hash` and `old_hash` are SHA-256s of the blueprint. `old_hash` is
`null` when there was none.

## `auth`

One for each request refused for its token, with `event=refused`. The
token itself is never recorded.

```text
ts=2026-10-03T20:11:15.888Z level=info stream=audit target=submilli_server::audit msg=auth event=refused event_id=9de64a66-aa15-47a0-8327-82e557fec87e reason=admin_required remote_address=127.0.0.1:55111 route=/v1/status schema=submilli.audit/1 type=auth
```

| Field | Value |
| --- | --- |
| `reason` | `missing_token`, no token was sent; `unknown_token`, the token matches none the server holds; `admin_required`, a `user` token on an admin endpoint |
| `route` | The request's path |
| `remote_address` | The address the request came from |

## `server`

```text
ts=2026-10-03T20:02:26.100Z level=info stream=audit target=submilli_server::audit msg=server allow_unauthenticated=false event=started event_id=47cc5240-5811-4399-b4ae-5515a897e11c schema=submilli.audit/1 settings_hash=72cdde19df1b28e43588b3dcb1da965498144093975dcdaf5d8d5a940c280150 type=server version=0.1.6
```

| `event` | When | Other fields |
| --- | --- | --- |
| `started` | The server began serving | `version`; `settings_hash`, a SHA-256 of its effective settings, without credentials, so a changed setting changes it; `allow_unauthenticated`; `egress_grants`, the outbound grants added by environment variables, when there are any |
| `stopped` | The server stopped | |

## Sensitive data

Secret-management and authentication records omit secret values and API
tokens. Execution records omit program source and model output. Decision
records store the complete context supplied to the permission check in
`payload_json`. If a caller puts a body, prompt, token, or file contents
in that context, the audit destination holds it.
