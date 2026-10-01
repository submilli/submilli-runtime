---
title: "Testing packages"
description: "Writing and running package tests with submilli build test: test files, assert, labels, expected errors, secrets for live tests, HTTP tests, checked readme examples, and what package tests don't prove."
slug: testing-packages
sidebar:
  order: 18
---

A package is code an agent's programs depend on, reviewed once and then
called many times by programs nobody reviews. Tests are how it stays
correct. `submilli build test` compiles the project, runs every test file,
and compiles the examples in each readme.

## A test file

A test is a program. It imports from the package by name, as a program
would, and has a `main` that returns nothing:

```typescript title="packages/billing/tests/lib.test.ts"
import { label, expectException } from "submilli:test";
import { applyCredit, invoicePath } from "@acme/billing";

function main(): void {
    label("credits the customer named");
    const credit = applyCredit("cus_northwind", 1500);
    assert(credit.customerId === "cus_northwind", "credit names the customer");
    assert(credit.amount === 1500, "credit carries the amount");

    label("refuses a zero amount");
    const error = expectException(() => { applyCredit("cus_northwind", 0); }, "RangeError");
    assert(error.message.includes("positive"), "message says what is wrong");

    label("escapes the customer id in the path");
    assert(invoicePath("a/b") === "/customers/a%2Fb/invoices/latest", "slash is escaped");
}
```

```sh
submilli build test
```

```text
ok   packages/billing/tests/lib.test.ts :: credits the customer named
ok   packages/billing/tests/lib.test.ts :: refuses a zero amount
ok   packages/billing/tests/lib.test.ts :: escapes the customer id in the path
ok   packages/billing/docs/readme.md :: example 1 (compile)

4 passed, 0 failed across 2 files
```

Test files are named `*.test.ts` and live anywhere under the package's
`tests/` directory. `-p @acme/billing` runs one package's tests.

| | |
| --- | --- |
| `assert(condition, message)` | Always in scope. Throws `Error(message)` when the condition is false. |
| `label(text)` | From `submilli:test`. Starts a named test. |
| `expectException(fn, errorType)` | From `submilli:test`. Fails unless `fn` throws, and returns the error it threw. |

`errorType` is the error's name as a string, `"RangeError"`, not the class.
Leave it out to accept any error.

## Labels and failures

Each `label` starts a test that runs to the next `label`, or to the end of
`main`. A file with no labels is one test.

The first failure ends the file. The tests before it passed, the one it
happened in failed, and the ones after it didn't run and aren't counted:

```text
…
ok   packages/support/tests/lib.test.ts :: apologizes with a credit
FAIL packages/support/tests/lib.test.ts :: reports the amount in dollars

6 passed, 1 failed across 4 files
```

```text
error: Error: reports dollars
  at main (packages/support/tests/lib.test.ts:9:59)  [thrown here]
 8 |     label("reports the amount in dollars");
 9 |     assert(apologize("cus_northwind") === "credited $15", "reports dollars");
   |                                                           ^
```

A run with a failure exits 1. Each file runs on its own, with a fresh,
empty filesystem, so tests in different files can't affect each other.
Put tests that must not stop each other in separate files.

## What to test without the service

Most of a package can be tested with no network and no credential, if you
write it that way. Keep the parts that build a request and read a response
as functions of their own, and export them: `invoicePath` above is one. Then
test that a path escapes its arguments, that a request carries only the
fields the caller set, and that an error names what went wrong.

## Tests that call the service

Tests receive no credentials by default. Package code calling `secrets.get`
gets `null` unless you supply the credential explicitly. Package tests run with
the package's identity, including their `main` function. This does not grant
ordinary script callers access to secrets.

```typescript title="packages/billing/tests/network.test.ts"
import { latestInvoice } from "@acme/billing";

function main(): void {
    const invoice = latestInvoice("cus_northwind");
    assert(invoice !== null, "northwind has an invoice");
}
```

Run with a named process variable, or an explicitly selected file:

```sh
submilli build test -p @acme/billing --env-var BILLING_API_KEY
submilli build test -p @acme/billing --env-file .env
```

`--env-var NAME` accepts repeated flags and comma-separated names. `--all-env`
supplies all Unicode process environment variables. When combined, precedence
is `--env-var` > `--env-file` > `--all-env`, regardless of argument order.
A missing selected process variable or unreadable file fails the run and names
it. Relative file paths use the current directory; `.env` is never read implicitly.
In CI, inject the key into the job's environment and use the `--env-var` command.

If package code returns early for a missing key, the test is reported `ok` since
`main` returned. Packages that require the credential may instead fail the test.
Keep `.env` out of source control, and make live tests read-only unless they
have a target that is safe to change.

Name a file that opens connections `network.test.ts` or
`network_<something>.test.ts` (the `.subm` variants are also supported).
`--skip-network` leaves those files out anywhere under `tests/`, before they
are compiled. The filename determines selection, not detection of network calls:

```sh
submilli build test -p @acme/billing --skip-network
```

Each skipped file is reported as `skip <path> (--skip-network)`, followed by
`N HTTP test files skipped (--skip-network)`. Other tests and readme examples
still run. Without the option, network files run; `SUBMILLI_SKIP_HTTP_TESTS`
has no effect on package tests.

## Readme examples are compiled

Every `ts` or `typescript` example in `docs/readme.md` is compiled against
the package, and counted as a test. An example that names a function the
package doesn't have fails the run, with the line in the readme:

```text
error: package `@acme/billing` does not export `refund`
  --> packages/billing/docs/readme.md:15:10
   |
15 | import { refund } from "@acme/billing";
   |          ^^^^^^
help: exports: `Credit`, `Invoice`, `applyCredit`, `invoicePath`, `latestInvoice`
FAIL packages/billing/docs/readme.md :: example 2 (compile)
```

Examples are compiled, not run. Mark a fragment that isn't a whole program
as `ts ignore` and it is left alone.

## What package tests don't prove

Tests run with no blueprint. Every `check` is allowed, and printed:

```text
[security] caller=main capability=acme.com/credits.apply context={"amount":1500,"customerClass":"premium","customerId":"cus_northwind"}
```

That line shows what a rule would see, which is worth reading: it is the
package's side of the contract. But a passing test says nothing about what a
program is refused. For that, publish the package and run programs under the
blueprint, one that should be allowed and one that shouldn't, as
[publish it](/docs/package-anatomy#publish-it) does.

## With a coding agent

A coding agent with the [Submilli skill](/docs/skill) writes a package's
tests and runs them. This prompt was run with Claude Code in the project
above, with the skill installed and one test in `@acme/billing`.

```text
Add tests for @acme/billing. I don't have the billing API key on this machine.
```

The agent read the package and ran the existing test before writing any.
Then it wrote tests that need no key: the invoice path for a hostile id,
`cus/../admin?x=1`, which must stay inside the customer's segment of the
path; credits for a premium and a standard customer; a zero and a negative
amount, refused before the permission check; and `latestInvoice` with no
key, which must fail naming `BILLING_API_KEY`. At the end of the same file
it added a live read that runs only when the key is set. It didn't put that
read in a `network.test.ts`, so `--skip-network` doesn't skip it.

Its first expectation for the hostile path didn't match what
`encodeComponent` returns. It ran the function to
see what it returns, decided the package was right and its test wrong, fixed
the test, and said so in its report. All the tests passed, about a minute
after the prompt.

The report separated what ran from what didn't. The live read hadn't run,
so the report called it unverified until someone runs it with the key.
It also repeated the limit in the section above: the tests show the package
works, not that a blueprint refuses what it should.
