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
owner.

Call `check` directly in the body of a function the package root
(`src/lib.ts`) exports: not in a helper, a function of another module that the
root does not re-export, or a nested function. Helpers take values the
exported function already checked.

### Read caller values once

An object, array or `Map` argument belongs to the caller. `main` can pass a
class instance whose getter returns a different value on each read, so a
function that reads `input.channelId` for `check` and again for the request
can check one value and send another.

This matters only for values that reach a `check`: those in its context,
directly or through a `const` or a helper that computes it, and those that
decide whether or which `check` runs. Every other value may be read and passed
on freely, because the caller could pass any value in its place anyway. When
the compiler cannot follow what reaches a `check`, it examines every caller
value in that function, and each warning's note names the reason: restructure
as the note suggests, or read everything once. Typical reasons: the check
depends on `this` or a module `let`; the function writes `this`, a module
variable or the caller's object; a container that feeds the check is handed
to a helper together with a caller object, captured by a nested function, or
aliased; the function calls itself. For a value that does reach a `check`:

- Read each checked property once into a `const`, and use that `const` for the
  check and the request. For a nested object, read the parent once, then each
  of its properties once.
- Do not pass on the object that holds it, to a helper or as the `check`
  context. Pass the `const`s, and pass the unchecked parts of the input
  separately. Comparing, one `for...of`, and one read of an element are fine.
- A checked array: copy it with one `for...of` into an array the package owns.

```ts
import { check } from "submilli:security";

/** A message for one channel. */
export interface MessageInput {
    /** Destination channel. */
    channelId: string;
    /** Message body. */
    text: string;
}

/**
 * Post one message. Flat input: destructure once, pass the consts on.
 * @capability acme.com/messages.post { channelId: $input.channelId }
 */
export function postMessage(input: MessageInput): void {
    const { channelId, text } = input;
    check("acme.com/messages.post", { channelId: channelId });
    send(channelId, text);
}

/**
 * Post one message to several channels. Array of strings: one `for...of` into
 * the package's own array.
 * @capability acme.com/messages.broadcast { channelIds }
 */
export function broadcast(channelIds: string[], text: string): void {
    const targets: string[] = [];
    for (const channelId of channelIds) {
        targets.push(channelId);
    }
    check("acme.com/messages.broadcast", { channelIds: targets });
    for (const channelId of targets) {
        send(channelId, text);
    }
}

/**
 * Post a message with custom fields. `fields` never reaches the check, so it
 * needs no copy.
 * @capability acme.com/messages.post { channelId }
 */
export function postFields(channelId: string, fields: Map<string, string>): void {
    check("acme.com/messages.post", { channelId: channelId });
    for (const [name, value] of fields) {
        send(channelId, name + ": " + value);
    }
}

function send(channelId: string, text: string): void {
    // The request, built from checked values only.
}
```

`build check` reports violations as warnings and still succeeds, so read its
output. The common ones:

| Warning | Fix |
| --- | --- |
| `` `check()` is called in `send`, which is not part of the package's public API `` | Move the `check` into each exported function that reaches `send` |
| `` `input.channelId` is read more than once in `postMessage`, which calls `check()` `` | Read it once into a `const` and use the `const` in the check and the request |
| `` caller-supplied `input` is passed to `request` in `cancelOrder`, which calls `check()` `` | Pass the consts read from it, and the unchecked parts of `input` separately |

The last form names what was done with the value: passed to a function or to
`check()` as its context, called, had a method called on it, used as an
operand, stored, spread, returned, thrown, written to, or captured by a nested
function. The fix is the same for each.

Mistakes the warnings are about, each a way to check one value and use
another. The fix for each is a single read: into a `const`, or for an array,
one `for...of` into the package's own array:

```ts
// Read for the check, read again for the request.
check("acme.com/messages.post", { channelId: input.channelId });
send(input.channelId, input.text);

// A condition picks which check runs, and a second read picks the operation.
if (input.kind === "reply") check("acme.com/messages.reply", { threadId });
else check("acme.com/messages.post", { channelId });
if (input.kind === "reply") reply(threadId, text); else post(channelId, text);

// The decision goes through a flag, and the request reads the value again.
let notify = false;
if (input.mentions !== null) notify = true;
if (notify) check("acme.com/mentions.notify", {});
sendMentions(input.mentions);  // null on the first read, a list now: unchecked

// The object holding a checked field is handed to a helper that reads it.
check("acme.com/messages.post", { channelId: input.channelId });
request(input);

// A checked array is iterated once to check and again to send.
for (const id of input.channelIds) check("acme.com/messages.post", { channelId: id });
for (const id of input.channelIds) send(id, input.text);
```

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
    const limit = page === null ? null : page.limit;
    const cursor = page === null ? null : page.cursor;
    const query = pageQuery(limit, cursor);
    check("acme.com/orders.list", { customerId: customerId });
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
    const reason = input === null ? null : input.reason;
    const order = fetchOrder(orderId);
    if (order === null) {
        throw new Error("order not found: " + orderId);
    }
    check("acme.com/orders.cancel", {
        customerId: order.customerId,
        orderId: orderId,
        totalCents: order.totalCents,
    });
    const body: CancelInput = reason === null ? {} : { reason: reason };
    const response = request("POST", "/orders/" + encodeComponent(orderId) + "/cancel", body);
    return JSON.parse(response.body) as Order;
}

/** Build the `?limit=&cursor=` suffix; exported so tests cover it without a token. */
export function buildPageQuery(page: PageOptions | null): string {
    if (page === null) {
        return pageQuery(null, null);
    }
    return pageQuery(page.limit, page.cursor);
}

function pageQuery(requested: number | null, cursor: string | null): string {
    const limit = requested === null ? 50 : requested;
    if (!(limit >= 1 && limit <= 100)) {
        throw new RangeError("limit must be between 1 and 100, got " + limit.toString());
    }
    let query = "?limit=" + limit.toString();
    if (cursor !== null) {
        query = query + "&cursor=" + encodeComponent(cursor);
    }
    return query;
}

// Internal: no check here. `check` belongs in the exported operation, which
// checks the owner this lookup resolved before it writes.
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
import { buildPageQuery } from "@acme/orders";
function main(): void {
  label("defaults to a page of 50");
  assert(buildPageQuery(null) === "?limit=50", "default page size");
  label("rejects a limit above 100");
  const error = expectException(() => { buildPageQuery({ limit: 101 }); }, "RangeError");
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
