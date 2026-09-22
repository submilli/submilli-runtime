# Design capabilities, packages, and blueprints

This is the procedure for turning an application and its service APIs into a
package vocabulary and a set of policies. Run it after
[discovery](discovery.md) has established the agent's job and authority, and
before writing code with [packages](packages.md) and
[blueprints](blueprints.md). The output is a table the developer can review.

## 1. Inventory what exists

From the code, list every service the agent would touch, the client or SDK
that reaches it, the credential it uses, and where the application already
authenticates the user and derives tenant or role. Note every current agent
tool or direct client call: each one is a route around the policy until it is
removed or wrapped.

## 2. Name the business operations

Write the operations as verbs a support engineer would recognize:
`listOrders(customerId)`, `cancelOrder(orderId)`, `postMessage(channelId,
text)`. Include only what the agreed job needs; an available endpoint is not a
reason to expose it. Split by intent, not by HTTP route: a `GET` that returns
another tenant's data under some parameters is two operations or one with an
ownership check.

Reject passthroughs. `httpGet(url)`, `runSql(query)`, `callTool(name, args)`,
and `graphql(query)` hide the very arguments a filter needs. Wrap the specific
queries instead.

## 3. Choose the capability fields

For each operation, ask: which arguments must the operator be able to
constrain? Those become `check` fields and appear in `@capability`. Typical
fields: the identity scope (`customerId`, `tenantId`, `repo`, `channelId`),
the amount or count, the destination, the status transition, the time range.
Leave out free text and internal ids that carry no authority.

Field selection decides what policy can ever express. If a filter must limit
refunds by amount, `amount` has to be in the check. If it must limit an update
to the bound customer, the update's check needs `customerId` even when the
caller passes only a record id.

Fields must be **truthful**: the value in the check is the value the operation
uses. Normalize first, check the normalized value, and use it in the request.

## 4. Resolve ownership inside the package

When a filter needs a fact the caller does not supply, fetch it in trusted
package code and check the fetched value: a cancel resolves the order to find
its customer; a message edit resolves the message to find its channel and
author. Never accept an ownership claim as an argument. If a service cannot
tell you the owner, the operation cannot be scoped by owner and should be
limited another way or excluded from customer-facing roles.

## 5. Group into packages

One package per service or cohesive domain with a shared credential and a
shared capability namespace (`acme.com/...`). Separate packages when
credentials, owning teams, or review boundaries differ. Reuse a maintained
package when one exists and its capabilities carry the fields you need;
otherwise wrap the gap in a small package of your own rather than editing the
maintained one. An existing MCP server can be bridged through the blueprint's
`mcp` block with a `tool` filter, but its arguments are opaque to policy, so
wrap it when argument-level rules matter.

## 6. Group into blueprints by role

List the distinct combinations of who acts, on whose behalf, and with what
authority. Each becomes a blueprint. Within one, allow the business
capabilities the role needs, filtered by the bound identity variable, and let
`add-package` write the package's infrastructure grants. Mark consequential
writes `ask-human` only where the harness implements approval; otherwise keep
them out of the first slice or expose a draft operation.

## 7. Produce the mapping

Deliver this before implementing, with file and function citations:

| Workflow | Existing code | Package operation and check fields | Blueprint, caller rules, bound variables | Verification |
| --- | --- | --- | --- | --- |
| Support balance lookup | `billing.ts readBalance`, identity from `auth.ts requireSession` | `@acme/billing` `readBalance(customerId)` checks `{ customerId }` | `support-read`: main allows `balance.read` where `customerId == ${vars.customerId}`; `customerId` required | own customer allowed; other customer denied; missing binding rejected |
| Small refunds | `billing.ts refund(chargeId, amountCents)` | `refund(chargeId, amountCents)` resolves the charge's customer, checks `{ customerId, chargeId, amountCents }` | `support-refund`: `ask-human` where `customerId == ${vars.customerId} and amountCents <= 5000`; package gets `http.post` to the billing host and its secret | denied over limit; denied for another customer; no side effect on denial |
| Finance export | `billing.ts exportCustomers` | `exportCustomers()` checks `{}` | `finance-report`: no customer variable; selected by the app only for the finance role | support blueprint cannot call it |

Unresolved authority questions go under the table, not into the grants.

## Anti-patterns

- A filter mentioned in the prompt but absent from the package's check.
- One capability for a whole package, so reads and writes cannot be separated.
- A blueprint per customer instead of one parameterized by a required variable.
- The model choosing the blueprint or the identity variable.
- `default: allow` or a wildcard host to make a denial go away.
- `main` granted raw HTTP to a host a package already wraps.
- An exported helper that returns a credential or accepts an arbitrary URL.
- Package unit tests offered as proof that generated code is constrained.
