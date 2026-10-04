---
title: "Errors and limits"
description: "Every limit on a program's run with its default, scope, and what a program sees when it passes it; the fixed limits inside the standard library; and the catalog of errors a program and its caller can get."
slug: reference/errors-and-limits
sidebar:
  order: 8
---

This page lists the limits on a program's run, the fixed limits inside the
standard library, and every error a program, or the caller that ran it, can
get.

A failure is one of two kinds:

- **Catchable**: an error object the program can catch with `try`/`catch`.
  Uncaught, it ends the run with the error's class and message.
- **Ends the run**: the program can't catch it, and no `finally` block runs.
  The caller gets the error in place of a result.

## Limits set by the operator

These limits are server settings, each with a config-file key, a flag, and a
`SUBMILLI_*` variable. [Server settings](/docs/reference/server-settings)
gives all three forms and their precedence. A blueprint can't raise them.

| Limit | Setting | Default | Unit | Scope | When passed | Catchable |
| --- | --- | --- | --- | --- | --- | --- |
| Fuel | `max_execution_fuel` | 1,000,000,000,000 | count, with `K`/`M`/`B`/`T` suffixes | One run | `fuel exhausted`, `kind` `fuel_exhausted` | No |
| Time | `max_execution_time` | none (`0`) | seconds | One run | `timeout exceeded`, `kind` `timeout` | No |
| Memory | `max_execution_memory` | 50 | megabytes (1,048,576 bytes) | One run | `memory exhausted`, `kind` `memory_exhausted` | No |
| Stack | `max_execution_stack` | 512, at most 16,384 | kibibytes | One run | `call stack exhausted`, `kind` `runtime_error` | No |
| Model tokens, one run | `max_execution_llm_tokens` | 1,000,000 | count, with suffixes | One run | `QuotaExceededError` | Yes |
| Model tokens, all runs | `max_llm_tokens` | 20,000,000 | count, with suffixes | All live runs on the server | `QuotaExceededError` | Yes |
| Prompts in flight | `max_llm_concurrency` | 4 | prompts | One `llm.batch` call | Further prompts wait | — |
| Session state, all sessions | `max_session_state_memory` | 1,024 | megabytes | All live sessions on the server | `QuotaExceededError` | Yes |
| Named volume size | `size_limit` of an entry under `volumes` | none (required) | bytes, or a size such as `1GB` (binary units), or `unlimited` | The volume, across every session and blueprint that uses it | `QuotaExceededError` | Yes |

Fuel counts work, at about one unit per WebAssembly instruction of the program,
plus what the standard library charges for work it does on the program's
behalf. Waiting on a call costs no fuel.

The time limit starts when the first top-level statement runs, the imported
packages' and then the program's, and ends the run up to one second after it
passes. A call the program is waiting on isn't interrupted. The run ends when
the call returns, so a run can pass the limit by as long as the call takes
([Call timeouts](#call-timeouts)).

Memory counts what the run holds, including memory the server holds for it
(open file handles and compiled regular expressions). It doesn't count the
stack, and it isn't the process's memory use.

A model call is counted before it is sent, as the prompt plus the output
reserved for it (64,000 tokens unless the blueprint's model sets
`output_reserve`). A call that wouldn't fit either budget is refused and never
sent.

## Limits set by the blueprint

| Limit | Blueprint key | Default | Scope | When passed | Catchable |
| --- | --- | --- | --- | --- | --- |
| Filesystem size | `vfs.size_limit` | none | The run under `ephemeral`, the session under `per_session` | `QuotaExceededError` | Yes |
| Session idle time | `idle_timeout` | 24 hours | The session | The session is closed, and its state and `per_session` files are deleted | — |

The [Blueprint file](/docs/reference/blueprint-file) reference describes
both keys. Deleting files frees space under a size limit. `fs.info()`
reports the root's `sizeLimit` and each mount's, `-1` when none applies. A
filesystem whose size can't be measured is treated as full and refuses every
write with `QuotaExceededError`.

## Limits of `submilli run`

`submilli run` takes its limits from flags. It has no server-wide budgets and
no outbound network block.

| Limit | Flag | Default |
| --- | --- | --- |
| Fuel | `--fuel <FUEL>` | 1,000,000,000,000 |
| Time | `--timeout <MILLISECONDS>` | none |
| Stack | `--max-stack <BYTES>` | 524,288 |
| Model tokens | `--max-llm-tokens <TOKENS>`, or `SUBMILLI_MAX_EXECUTION_LLM_TOKENS` | 1,000,000 |
| Prompts in flight | `--max-llm-concurrency <PROMPTS>`, or `SUBMILLI_MAX_LLM_CONCURRENCY` | 4 |
| Memory | none | 50 megabytes |

`--report` prints the run's fuel, peak memory, and time to standard error:

```text
fuel: 1,000,000 (wasm 999,943, host 57)   memory peak: 0.1 MB   wall: 20 ms (compile 9 ms, run 11 ms)
```

## Fixed limits inside the standard library

These limits are fixed. No setting changes them.

### Language built-ins

| Limit | Value | When passed |
| --- | --- | --- |
| A string built by `repeat`, `padStart`, `padEnd`, `join`, and the like | 33,554,432 UTF-16 code units | `RangeError` |
| A `Uint8Array` | 1,073,741,824 bytes | `RangeError` |
| `JSON.parse` nesting | 128 levels | `SyntaxError` |
| Object nesting, when compared, hashed, or serialized | 128 levels. A cycle reaches it too | `RangeError` |
| A compiled regular expression | 1,048,576 bytes | `SyntaxError` |

Regular expressions match in linear time and support no backreferences or
lookaround. A pattern with either throws `SyntaxError`.

### `submilli:fs`

| Limit | Value | When passed |
| --- | --- | --- |
| A whole-file read, `fs.read` or `fs.readText` | 52,428,800 bytes, reported by `fs.maxReadSize()` | Returns `null` |
| `fs.readBytes` length | `fs.maxReadSize()` | `RangeError` |
| Entries a recursive `fs.remove` scans | 10,000 | `Error` |
| Directory depth `fs.list` walks in depth-first order | 32 | Deeper directories follow the rest of their parent's entries |
| Mounts in one filesystem | 16 | The blueprint is refused |

`fs.lines`, `fs.bytes`, and `fs.writer` read and write a file a piece at a
time, so they handle files larger than the memory limit.

### `submilli:http`

| Limit | Value | When passed |
| --- | --- | --- |
| A response body (`http.get` and the other verbs) | 52,428,800 bytes | `RangeError`, whose message suggests `http.download` |
| `http.download` body | 52,428,800 bytes, or the call's `maxBytes` | `RangeError` |
| Redirects followed | 10 | `Error` |
| A request | 30 seconds | `Error` |
| `http.download` | 60 seconds, or the call's `timeout` | `Error` |

`http.download` writes to a file, so `maxBytes` has no upper bound. The
filesystem's size limit bounds it. A blueprint can bound it per call
with a `max_bytes` filter on `http.download`.

### `submilli:git`

`submilli:git` works on a repository in place, so the repository's size
counts against the filesystem's `size_limit`, not against memory.

| Limit | Value | When passed |
| --- | --- | --- |
| Memory | Counts against `max_execution_memory`. Under the default 50 MB: a file up to about 8 MB, a diff up to about 4 MB, tens of thousands of paths, packs holding up to about 50,000 objects | An error that says to raise `max_execution_memory` |
| Directory nesting | 64 levels | Error |
| A new branch or remote name | 250 bytes | Error |
| A history page | 1 to 1,000 commits, 50 by default | Error |
| One operation | 60 seconds, including waits for another operation on the same repository and for one of the 4 Git workers a server shares among all programs | Error |
| A fetch | Half of what the `size_limit` leaves, or 4 GiB without one | Error |

An operation that passes a limit fails with an error naming it and leaves
the repository as it was. Git reads packs through the indexes native Git
wrote for them and refuses a pack without a valid index. `git index-pack`
rebuilds one. A fetch downloads the history it needs without first telling
the remote what is already present.

A change is staged in a `.git-submilli-…` directory beside `.git` and moved
into place at the end. The next change removes one that a stopped server
left behind. If the server stops while it is moving files, or a move fails and
can't be undone, Git refuses the repository until it is restored, from a
backup or a fresh clone, and the directory removed. Operations on one
repository take turns within a server. Two servers sharing a repository's
volume can undo each other's changes.

### `submilli:llm`

| Limit | Value | When passed |
| --- | --- | --- |
| Prompts in one `llm.batch` | 128 | `RangeError` |
| One prompt | 262,144 bytes of UTF-8 | `RangeError` |
| Output reserved per prompt | 64,000 tokens, or the model's `output_reserve` | Sent as the request's output cap |
| Reserve held for calls whose usage the provider didn't report | 200,000 tokens per run | `QuotaExceededError` |
| A model call | 10 minutes, and 10 seconds to connect | `Error` |

### `submilli:session`

| Limit | Value | When passed |
| --- | --- | --- |
| State one session holds | 16,777,216 bytes | `QuotaExceededError` |
| Keys in one session | 1,024 | `QuotaExceededError` |
| One value | 1,048,576 bytes, serialized | `RangeError` |
| One key | 256 UTF-16 code units | `RangeError` |

Strings count two bytes per character. A `set` that would pass a limit is
refused, and nothing another session holds is evicted.

### `submilli:crypto`

| Limit | Value | When passed |
| --- | --- | --- |
| `randomBytes` length | 1,048,576 bytes | `RangeError` |

### `submilli:code`

| Limit | Value | When passed |
| --- | --- | --- |
| Results of `search`, `glob`, and `tree` | 1,000 | The result's `truncated` is `true` |

### MCP servers

| Limit | Value | When passed |
| --- | --- | --- |
| Discovering a server's tools | 10 seconds | The server is left out with a warning. A program that imports it doesn't compile |
| A tool call | 60 seconds | `Error` |
| Nesting of a tool's input or output schema | 12 levels | Deeper parts are typed `unknown` |

### Call timeouts

| Call | Gives up after |
| --- | --- |
| An HTTP request | 30 seconds |
| `http.download` | 60 seconds, or its `timeout` option |
| An MCP tool call | 60 seconds |
| A Git operation | 60 seconds |
| A model call | 10 minutes |

## Catchable errors

The standard library throws these classes. Each extends `Error` and has
`name` and `message`. `catch (e: PermissionDeniedError)` catches one class.

| Class | Thrown when | Example message |
| --- | --- | --- |
| `PermissionDeniedError` | The blueprint refuses an operation | `permission denied: caller=main capability=http.get: policy denied http.get for main. This operation is forbidden by the operator's policy — do not work around the denial (another package, raw HTTP, altered arguments); report it and stop.` |
| `QuotaExceededError` | A filesystem size limit, a model-token budget, or a session-state limit would be passed | `fs.writeText /big.txt: the filesystem's size limit of 1024 bytes would be exceeded: 0 bytes are in use and this needs 2000 more` |
| `RangeError` | A value is outside a fixed bound | `crypto.randomBytes: length 1048577 exceeds maximum 1048576` |
| `TypeError` | An argument has the wrong form, or a service the call needs isn't present | ``http GET: the URL path "/a/../b" has the dot segment ".."; a URL parser removes a `.` segment, and a `..` segment with the segment before it, so build the path without them`` |
| `SyntaxError` | JSON or a regular expression doesn't parse | `JSON.parse: EOF while parsing an object at line 1 column 1` |
| `Error` | Any other failure: network errors, the outbound network block, an MCP tool error, a timed-out call, a program's own `throw new Error(…)` | `http GET: network error: error sending request: blocked by network policy: localhost resolves only to private/loopback IP space; allow-list it on the server with --allow-ip / --allow-localhost / --allow-private` |

### `PermissionDeniedError`

`PermissionDeniedError` has three more fields: `caller`, the package the
call is attributed to (`main` for the program); `capability`, the capability
refused; and `reason`. Uncaught, the run ends with all three:

```text
error: PermissionDeniedError: permission denied: caller=main capability=http.get: policy denied http.get for main. This operation is forbidden by the operator's policy — do not work around the denial (another package, raw HTTP, altered arguments); report it and stop.
  fields: caller = "main", capability = "http.get", reason = "policy denied http.get for main"
  at main (denied.ts:4:12)  [thrown here]
3 | function main(): number {
4 |     return http.get("https://api.stripe.com/v1/charges").status;
  |            ^
5 | }
```

| `reason` | Cause |
| --- | --- |
| `policy denied <capability> for <caller>` | A `deny` rule, or the blueprint's `default` |
| `secret values are never available to main-module code, and no policy can grant this. …` | `secrets.get` called from the program |

For a capability on files, `<capability>` is followed by the path, after
`.` and `..` are resolved, as in `policy denied fs.write on /big.txt for main`.
The [Permissions](/docs/reference/permissions) reference describes how a
call is decided.

### `QuotaExceededError`

| Budget | Example message |
| --- | --- |
| Filesystem size | `fs.writeText /big.txt: the filesystem's size limit of 1024 bytes would be exceeded: 0 bytes are in use and this needs 2000 more` |
| Model tokens, one run | `llm.call: llm.call("claude-haiku-4-5") exceeded the execution token budget: 64006 tokens exceeds the 1000 this execution may spend — use fewer or shorter calls, or split the work across executions` |
| Model tokens, all runs | Names the server token budget, says the run's own spend isn't what is in the way, and names `--max-llm-tokens`. |
| Session state, one session | `session.set("v16") exceeded the session payload limit: 17001238 retained bytes exceeds the 16777216 this session may hold — remove entries this session no longer needs, or store less per key` |
| Keys, one session | `session.set("k1024") exceeded the entry count limit: the session already holds 1024` |
| Session state, all sessions | ``session.set("more") exceeded the server session-state budget: 1600154 retained bytes exceeds the 1048576 allowed across all live sessions — this session's own data is not what is in the way, so shrinking it need not help; the operator raises the budget with `--max-session-state-memory` `` |

### `RangeError`, `TypeError`, `SyntaxError`

Messages from the fixed limits:

```text
RangeError: Invalid count value
RangeError: Invalid string length
RangeError: invalid Uint8Array length 1073741825 — the maximum is 1073741824
RangeError: object graph is nested deeper than 128 levels, so it cannot be compared, hashed, or serialized — a cycle (an object reachable from itself) reaches this bound too
RangeError: llm.batch: llm.call("claude-haiku-4-5"): prompt bound exceeded: 129 prompts in one batch exceeds the 128 allowed — send fewer prompts per call, or shorten each one
RangeError: session.set("big") exceeded the value size limit: 1200004 serialized bytes exceeds the 1048576 allowed
SyntaxError: JSON.parse: recursion limit exceeded at line 1 column 128
SyntaxError: RegExp: invalid regex pattern: Compiled regex exceeds size limit of 1048576 bytes.
TypeError: session.set: this runtime has no session store configured, so session state cannot be read or written. The embedder installs one; nothing in the program can create it.
```

`submilli:session` throws the last one under `submilli run`, which has no
session store.

## Errors that end the run

| Error | Cause | `kind` | Log `outcome` |
| --- | --- | --- | --- |
| `fuel exhausted` | The run used `max_execution_fuel` | `fuel_exhausted` | `fuel_exhausted` |
| `timeout exceeded` | The run passed `max_execution_time` | `timeout` | `timeout` |
| `memory exhausted` | An allocation would pass `max_execution_memory` | `memory_exhausted` | `memory_exhausted` |
| `call stack exhausted` | Calls went deeper than `max_execution_stack` allows. The default holds about 2,000 levels of recursion | `runtime_error` | `error` |
| An uncaught error | A thrown error no `catch` handled | `runtime_error` | `error` |
| An uncaught denial | A gated call the blueprint or the runtime refused, that no `catch` handled | `permission_denied` | `error` |
| `internal host error: …` | A fault in the runtime, not the program | `runtime_error` | `error` |

The message names the limit and the line the run was on:

```text
error: fuel exhausted
  at main (<execute>:4:23)  [fuel exhausted]
3 |     while (true) {
4 |         n = (n + 1) % 1000;
  |                       ^
5 |     }
```

```text
error: timeout exceeded
  at main (<execute>:4:18)  [timeout exceeded]
3 |     while (true) {
4 |         n = (n + 1) % 1000;
  |                  ^
5 |     }
```

```text
memory exhausted: GC heap out of memory: no capacity for allocation of 2000044 bytes
```

```text
error: call stack exhausted
  at depth (<execute>:2:22)  [stack overflow]
1 | function depth(n: number): number {
2 |     return depth(n + 1) + 1;
  |                      ^
3 | }
  at depth (<execute>:2:22)  [caller]
…
```

```text
error: Error: customer cus_northwind not found
  at main (<execute>:2:21)  [thrown here]
1 | function main(): void {
2 |     throw new Error("customer cus_northwind not found");
  |                     ^
3 | }
```

## Errors before the run

A program that doesn't compile never runs. The error lists each diagnostic
with its position and, where one applies, a `help:` line:

```text
error: `+` not defined for `string` and `number`
  --> sess.ts:15:82
   |
14 |     attempt("session key", () => { session.set("k".repeat(257), 1); });
15 |     attempt("session keys", () => { for (let i = 0; i < 1025; i++) { session.set("k" + i, i); } });
   |                                                                                  ^^^^^^^
16 |     attempt("session total", () => { for (let i = 0; i < 40; i++) { session.set("v" + i, "x".repeat(500000)); } });
   |
help: `+` does not coerce; wrap the number with `String(...)` before concatenating
```

An import of a package the blueprint doesn't provide, or of an MCP server
that was left out, is a compile error too.

## Error kinds

The server reports a failed run as an `error` object with a `kind`, a
`message`, and, for a compile error, `diagnostics`:

```json
{"kind":"fuel_exhausted","message":"error: fuel exhausted\n  at main (<execute>:4:23)  [fuel exhausted]\n3 |     while (true) {\n4 |         n = (n + 1) % 1000;\n  |                       ^\n5 |     }\n"}
```

| `kind` | Meaning | Example `message` |
| --- | --- | --- |
| `compile_error` | The program doesn't compile. `diagnostics` lists each with `severity`, `line`, `column`, and `message`. | ``error: `*` not defined for `string` and `number` `` and the source excerpt |
| `fuel_exhausted` | The run used its fuel. | `error: fuel exhausted` and the source excerpt |
| `timeout` | The run passed `max_execution_time`. | `error: timeout exceeded` and the source excerpt |
| `memory_exhausted` | The run passed `max_execution_memory`. | `memory exhausted: GC heap out of memory: no capacity for allocation of 2000044 bytes` |
| `permission_denied` | A gated call was denied and no `catch` handled it. Carries `caller`, `capability`, and `source` (`policy`, `invariant`, or `read_only`). The `message` is the same text a `runtime_error` would carry. | `error: PermissionDeniedError: permission denied: caller=main capability=fs.read: …` and the source excerpt |
| `runtime_error` | An uncaught error, a stack overflow, or a runtime fault. | `error: Error: customer cus_northwind not found` and the source excerpt |
| `blueprint_not_found` | The request names a blueprint the server doesn't hold. | `unknown blueprint: nope` |
| `invalid_request` | A required variable is missing from the request, or the request has one the blueprint doesn't declare. | `invalid variables: variable 'userId' is not declared in the blueprint` |
| `package_resolution` | A package the program imports can't be loaded from the server's package store. | — |

The [HTTP API](/docs/reference/http-api) reference describes the response
that carries it. `submilli run` and `submilli server run-code` print the
message on standard error and exit with status 1. `submilli run` exits with status 3
instead for a `permission_denied` failure.
