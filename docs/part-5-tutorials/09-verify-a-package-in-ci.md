---
title: "Verify a package in CI"
description: "Build a GitHub Actions job that fails a pull request when a package's tests fail, with the tests that call the service run from the repository's secrets and skipped where there are none, and an agent's security review beside them."
slug: tutorials/verify-a-package-in-ci
# The workflows have not run on GitHub: the installer is not public yet.
# SUB-1309 runs them. Every command inside them was run locally. The
# security review needs `build security-review`, included in v0.2.0.
# Its outputs are from Codex 0.160.0 with gpt-6.1-sol
# on 2026-10-04, with the CLI built from main b5002307.
sidebar:
  order: 9
authorship:
  label: ai-assisted
  confirmed: true
  contentHash: "8bc2fc74a127cdacf6905f2070ea6de459e3f8c0843ca857d5db4036b21c1e13"
  confirmedAt: "2026-10-05T10:59:51.480Z"
---

A package is reviewed once and then called by programs nobody reviews,
so a change to it has to prove itself before it merges. The diff can't
tell the reviewer whether the package still does what its tests say. A
job can.

In this tutorial we will build a GitHub Actions job that runs a
package's tests on every pull request and every push to main, and fails
when one breaks. The
package is a small charge lookup over fixed data, written here from
scratch so that it needs no service and no key. Later sections add the
tests that do call a service, and an agent's review for the bugs tests
miss.

## The package

In an empty repository, scaffold the package and replace its source and
its test:

```sh
submilli build init @acme/billing packages/billing
```

```typescript title="packages/billing/src/lib.ts"
// A charge lookup for a customer, standing in for a real billing API.

import { check } from "submilli:security";

/** One charge on a customer's account. */
export interface Charge {
    /** The customer the charge belongs to. */
    customerId: string;
    /** Charge identifier, as the billing system issued it. */
    id: string;
    /** Amount in cents. */
    amount: number;
}

const LEDGER: Charge[] = [
    { customerId: "cus_northwind", id: "ch_a1", amount: 4900 },
    { customerId: "cus_northwind", id: "ch_a2", amount: 1250 },
    { customerId: "cus_initech", id: "ch_a3", amount: 39900 },
];

/**
 * List the charges on one customer's account.
 * @param customerId Billing customer ID, such as `cus_northwind`.
 * @returns The customer's charges; empty when they have none.
 * @capability acme.com/charges.list { customerId: string }
 */
export function listCharges(customerId: string): Charge[] {
    check("acme.com/charges.list", { customerId });

    // In Production, this would call an API endpoint.
    const found: Charge[] = [];
    for (const charge of LEDGER) {
        if (charge.customerId === customerId) {
            found.push(charge);
        }
    }
    return found;
}
```

```typescript title="packages/billing/tests/lib.test.ts"
import { label } from "submilli:test";
import { listCharges } from "@acme/billing";

function main(): void {
    label("lists a customer's own charges");
    const charges = listCharges("cus_northwind");
    assert(charges.length === 2, "cus_northwind has two charges in the fixture");
    assert(charges[0].amount === 4900, "the first is the 4900-cent charge");

    label("scopes the lookup to the customer asked for");
    assert(listCharges("cus_initech").length === 1, "cus_initech has one charge");
    assert(listCharges("cus_unknown").length === 0, "an unknown customer has none");
}
```

## Run the tests by hand first

```sh
submilli build test
```

```text
[security] caller=main capability=acme.com/charges.list context={"customerId":"cus_northwind"}
[security] caller=main capability=acme.com/charges.list context={"customerId":"cus_initech"}
[security] caller=main capability=acme.com/charges.list context={"customerId":"cus_unknown"}
ok   packages/billing/tests/lib.test.ts :: lists a customer's own charges
ok   packages/billing/tests/lib.test.ts :: scopes the lookup to the customer asked for

2 passed, 0 failed across 1 files
```

Tests run with no blueprint, so every `check` is allowed and printed.
A rule would see the `[security]` lines. The job prints the same when
the pull request is good.

## The workflow

```yaml title=".github/workflows/test.yml"
name: Test

on:
  pull_request:
  push:
    branches: [main]

jobs:
  test:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4

      - name: Install Submilli
        run: |
          curl -fsSL https://submilli.ai/install.sh | sh -s -- --version v0.2.0
          echo "$HOME/.local/bin" >> "$GITHUB_PATH"

      - name: Test the package
        run: submilli build test
```

The job runs on pull requests, to stop a bad change before it merges,
and on pushes to main, to catch what reached it another way. The
installer is pinned to a release, so a new CLI can't change what the
job does until you change the line. `submilli build test` exits 1 when a
test fails, and that fails the job. Commit the workflow, open a pull
request, and the check runs and passes.

## See it fail

Now break the package the way a careless edit would. Drop the comparison
that keeps a lookup to one customer, so every charge comes back.

```typescript title="packages/billing/src/lib.ts (fragment)"
    for (const charge of LEDGER) {
        if (true) {
            found.push(charge);
        }
    }
```

```sh
submilli build test
```

```text
[security] caller=main capability=acme.com/charges.list context={"customerId":"cus_northwind"}
FAIL packages/billing/tests/lib.test.ts :: lists a customer's own charges
error: Error: cus_northwind has two charges in the fixture
  at main (packages/billing/tests/lib.test.ts:7:34)  [thrown here]
 6 |     const charges = listCharges("cus_northwind");
 7 |     assert(charges.length === 2, "cus_northwind has two charges in the fixture");
   |                                  ^
 8 |     assert(charges[0].amount === 4900, "the first is the 4900-cent charge");

0 passed, 1 failed across 1 files
```

Notice that the `check` still passed. The package asked about
`cus_northwind` and was told yes. A blueprint can't catch this. Only the
package's tests can, so they gate the merge. Restore the
line.

## Tests that call the service

A package over a real API also has tests that call it, and they need the
service's key. Three steps get it to them in CI, and only to them.

**1. Keep them apart.** Put every test that calls the service in
`tests/network.test.ts`, or `tests/network_<name>.test.ts`, with nothing
else in those files. `--skip-network` leaves those files out by name,
which matters below, when there is no key.

**2. Add the key to the repository's secrets.** On GitHub, open the
repository's **Settings**, then **Secrets and variables**, **Actions**,
and **New repository secret**. Name it `BILLING_API_KEY`, the name the
package reads, and paste the key as its value. With the GitHub CLI,
from a checkout of the repository:

```sh
gh secret set BILLING_API_KEY
```

It prompts for the value, so the key never lands in your shell history.
Use the service's test key, because the tests will run on every
pull request.

**3. Give the key to the test step.** A secret reaches a step only when
the step names it under `env:`, and a test reaches it only when the
command passes it with `--env-var`. The workflow becomes:

```yaml title=".github/workflows/test.yml"
name: Test

on:
  pull_request:
  push:
    branches: [main]

jobs:
  test:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4

      - name: Install Submilli
        run: |
          curl -fsSL https://submilli.ai/install.sh | sh -s -- --version v0.2.0
          echo "$HOME/.local/bin" >> "$GITHUB_PATH"

      - name: Test the package
        env:
          BILLING_API_KEY: ${{ secrets.BILLING_API_KEY }}
        run: submilli build test --env-var BILLING_API_KEY
```

```text
ok   packages/billing/tests/lib.test.ts :: lists a customer's own charges
ok   packages/billing/tests/lib.test.ts :: scopes the lookup to the customer asked for
ok   packages/billing/tests/network.test.ts

3 passed, 0 failed across 2 files
```

Every run now tests against the service, and a run without the key
fails. A test that should have reached the service can't, and the job
turns red rather than passing on less than it claims.

Pull requests from forks need a decision, since GitHub
runs them without your secrets. To test those without the service instead
of failing them, give them a separate step. Pushes to main, and pull
requests from branches of this repository, keep the key:

```yaml title=".github/workflows/test.yml"
name: Test

on:
  pull_request:
  push:
    branches: [main]

jobs:
  test:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4

      - name: Install Submilli
        run: |
          curl -fsSL https://submilli.ai/install.sh | sh -s -- --version v0.2.0
          echo "$HOME/.local/bin" >> "$GITHUB_PATH"

      - name: Test the package
        if: github.event_name == 'push' || github.event.pull_request.head.repo.full_name == github.repository
        env:
          BILLING_API_KEY: ${{ secrets.BILLING_API_KEY }}
        run: submilli build test --env-var BILLING_API_KEY

      - name: Test the package without the service
        if: github.event_name == 'pull_request' && github.event.pull_request.head.repo.full_name != github.repository
        run: submilli build test --skip-network
```

The second step says what it left out:

```text
ok   packages/billing/tests/lib.test.ts :: lists a customer's own charges
ok   packages/billing/tests/lib.test.ts :: scopes the lookup to the customer asked for
skip packages/billing/tests/network.test.ts (--skip-network)

2 passed, 0 failed across 1 files
1 network test files skipped (--skip-network)
```

Tests see no credential unless the command passes one, so the key
reaches only the run that names it, and the package reads it the way it
would on a server. Keep live tests read-only unless they have a target
that is safe to change, such as the service's test mode. [Write
tests](/docs/packages/write-tests) has the details.

## Have an agent review it

Tests check the cases they try. Add a lookup for one charge that checks
the customer but finds the charge by its ID alone:

```typescript title="packages/billing/src/lib.ts (added at the end)"
/**
 * Look up one of a customer's charges.
 * @param customerId Billing customer ID, such as `cus_northwind`.
 * @param chargeId Charge identifier, such as `ch_a1`.
 * @returns The charge, or `null` when there is none.
 * @capability acme.com/charges.get { customerId: string }
 */
export function getCharge(customerId: string, chargeId: string): Charge | null {
    check("acme.com/charges.get", { customerId });

    for (const charge of LEDGER) {
        if (charge.id === chargeId) {
            return charge;
        }
    }
    return null;
}
```

A test that looks up Northwind's own charge passes, and so would any rule,
because `check` was asked about the right customer. An agent reading the source
catches it. Install Codex, sign in, and review the project:

```sh
npm install -g @openai/codex@0.160.0
codex login
submilli build security-review -a codex -m gpt-6.1-sol -e high --output review.json
```

```text
Security review: complete (2 finding(s))
High packages/billing/src/lib.ts:51 — Charge lookup checks a customer unrelated to the returned charge
  … A caller authorized for cus_northwind can call getCharge("cus_northwind", "ch_a3") and receive cus_initech's charge …
Medium packages/billing/src/lib.ts:34 — Read operations expose mutable internal ledger records
  …
```

The high finding fails the command, which exits 1. The medium one, that
callers get the ledger's objects rather than copies, is below the
default `--fail-on high`. Wording and severity vary between runs, and a
review can miss a bug, so it sits beside the tests.
Fix the lookup, and the review exits 0:

```typescript title="packages/billing/src/lib.ts (fragment)"
        if (charge.id === chargeId && charge.customerId === customerId) {
```

In CI the review needs an OpenAI API key, from [API
keys](https://platform.openai.com/api-keys), as a secret
(`gh secret set CODEX_API_KEY`), and a separate job under `jobs:`:

```yaml title=".github/workflows/test.yml (added under jobs:)"
  review:
    if: github.event_name == 'push' || github.event.pull_request.head.repo.full_name == github.repository
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4
      - uses: actions/setup-node@v4
        with:
          node-version: "22"
      - run: npm install -g @openai/codex@0.160.0
      - run: |
          curl -fsSL https://submilli.ai/install.sh | sh -s -- --version v0.2.0
          echo "$HOME/.local/bin" >> "$GITHUB_PATH"
      - env:
          CODEX_API_KEY: ${{ secrets.CODEX_API_KEY }}
        run: submilli build security-review -a codex -m gpt-6.1-sol -e high --output "$RUNNER_TEMP/review.json"
      - if: always()
        uses: actions/upload-artifact@v4
        with:
          name: security-review-${{ github.sha }}
          path: ${{ runner.temp }}/review.json
```

The report is kept even when the review fails, with a hash of every file
it read. A review that can't finish exits 2 and fails the job too. A
pull request from a fork has no secrets, so it is reviewed after the
merge.

You have a job that tests a package on every pull request and an
agent's review beside it, each catching a bug no rule would. [Review a
package's security](/docs/packages/review-package-security) runs the
review with Claude Code. Next: [Manage blueprints in
Git](/docs/tutorials/manage-blueprints-in-git), the same idea for the
blueprints that grant the package.
