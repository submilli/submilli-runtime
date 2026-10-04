---
title: "Permissions"
description: "How a call is decided, callers, actions, the refusals no rule changes, the capabilities and their fields, the errors when a blueprint is read, and the denials at run time."
slug: reference/permissions
sidebar:
  order: 14
---

This page describes the `permissions` and `default` blocks of a blueprint:
how a gated operation is decided, the capabilities and their fields, and
the errors and denials a rule produces. A rule's `filter` is written in the
[filter language](/docs/reference/filter-language).

## How a call is decided

```yaml title="blueprint.yaml (fragment)"
default: deny

permissions:
  main:
  - capability: fs.write
    filter: path glob "/notes/*"
    action: allow
  - capability: fs.read
    action: allow

  '@acme/billing':
  - capability: http.get
    filter: host == "billing.internal.example.com"
    action: allow
```

When code performs a gated operation, the runtime names the
[caller](#callers), applies the [refusals no rule can
change](#refusals-no-rule-can-change), then reads the caller's list from the
top. The first rule whose `capability` equals the operation's and whose
`filter`, if any, is true decides. If none matches, or the caller has no
list, `default` decides.

Names are matched exactly, with no wildcards: a rule for `fs.write` doesn't
match `fs.mkdir`. A rule without a filter matches every use of its
capability.

| Field | Type | Required | Meaning |
| --- | --- | --- | --- |
| `capability` | string | Yes | The operation's name, such as `fs.write`, `acme.com/credits.apply`, or `mcp.linear` |
| `filter` | string | No | A condition on the operation's fields, in the [filter language](/docs/reference/filter-language) |
| `action` | `allow` or `deny` | Yes | What happens when the rule matches |

## Actions and the default

| Action | Effect |
| --- | --- |
| `allow` | The operation proceeds |
| `deny` | The operation throws `PermissionDeniedError` |

`default` takes the same values and is `deny` when the blueprint doesn't set
it. Under `default: allow`, every operation no rule matches is permitted,
including ones a package added later provides.

## Callers

Each list under `permissions` belongs to a **caller**:

| Caller | Its rules apply to |
| --- | --- |
| `main` | The program |
| A package's name, such as `'@acme/billing'` | That package's own code, whether listed under `packages:` or a dependency of one |

The caller is the code that is running, not anything the program passes. A
standard-library operation is attributed to the code that calls it, so a
package's HTTP request is judged under the package's list. An operation a
package checks with `check` is attributed to the code that called the
package: a program calling `listCharges` is judged under `main` for
`acme.com/charges.list`, and the request the package then sends under
`'@acme/billing'`. A package with no list may do only what `default` allows.

## Refusals no rule can change

These are refused before the rules are read, whatever `default` says:

- `secrets.get` from `main`. A secret's value is available only to packages,
  and `submilli blueprint capability add` refuses to write the rule.
- A write to a volume the blueprint mounts read-only.

## Capabilities

| Source | Names | Fields |
| --- | --- | --- |
| The standard library | `fs.*`, `git.*`, `http.*`, `llm.call`, `secrets.get`, `session.*` | In the tables below |
| A package | Chosen by its author, such as `acme.com/credits.apply` | Declared by its `@capability` tags ([Package manifest](/docs/reference/package-manifest)) |
| An MCP server the blueprint declares | `mcp.<server>`, such as `mcp.linear` | `tool`, `transport` ([MCP servers](/docs/reference/mcp-servers)) |

`submilli blueprint capability list` prints every capability a blueprint can
use, with its fields and the rules written for it.

A field is a `string`, a `number`, or a `boolean`; a package field may also
be an object, whose members a filter names with a dot.

### Fields only some calls report

A call that doesn't supply a field leaves it out, and a condition on a
missing field is false.

| Capability | Field | Reported by |
| --- | --- | --- |
| `fs.read` | `length` | `readBytes` |
| `fs.read` | `chunkSize` | `bytes` |
| `fs.read`, `fs.stat` | `recursive` | The `submilli:code` workspace tools, always `true` |
| `fs.write` | `length` | `write`, `writeText`, `append`, `appendText`, and code edits |
| `fs.write` | `max_bytes` | `http.download`, and packages that stream a download to a file |
| `fs.write` | `diff` | Code edits |
| `http.*` | `body_size`, `timeout_ms` | Every request, except a redirect turned into a `GET` |

### Normalized fields

| Field | Capabilities | A rule sees |
| --- | --- | --- |
| `path`, `from`, `to`, `vfs_path` | `fs.*`, `git.*`, `http.download` | The absolute path, resolved against the working directory, with `.` and `..` resolved: `/notes/../secrets.txt` is `/secrets.txt` |
| `host` | `http.*` | The host without port or trailing dot: `https://api.example.com./` is `api.example.com` |
| `remote` | `git.clone`, `git.fetch` | The full HTTPS URL, host in lowercase, default port removed: `https://GitHub.com:443/acme/./project.git` is `https://github.com/acme/project.git` |

### Checks some operations make

- **Git:** `git.clone` and `git.fetch` are checked with the branch the call
  names, then once for each branch fetched; a call that names none is
  checked with `branch` set to `""`, so a rule testing `branch` matches only
  calls that name it. `git.clone` needs no `git.init` or `fs.*` rule.
  Reading history, staging, and branching aren't gated.
- **HTTP redirects:** each redirect is checked before it is sent, with the
  new URL's `host` and `path`, under the same caller. A redirect that turns
  the request into a `GET` is checked as `http.get`.
- **Models:** `prompt_count` is `1` for `call` and the number of prompts for
  `batch`; the prompt text is never in the context. `models()` lists only
  the models a rule allows.
- **Session state:** `session.list` is checked with `prefix`, and each key it
  would return as `session.read`; a refused key is left out.
- **MCP tools:** the tool is a field, not part of the name: write
  `capability: mcp.linear` with `filter: tool == "save_issue"`.

<!-- generated:capabilities -->

### `submilli:fs`

| Capability | Fields | Operation | Example filter |
| --- | --- | --- | --- |
| `fs.read` | `path: string`, `length: number`, `chunkSize: number`, `recursive: boolean` | Read files and code workspace content (including search and ignore rules) | `path glob "*.csv"` |
| `fs.write` | `path: string`, `length: number`, `max_bytes: number`, `diff: string` | Create, write, append, or apply code edits to files | `path glob "/out/*"` |
| `fs.stat` | `path: string`, `recursive: boolean` | Inspect metadata (including code workspace discovery) | `path glob "/data/*"` |
| `fs.list` | `path: string`, `recursive: boolean` | List directory entries (including code search, glob and tree) | `path glob "/data/*"` |
| `fs.mkdir` | `path: string`, `recursive: boolean` | Create directories | `path glob "/tmp/*"` |
| `fs.remove` | `path: string`, `recursive: boolean` | Delete files or directories | `path glob "/tmp/*"` |
| `fs.move` | `from: string`, `to: string` | Move or rename a path | `to glob "/archive/*"` |
| `fs.copy` | `from: string`, `to: string`, `recursive: boolean` | Copy a path | `to glob "/backup/*"` |

### `submilli:git`

| Capability | Fields | Operation | Example filter |
| --- | --- | --- | --- |
| `git.init` | `path: string` | Create a local repository and its VFS directory | `path == "/repo"` |
| `git.clone` | `path: string`, `remoteName: string`, `remote: string`, `branch: string` | Clone an HTTPS repository into a VFS directory | `path == "/repo" and remote == "https://github.com/acme/project.git"` |
| `git.fetch` | `path: string`, `remoteName: string`, `remote: string`, `branch: string` | Fetch or pull HTTPS remote branches into an existing repository | `path == "/repo" and remote == "https://github.com/acme/project.git"` |
| `git.commit` | `path: string`, `branch: string` | Commit staged changes with blueprint identity | `path == "/repo" and branch == "main"` |

### `submilli:http`

| Capability | Fields | Operation | Example filter |
| --- | --- | --- | --- |
| `http.get` | `host: string`, `path: string`, `body_size: number`, `timeout_ms: number` | HTTP GET | `host == "api.example.com"` |
| `http.post` | `host: string`, `path: string`, `body_size: number`, `timeout_ms: number` | HTTP POST | `host == "api.example.com"` |
| `http.put` | `host: string`, `path: string`, `body_size: number`, `timeout_ms: number` | HTTP PUT | `host == "api.example.com"` |
| `http.patch` | `host: string`, `path: string`, `body_size: number`, `timeout_ms: number` | HTTP PATCH | `host == "api.example.com"` |
| `http.delete` | `host: string`, `path: string`, `body_size: number`, `timeout_ms: number` | HTTP DELETE | `host == "api.example.com"` |
| `http.head` | `host: string`, `path: string`, `body_size: number`, `timeout_ms: number` | HTTP HEAD | `host == "api.example.com"` |
| `http.options` | `host: string`, `path: string`, `body_size: number`, `timeout_ms: number` | HTTP OPTIONS | `host == "api.example.com"` |
| `http.download` | `host: string`, `url_path: string`, `vfs_path: string`, `max_bytes: number`, `overwrite: boolean`, `decompress: boolean` | Download a URL straight to the VFS | `host == "cdn.example.com" and overwrite == false` |
| `http.<method>` | `host: string`, `path: string`, `body_size: number`, `timeout_ms: number` | Any other HTTP method, through `http.request`: `http.trace` gates TRACE | `host == "api.example.com"` |

### `submilli:llm`

| Capability | Fields | Operation | Example filter |
| --- | --- | --- | --- |
| `llm.call` | `model: string`, `prompt_count: number` | Call a model (call, batch) and enumerate the models it may call (models). Narrowing `model` also narrows what `models()` reveals: every candidate is filtered through this same rule, so a listing never offers a model the caller would be denied at call time | `model glob "claude-*"` |

### `submilli:secrets`

| Capability | Fields | Operation | Example filter |
| --- | --- | --- | --- |
| `secrets.get` | `name: string` | Read a blueprint-declared secret value | `name == "STRIPE_API_KEY"` |

### `submilli:session`

| Capability | Fields | Operation | Example filter |
| --- | --- | --- | --- |
| `session.read` | `key: string` | Read session state (get, has), and decide which keys a list may reveal | `key glob "triage/*"` |
| `session.write` | `key: string` | Store or overwrite a session value (set) | `key glob "triage/*"` |
| `session.remove` | `key: string` | Delete a session key | `key glob "triage/*"` |
| `session.list` | `prefix: string` | Enumerate session keys under a prefix | `prefix == "triage/"` |

### `@mcp`

| Capability | Fields | Operation | Example filter |
| --- | --- | --- | --- |
| `mcp.<server>` | `tool: string`, `transport: string` | Call tools on a declared outbound MCP server (streamable_http) (declare a server with `blueprint add-mcp` to concretize) | `tool == "create_issue"` |

<!-- /generated:capabilities -->

## Errors when the blueprint is read

`submilli blueprint lint`, the server's registration, and
`submilli run --blueprint` stop on these:

| Mistake | Error |
| --- | --- |
| A filter that doesn't parse, or names an undeclared variable | See [Filter language](/docs/reference/filter-language#errors) |
| An action or `default` other than `allow` or `deny` | ``unknown variant `maybe`, expected one of …`` |
| A key in a rule other than `capability`, `filter`, `action` | ``unknown field `extra`, expected one of `capability`, `filter`, `action` `` |
| An MCP capability with the tool in its name | `permission rule 'mcp.linear/save_issue': use capability 'mcp.linear' with a filter such as 'tool == "name"' instead of '/tool'` |
| A rule for an undeclared MCP server or `llm` model | `permission rule 'mcp.x' references undeclared mcp server 'x'` |
| A package requires an operation its own list has no rule for (lint and registration) | ``package `@acme/billing` requires `http.get` with filter `…`, but `permissions.@acme/billing` has no matching rule``; `submilli blueprint lint --fix` adds it |
| A filter tests a field the capability doesn't report (lint and registration) | ``rule 1 for `fs.read` tests `owner`, which the operation doesn't report, …`` |

`submilli blueprint lint` also warns, without stopping, about:
`default: allow`; a rule for `secrets.get` under `main`; a rule that an
earlier unfiltered rule always decides first; a capability name nothing
provides, with a suggestion; an `http.<method>` name that only
`http.request` reaches; a package rule that differs from what the package
requires; and a package list for a package the blueprint doesn't use.
With `--deny-warnings`, or `SUBMILLI_DENY_WARNINGS=1`, any of these fails
the lint.

## Denials at run time

A refused operation throws `PermissionDeniedError`, which a program can
catch; `e.caller`, `e.capability`, and `e.reason` hold its fields.

```text
error: PermissionDeniedError: permission denied: caller=main capability=fs.write: policy denied fs.write on /secrets.txt for main. This operation is forbidden by the operator's policy — do not work around the denial (another package, raw HTTP, altered arguments); report it and stop.
  fields: caller = "main", capability = "fs.write", reason = "policy denied fs.write on /secrets.txt for main"
  at main (w.ts:4:43)  [thrown here]
```

The message is `permission denied: caller=<caller> capability=<capability>: <reason>.`
and a closing sentence addressed to the model. It doesn't name the rule
that refused.

| Reason | Cause |
| --- | --- |
| `policy denied <capability><target> for <caller>` | A `deny` rule, or the default |
| `secret values are never available to main-module code, …` | `secrets.get` from `main` |
| `<path> is in the volume mounted read-only at <mount>` | A write to a read-only volume |

`<target>` is ` on <path>` for the `fs.*`, `git.*`, and `http.download`
capabilities (` from <path> to <path>` for `fs.copy` and `fs.move`), and
empty otherwise. [Diagnose a denial](/docs/tutorials/diagnose-a-denial)
traces a denial back to the rule that decided it.
