---
title: "Blueprint file"
description: "Every top-level key and field of a blueprint file: types, defaults, allowed values, where variables and secrets may be referenced, and the errors that refuse a file at lint and at registration."
slug: reference/blueprint-file
sidebar:
  order: 1
---

A blueprint file is one YAML document. This page describes each of its
top-level keys and their fields, in the order the CLI writes them, and the
errors that refuse a file when it is linted or registered.

## Top-level keys

| Key | Type | Required | Default |
| --- | --- | --- | --- |
| [`kind`](#kind) | string | no | absent |
| [`name`](#name) | string | yes | |
| [`allow_insecure_http`](#allow_insecure_http) | boolean | no | `false` |
| [`idle_timeout`](#idle_timeout) | duration | no | `24h` |
| [`vfs`](#vfs) | mode name or map | no | `ephemeral` |
| [`secrets`](#secrets) | map | no | empty |
| [`variables`](#variables) | map | no | empty |
| [`packages`](#packages) | list | no | empty |
| [`auth_proxy`](#auth_proxy) | list | no | empty |
| [`git`](#git) | map | no | absent: `submilli:git` disabled |
| [`default`](#default) | `deny`, `allow` | no | `deny` |
| [`permissions`](#permissions) | map | no | empty |
| [`mcp`](#mcp) | map | no | empty |
| [`llm`](#llm) | map | no | empty |

Any other top-level key is a parse error, and so is an unknown field in any
block. A key repeated at the top level or within one block's fields is
refused. Repeated names in `secrets`, `variables`, `permissions`, `mcp`,
`llm.providers`, `llm.models`, and the map form of `packages` are also
refused. The same rule applies to MCP and auth-proxy header maps,
auth-proxy query maps, and paths in `vfs.mounts`.

```text
error: case.yaml: blueprint parse error: unknown field `permision`, expected one of `kind`, `name`, `allow_insecure_http`, `idle_timeout`, `vfs`, `secrets`, `variables`, `packages`, `auth_proxy`, `git`, `default`, `permissions`, `mcp`, `llm` at line 2 column 1
```

```text
error: case.yaml: blueprint parse error: duplicate field `name`
```

The CLI's editing commands (`submilli blueprint secret`, `variable`,
`auth-proxy`, `add-package`, `git`, `capability`, `add-mcp`) rewrite the
whole file: keys in the order of the table, map entries sorted by key,
durations in seconds (`'3600s'`), sizes in bytes, and comments removed.

### Example

Every key except `allow_insecure_http` and `packages`, as the CLI writes it:

```yaml title="blueprint.yaml"
kind: blueprint
name: support
idle_timeout: '3600s'
vfs:
  mode: per_session
  size_limit: 104857600
  cwd: /notes
  mounts:
    /notes:
      mode: named
      volume: notes
      subPath: users/${vars.customerId}
secrets:
  ANTHROPIC_API_KEY:
    store: anthropic_api_key
  LINEAR_API_KEY:
    harness:
      required: true
  STATUS_TOKEN:
    store: status_token
variables:
  customerId:
    required: true
  region:
    default: us
auth_proxy:
- host: status.acme.com
  auth:
    bearer: STATUS_TOKEN
git:
  identity:
    name: Support Agent (${vars.customerId})
    email: agent@acme.example
default: deny
permissions:
  main:
  - capability: http.get
    filter: host == "status.acme.com"
    action: allow
mcp:
  linear:
    url: https://mcp.linear.app/mcp
    headers:
      Authorization: Bearer ${secrets.LINEAR_API_KEY}
llm:
  providers:
    anthropic:
      type: anthropic
      api_key: ${secrets.ANTHROPIC_API_KEY}
  models:
    claude-haiku-4-5:
      provider: anthropic
      context_window: 200000
      output_reserve: 4000
      description: Cheap and fast.
```

### References to variables and secrets

`${vars.NAME}` is replaced by the session's value of a declared
[variable](#variables). `${secrets.NAME}` is replaced, outside the program,
by the value of a declared [secret](#secrets). Each may appear only in these
fields. Elsewhere the text is taken literally. A reference to a name the
file doesn't declare is an error.

| Reference | Fields |
| --- | --- |
| `${vars.NAME}` | `permissions` rule `filter`; `git.identity.name`, `git.identity.email`, `git.username`; `vfs.subPath`, `vfs.cwd`, and a mount's `subPath`, as a whole path component |
| `${secrets.NAME}` | `auth_proxy` `headers` and `query` values; `mcp` `headers` values; `mcp` `auth` `client_id`, `authorization_endpoint`, `token_endpoint`, `scopes`; `llm.providers` `api_key` and `base_url` |
| A secret name, bare | `auth_proxy` `auth.bearer` and `auth.basic.password` |

## kind

| | |
| --- | --- |
| Type | string |
| Required | no |
| Allowed values | `blueprint` |

```text
error: case.yaml: invalid blueprint kind: unknown kind 'policy': expected `blueprint` (or omit `kind`)
```

## name

| | |
| --- | --- |
| Type | string |
| Required | yes |
| Constraints | non-empty, with only ASCII letters, digits, `_` and `-` |

The name a server registers the blueprint under and an application names
when it opens a session. `submilli server blueprint apply` registers the
file under this name, replacing a blueprint registered with it.

```text
error: case.yaml: blueprint parse error: missing field `name`
```

```text
error: case.yaml: invalid blueprint name: blueprint name 'my agent' contains characters outside [A-Za-z0-9_-]
```

## allow_insecure_http

| | |
| --- | --- |
| Type | boolean |
| Default | `false` |

`true` permits `http://` requests through `submilli:http`, including
package calls and downloads. With `false`, only `https://` is permitted. An
`http://` request to a host that has an [`auth_proxy`](#auth_proxy) rule is
permitted only when that rule's own `allow_insecure_http` is `true` as well.
The check applies to every redirect too.

## idle_timeout

| | |
| --- | --- |
| Type | duration, a whole number followed by `s`, `m` or `h` |
| Default | `24h` |

How long a session may go unused before the server closes it. Closing a
session deletes its `submilli:session` state and its `per_session` files.
The CLI writes the value back in seconds, so `1h` becomes `'3600s'`.

```text
error: case.yaml: blueprint parse error: idle_timeout: duration '10' needs a unit (s, m, or h)
```

```text
error: case.yaml: blueprint parse error: idle_timeout: unknown duration unit 'd' in '1d' (use s, m, or h)
```

## vfs

The program's filesystem. Either a mode name, `vfs: per_session`, or a map
with `mode` and that mode's fields.

| `mode` | The program's `/` |
| --- | --- |
| `none` | No filesystem. Every `submilli:fs` call fails |
| `ephemeral` (default) | A directory created for the run and deleted when it returns |
| `per_session` | A directory that lasts as long as the session |
| `named` | A named volume declared in the server's config. It is kept across sessions and restarts, and shared with every blueprint that names it |

| Field | Type | Modes | Default | Constraints |
| --- | --- | --- | --- | --- |
| `mode` | `none`, `ephemeral`, `per_session`, `named` | all | `ephemeral` | |
| `size_limit` | size | `ephemeral`, `per_session` | no limit | A named volume's limit is set in the server's config |
| `volume` | string | `named` | | Required under `named`. The non-empty name of a volume declared on the server |
| `subPath` | string | `named` | the volume's root | A relative, normalized path inside the volume |
| `access` | `read_only`, `read_write` | `named` | the server's declaration | Can only narrow the server's declaration |
| `cwd` | string | `ephemeral`, `per_session`, `named` | `/` | An absolute, normalized guest path |
| `mounts` | map | `ephemeral`, `per_session`, `named` | none | See [Mounts](#mounts) |
| `grace_period` | duration | `per_session` | | Accepted and ignored |
| `path_limit` | number | `ephemeral`, `per_session` | | Accepted and ignored |

A field that doesn't belong to the block's mode is refused, with a message
naming the field and the mode:

```text
error: case.yaml: blueprint parse error: vfs.size_limit: invalid vfs config: 'size_limit' is not valid for vfs mode 'named'; a named volume's size limit is set by the operator where the server declares it at line 5 column 15
```

```text
error: case.yaml: blueprint parse error: vfs: invalid vfs config: `volume` is only valid under `mode: named`, and this vfs block has no `mode:` (it defaults to `ephemeral`); add `mode: named` to this vfs block, or move the volume under `mounts:` to keep an ephemeral root at line 3 column 3
```

```text
error: case.yaml: blueprint parse error: vfs: invalid vfs config: vfs mode 'named' requires a 'volume': the name of a volume the operator declared in the server config at line 3 column 3
```

Two retired forms are refused with the replacement: mode `persistent`, and
the `path` key.

```text
error: case.yaml: blueprint parse error: vfs: vfs mode `persistent` was removed: write `mode: named` and keep the `volume:` line (`vfs: {mode: named, volume: <name>}`). A named volume keeps its files across calls, sessions and restarts as before; leave `access:` out to keep the access the server declares for it at line 2 column 6
```

```text
error: case.yaml: blueprint parse error: vfs.path: the `path` key is retired: a blueprint can no longer name a host directory. Use `volume: <name>` under `mode: named` — the operator declares each volume by name in the server config at line 4 column 9
```

`submilli run` refuses a blueprint that names a volume, as the root or as a
mount, because volumes are declared only in a server's config.

### Sizes

A size is a byte count, `104857600`, or a whole number followed by a unit:
`B`, `KB`, `MB`, `GB`, `TB`, with `K`, `M`, `G`, `T` and `KiB`, `MiB`,
`GiB`, `TiB` accepted as the same units. Units are 1024-based and
case-insensitive, so `100MB` is 104,857,600 bytes.

```text
error: case.yaml: blueprint parse error: vfs: invalid vfs config: unknown size unit 'XB' in '10XB' (use B, KB, MB, GB, TB) at line 3 column 3
```

Under `per_session`, `size_limit` covers all of the session's files. A write
that would pass it throws `QuotaExceededError`.

### Mounts

`mounts` maps an absolute guest path to a named volume mounted there, below
the root.

```yaml title="blueprint.yaml (fragment)"
vfs:
  mode: per_session
  mounts:
    /memory:
      mode: named
      volume: project-memory
      access: read_write
    /handbook:
      mode: named
      volume: company-handbook
      access: read_only
```

| Field | Type | Required | Default | Constraints |
| --- | --- | --- | --- | --- |
| `mode` | `named` | yes | | The only mount mode |
| `volume` | string | yes | | The non-empty name of a volume declared on the server |
| `subPath` | string | no | the volume's root | A relative, normalized path inside the volume |
| `access` | `read_only`, `read_write` | no | the server's declaration | Can only narrow the server's declaration |

A mount path:

- is absolute and is not `/`
- has at most 4,096 bytes and 64 components
- contains only ASCII letters, digits, `.`, `_`, `-` and `/`
- has no empty, `.` or `..` component, no trailing `/`, and no component ending in `.`
- names no `.git` component
- is not inside another mount, does not contain one, and differs from every
  other mount by more than letter case

A blueprint has at most 16 mounts. The same volume may be mounted at several
paths. `mounts` under `mode: none` is refused.

```text
error: case.yaml: blueprint parse error: vfs.mounts./a/b: mount `/a/b` is inside mount `/a`; mounts may not nest — mount the volumes side by side, such as `/a` and `/b` at line 5 column 11
```

```text
error: case.yaml: blueprint parse error: vfs.mounts./a: a mount needs `mode: named`: mounts are named volumes the operator declares in the server config at line 4 column 9
```

```text
error: case.yaml: blueprint parse error: vfs.mounts./a.size_limit: unknown field `size_limit` in a mount, expected one of `mode`, `volume`, `access`, `subPath` at line 4 column 46
```

### subPath and cwd

`subPath` (on a named root or a mount) is relative to the volume's root. The
program sees the selected directory as the root of that volume. `cwd` is an
absolute guest path, and the directory relative paths resolve against for
`submilli:fs`, packages, and `http.download` destinations. `fs.cwd()`
returns it. In both, `${vars.NAME}` may stand for one whole component, and
must resolve to a non-empty name containing no `/`, `\` or NUL that is not
`.` or `..`. A path is at most 4,096 bytes, before and after substitution.

```yaml title="blueprint.yaml (fragment)"
variables:
  userId:
    required: true
vfs:
  mode: ephemeral
  cwd: /notes
  mounts:
    /notes:
      mode: named
      volume: notes
      subPath: users/${vars.userId}
```

```text
error: case.yaml: invalid vfs config: must be a relative volume path of at most 4096 bytes
```

```text
error: case.yaml: invalid vfs config: use ${vars.NAME} as a whole path component
```

```text
error: case.yaml: invalid vfs config: must be an absolute guest path of at most 4096 bytes
```

## secrets

A map from secret name to the source its value comes from.
`${secrets.NAME}` and `auth_proxy` `auth` fields reference a name declared
here. The file holds names, never values.

| Source | YAML | The value comes from |
| --- | --- | --- |
| `store` | `store: <key>` | The secret store under `<key>`. On a server, that is the server's encrypted store (`submilli server secret put`), and in `submilli run`, the local store (`submilli secret put`). Read on each use. |
| `harness` | `harness: {}` or `harness: {required: true}` | The application, when it opens or rebinds a session |

| Field | Type | Default | |
| --- | --- | --- | --- |
| `harness.required` | boolean | `false` | `true` refuses a session that doesn't supply a non-empty value |

```yaml title="blueprint.yaml (fragment)"
secrets:
  BILLING_API_KEY:
    store: billing_api_key
  LINEAR_API_KEY:
    harness:
      required: true
```

A session that supplies a value for a name the file doesn't declare, or for
a `store` secret, is refused. An empty supplied value counts as absent.

```text
error: case.yaml: blueprint parse error: secrets: a secret needs exactly one source: store / harness at line 3 column 3
```

```text
error: case.yaml: blueprint parse error: secrets.A: unknown field `env`, expected `store` or `harness` at line 4 column 5
```

Registration on a server checks that every `store` secret has a value in
the server's store. See [Registration](#registration).

## variables

A map from variable name to its rule. An application supplies variable
values, as strings, when it opens a session. The program can't read or
change them.

| Field | Type | Default | Constraints |
| --- | --- | --- | --- |
| `required` | boolean | `false` | `true` refuses a session that doesn't supply a non-empty value |
| `default` | string | none | The value bound when the session supplies none. Can't be combined with `required: true` |

An optional variable with no value and no default is unbound, and a filter
comparing against it doesn't match. A session that supplies a name the file
doesn't declare is refused.

```text
error: case.yaml: invalid variables config: variable 'a': `required: true` and `default:` are mutually exclusive
```

```text
error: case.yaml: invalid variables config: permissions for 'main': filter references undeclared variable '${vars.p}'
```

## packages

A list of package names a program may import. A map whose keys are the
names is accepted as well, and its values are ignored.

Each name:

- has the scoped form `@org/name`, with exactly one `/`
- has an `org` of 1 to 39 ASCII letters, digits, and single inner hyphens,
  with no leading or trailing hyphen
- has a `name` of ASCII letters, digits, `.`, `_` and `-`, other than `.`
  and `..`
- is not a `submilli:*` module and not an `@mcp/*` package
- is listed once

```text
error: case.yaml: invalid packages config: package `@acme.co/billing` scope `@acme.co` must be a GitHub org: ASCII letters, digits, and single internal hyphens only (no dots), 1–39 characters
```

```text
error: case.yaml: invalid packages config: `@mcp/linear` is an MCP virtual package; declare the server in `mcp:` instead
```

```text
error: case.yaml: blueprint parse error: duplicate package `@acme/a` in packages:
```

`submilli blueprint lint` checks each package against the local package
store, and registration against the server's:

```text
error: case.yaml: cannot validate package `@acme/billing` capabilities: package `@acme/billing` was not found in /…/packages; no packages are available
```

## auth_proxy

A list of rules, each adding credentials to outbound `submilli:http`
requests to one host. For a request, the first rule whose `host` equals the
request's host applies. The permission check runs before it. A request that
received credentials follows a redirect only to the same scheme, host, and
port.

| Field | Type | Required | Default | Constraints |
| --- | --- | --- | --- | --- |
| `host` | string | yes | | Matched exactly |
| `allow_insecure_http` | boolean | no | `false` | Also needs the top-level [`allow_insecure_http`](#allow_insecure_http) |
| `auth.bearer` | secret name | one of `auth`, `headers`, `query` | | Sends `Authorization: Bearer <value>` |
| `auth.basic.username` | string | with `auth.basic` | | A literal |
| `auth.basic.password` | secret name | with `auth.basic` | | Sends `Authorization: Basic <base64(username:value)>` |
| `headers` | map of name to string | one of `auth`, `headers`, `query` | | Values may hold `${secrets.NAME}`. No `Authorization` header alongside `auth` |
| `query` | map of name to string | one of `auth`, `headers`, `query` | | Values may hold `${secrets.NAME}` |

`auth` sets exactly one of `bearer` and `basic`.

```yaml title="blueprint.yaml (fragment)"
auth_proxy:
- host: status.acme.com
  auth:
    bearer: STATUS_TOKEN
- host: legacy.acme.com
  headers:
    X-Api-Key: ${secrets.LEGACY_KEY}
```

```text
error: case.yaml: invalid auth_proxy config: auth_proxy rule for host 'a.com' must set auth, headers, and/or query
```

```text
error: case.yaml: invalid auth_proxy config: auth_proxy rule for host 'a.com' must set exactly one of `auth.bearer` or `auth.basic`
```

```text
error: case.yaml: invalid auth_proxy config: auth_proxy rule for host 'a.com' sets both `auth:` and an explicit `Authorization` header — use one or the other
```

```text
error: case.yaml: invalid auth_proxy config: auth_proxy rule for host 'a.com' references undeclared secret 'K'
```

## git

The commit identity for `submilli:git`. Without a `git` key, programs can't
import `submilli:git`.

| Field | Type | Required | Constraints |
| --- | --- | --- | --- |
| `identity.name` | string | yes | Non-empty, with no control characters, `<` or `>` |
| `identity.email` | string | yes | Non-empty, with no control characters, `<` or `>` |
| `username` | string | no | Non-empty, with no control characters or `:`. The HTTPS username for private repositories, sent with the secret named `GIT_TOKEN` |

Each value may hold `${vars.NAME}`, resolved when the session opens. No
other `${…}` reference is accepted.

```yaml title="blueprint.yaml (fragment)"
git:
  identity:
    name: Support Agent (${vars.customerId})
    email: agent@acme.example
  username: agent
```

```text
error: case.yaml: blueprint parse error: git: missing field `identity` at line 3 column 3
```

```text
error: case.yaml: invalid git config: only ${vars.NAME} references are supported
```

```text
error: case.yaml: invalid git config: must be nonempty and contain no control characters or identity delimiters
```

## default

| | |
| --- | --- |
| Type | string |
| Allowed values | `deny`, `allow` |
| Default | `deny` |

The action for a capability check that no [`permissions`](#permissions)
rule matches.

```text
error: case.yaml: blueprint parse error: default: unknown variant `block`, expected one of … at line 2 column 10
```

## permissions

A map from caller to an ordered list of rules. The caller is `main` for the
program, or a package name for that package's own calls.

| Field | Type | Required |
| --- | --- | --- |
| `capability` | string, non-empty | yes |
| `filter` | filter expression | no |
| `action` | `allow`, `deny` | yes |

```yaml title="blueprint.yaml (fragment)"
permissions:
  main:
  - capability: acme.com/credits.apply
    filter: customerId == ${vars.customerId} and customerClass == "premium"
    action: allow
```

How rules are matched, and every capability and its fields, are in
[Permissions](/docs/reference/permissions). The filter grammar is in
[Filter language](/docs/reference/filter-language). A filter is
parsed with the file, so a malformed one is a parse error:

```text
error: case.yaml: blueprint parse error: permissions.main[0]: invalid filter `path ===`: expected `==`; a single `=` is not an operator
  path ===
         ^ at line 4 column 5
```

## mcp

A map from a server name, chosen by the file, to an outbound MCP server.
The name becomes the package `@mcp/<name>` and the capability
`mcp.<name>`.

| Field | Type | Required | Default |
| --- | --- | --- | --- |
| `url` | string | yes | |
| `transport` | `streamable_http`, `sse` | no | `streamable_http` |
| `headers` | map of name to string | no | none |
| `auth` | map with `type: oauth2` | no | none |

```yaml title="blueprint.yaml (fragment)"
mcp:
  linear:
    url: https://mcp.linear.app/mcp
    auth:
      type: oauth2
```

Every field, the OAuth fields, and the errors are in
[MCP servers](/docs/reference/mcp-servers).

## llm

The model providers `submilli:llm` reaches and the models a program may
name. A model this block doesn't declare can't be called.

| Field | Type | Default |
| --- | --- | --- |
| `providers` | map from provider name to [provider](#providers) | empty |
| `models` | map from model name to [model](#models) | empty |

```yaml title="blueprint.yaml (fragment)"
llm:
  providers:
    anthropic:
      type: anthropic
      api_key: ${secrets.ANTHROPIC_API_KEY}
  models:
    claude-haiku-4-5:
      provider: anthropic
      output_reserve: 4000
      description: "Cheap and fast; use for bulk per-item classification."
    claude-sonnet-5:
      provider: anthropic
```

The block has no budget fields. Token budgets are server settings. See
[Server settings](/docs/reference/server-settings).

### providers

| Field | Type | Required | Default | Constraints |
| --- | --- | --- | --- | --- |
| `type` | `anthropic`, `google`, `openai`, `openai-compatible` | yes | | |
| `base_url` | string | for `openai-compatible` | the provider's own endpoint | An absolute `https://` URL that is not `localhost` or a loopback, unspecified, link-local, private, broadcast, or carrier-grade NAT address literal. May hold `${secrets.NAME}` |
| `api_key` | string | no | none | Normally `${secrets.NAME}` |
| `supports_structured_outputs` | boolean | no | `true` | `false` sends no JSON Schema with typed calls |

```text
error: case.yaml: invalid llm config: llm provider 'p': unknown type 'mistral' (use one of: anthropic, google, openai, openai-compatible)
```

```text
error: case.yaml: invalid llm config: llm provider 'p': type 'openai-compatible' has no default endpoint; set 'base_url' to the https:// URL of the endpoint
```

```text
error: case.yaml: invalid llm config: llm provider 'p': base_url 'http://api.x.com/v1' uses the 'http' scheme; the API key travels in the Authorization header, so the endpoint must be https://
```

```text
error: case.yaml: invalid llm config: llm provider 'p': base_url 'https://10.0.0.5/v1' resolves to loopback, link-local, or private address space, which a credentialed client must not be pointed at; use the endpoint's public https:// hostname
```

```text
error: case.yaml: invalid llm config: llm provider 'p' references undeclared secret 'K'; add it under 'secrets:'
```

### models

The key is the name a program passes to `llm.call`, `llm.batch`, and the
name `llm.models()` returns.

| Field | Type | Required | Default | Constraints |
| --- | --- | --- | --- | --- |
| `provider` | string | yes | | A key of `llm.providers` |
| `context_window` | non-negative integer | no | unknown | Returned by `models()` as `contextWindow` |
| `output_reserve` | non-negative integer | no | 64,000 reserved, no output cap sent | Output tokens per prompt |
| `description` | string | no | none | One line of at most 512 characters, with no control or invisible formatting characters. Returned by `models()` |

`output_reserve` is counted against the run's and the server's token budgets
for each prompt, with the prompt's estimated size, before the prompt is
sent. A prompt that doesn't fit is refused with `QuotaExceededError`
without being sent. When set, it is also sent as the request's output cap.
When absent, 64,000 tokens are reserved and the request sets no cap, except
Anthropic requests, which always carry one and use 4,096.

```text
error: case.yaml: invalid llm config: llm model 'm' names undeclared provider 'p'; declare it under 'llm.providers:' or point the model at one of: (none declared)
```

```text
error: case.yaml: invalid llm config: llm model 'm': description contains a newline; it reaches a model's selection reasoning verbatim, so keep it to a single line of printable text
```

```text
error: case.yaml: blueprint parse error: llm.models.m.output_reserve: invalid type: string "4k", expected u64 at line 6 column 38
```

An `llm.call` rule whose filter tests `model == "…"` must name a declared
model:

```text
error: case.yaml: invalid llm config: caller 'main': permission filter names undeclared llm model 'gpt-9'; declare it under 'llm.models:' or filter on one of: m
```

## Errors

`submilli blueprint lint`, `submilli run --blueprint`, and registration
parse the file the same way, and parsing stops at the first error. The CLI prints it
as `error: <file>: <prefix>: <message>`, with the YAML path and the line and
column when they are known. Over HTTP, the same error is a `400` response
whose `error` field names the class. See [HTTP API](/docs/reference/http-api).

| Prefix | `error` over HTTP | Raised by |
| --- | --- | --- |
| `blueprint YAML is empty` | `parse_error` | An empty file |
| `blueprint parse error` | `parse_error` | YAML syntax, unknown or repeated keys, wrong types, unknown values, `vfs` fields and mounts, filters, `idle_timeout` |
| `invalid blueprint kind` | `invalid_kind` | [`kind`](#kind) |
| `invalid blueprint name` | `invalid_name` | [`name`](#name) |
| `invalid vfs config` | `invalid_vfs` | `subPath` and `cwd` |
| `invalid packages config` | `invalid_packages` | [`packages`](#packages) names |
| `invalid variables config` | `invalid_variables` | [`variables`](#variables), undeclared `${vars.NAME}` in filters |
| `invalid auth_proxy config` | `invalid_auth_proxy` | [`auth_proxy`](#auth_proxy) |
| `invalid permissions config` | `invalid_permissions` | Empty caller or capability names |
| `invalid git config` | `invalid_git` | [`git`](#git) |
| `invalid mcp config` | `invalid_mcp` | [`mcp`](#mcp), `mcp.<name>` rules |
| `invalid llm config` | `invalid_llm` | [`llm`](#llm), `llm.call` model filters |

### Registration

`submilli server blueprint apply` sends the file to the server, which parses
it and then checks it against what the server holds. A file that fails a
check isn't registered.

| Check | Message |
| --- | --- |
| Every volume, as the root or a mount, is declared on the server | `volume 'team' is not declared on this server; declared volumes: handbook, notes` |
| `access` doesn't exceed the server's declaration | ``volume 'handbook' is read_only on this server; drop `access: read_write` (or write `access: read_only`), or ask the operator to declare it read_write`` |
| Every `store` secret has a value in the server's store | `secret check failed: missing secret 'K'` |
| Every package, and every package it depends on, is installed on the server | ``package check failed: package `@acme/billing` is not installed; install it with `submilli server packages install <org/repo> @acme/billing` `` |
| Each capability a package requires for its own calls has a rule under that package's caller | `package check failed:` followed by the package, the capability, and the missing rule |
| A filter tests only fields its capability reports, for standard-library, package, and `mcp.<name>` capabilities | The rule, the field, and the fields the capability reports |

`submilli blueprint lint` makes the package and filter checks against the
local package store, and reports every filter field it finds:

```text
error: case.yaml: `permissions.main` rule 2 for `fs.read` tests `host`, which the operation doesn't report, so a condition on it is false for every call, and true under `not`; its fields are: chunkSize, length, path, recursive
```

`harness` secrets aren't checked at registration. The secret check is made
once, so a value removed from the store later fails the call that needs it.
