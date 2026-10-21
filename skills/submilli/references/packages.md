# Build a package

A package is the only way generated code reaches a service. It is a small,
reviewed TypeScript-subset library that exposes narrow typed operations, runs
a `check(...)` before every protected effect, and owns the credential. Design
the operations and their capability fields first using
[capability design](capability-design.md); this reference covers building.

## Layout and commands

```sh
submilli build init @pied-piper/billing packages/billing   # first package: writes submilli.toml
submilli build new  @pied-piper/billing packages/billing   # add to an existing submilli.toml
submilli build check                                 # compile, derive capabilities.yaml
submilli build test [-p @pied-piper/billing] [--skip-network] # run tests + compile docs examples
submilli build publish-local [-p @pied-piper/billing]      # install into the local store
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

## Package build configuration

Use `<package>/src/config.ts` for deployment choices fixed at installation, such
as a self-hosted service URL. Import it with ordinary relative imports:

```typescript
// src/config.ts
export declare const baseUrl: string;
export const timeoutMs = 5000;
```

`export declare const` requires a build input. Exported `const` primitive
literals provide defaults; optional primitive annotations must match. Only
strings, finite numbers and booleans are supported. This root file is reserved
for these declarations and comments; imports, functions and computed defaults
are refused. There is no TOML configuration schema or special module import.

Supply repeatable `--config name=value` with an explicitly selected package:
`submilli build test -p @pied-piper/search --config baseUrl=https://search.test`,
`submilli install OWNER/REPOSITORY @OWNER/search --config baseUrl=https://search.internal`,
or `submilli server packages install` with the same repository, package, and
flags. Install examples use placeholders: replace `OWNER/REPOSITORY` and
`@OWNER/search` with the real repository and declared Package; the scope must
match its GitHub owner. Build check/publish-local/authority-map/security-review also accept inputs.
Local builds use flags and source defaults, not the selected package's installed
overrides. Credentials remain secrets; session identity remains a variable.

Installs preserve explicit overrides across upgrades while adopting new source
defaults. Changing installed source or configuration requires `--upgrade`.
Install commands accept `--reset-config name` to remove an explicit override.
Configuration values are inspectable in artifacts and never grant permissions.

Inputs apply only to the selected package. If a dependency needs a required value,
use the error's pinned local/server command to install that dependency first,
then retry the original installation. Never forward parent inputs into dependencies
or invent endpoint values. Dependencies with defaults remain automatic.
Playground rebuilds retain overrides previously supplied by publish-local.

## The smallest slice

```typescript
import { check } from "submilli:security";

/**
 * Return a customer's fixture balance in cents.
 * @param customerId The customer's id.
 * @returns The balance, in cents.
 * @capability pied-piper.example/balance.read { customerId: string }
 */
export function readBalance(customerId: string): number {
    check("pied-piper.example/balance.read", { customerId });
    return 6150;
}
```

Use a fixture like this to prove the policy path end to end before touching a
real service. [blueprints](blueprints.md) grants and verifies it.

## Capability annotations and `check`

The `@capability` tag declares the operation's name and the policy-visible
fields; `check` enforces it at runtime. The annotation alone enforces nothing.

```text
@capability pied-piper.example/orders.cancel { customerId: string, orderId: $orderId, totalCents: number }
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
- A module `let`, the package's own or one it imports, or a writable static
  field: read it once into a `const`, even to compare it. The caller's code,
  such as a getter, can call a function that reassigns it between two reads.

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
 * @param input The channel and the text to post.
 * @capability pied-piper.example/messages.post { channelId: $input.channelId }
 */
export function postMessage(input: MessageInput): void {
    const { channelId, text } = input;
    check("pied-piper.example/messages.post", { channelId: channelId });
    send(channelId, text);
}

/**
 * Post one message to several channels. Array of strings: one `for...of` into
 * the package's own array.
 * @param channelIds The channels to post to.
 * @param text The message body.
 * @capability pied-piper.example/messages.broadcast { channelIds }
 */
export function broadcast(channelIds: string[], text: string): void {
    const targets: string[] = [];
    for (const channelId of channelIds) {
        targets.push(channelId);
    }
    check("pied-piper.example/messages.broadcast", { channelIds: targets });
    for (const channelId of targets) {
        send(channelId, text);
    }
}

/**
 * Post a message with custom fields. `fields` never reaches the check, so it
 * needs no copy.
 * @param channelId The destination channel.
 * @param fields Name and value pairs, posted one per line.
 * @capability pied-piper.example/messages.post { channelId }
 */
export function postFields(channelId: string, fields: Map<string, string>): void {
    check("pied-piper.example/messages.post", { channelId: channelId });
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
check("pied-piper.example/messages.post", { channelId: input.channelId });
send(input.channelId, input.text);

// A condition picks which check runs, and a second read picks the operation.
if (input.kind === "reply") check("pied-piper.example/messages.reply", { threadId });
else check("pied-piper.example/messages.post", { channelId });
if (input.kind === "reply") reply(threadId, text); else post(channelId, text);

// The decision goes through a flag, and the request reads the value again.
let notify = false;
if (input.mentions !== undefined) notify = true;
if (notify) check("pied-piper.example/mentions.notify", {});
sendMentions(input.mentions);  // undefined on the first read, a list now: unchecked

// The object holding a checked field is handed to a helper that reads it.
check("pied-piper.example/messages.post", { channelId: input.channelId });
request(input);

// A checked array is iterated once to check and again to send.
for (const id of input.channelIds) check("pied-piper.example/messages.post", { channelId: id });
for (const id of input.channelIds) send(id, input.text);
```

## A real service

The pattern below wraps a REST API. It reads the token by literal secret name
inside the package, builds URLs from a top-level constant so the compiler can
derive `host == "api.pied-piper.example"` into `requires`, validates input
before any check or request so tests reach it offline, and resolves ownership
before a write.

```typescript
// Orders for one customer, wrapping the internal REST API at api.pied-piper.example.
// The API token is read from the ORDERS_API_TOKEN secret inside this package;
// generated code never sees it.

import { get, post, Response } from "submilli:http";
import { encodeComponent } from "submilli:url";
import secrets from "submilli:secrets";
import { check } from "submilli:security";

const BASE = "https://api.pied-piper.example/v1";

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
 * @capability pied-piper.example/orders.list { customerId }
 */
export function listOrders(customerId: string, page?: PageOptions): Page<Order> {
    const query = pageQuery(page?.limit, page?.cursor);
    check("pied-piper.example/orders.list", { customerId: customerId });
    const path = "/customers/" + encodeComponent(customerId) + "/orders" + query;
    const response = request("GET", path, null);
    return JSON.parse(response.body) as Page<Order>;
}

/**
 * Cancel an open order. The order's owner is resolved from the service, so the
 * policy filter sees the real customer even though the caller passes only an id.
 * @capability pied-piper.example/orders.cancel { customerId: string, orderId: $orderId, totalCents: number }
 */
export function cancelOrder(orderId: string, input?: CancelInput): Order {
    const reason = input?.reason;
    const order = fetchOrder(orderId);
    if (order === null) {
        throw new Error("order not found: " + orderId);
    }
    check("pied-piper.example/orders.cancel", {
        customerId: order.customerId,
        orderId: orderId,
        totalCents: order.totalCents,
    });
    const body: CancelInput = reason === undefined ? {} : { reason: reason };
    const response = request("POST", "/orders/" + encodeComponent(orderId) + "/cancel", body);
    return JSON.parse(response.body) as Order;
}

function pageQuery(requested: number | undefined, cursor: string | undefined): string {
    const limit = requested === undefined ? 50 : requested;
    if (!(limit >= 1 && limit <= 100)) {
        throw new RangeError("limit must be between 1 and 100, got " + limit.toString());
    }
    let query = "?limit=" + limit.toString();
    if (cursor !== undefined) {
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
    if (token === undefined) {
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
`http.get` and `http.post` filtered to `host == "api.pied-piper.example"` plus
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
  interface. Optional fields (`reason?: string`) read as `undefined` when
  absent; `null` is a distinct, explicit value, and a cast rejects it for
  `reason?: string`. Type a field the API may send as null `reason?: string |
  null`. Unset fields are omitted from the request, so an update touches only
  fields the caller set.
- Use `submilli:url` for `encodeComponent`, `encodeQuery`, and `parse`. Take a
  capability's `host` field from `parse(url).host`: it is lower-case with no
  trailing dot, the spelling `http.*` rules see.
- `encodeComponent` does not validate. `submilli:http` and `build` throw
  `TypeError` for a path with a `.` or `..` segment, so validate an id you put
  in a path for a clearer error, not for safety.
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
   `NAME` under `secrets:` with a `store` or `harness` source.
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

A package has two readmes. The root `README.md` is for the people who install
and grant it: install and secret commands, blueprint YAML, which capabilities
to grant and how to filter them. `docs/readme.md` is for the agent writing a
program, which can run none of that and cannot change its blueprint, so keep
setup out of it. A capability name belongs there only where it explains a
denial the agent will see.

`docs/readme.md` is what the agent reads through the `packages.docs` tool. Its
`ts`/`typescript` fences are compiled against the package by `build test`, so
examples cannot rot; use `ts ignore` for illustrative fragments. Cover: what
the package is for, each operation's behavior in its own region, argument and
return types, pagination and how to request the next page, error behavior and
`null` returns, that a denial means the operation is forbidden rather than
mis-called, and that credentials are supplied internally so no key is ever
passed. End with one complete `main` example that returns a compact result,
inside the region of the operations it calls.

An agent reads only what its blueprint could let it call. Wrap each
operation's section in a capability region, matching the `@capability` tags of
the functions it describes:

```markdown
<!-- submilli:if capability="pied-piper.example/orders.cancel" -->
## Cancel an order
...
<!-- submilli:endif -->
```

`all="a b"` needs every name and `any="a b"` at least one; a function with
several tags needs `all`. Directive lines stand alone, unindented, outside
fenced code, and may sit between list items or table rows. Do not list every
operation in prose outside regions: make it a list with one region per item,
or leave the listing to the declarations, which agents already get narrowed.
The build rejects malformed directives and names nothing provides.

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
import { listOrders } from "@pied-piper/orders";
function main(): void {
  label("rejects a limit above 100 before any request");
  const error = expectException(() => { listOrders("pied_piper", { limit: 101 }); }, "RangeError");
  assert(error.message.includes("limit"), "message names the argument");
}
```

- Export only the operations an agent calls: an exported helper is shown to
  every agent, whatever its blueprint allows. Test input validation (limits,
  required fields, formats) through those operations: it runs before `check()`
  and `secrets.get`, so it needs no network or secret.
- Tests receive no credentials by default. Use `--env-var NAME` (repeatable or
  comma-separated), `--env-file PATH` (relative to the current directory), or
  `--all-env`. Precedence is `--env-var` > `--env-file` > `--all-env`, regardless
  of flag order. Missing selected process variables and unreadable files fail
  the run. `.env` is read only when explicitly named.
- Package tests, including their `main` function, run with the package's
  identity and may read supplied secrets. Ordinary script callers remain
  restricted. A missing-credential early return is reported `ok`; use
  `--skip-network` when you want files explicitly reported as skipped.
  Live tests should be read-only unless a disposable target is configured.
- Use `--skip-network` to skip `network.test.{ts,subm}` and
  `network_*.test.{ts,subm}` anywhere under `tests/`. Selection uses filenames,
  not detection of network calls. Other tests and readme examples still run.
  `SUBMILLI_SKIP_HTTP_TESTS` has no effect on package-test selection.
- In a CI job with an injected secret, run
  `submilli build test --deny-warnings -p @pied-piper/billing --env-var BILLING_API_KEY`.
- In tests gated on the secret, check that a write operation sends only the
  fields set, that a missing resource yields `null`, and that a service error
  surfaces its detail.

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

## Session-owned remote resources

When a package creates a remote resource that must survive individual executions,
register its cleanup internally before returning the handle. Use
`submilli:session.registerFinalizer(key, value)` and export the fixed entry point
`finalizer(key: string, value: T): void` from the package root. The engine owns
package namespacing: supply only the local resource key. Two packages can use
the same key without conflict; the finalizer receives the original local key.
A duplicate live key in the same package/session is rejected.

```typescript
import * as session from "submilli:session";
import * as http from "submilli:http";

export interface ResourceHandle { id: string; url: string; }

export function finalizer(key: string, value: ResourceHandle): void {
    const response = http.delete(value.url);
    if (response.status !== 204 && response.status !== 404) {
        throw new Error("Resource cleanup failed");
    }
}
```

After creating the resource, call `session.registerFinalizer(handle.id, handle)`.
Recorded test runs (including live-call modes) refuse registration with a catchable
error; release newly created resources through the same compensation path.
Registration commits serializable arguments and package/dependency versions
before returning. Code stays in the installed package store; upgrades, removals,
or unavailable versions may lose cleanup. If a registration refusal is catchable, attempt immediate
remote cleanup and rethrow the original error. Storage failures with an uncertain
commit terminate execution; a remote lease/idle timeout must cover that uncertainty
and a crash between creation and registration.

For explicit early release, close the remote resource first, then call
`session.unregisterFinalizer(handle.id)`. Missing registrations are harmless.
Make remote closure idempotent: recovery can repeat an attempt interrupted after
the remote effect but before success was recorded.

Build derives `session.finalize` requirements, filtered by local key. Cleanup
runs under current policy and credentials, with independent execution budgets;
revoked access may make it fail. Session close waits for every attempt.
Thrown errors and traps are terminal and are never automatically retried;
restart recovery attempts unfinished work. Inspect
`GET /v1/sessions/{id}/finalizers` for sanitized status.

Finalizers cannot register/unregister other finalizers. Treat them as resource
lifecycle hooks, not background jobs. Removing/replacing a session KV value does
not run cleanup. Store a serializable handle in KV to reuse the resource across
executions; registration does not checkpoint remote process memory.


Call `registerFinalizer` directly (namespace and named imports are supported)
so the compiler can validate its value against the package finalizer signature.
The REST session-close response includes sanitized finalizer outcomes when
registrations exist; sessions without registrations retain the empty 204 response.
Embedders can use `end_session_with_finalizers` for the same report; the existing
`end_session` boolean API remains available.
