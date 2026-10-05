---
title: "Export a function"
description: "How to export a function a Blueprint can allow, filter, or deny: declare it with a @capability tag, enforce it with check, design the payload, call the service with a key the program never sees, and build it."
slug: packages/export-a-function
sidebar:
  order: 2
authorship:
  label: ai-assisted
  confirmed: true
  contentHash: "ff885f87e7cd970871899431711b0fcba96e34a2ce860cde373b35be918b8470"
  confirmedAt: "2026-10-05T10:59:51.488Z"
---

The agent's program is going to call a function of yours, and the program
was written by a model that reads untrusted text. The function is safe to
hand it because it asks the Blueprint before it acts, with the facts a
rule can test, such as which customer, how much, and what kind of
account. In the Package, that is two lines:

```typescript
/** @capability acme.com/credits.apply { customerId: string, customerClass: string, amount: number } */
export function applyCredit(customerId: string, amount: number): Credit {
    …
    check("acme.com/credits.apply", { customerId: id, customerClass, amount });
```

The first, in the doc comment, names the capability a Blueprint rule
refers to and the facts it reports. The second asks the Blueprint with
those facts, before anything runs. The facts you choose decide what a
policy can ever be precise about.

This guide shows you how to export a function that a Blueprint can
allow, filter, or deny, called an **operation**. The example is
`applyCredit` in Acme's billing Package, which runs on Stripe in test
mode. Substitute your operation and your service.

## Declare and enforce it

Replace the scaffold's `src/lib.ts`. The `@capability` tag in the doc
comment **declares** the operation by giving it a name and the fields a
rule may test. The `check` call, from `submilli:security`, **enforces**
it. It asks the Blueprint whether the caller may do this with these
values, and throws `PermissionDeniedError` if not. Until the service call is written,
the function returns the credit it was asked for:

```typescript title="packages/billing/src/lib.ts"
import { check } from "submilli:security";

/** A credit applied to a customer's account. */
export interface Credit {
    /** The customer credited. */
    customerId: string;
    /** Amount in cents. */
    amount: number;
}

/**
 * Add a goodwill credit to a customer's account.
 * @param customerId The customer's id in the billing system, such as `cus_northwind`.
 * @param amount The credit, in cents; must be positive.
 * @returns The credit as recorded.
 * @capability acme.com/credits.apply { customerId: string, amount: number }
 */
export function applyCredit(customerId: string, amount: number): Credit {
    if (amount <= 0) {
        throw new RangeError("amount must be positive");
    }
    check("acme.com/credits.apply", { customerId, amount });

    // The call to the billing service comes later on this page.
    return { customerId, amount };
}
```

```sh
submilli build check
```

```text
checked @acme/billing v0.1.0
```

:::tip[The compiler checks the tag against the check]
`submilli build check` compares the `@capability` tag with the `check`
call in its function. A field the `check` sends that the tag doesn't
declare is an error, and the build stops. With `amount` left out of the
tag, it prints:

```text
error: payload key `amount` missing from `@capability` binding
  --> packages/billing/src/lib.ts:22:37
   |
21 |     }
22 |     check("acme.com/credits.apply", { customerId, amount });
   |                                     ^^^^^^^^^^^^^^^^^^^^^^
23 | 
   |
help: add `amount` to the matching `@capability` binding
```

The other disagreements are warnings. They are a tag field the `check`
doesn't send, a tag with no `check`, a `check` with no tag, or a tag that names
a parameter the function doesn't have.
:::

Name a capability `<domain>/<resource>.<verb>`, one per operation, so a
Blueprint can allow reading without allowing writing. The tag alone
enforces nothing. The `check` call does, by throwing. So call
`check` before the request, the write, or whatever else the operation
does, and that effect never happens when the Blueprint says no. A field
in the tag is written one of four ways:

| Form | Meaning |
| --- | --- |
| `customerId` | A parameter of that name, with the parameter's type |
| `orderId: $id`, `team: $input.teamId` | A parameter, or a path inside one, under another name |
| `amount: number`, `tags: string[]` | A value the Package computes, with its type |
| `kind: "order"` | A fixed value |

Call `check` directly in the body of the exported function. The compiler
warns about a `check` anywhere else. A function the Package doesn't
export runs only if some exported function happens to call it, and a
nested function may run later, more than once, or never:

```text
warning: `check()` is called inside a nested function in `applyCredit`
  --> packages/billing/src/lib.ts:22:33
   |
21 |     }
22 |     const guard = () => { check("acme.com/credits.apply", { customerId, amount }); };
   |                                 ^^^^^^^^^^^^^^^^^^^^^^^^
23 |     guard();
   |
help: Call `check()` directly in the body of `applyCredit`; a nested function may run later, repeatedly, or never
   |
note: the nested function starts here
  --> packages/billing/src/lib.ts:22:19
   |
21 |     }
22 |     const guard = () => { check("acme.com/credits.apply", { customerId, amount }); };
   |                   ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^
23 |     guard();
```

A warning doesn't stop the build. After it, `submilli build check` still
prints `checked @acme/billing v0.1.0`. Each of these warnings is a gap in
what a Blueprint can enforce, so make them fail in CI or before you
publish, with `--deny-warnings`, or set `SUBMILLI_DENY_WARNINGS=1` for a
CI job:

```sh
submilli build check --deny-warnings
```

```text
warning: `check()` is called inside a nested function in `applyCredit`
…
error: 1 warning(s) treated as errors (--deny-warnings)
```

## Design the payload

A rule never sees the request the Package sends to the service. It sees
the capability's name and the payload the Package passes to `check`, and
nothing else, so a fact that isn't in the payload can never appear in a
rule. Build the payload for the rules people will want to write, not from
the arguments the function happens to take. Ask what an operator would
want to limit, such as which customer, how much, or from which state to
which.

The field that matters most is the scope the application authorizes a
session for. Here that is the customer. In a multi-tenant system it is
the tenant, and in Linear the team. Put it in every
operation's payload, including operations that take only an id. An
operation that comments on an issue fetches the issue, reads its team,
and checks `{ teamId, issueId }`. Then one rule,
`teamId == ${vars.teamId}`, holds across every operation. An
operation that leaves the scope out is the way around it.

Add facts the caller didn't pass. `applyCredit` takes a customer and an
amount. Whether the customer is premium is a fact about the account, so
the Package looks it up and puts `customerClass` in the payload, and a
Blueprint can then allow credits for premium customers only. Don't take
such a fact from the caller, because the caller is the program you are
guarding against.

Normalize before the check. Check the value you will send, so the
rule sees what the service receives. If the program passes an
object, read each property you need once, into a `const`, before the
`check`, and use that `const` for the effect too. A property can return
a different value each time it is read, and the compiler warns about a
second read of any value that reaches a `check`.

For `applyCredit`, that is a trimmed id and the customer's class, in the
`check` and in the tag. The lookup stands in for the service call for
now:

```typescript title="packages/billing/src/lib.ts (fragment)"
/**
 * Add a goodwill credit to a customer's account.
 * @param customerId The customer's id in the billing system, such as `cus_northwind`.
 * @param amount The credit, in cents; must be positive.
 * @returns The credit as recorded.
 * @capability acme.com/credits.apply { customerId: string, customerClass: string, amount: number }
 */
export function applyCredit(customerId: string, amount: number): Credit {
    if (amount <= 0) {
        throw new RangeError("amount must be positive");
    }
    const id = customerId.trim();
    const customerClass = lookUpClass(id);
    check("acme.com/credits.apply", { customerId: id, customerClass, amount });

    // The call to the billing service comes later on this page.
    return { customerId: id, amount };
}

/**
 * Whether the customer is premium or standard. Read from the account later on this page.
 * @param customerId The customer's id, already trimmed.
 * @returns `"premium"` or `"standard"`.
 */
function lookUpClass(customerId: string): string {
    return "standard";
}
```

The new field goes in both places. If you add it to the `check` alone,
the build warns that `customerClass` is missing from the tag.

## Call the service

Now the request. Replace the placeholder return and the stand-in lookup,
and keep the credential inside the Package:

```typescript title="packages/billing/src/lib.ts (fragment)"
import secrets from "submilli:secrets";
import { get, post } from "submilli:http";
import { encodeComponent, encodeQuery } from "submilli:url";

const BASE = "https://api.stripe.com/v1";

export function applyCredit(customerId: string, amount: number): Credit {
    …
    check("acme.com/credits.apply", { customerId: id, customerClass, amount });

    const form = new Map<string, string>();
    form.set("amount", (-amount).toString());
    form.set("currency", "usd");
    form.set("description", "Goodwill credit");
    const response = post(BASE + customerPath(id) + "/balance_transactions", encodeQuery(form), requestHeaders(true));
    if (!response.ok) {
        throw new Error("billing API failed: HTTP " + response.status.toString());
    }
    const transaction = response.json() as BalanceTransaction;
    return { customerId: id, amount, balance: transaction.ending_balance };
}

/**
 * The path of one customer under the billing API.
 * @param customerId The customer's id, escaped into the path.
 * @returns The path, without the base URL.
 */
function customerPath(customerId: string): string {
    return "/customers/" + encodeComponent(customerId);
}

/**
 * Whether the customer is premium or standard, read from the account.
 * @param customerId The customer's id, already trimmed.
 * @returns `"premium"` or `"standard"`.
 */
function lookUpClass(customerId: string): string {
    const response = get(BASE + customerPath(customerId), requestHeaders(false));
    if (response.status === 404) {
        throw new Error("no such customer: " + customerId);
    }
    response.throwForStatus();
    const customer = response.json() as Customer;
    const customerClass = customer.metadata["class"];
    return customerClass === null ? "standard" : customerClass;
}

function requestHeaders(form: boolean): Map<string, string> {
    const key = secrets.get("BILLING_API_KEY");
    if (key === null) {
        throw new Error("BILLING_API_KEY is not configured for this blueprint");
    }
    const headers = new Map<string, string>();
    headers.set("Authorization", "Bearer " + key);
    if (form) {
        headers.set("Content-Type", "application/x-www-form-urlencoded");
    }
    return headers;
}
```

Three habits keep the Package's own grants narrow and the credential
inside it:

- Keep the host in a constant. The build reads the host out of `BASE`
  and writes `host == "api.stripe.com"` into what the Package requires. A
  host that arrives in a parameter can't be derived, and the build says
  the filter is lost:

  ```text
  warning: cannot statically resolve the host in the URL passed to `http.post`; no host capability filter was derived
  ```

- Read the secret by its literal name. `secrets.get("BILLING_API_KEY")`
  becomes `name == "BILLING_API_KEY"`. The Blueprint says where the value
  comes from, and the Package only names it. A program can't call
  `secrets.get` itself, whatever the Blueprint says, so the Package is the
  only place the value exists.
- Never return the credential. Don't export a function that returns
  the key, accept a destination that will carry it, or log the headers.

Give the service's shapes their own types, with only the fields the
Package reads, and return the Package's type, so the program never sees
the service's field names. `Credit` gains the balance Stripe reports:

```typescript title="packages/billing/src/lib.ts (fragment)"
/** A credit applied to a customer's account. */
export interface Credit {
    /** The customer credited. */
    customerId: string;
    /** Amount in cents. */
    amount: number;
    /** The customer's balance after the credit, in cents. Negative means Acme owes the customer. */
    balance: number;
}

/** The fields the package reads from the billing API's customer object. */
interface Customer {
    id: string;
    metadata: Record<string, string>;
}

/** The fields the package reads from the billing API's balance transaction. */
interface BalanceTransaction {
    id: string;
    ending_balance: number;
}
```

`submilli:http` and `submilli:url` are two modules of the standard
library. Files, Git, session state, and model calls are reached the same
way. Refer to the
[standard library reference](/docs/reference/standard-library) for
each module and what gates it.

## Build it

```sh
submilli build check
```

```text
checked @acme/billing v0.1.0
```

There is no warning, because the tag and the `check` agree, the `check`
is where it should be, and the host was derived. The build also generates `capabilities.yaml`
beside the source. It is derived from the tags and the calls, and
rewritten on every build, so don't edit it:

```yaml title="packages/billing/capabilities.yaml"
namespace: acme
provides:
- name: acme.com/credits.apply
  description: Add a goodwill credit to a customer's account.
  fields:
    amount:
      type: number
    customerClass:
      type: string
    customerId:
      type: string
requires:
- capability: http.get
  filter: host == "api.stripe.com"
- capability: http.post
  filter: host == "api.stripe.com"
- capability: secrets.get
  filter: name == "BILLING_API_KEY"
```

`provides` lists the capabilities a Blueprint grants to programs, and the
fields their rules may test. `requires` is what the Package itself needs.
A Blueprint reads this file once the Package is published.
[Publish a Package](/docs/packages/publish-a-package) adds it to one and
runs it.

## The whole file

```typescript title="packages/billing/src/lib.ts"
import { check } from "submilli:security";
import secrets from "submilli:secrets";
import { get, post } from "submilli:http";
import { encodeComponent, encodeQuery } from "submilli:url";

const BASE = "https://api.stripe.com/v1";

/** A credit applied to a customer's account. */
export interface Credit {
    /** The customer credited. */
    customerId: string;
    /** Amount in cents. */
    amount: number;
    /** The customer's balance after the credit, in cents. Negative means Acme owes the customer. */
    balance: number;
}

/**
 * Add a goodwill credit to a customer's account.
 * @param customerId The customer's id in the billing system, such as `cus_northwind`.
 * @param amount The credit, in cents; must be positive.
 * @returns The credit as recorded, with the customer's balance after it.
 * @capability acme.com/credits.apply { customerId: string, customerClass: string, amount: number }
 */
export function applyCredit(customerId: string, amount: number): Credit {
    if (amount <= 0) {
        throw new RangeError("amount must be positive");
    }
    const id = customerId.trim();
    const customerClass = lookUpClass(id);
    check("acme.com/credits.apply", { customerId: id, customerClass, amount });

    const form = new Map<string, string>();
    form.set("amount", (-amount).toString());
    form.set("currency", "usd");
    form.set("description", "Goodwill credit");
    const response = post(BASE + customerPath(id) + "/balance_transactions", encodeQuery(form), requestHeaders(true));
    if (!response.ok) {
        throw new Error("billing API failed: HTTP " + response.status.toString());
    }
    const transaction = response.json() as BalanceTransaction;
    return { customerId: id, amount, balance: transaction.ending_balance };
}

/**
 * The path of one customer under the billing API.
 * @param customerId The customer's id, escaped into the path.
 * @returns The path, without the base URL.
 */
function customerPath(customerId: string): string {
    return "/customers/" + encodeComponent(customerId);
}

/**
 * Whether the customer is premium or standard, read from the account.
 * @param customerId The customer's id, already trimmed.
 * @returns `"premium"` or `"standard"`.
 */
function lookUpClass(customerId: string): string {
    const response = get(BASE + customerPath(customerId), requestHeaders(false));
    if (response.status === 404) {
        throw new Error("no such customer: " + customerId);
    }
    response.throwForStatus();
    const customer = response.json() as Customer;
    const customerClass = customer.metadata["class"];
    return customerClass === null ? "standard" : customerClass;
}

function requestHeaders(form: boolean): Map<string, string> {
    const key = secrets.get("BILLING_API_KEY");
    if (key === null) {
        throw new Error("BILLING_API_KEY is not configured for this blueprint");
    }
    const headers = new Map<string, string>();
    headers.set("Authorization", "Bearer " + key);
    if (form) {
        headers.set("Content-Type", "application/x-www-form-urlencoded");
    }
    return headers;
}

/** The fields the package reads from the billing API's customer object. */
interface Customer {
    id: string;
    metadata: Record<string, string>;
}

/** The fields the package reads from the billing API's balance transaction. */
interface BalanceTransaction {
    id: string;
    ending_balance: number;
}
```
