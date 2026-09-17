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
| `kind: blueprint` | Document discriminator for `submilli apply` |
| `name` | Registered name; the REST `blueprint` field and the MCP path `/mcp/<name>` |
| `variables` | Session variables. Each has `required: true` or `default: "value"`, never both. Referenced as `${vars.NAME}` in filters |
| `packages` | Packages the program may import. Unlisted packages do not exist for it |
| `secrets` | Declared secret names and sources: `{ env: VAR }`, `{ file: /path }`, `{ store: key }`, or `{ harness: { required: true } }` for a value the trusted application binds per session. A server accepts only `store` and `harness` sources in a blueprint registered over its API (see Workflow) |
| `auth_proxy` | Host-keyed credential injection for direct HTTP: `host`, then `auth: { bearer: "${secrets.X}" }`, `auth: { basic: { username, password } }`, `headers`, or `query` |
| `default` | Fall-through action: `deny` (the default and the norm), `allow`, or `ask-human` |
| `permissions` | Per-caller rule lists; see below |
| `mcp` | Outbound MCP servers keyed by local name: `url`, optional `transport` (`streamable_http`), `headers` with `${secrets.X}`, or `auth: { type: oauth2, ... }`. Imported as `@mcp/<name>`; gated by the `mcp.<name>` capability with a `tool` field |
| `vfs` | `none`, `ephemeral` (default; wiped per execute), `per_session` (kept for the session), or `persistent: { volume }` |
| `idle_timeout` | Session reaping window, e.g. `3600s`; default one day |

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
| `http.get` `post` `put` `patch` `delete` `head` `options` | `host`, `path`, `method`, `body_size`, `timeout_ms` |
| `http.download` | `host`, `url_path`, `vfs_path`, `max_bytes`, `overwrite`, `decompress` |
| `fs.read` `write` `stat` `list` `mkdir` `remove` `move` `copy` | `op`, `path` (or `from`, `to`), `recursive` |
| `session.read` `write` `remove` `list` | `op`, `key` or `prefix` |
| `secrets.get` (packages only) | `name` |
| `mcp.<server>` | `tool`, `transport` |

## A real service

For the orders package in [packages](packages.md): a required customer
binding, a server-side token, read allowed for the bound customer, and small
cancellations routed to human approval.

```yaml
kind: blueprint
name: support-orders

variables:
  customerId:
    required: true

secrets:
  ORDERS_API_TOKEN:
    env: ORDERS_API_TOKEN

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
      filter: host == "api.acme.com" and method == "GET"
      action: allow
    - capability: http.post
      filter: host == "api.acme.com" and method == "POST"
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

`ask-human` routes the call to the operator's approval flow. It is meaningful
only when the client integration implements that flow; confirm the harness
supports it before relying on it, and use a separate approval-stage blueprint
or a draft operation when it does not.

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

## Workflow

```sh
submilli blueprint init support-orders
submilli blueprint secret add ORDERS_API_TOKEN --env ORDERS_API_TOKEN
submilli blueprint add-package '@acme/orders' --capabilities acme.com/orders.list
submilli blueprint capability add acme.com/orders.cancel \
  --filter 'customerId == ${vars.customerId} and totalCents <= 5000' --action ask-human
submilli blueprint lint blueprint.yaml          # --fix adds missing package rules
submilli blueprint prompt                       # what the model will be told
```

`add-package` writes the package's derived `requires` grants for you and warns
about undeclared secrets. Add `variables` by editing the file; the CLI editors
rewrite YAML and drop comments. Lint warns on unreachable `main` rules (an
earlier rule shadows a later one), on `default: allow`, and on provided
capabilities with no `main` rule.

Register and run:

```sh
submilli-server                                  # separate terminal
submilli server blueprint apply blueprint.yaml   # or: submilli apply -f dir/
submilli server run-code --blueprint support-orders program.ts
```

`apply` rejects a blueprint whose secrets use `env:` or `file:` with "not
allowed for a blueprint registered over the API": the server has no inbound
authentication, so those sources would let any caller read its environment or
filesystem. They work only for local runs (`submilli run --blueprint`). For a
server, declare the secret with `--store` and provision the value into the
server's secret store, or use a `harness` source bound per session:

```sh
submilli blueprint secret add ORDERS_API_TOKEN --store ORDERS_API_TOKEN
printf '%s' "$ORDERS_API_TOKEN" | submilli server secret put ORDERS_API_TOKEN
```

REST is `POST /v1/execute` with `{ "blueprint", "code", "variables", "secrets" }`;
the response carries `result`, `console`, and on failure `error.kind` and
`error.message`. A missing required variable is rejected as `invalid_request`
before compilation. MCP clients bind variables in the `submilli-variables`
header or `initialize` `_meta.variables`; see [harnesses](harnesses.md).
`submilli run --blueprint file.yaml program.ts` applies policy locally but has
no variable binding, so a `${vars.*}` filter never matches there.

## Verification matrix

Prove the boundary with deterministic calls, not a model ignoring bait:

| Case | Expect |
| --- | --- |
| Bind `customerId=cus_northwind`, program reads that customer | Result returned |
| Same binding, program passes `cus_initech` | `permission denied ... capability=acme.com/...`, not a network error or empty list |
| Omit the required variable | `invalid_request` naming the variable; program never runs |
| Call an unlisted capability or a raw `http.get` from `main` | Denied |
| A write under `ask-human` | The approval path fires and nothing changes until approved |
| Denied write | No side effect at the service; note that earlier successful calls in the same program are not rolled back |

Then run controls that prove the assertions are load-bearing, as the
quickstart's `verify.sh` does: remove the filter and the cross-customer call
must succeed; flip the bound customer and both outcomes must invert. Finally,
optionally run the real harness with an injected instruction and capture the
executed code and the denial. A denial is a result to report, never a reason
to widen policy without the developer's decision.
