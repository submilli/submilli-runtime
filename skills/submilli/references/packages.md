# Build a package

A package is the only way generated code reaches a service. It is a small,
reviewed TypeScript-subset library that exposes narrow typed operations, runs
a `check(...)` before every protected effect, and owns the credential. Design
the operations and their capability fields first using
[capability design](capability-design.md); this reference covers building.

## Layout and commands

```sh
submilli build init @acme/billing packages/billing   # first package: writes submilli.toml
submilli build new  @acme/billing packages/billing   # add to an existing submilli.toml
submilli build check                                 # compile, derive capabilities.yaml
submilli build test [-p @acme/billing]               # run tests/**/*.test.ts + docs examples
submilli build publish-local [-p @acme/billing]      # install into the local store
```

`submilli.toml` holds one `[[package]]` block per package: `name`, `version`,
`description`, `keywords`, `path`. Inside the package: `src/lib.ts` is the
entrypoint, `docs/readme.md` is the model-facing documentation, and
`tests/*.test.ts` are the tests. The scaffold stubs (`hello()` and a one-line
readme) are placeholders to replace, not patterns to extend.

`build check` derives `capabilities.yaml` from the source. `provides` lists
each `@capability` with its field types; `requires` lists the stdlib
capabilities the package itself uses (`http.*`, `secrets.get`) with filters
derived from the code. `submilli blueprint add-package` and
`submilli blueprint lint --fix` turn `requires` into the package's grants, so
never hand-write those rules from memory.

## The smallest slice

```typescript
import { check } from "submilli:security";

/**
 * Return a customer's fixture balance in cents.
 * @capability acme.com/balance.read { customerId: string }
 */
export function readBalance(customerId: string): number {
    check("acme.com/balance.read", { customerId });
    return 6150;
}
```

Use a fixture like this to prove the policy path end to end before touching a
real service. [blueprints](blueprints.md) grants and verifies it.

## Capability annotations and `check`

The `@capability` tag declares the operation's name and the policy-visible
fields; `check` enforces it at runtime. The annotation alone enforces nothing.

```text
@capability acme.com/orders.cancel { customerId: string, orderId: $orderId, totalCents: number }
```

Each binding inside `{ }` becomes a field in the schema:

| Binding form | Meaning |
| --- | --- |
| `customerId` | Field named after a parameter; its type is the parameter's type |
| `orderId: $ref` or `owner: $input.teamId` | Bound to a parameter or a path inside it; type is taken from there |
| `amount: number`, `tags: string[]` | Declared by type when the value is computed, not a parameter |
| `kind: "order"` | Literal, for fixed context values |

Type aliases are peeled to their primitive so filters see `string`, `number`,
`boolean`, or `string[]`. Name capabilities `<domain>/<resource>.<verb>` or
`<domain>/<operation>` and keep one capability per operation, so a blueprint
can allow reads without allowing writes.

`check(capability, context)` serializes `context` to JSON and asks the policy
engine whether the *consumer* of this package (`main`, the generated program)
may perform it. It throws a catchable `PermissionDeniedError` on denial, so
call it before the effect: nothing protected may run before the check passes.
The context's field names must match the annotation and carry the values the
operation will actually use. Normalize arguments first and use the normalized
value in both the check and the request. `main` cannot call `check` itself;
policy for generated code lives in package checks and stdlib gates.

When authorization depends on a fact the caller did not pass, resolve it in
package code and check the resolved value. A cancel that takes only an order id
must fetch the order and check its `customerId`; do not trust a caller-supplied
owner. Internal helpers that are not exported need no check of their own.

## A real service

The pattern below wraps a REST API. It reads the token by literal secret name
inside the package, builds URLs from a top-level constant so the compiler can
derive `host == "api.acme.com"` into `requires`, separates pure builders from
network code so they can be unit tested, and resolves ownership before a write.

```typescript
// Orders for one customer, wrapping the internal REST API at api.acme.com.
// The API token is read from the ORDERS_API_TOKEN secret inside this package;
// generated code never sees it.

import { get, post, Response } from "submilli:http";
import { encodeComponent } from "submilli:url";
import secrets from "submilli:secrets";
import { check } from "submilli:security";

const BASE = "https://api.acme.com/v1";

/** One order on a customer's account. */
export interface Order {
    /** Order identifier, as the orders service issued it. */
    id: string;
    /** Owning customer. */
    customerId: string;
    /** "open" | "shipped" | "cancelled". */
    status: string;
    /** Total in cents. */
    totalCents: number;
}

/** Cursor pagination for list calls. */
export interface PageOptions {
    /** Page size, 1-100 (default 50). */
    limit?: number;
    /** Cursor from a previous page's `nextCursor`. */
    cursor?: string;
}

/** One page of results. */
export interface Page<T> {
    /** Items on this page. */
    items: T[];
    /** Cursor for the next page, or null on the last page. */
    nextCursor: string | null;
}

/** Fields a caller may set when cancelling. Absent fields are not sent. */
export interface CancelInput {
    /** Free-text reason recorded on the order. */
    reason?: string;
}

/**
 * List one customer's orders, newest first.
 * @capability acme.com/orders.list { customerId }
 */
export function listOrders(customerId: string, page: PageOptions | null = null): Page<Order> {
    check("acme.com/orders.list", { customerId: customerId });
    const query = buildPageQuery(page);
    const path = "/customers/" + encodeComponent(customerId) + "/orders" + query;
    const response = request("GET", path, null);
    return JSON.parse(response.body) as Page<Order>;
}

/**
 * Cancel an open order. The order's owner is resolved from the service, so the
 * policy filter sees the real customer even though the caller passes only an id.
 * @capability acme.com/orders.cancel { customerId: string, orderId: $orderId, totalCents: number }
 */
export function cancelOrder(orderId: string, input: CancelInput | null = null): Order {
    const order = fetchOrder(orderId);
    if (order === null) {
        throw new Error("order not found: " + orderId);
    }
    check("acme.com/orders.cancel", {
        customerId: order.customerId,
        orderId: orderId,
        totalCents: order.totalCents,
    });
    const body: CancelInput = input === null ? {} : input;
    const response = request("POST", "/orders/" + encodeComponent(orderId) + "/cancel", body);
    return JSON.parse(response.body) as Order;
}

/** Build the `?limit=&cursor=` suffix; exported so tests cover it without a token. */
export function buildPageQuery(page: PageOptions | null): string {
    let limit = 50;
    let cursor: string | null = null;
    if (page !== null) {
        const requested = page.limit;
        if (requested !== null) {
            limit = requested;
        }
        const requestedCursor = page.cursor;
        if (requestedCursor !== null) {
            cursor = requestedCursor;
        }
    }
    let query = "?limit=" + limit.toString();
    if (cursor !== null) {
        query = query + "&cursor=" + encodeComponent(cursor);
    }
    return query;
}

// Internal: no check here — this is not an operation the agent can call, and
// the operations that use it run their own check with the resolved owner.
function fetchOrder(orderId: string): Order | null {
    const response = request("GET", "/orders/" + encodeComponent(orderId), null);
    if (response.status === 404) {
        return null;
    }
    return JSON.parse(response.body) as Order;
}

function request(method: string, path: string, body: {} | null): Response {
    const token = secrets.get("ORDERS_API_TOKEN");
    if (token === null) {
        throw new Error("ORDERS_API_TOKEN is not configured for this blueprint");
    }
    const headers = new Map<string, string>();
    headers.set("Authorization", "Bearer " + token);
    headers.set("Accept", "application/json");
    const response = method === "GET" ? get(BASE + path, headers) : post(BASE + path, body, headers);
    if (response.status === 404) {
        return response;
    }
    if (!response.ok) {
        throw new Error("orders API failed: HTTP " + response.status.toString() + " " + response.statusText);
    }
    return response;
}
```

`build check` derives from this: `provides` with `orders.list { customerId }`
and `orders.cancel { customerId, orderId, totalCents }`, and `requires` of
`http.get` and `http.post` filtered to `host == "api.acme.com"` plus
`secrets.get` filtered to `name == "ORDERS_API_TOKEN"`. If the URL is built
through an intermediate variable or a parameter carries the host, the compiler
warns that it cannot resolve the host and the derived filter loses it. Keep
the host in a top-level constant and pass the concatenation directly.

Runtime rules that shape package code:

- `submilli:http` verbs are synchronous and return a `Response` with `ok`,
  `status`, `statusText`, `headers`, `body` (UTF-8 text), and `url`. Transport
  failures throw. Object bodies are JSON-encoded with `application/json`.
  `Headers` is `Map<string, string>`.
- Parse bodies with `JSON.parse(text) as T` where `T` is a data-only
  interface. Model absence as `T | null`; there is no `undefined`. Optional
  input fields (`reason?: string`) read as `null` when absent and are omitted
  from the request when unset, so an update touches only fields the caller set.
- Use `submilli:url` for `encodeComponent`, `encodeQuery`, and `parse`. Take a
  capability's `host` field from `parse(url).host`: it is lower-case with no
  trailing dot, the spelling `http.*` rules see.
- No npm imports, `async`/`await`, `any`, `process.env`, `fetch`, or `Date`
  (use `Temporal`). Explicit return types on every function.
- Pagination: return a page type with items and a cursor or token, accept a
  page options bag, and always send an explicit page size. Never loop
  unbounded inside the package.
- Errors: throw `Error` with the service's actionable detail (status, error
  code, offending field) rather than a generic message. Return `null` for a
  missing single resource.
- GraphQL services follow the same shape with one `post` helper; see
  `packages/linear` in the Submilli runtime repository for a complete client.

## Credentials

Three routes, in order of preference:

1. **Package reads the secret** with `secrets.get("NAME")` using a literal
   name, so the derived `secrets.get` filter is static. The blueprint declares
   `NAME` under `secrets:` with an `env`, `file`, `store`, or `harness` source.
   Generated code can never call `secrets.get`; no policy can grant it.
2. **Secret name as an argument** when one package serves several accounts:
   `sendMail(secretName, input)`. The package still resolves the value and
   never returns it.
3. **`auth_proxy`** when generated code is deliberately allowed direct
   `submilli:http` access to a host: the server injects the header for that
   host. This is a blueprint feature, not package code.

Never export an operation that returns a token, accept a caller-supplied
destination URL that will carry the credential, or log headers. Provision
values with `submilli secret put` or the server's secret commands, never by
pasting them into chat or source control.

## Model-facing documentation

`docs/readme.md` is what the agent reads through the `packages.docs` tool. Its
`ts`/`typescript` fences are compiled against the package by `build test`, so
examples cannot rot; use `ts ignore` for illustrative fragments. Cover: what
the package is for, every operation grouped by read and write, argument and
return types, pagination and how to request the next page, error behavior and
`null` returns, which fields are policy-visible and that a denial means the
operation is forbidden rather than mis-called, and that credentials are
supplied internally so no key is ever passed. End with one complete `main`
example that returns a compact result. Write it for an agent choosing calls,
not for a human onboarding.

## Tests

Tests import from the package name, define `function main(): void`, and use
the global `assert(condition, message)`. `submilli:test` adds `label(text)`
and `expectException(fn, errorType)`. `errorType` is the error's name as a
string, not a class: passing `Error` or `RangeError` fails to compile with
"`Error` is a class, not a value". Omit it, or pass `""`, to accept any thrown
error. The call returns the caught error and fails the test if `fn` returns
normally:

```typescript
import { label, expectException } from "submilli:test";
import { pageQuery } from "@acme/orders";
function main(): void {
  label("rejects a limit above 100");
  const error = expectException(() => { pageQuery(101, null); }, "RangeError");
  assert(error.message.includes("limit"), "message names the argument");
}
```

- Unit-test pure builders (query strings, filter variables, error rendering)
  without network or secrets. Export them for that purpose.
- Gate live tests on the secret: `if (secrets.get("NAME") === null) return;`.
  `build test` bridges `NAME` from the environment or a `.env` in the manifest
  directory to `secrets.get`; an unset name reads as `null`. Live tests should
  be read-only unless a disposable target is configured by an explicit
  variable.
- Test that a write operation sends only the fields set, that a missing
  resource yields `null`, and that a service error surfaces its detail.

Package tests run with the package's own identity and an unrestricted policy.
They do not prove that a `main` caller is constrained; the allowed and denied
matrix in [blueprints](blueprints.md) does that.

## Publishing

`build publish-local` installs into the local store for a local server. A
remote server installs from GitHub with `submilli server packages install`, and
a developer machine with `submilli install org/repo[@ref] [@scope/name]`, pinned
to a commit. Local publication uploads nothing anywhere. Maintained examples
live under `packages/` in https://github.com/submilli/submilli-runtime:
`linear` (GraphQL), `github` (REST with page tokens), and `slack-bot`.
