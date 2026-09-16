# Build a package

Use `submilli build init @acme/billing packages/billing` for a first package;
use `submilli build new @acme/billing packages/billing` at an existing manifest
root. Inspect the scaffold instead of inventing a manifest format.

Package source is `src/lib.ts`; tests are `tests/*.test.ts`; model-facing
documentation belongs in `docs/readme.md`. The compiler produces declarations
and a `capabilities.yaml` schema. Keep docs usable by an agent: input/output
types, intended operation, errors, credential names, pagination, and examples.

This minimal capability is useful for the first offline slice:

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

The annotation alone does not enforce policy. Call `check` before protected
reads, credential access, or writes, with fields that match the operation
actually performed. Validate and normalize arguments consistently; a checked
customer ID must not be replaced with another one in the outbound URL. If
authorization needs a resource's owner, resolve and verify that relationship
in trusted code rather than accepting a caller's assertion of ownership.

For real services, inspect `submilli docs submilli:http`,
`submilli docs submilli:url`, and the relevant built-ins before coding. Use
Submilli's synchronous APIs and explicit return types. No npm/Python SDKs,
Node `process.env`, browser `fetch`, `async`/`await`, `any`, or `undefined`
inside the package. Use concrete types, `unknown` with narrowing, and `null`.
The host application can still use its normal SDKs and async code.

Use credentials through package-only secret access or the blueprint's auth
proxy; do not expose a secret-returning operation, log tokens, or accept
arbitrary destinations carrying credentials. Keep business capability grants
under `main` and package HTTP/secret grants under the package caller. Read
generated `requires` and capabilities before writing those rules. Secrets
should be provisioned through CLI/server secret commands using their help,
outside source control. Do not ask the user to paste values into chat.

Test input validation, success, service failures, pagination, and write
boundaries relevant to the operation. Submilli tests use `function main(): void`
and `assert(...)`; consult `submilli docs submilli:test` for supported helpers.

```sh
submilli build check
submilli build test
submilli build publish-local
```

These are not enough to establish a policy boundary: also run the caller-level
allowed and denied tests in [blueprints](blueprints.md). Package tests may run
with the package identity and do not prove a `main` caller is constrained.

The local store is for local execution; remote servers need their own installed
packages. For GitHub delivery, inspect `submilli server packages install --help`
and pin the source as supported by the installed CLI. Do not assume local
publication uploaded anything to production.
