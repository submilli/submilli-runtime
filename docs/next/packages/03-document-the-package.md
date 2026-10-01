---
title: "Document the package"
description: "How to document a package for its two readers: the doc comments and docs/readme.md the model reads, with examples the build compiles, and the readme the person who installs and grants it reads."
slug: next/packages/document-the-package
pagefind: false
sidebar:
  order: 3
  hidden: true
---

Two readers decide how to use the package, and neither reads its source.
The person who chooses, installs, and grants it, or their coding agent,
reads the package's readme. The model reads the declarations and
`docs/readme.md` through its documentation tool, seconds before it writes
a program nobody reviews. What each file leaves out, that reader guesses.

This guide shows you how to document a package for both: the doc comments
the declarations are printed from, the readme the model reads, whose
examples the build compiles, and the readme people read. The example is
Acme's billing package; substitute your operations and your service.

## Doc comments are the API

Every export and every field of a type the package returns gets a doc
comment; the build warns about an export without one. The comments are
what `submilli docs` prints, and what the model's documentation tool
returns:

```sh
submilli docs @acme/billing
```

```typescript
@acme/billing — Goodwill credits for one customer of Acme's billing service.

/**
 * Add a goodwill credit to a customer's account.
 * @capability acme.com/credits.apply { customerId: string, customerClass: string, amount: number }
 */
function applyCredit(customerId: string, amount: number): Credit;

/**
 * A credit applied to a customer's account.
 */
interface Credit {
  /**
   * Amount in cents.
   */
  amount: number;
  /**
   * The customer's balance after the credit, in cents. Negative means Acme owes the customer.
   */
  balance: number;
  /**
   * The customer credited.
   */
  customerId: string;
}
```

The first line is the description from `submilli.toml`. Write a comment for the
reader who sees only this: what the operation does, in which unit, and
what the fields mean.

## The readme the model reads

`docs/readme.md` is returned to the model with the declarations. Say what
the package is for, what each operation takes and returns, when it
throws, what a denial means and that the program should stop, and end
with one complete `main`:

````markdown title="packages/billing/docs/readme.md"
# @acme/billing

Credits for one customer of Acme's billing service. The customer is the
one the session was opened for; a credit for any other customer is
refused.

## applyCredit(customerId, amount)

Adds a goodwill credit of `amount` cents to the customer's account and
returns a `Credit` with the customer's balance afterwards. Throws
`RangeError` if the amount is not positive, and `Error` if the customer
does not exist.

A `PermissionDeniedError` means the blueprint does not allow this credit:
the customer is not the session's, or the amount is more than the policy
allows. Do not retry with another customer or amount; report it and stop.

```ts
import { applyCredit } from "@acme/billing";

function main(): string {
    const credit = applyCredit("cus_northwind", 1500);
    return `credited ${credit.amount} cents, balance ${credit.balance}`;
}
```
````

## The examples are compiled

Every `ts` or `typescript` example in `docs/readme.md` is compiled
against the package by `submilli build test` and counted as a test, so
an example can't drift from the API it shows:

```sh
submilli build test -p @acme/billing
```

```text
ok   packages/billing/tests/lib.test.ts :: refuses a zero amount
ok   packages/billing/docs/readme.md :: example 1 (compile)

2 passed, 0 failed across 2 files
```

An example that names a function the package doesn't have fails the run,
with the line in the readme:

```text
error: package `@acme/billing` does not export `refund`
  --> packages/billing/docs/readme.md:28:10
   |
28 | import { refund } from "@acme/billing";
   |          ^^^^^^
help: exports: `Credit`, `applyCredit`
FAIL packages/billing/docs/readme.md :: example 2 (compile)

2 passed, 1 failed across 2 files
```

Examples are compiled, not run. Mark a fragment that isn't a whole
program as `ts ignore` and it is left alone.

## The readme people read

`README.md` in the package's directory is for the person deciding to use
the package, and for the coding agent doing it for them. Replace the
scaffold's placeholder: say what the package is for, which credential to
bind and how the service issues it, what the credential needs on the
service's side, which operations to grant and the fields their filters
can test, and give the install and grant commands:

````markdown title="packages/billing/README.md"
# @acme/billing

Goodwill credits for Acme's customers, on Stripe. Programs call
`applyCredit(customerId, amount)`; the package reads the customer's class
from the account and asks the blueprint before it credits anything.

## Credential

The package reads `BILLING_API_KEY`, a Stripe secret key. Create one in
the Stripe Dashboard under Developers → API keys; a restricted key needs
write access to Customers. Declare it in the blueprint and put the value
in the store:

```sh
submilli blueprint secret add BILLING_API_KEY --store billing_api_key
submilli secret put billing_api_key
```

## Operations

| Capability | Fields | Grant it for |
| --- | --- | --- |
| `acme.com/credits.apply` | `customerId`, `customerClass`, `amount` | Credits to the session's customer |

```sh
submilli install acme/billing-package @acme/billing
submilli blueprint add-package @acme/billing --no-capabilities
submilli blueprint capability add acme.com/credits.apply \
  --filter 'customerId == ${vars.customerId} and amount <= 5000'
```
````

Nothing compiles this file. When an operation changes, the compiled
example catches the model's readme, and this one is yours to review in
the same change.
