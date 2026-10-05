---
title: "Write tests"
description: "How to test a Package with submilli build test: a test file and its helpers, labels and failures, a live test run with the key or skipped, and what Package tests don't prove."
slug: packages/write-tests
sidebar:
  order: 5
authorship:
  label: ai-assisted
  confirmed: true
  contentHash: "57e4edb172b8312e6b8f5531e4706f4fcc5a474646c378764e385b65b6eb6598"
  confirmedAt: "2026-10-05T13:01:53.009Z"
---

A Package is reviewed once and then called by programs nobody reviews.
Tests are how it stays correct as the service and the Package change.
`submilli build test` compiles the project, runs every test file, and
compiles the examples in each readme.

This guide shows you how to test a Package. The example is Acme's
billing Package on Stripe. Substitute your Package and its credential.

## A test file

A test is a program. It imports from the Package by name, as a program
would, and has a `main` that returns nothing. Replace the scaffold's
`tests/lib.test.ts`:

```typescript title="packages/billing/tests/lib.test.ts"
import { label, expectException } from "submilli:test";
import { applyCredit } from "@acme/billing";

function main(): void {
    label("refuses a zero amount");
    const error = expectException(() => { applyCredit("cus_northwind", 0); }, "RangeError");
    assert(error.message.includes("positive"), "message says what is wrong");
}
```

```sh
submilli build test -p @acme/billing
```

```text
ok   packages/billing/tests/lib.test.ts :: refuses a zero amount
ok   packages/billing/docs/readme.md :: example 1 (compile)

2 passed, 0 failed across 2 files
```

Test files are named `*.test.ts` and live anywhere under the Package's
`tests/` directory. `-p` runs one Package's tests. The zero amount is
refused before the service is reached, so this test needs no key. The
second line is the readme's example, compiled as [Document the
Package](/docs/packages/document-the-package) describes.

| | |
| --- | --- |
| `assert(condition, message)` | Always in scope. Throws `Error(message)` when the condition is false. |
| `label(text)` | From `submilli:test`. Starts a named test. |
| `expectException(fn, errorType)` | From `submilli:test`. Fails unless `fn` throws, and returns the error it threw. |

`errorType` is the error's name as a string, `"RangeError"`, not the
class. Leave it out to accept any error. `submilli:test` is importable
only under `submilli build test`.

## Labels and failures

Each `label` starts a test that runs to the next `label`, or to the end
of `main`. A file with no labels is one test. The first failure ends the
file. The tests before it passed, the one it happened in failed, and the
ones after it didn't run and aren't counted:

```text
ok   packages/billing/tests/lib.test.ts :: refuses a zero amount
FAIL packages/billing/tests/lib.test.ts :: refuses a negative amount
error: Error: message says negative
  at main (packages/billing/tests/lib.test.ts:10:48)  [thrown here]
 9 |     const error = expectException(() => { applyCredit("cus_northwind", -500); }, "RangeError");
10 |     assert(error.message.includes("negative"), "message says negative");
   |                                                ^
ok   packages/billing/docs/readme.md :: example 1 (compile)

2 passed, 1 failed across 2 files
```

A run with a failure exits 1. Each file runs on its own, with a fresh,
empty filesystem, so put tests that must not stop each other in separate
files.

## Tests that call the service

A live test calls the operation for real. Name a file that opens
connections `network.test.ts` or `network_<something>.test.ts`, and
write it like any other test:

```typescript title="packages/billing/tests/network.test.ts"
import { applyCredit } from "@acme/billing";

function main(): void {
    const credit = applyCredit("cus_VMQR3azuTWVAWs", 100);
    assert(credit.amount === 100, "credit carries the amount");
}
```

Tests see no credentials unless you pass them. Run as before, and this
test fails and says which secret it lacked:

```text
ok   packages/billing/tests/lib.test.ts :: refuses a zero amount
[security] caller=@acme/billing capability=secrets.get context={"name":"BILLING_API_KEY"}
FAIL packages/billing/tests/network.test.ts
error: Error: BILLING_API_KEY is not configured for this blueprint
  at requestHeaders (@acme/billing/lib:73:25)  [thrown here]
72 |     if (key === null) {
73 |         throw new Error("BILLING_API_KEY is not configured for this blueprint");
   |                         ^
74 |     }
  at lookUpClass (@acme/billing/lib:60:74)  [caller]
59 | function lookUpClass(customerId: string): string {
60 |     const response = get(BASE + customerPath(customerId), requestHeaders(false));
   |                                                                          ^
61 |     if (response.status === 404) {
  at applyCredit (@acme/billing/lib:30:39)  [caller]
29 |     const id = customerId.trim();
30 |     const customerClass = lookUpClass(id);
   |                                       ^
31 |     check("acme.com/credits.apply", { customerId: id, customerClass, amount });
  at main (packages/billing/tests/network.test.ts:4:54)  [entry]
3 | function main(): void {
4 |     const credit = applyCredit("cus_VMQR3azuTWVAWs", 100);
  |                                                      ^
5 |     assert(credit.amount === 100, "credit carries the amount");
ok   packages/billing/docs/readme.md :: example 1 (compile)

2 passed, 1 failed across 3 files
```

Give the run the key one of three ways: `--env-file <path>` reads
`NAME=value` lines from a file, `--env-var NAME` passes one variable from
your environment, repeatable or as a comma-separated list, and
`--all-env` passes the whole environment. A named variable that is unset
or a file that can't be read fails the run, and when several are given,
`--env-var` wins over `--env-file`, which wins over `--all-env`. In CI,
put the key in the job's environment and pass `--env-var
BILLING_API_KEY`. Here the key is in a `.env` beside `submilli.toml`:

```sh title=".env"
BILLING_API_KEY=sk_test_…
```

```sh
submilli build test -p @acme/billing --env-file .env
```

```text
ok   packages/billing/tests/lib.test.ts :: refuses a zero amount
[security] caller=@acme/billing capability=secrets.get context={"name":"BILLING_API_KEY"}
[security] caller=@acme/billing capability=http.get context={"body_size":0,"host":"api.stripe.com","path":"/v1/customers/cus_VMQR3azuTWVAWs","timeout_ms":30000}
[security] caller=main capability=acme.com/credits.apply context={"amount":100,"customerClass":"premium","customerId":"cus_VMQR3azuTWVAWs"}
[security] caller=@acme/billing capability=http.post context={"body_size":54,"host":"api.stripe.com","path":"/v1/customers/cus_VMQR3azuTWVAWs/balance_transactions","timeout_ms":30000}
ok   packages/billing/tests/network.test.ts
ok   packages/billing/docs/readme.md :: example 1 (compile)

3 passed, 0 failed across 3 files
```

The credit was applied, in Stripe's test mode. A live test that writes
needs a target that is safe to change, such as a test mode or a test
account. Without one, keep live tests read-only. Keep `.env` out of
source control. On a machine that has no key, don't let the live tests
fail the run. `--skip-network` leaves out every file named
`network.test.ts` or `network_<something>.test.ts`, by name alone, and
says so:

```sh
submilli build test -p @acme/billing --skip-network
```

```text
ok   packages/billing/tests/lib.test.ts :: refuses a zero amount
skip packages/billing/tests/network.test.ts (--skip-network)
ok   packages/billing/docs/readme.md :: example 1 (compile)

2 passed, 0 failed across 2 files
1 network test files skipped (--skip-network)
```

## What Package tests don't prove

Tests run with no Blueprint. Every `check` is allowed, and printed, as
the `[security]` lines above show. Read the line for the Package's
operation, because a rule would see the same thing:

```text
[security] caller=main capability=acme.com/credits.apply context={"amount":100,"customerClass":"premium","customerId":"cus_VMQR3azuTWVAWs"}
```

The customer's class is there, looked up from the account, and so is the
Package's side of the contract, meaning the key it read and the two
requests it made. But a passing test says nothing about what a program
is refused. For that, publish the Package and run programs under a
Blueprint, one that should be allowed and one that shouldn't, as
[Publish a Package](/docs/packages/publish-a-package) shows.
