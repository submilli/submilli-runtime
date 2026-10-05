# Design and verify a blueprint

A blueprint is the operator's YAML policy: which packages a program may
import, which capabilities each caller may use and under what filter, where
credentials come from, and which session variables the trusted application
must bind. The runtime denies everything the blueprint does not allow. Decide
the operations and their fields first with
[capability design](capability-design.md); this reference covers writing and
proving the policy.

## The smallest slice

For the fixture package in [packages](packages.md):

```yaml
kind: blueprint
name: support-read
variables:
  customerId:
    required: true
packages:
  - '@acme/billing'
default: deny
permissions:
  main:
    - capability: acme.com/balance.read
      filter: customerId == ${vars.customerId}
      action: allow
  '@acme/billing': []
```

Read it as a sentence: generated code may read a balance, only for the
customer this session was bound to, and nothing else. The package's own grant
list is empty only because the fixture makes no network or secret calls.

A program the agent might write against it:

```typescript
import { readBalance } from "@acme/billing";
function main(): number {
    return readBalance("cus_northwind");
}
```

## Schema

Every top-level key, all optional except `name`:

| Key | Meaning |
| --- | --- |
| `kind: blueprint` | Document discriminator for `submilli server apply` |
| `name` | Registered name; the REST `blueprint` field and the MCP path `/mcp/<name>` |
| `variables` | Session variables. Each has `required: true` or `default: "value"`, never both. Referenced as `${vars.NAME}` in filters |
| `packages` | Packages the program may import. Unlisted packages do not exist for it |
| `secrets` | Declared secret names and sources: `{ store: key }` or `{ harness: { required: true } }` for a value the trusted application binds per session. |
| `allow_insecure_http` | Defaults to `false`: script HTTP (including packages/downloads) requires HTTPS. Does not govern MCP/LLM connections or inbound server HTTP |
| `auth_proxy` | Host-keyed credential injection for direct HTTP: `host`, optional `allow_insecure_http: true` (also requires the blueprint flag), then `auth: { bearer: X }`, `auth: { basic: { username, password } }`, `headers`, or `query` |
| `default` | Fall-through action: `deny` (the default and the norm), `allow`, or `ask-human` |
| `permissions` | Per-caller rule lists; see below |
| `mcp` | Outbound MCP servers keyed by local name: `url`, optional `transport` (`streamable_http`), `headers` with `${secrets.X}`, or `auth: { type: oauth2, ... }`. Imported as `@mcp/<name>`; gated by the `mcp.<name>` capability with a `tool` field; see MCP servers below |
| `vfs` | The program's `/`: `none` (every `submilli:fs` call fails), `ephemeral` (default; a scratch directory deleted after the run), `per_session` (lasts as long as the session, like `submilli:session` state), or `named` (`{ mode: named, volume, access? }`: an operator-declared volume kept across sessions and restarts and shared with other blueprints naming it). `mounts` adds named volumes below an `ephemeral`, `per_session` or `named` root: `mounts: { /memory: { mode: named, volume: memory, access: read_only } }`. `access` can only narrow the server's; a write under a read-only volume throws `PermissionDeniedError`. Mounts may not nest, and mount points cannot be moved or removed. A named root or mount takes `subPath`, a directory inside the volume to expose instead of all of it, relative and normalized; a whole component may be `${vars.NAME}` (`subPath: users/${vars.userId}`), which is how one volume gives each user their own directory at the same path. A writable mount creates it. `cwd` (absolute, default `/`, may use `${vars.NAME}`) is where relative paths resolve for `fs`, packages, Git, and downloads; `fs.cwd()` returns it. It confines nothing: permissions still match absolute paths. A program run outside a session gets one that closes when it returns. `ephemeral` and `per_session` take `size_limit` (`100MB`, binary units: 104,857,600 bytes). Every write counts, packages' included: `fs` writes, appends, writers, copies, `http.download`, and Git; `fs.remove` and `fs.move` free space. One past it throws a catchable `QuotaExceededError`; under `per_session` the limit spans the session. It counts bytes, not entries. A named volume's limit is set in the server config and shared by everyone using it |
| `idle_timeout` | Session reaping window, e.g. `3600s`; default one day |
| `llm` | Models a program may call through `submilli:llm`: `providers` (name → `type`, `api_key: ${secrets.X}`) and `models` (name → `provider`, optional `description`). A model not listed cannot be called; descriptions reach the model writing the program, so they say which model is for what |

`submilli blueprint init` scaffolds a commented minimal file; `--full` lists
every stdlib capability as a deny rule with example filters.

## Callers and rules

`permissions` is keyed by caller id. `main` is the generated program. Each
package name is the caller for that package's own stdlib calls. Rules are an
ordered list; the first rule whose capability name matches exactly and whose
filter matches the call decides. No wildcards in names. A rule is
`capability`, optional `filter`, and `action` (`allow`, `deny`, `ask-human`).

Two kinds of capability, two homes:

- **Business capabilities** a package provides (`acme.com/orders.list`) go
  under `main`. An explicit `check` inside a package gates the package's
  consumer, so these rules answer "may the program do this?"
- **Infrastructure capabilities** a package requires (`http.post`,
  `secrets.get`) go under the package's own caller. Derive them from the
  package's `capabilities.yaml` with `submilli blueprint add-package` or
  `submilli blueprint lint --fix`; do not hand-write hosts and secret names.
  Then look for derived rules that leave a caller-chosen value open: `fs.*`
  with no `path` filter, `http.download` with no `vfs_path`. The package
  passes that value through from the program, so the program can steer the
  package there. To keep each user's files apart, mount only that user's
  directory of a named volume, with `subPath: ${vars.userId}`: the program
  and every package then see that directory alone, and no rule needs a
  path filter. When rules confine the resource instead, give the
  package's rule the same filter as `main`'s, such as `fs.write` with
  `path glob "/data/*"`, or the program routes around `main`'s rules
  through the package. Lint then warns that the rule differs from what the
  package declared, and `--fix` leaves it alone.

`secrets.get` can never be granted to `main`. Do not give `main` raw `http.*`
access to a host a package already wraps: that reopens every argument the
package constrains. Direct HTTP for `main` is a deliberate design for a
different job (fetching public data, a download), granted with a host filter
and, if credentials are needed, an `auth_proxy` rule rather than a secret.

## Filters

A filter is a boolean expression over the JSON context passed to `check` (or
the fields a stdlib gate supplies). A missing field or a wrong-kind value is a
non-match, which under `default: deny` means denied.

| Form | Example |
| --- | --- |
| Equality on string, number, boolean, null | `status == "open"`, `note != null` |
| Numeric comparison | `amountCents <= 5000`, `count < 100` |
| Shell glob | `channelId glob "C0*"`, `path glob "/v1/customers/*"` |
| Regular expression | `path matches "^/v1/customers/[a-z_0-9]+$"` |
| Array membership | `recipients contains "ops@acme.com"` |
| Nested field path | `input.teamId == "T1"` |
| Session variable | `customerId == ${vars.customerId}` |
| Variable inside a string | `key glob "triage/${vars.customerId}/*"` |
| Boolean structure | `not (a == 1) and (b < 2 or c == "x")` |

Variables are strings and are coerced to the compared field's kind. Inside a
quoted string a variable is escaped for the operator, so a bound value cannot
widen a glob. Every `${vars.NAME}` must be declared under `variables`; lint
rejects undeclared references. A filter is parsed when the blueprint is
registered, so syntax errors fail at `lint` or `apply`, never at first use.

Stdlib gates and their fields, from `submilli blueprint capability list`:

| Capability | Fields |
| --- | --- |
| `http.get` `post` `put` `patch` `delete` `head` `options` | `host`, `path`, `body_size`, `timeout_ms` |
| `http.download` | `host`, `url_path`, `vfs_path`, `max_bytes`, `overwrite`, `decompress` |
| `fs.read` `write` `stat` `list` `mkdir` `remove` `move` `copy` | `path` (or `from`, `to`), `recursive` |
| `session.read` `write` `remove` `list` | `key` or `prefix` |
| `secrets.get` (packages only) | `name` |
| `mcp.<server>` | `tool`, `transport` |
| `llm.call` (covers `call`, `batch`, `models()`) | `model`, `prompt_count`; narrowing `model` also narrows what `models()` lists |

## A real service

For the orders package in [packages](packages.md): a required customer
binding, a server-side token, read allowed for the bound customer, and small
cancellations held for a separate application approval workflow.

```yaml
kind: blueprint
name: support-orders

variables:
  customerId:
    required: true

secrets:
  ORDERS_API_TOKEN:
    store: ORDERS_API_TOKEN

packages:
  - '@acme/orders'

default: deny

permissions:
  main:
    - capability: acme.com/orders.list
      filter: customerId == ${vars.customerId}
      action: allow
    - capability: acme.com/orders.cancel
      filter: customerId == ${vars.customerId} and totalCents <= 5000
      action: ask-human

  '@acme/orders':
    - capability: http.get
      filter: host == "api.acme.com"
      action: allow
    - capability: http.post
      filter: host == "api.acme.com"
      action: allow
    - capability: secrets.get
      filter: name == "ORDERS_API_TOKEN"
      action: allow
```

The cancel rule works only because the package resolves the order's owner and
total and passes them to `check`. A filter can constrain only fields the
package puts in the context; if the field is missing, fix the package, never
the prompt. A cancel of another customer's order or one above the limit falls
through to `default: deny`.

The runtime currently treats `ask-human` as a denial. It does not pause,
dispatch an approval request, or resume the call after a human approves it.
This example therefore cannot execute cancellations. If approval is required,
the trusted application must collect it separately and execute the approved
operation through a separately authorized path with appropriately scoped
`allow` rules. Keep that path and its credentials outside the runtime agent's
control. A draft operation is another option when the service supports one.

## Roles and tenants

- **One blueprint per role, not per tenant.** Many customers with the same
  permissions share one blueprint parameterized by a required variable that
  the trusted application binds after authenticating the user.
- **Split read from write authority.** A triage agent that reads tickets and a
  refund executor that changes money get different blueprints even when they
  import the same package, so a compromised read session cannot write.
- **Operator workflows are separate.** Cross-customer reporting for staff is a
  distinct blueprint whose selection happens in application code after a role
  check, not a variable the model chooses.
- **Fixed values become filters, not variables**, when a workflow always
  targets the same repository, channel, or environment.
- **Variables are constraints, not authentication.** The server trusts its
  caller to bind them; the security property is that generated code cannot
  rebind them. Secure ingress to the server before exposing it beyond a
  trusted network.

## Direct HTTP with a credential, and model calls

When there is no package, only an HTTP API, `main` may call it under an
`http.get` rule and the auth proxy adds the credential on the way out. The
permission check runs first; then, for a request whose host exactly matches
an `auth_proxy` entry, the runtime adds the header. The token never enters the
program. A model call works the same way: the provider key stays in
`secrets`, the program names a model, and the response comes back.

Use HTTPS URLs. Do not enable `allow_insecure_http` merely to make a failing
request work: establish that the user intends cleartext traffic for this
blueprint and, separately, for each affected auth-proxy credential. The
blueprint flag alone permits no HTTP to a matching auth-proxy host; that
rule must also set `allow_insecure_http: true`. Both flags default to false,
including for localhost, and denial occurs before secret resolution.
`blueprint auth-proxy add --allow-insecure-http` sets only the rule flag;
edit the top-level flag in YAML. Existing HTTP blueprints need explicit
opt-ins or HTTPS URLs when upgrading.

Redirect destinations obey the same gates. Injected headers and query values
restrict redirects to the same scheme, host, and effective port; redirects
do not inject new credentials. Capability and network restrictions still
apply. Runs without a blueprint, MCP/LLM connections, and inbound server HTTP
retain their existing behavior; Git remains HTTPS-only.

```yaml
kind: blueprint
name: support-status
secrets:
  STATUS_TOKEN:
    store: status_token
  ANTHROPIC_API_KEY:
    store: anthropic_api_key
auth_proxy:
  - host: status.acme.com
    auth:
      bearer: STATUS_TOKEN
llm:
  providers:
    anthropic:
      type: anthropic
      api_key: ${secrets.ANTHROPIC_API_KEY}
  models:
    claude-haiku-4-5:
      provider: anthropic
      description: "Cheap and fast; use for bulk per-item classification."
default: deny
permissions:
  main:
    - capability: http.get
      filter: host == "status.acme.com"
      action: allow
    - capability: llm.call
      filter: model glob "claude-*"
      action: allow
```

`--bearer` and `--basic-username` with `--basic-password` cover the common
auth-proxy cases; `--header NAME=VALUE` and `--query KEY=VALUE` with
`${secrets.X}` values cover the rest.

## MCP servers

When a service has an MCP server and no package, declare the server and it
becomes the package `@mcp/<name>`, one function per tool:

```sh
submilli blueprint add-mcp playwright http://localhost:8931/mcp
submilli docs @mcp/playwright            # connects; prints every tool's signature
submilli blueprint capability add mcp.playwright \
  --filter 'tool == "browser_navigate" or tool == "browser_snapshot"'
```

`add-mcp` writes the `mcp` block and a `deny` rule for `mcp.<name>` under
`main`; `capability add` puts the allow ahead of it. One capability covers the
whole server, and `tool` (with `==` or `glob`) picks tools within it. Choose
tools from the `docs` listing, not from the task description: servers ship
tools that run arbitrary code or write (Playwright's `browser_evaluate`), and
a model given the server will reach for them. `mcp.<name>/<tool>` as a
capability name is refused. Only Streamable HTTP servers work; a stdio server
must be put behind an HTTP endpoint first.

A tool takes one object of arguments, is called synchronously, and returns
`unknown` unless the server publishes an output schema (or it is GitHub's
hosted server, whose schemas Submilli carries). Cast the result to an
interface declaring only the fields the program reads: the cast is checked,
and a declared field the server doesn't send fails it. A program's calls to
one server share one MCP session, closed when the program ends, so a stateful
sequence (navigate, then snapshot) belongs in one program.

Credentials, by what the server wants:

| Server wants | Declare with |
| --- | --- |
| An API key | `secret add KEY --store key`, then `add-mcp <name> <url> --authorization-bearer KEY` |
| Each user's own token | A `harness` secret, and `--header 'Authorization: Bearer ${secrets.KEY}'` |
| An OAuth login | `add-mcp` probes and writes `auth: type: oauth2` (or pass `--oauth`) |

An OAuth login is interactive: the developer opens a URL in a browser. Hand
them the command instead of running it: `submilli mcp authenticate <name>
--blueprint blueprint.yaml` locally, or `submilli server mcp authenticate
<blueprint> <name>` after the blueprint is applied. Local and server
credentials are separate. One login serves every session of the blueprint,
so every user acts as whoever logged in; use a per-user header when users
must act as themselves.

Verify with `submilli run --blueprint blueprint.yaml` locally: an allowed
tool returns, a tool outside the filter fails with `PermissionDeniedError ...
capability=mcp.<name>` before any request reaches the server. On a server,
private addresses are blocked, so a `localhost` MCP server needs
`submilli-server --allow-localhost`; an unreachable server is left out of the
blueprint with a `server unavailable` warning, and imports of it don't compile.

## Workflow

```sh
submilli blueprint init support-orders
submilli blueprint secret add ORDERS_API_TOKEN --store ORDERS_API_TOKEN
submilli blueprint add-package '@acme/orders' --no-capabilities   # list it; grant nothing yet
submilli blueprint capability list '@acme/orders'                 # names and filterable fields
submilli blueprint capability add acme.com/orders.list \
  --filter 'customerId == ${vars.customerId}'
submilli blueprint capability add acme.com/orders.cancel \
  --filter 'customerId == ${vars.customerId} and totalCents <= 5000' --action ask-human
submilli blueprint auth-proxy add --host status.acme.com --bearer STATUS_TOKEN
submilli blueprint lint blueprint.yaml          # --fix adds rules packages require
submilli blueprint capability list --unconfigured  # provided operations main has no rule for
submilli blueprint prompt                       # what the model will be told
```

`add-package` lists the package so imports resolve and writes its own derived
`requires` grants, plus those of the packages it depends on (which stay out
of `packages:`, so the program reaches them only through it); it warns about
undeclared secrets. The first matching rule wins, so an existing rule is
kept: an unfiltered `main` rule for a selected capability, any rule in the
package's own caller list for a capability it requires, and a dependency's
whole caller list if it has one (lint reports what that list lacks). With
`--no-capabilities` it grants `main` nothing, which is the right first step:
each operation the program may call is then one explicit `capability add`.
`--capabilities NAME,...` and `--all-capabilities` are the shortcuts.
`capability add` refuses a name it does not know, or one only a dependency
provides for `main` (which cannot import it), unless `--force` is given. It
accepts any other HTTP method as `http.<method>` (`http.request("TRACE", …)`
checks `http.trace`) and warns that the rule matches only that method, but
refuses a near miss of a cataloged operation such as `http.dlete`. Add
`variables`, `vfs`, `idle_timeout` and `llm` by editing the file; the CLI
editors rewrite YAML and drop comments, so keep hand-written commentary
elsewhere. Lint warns on the names `capability add` refuses or warns about (a
name nothing provides never matches), on `main` rules the runtime never
consults (such as `secrets.get`), on a rule
that follows an unfiltered rule for the same capability and caller (it never
matches; `capability add` warns when it writes one), and on `default: allow`,
and checks the `requires` of listed packages' dependencies like those of the
packages themselves. A provided capability with no `main` rule is withheld, not
a finding; `submilli blueprint capability list --unconfigured` lists them.

Register only after the server can resolve the blueprint's packages,
dependencies, secrets, and named volumes. `add` and `apply` reject a missing
package with `package_missing`; applying YAML does not install it.

For the `support-orders` example, build and publish `@acme/orders` locally
from its project directory. A server on the same machine can use that local
store when it has the same `SUBMILLI_HOME`. For a remote server, install the
package from its actual GitHub repository with `submilli server packages
install <org/repo> @acme/orders`. The curated `@submilli/*` packages live in
`submilli/submilli-runtime`; they do not supply custom `@acme/*` operations.

Start an authenticated server with its encrypted secret store configured as
in [setup](setup.md). Use its admin token in `SUBMILLI_SERVER_TOKEN` for the
CLI. The example YAML already declares `ORDERS_API_TOKEN` as a `store:`
secret. Populate it from the application's securely supplied environment
variable before applying the blueprint:

```sh
# From the @acme/orders project, sharing SUBMILLI_HOME with the local server.
submilli build publish-local --deny-warnings
printf '%s' "$ORDERS_API_TOKEN" | submilli server secret put ORDERS_API_TOKEN
submilli server blueprint apply blueprint.yaml
```

`submilli server packages install` builds packages from a GitHub repository,
pinned to a commit. `submilli server packages list` includes server-managed
packages and packages from the fallback CLI store, marking the latter as
unmanaged and showing their source.
For credentials supplied by the trusted application per session, use a
`harness` secret source instead of `store:`.

Registered blueprints and their revisions live in the server's SQLite
database. Manage them through the CLI/API; editing an old blueprint directory
does not update the active policy. See [setup](setup.md) for database
persistence and the one-time legacy import.

REST is `POST /v1/execute` with `{ "blueprint", "code", "variables", "secrets" }`
and `Authorization: Bearer $SUBMILLI_SERVER_TOKEN`;
the response carries `result`, `console`, and on failure `error.kind` and
`error.message`. A missing required variable is rejected as `invalid_request`
before compilation. MCP clients bind variables in the `submilli-variables`
header or `initialize` `_meta.variables`; see [harnesses](harnesses.md).
Locally, `submilli run --blueprint file.yaml --var customerId=cus_northwind
program.ts` applies the policy with that binding, the way an application binds
it; `--var` repeats, and a required variable left out is refused. No server is
needed, so run the verification matrix below this way first.

## Verification matrix

Prove the boundary with deterministic calls, not a model ignoring bait:

| Case | Expect |
| --- | --- |
| Bind `customerId=cus_northwind`, program reads that customer | Result returned |
| Same binding, program passes `cus_initech` | `permission denied ... capability=acme.com/...`, not a network error or empty list |
| Omit the required variable | `invalid_request` naming the variable; program never runs |
| Call an unlisted capability or a raw `http.get` from `main` | Denied |
| A write under `ask-human` | `PermissionDeniedError`, no side effect, and no built-in approval or resume flow |
| Denied write | No side effect at the service; note that earlier successful calls in the same program are not rolled back |

Then run controls that prove the assertions are load-bearing, as the
quickstart's `verify.sh` does, using only isolated fixture data: remove the filter and the cross-customer call
must succeed; flip the bound customer and both outcomes must invert. Finally,
with the user's authorization, run the real harness with an injected instruction and capture the
executed code and the denial. A denial is a result to report, never a reason
to widen policy without the developer's decision.
