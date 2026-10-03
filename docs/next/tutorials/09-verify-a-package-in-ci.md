---
title: "Verify a package in CI"
description: "Build a GitHub Actions job that fails a pull request when a package's tests fail, with the tests that call the service run from the repository's secrets and skipped where there are none."
slug: next/tutorials/verify-a-package-in-ci
pagefind: false
# The workflows have not run on GitHub: the installer is not public yet.
# SUB-1309 runs them. Every command inside them was run locally.
sidebar:
  order: 9
  hidden: true
---

A package is reviewed once and then called by programs nobody reviews,
so a change to it has to prove itself before it merges. The diff can't
tell the reviewer whether the package still does what its tests say; a
job can.

In this tutorial we will build a GitHub Actions job that runs a
package's tests on every pull request and every push to main, and fails
when one breaks. The
package is a small charge lookup over fixed data, written here from
scratch so that it needs no service and no key; the last section adds
the tests that do call a service.

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

Tests run with no blueprint, so every `check` is allowed and printed;
the `[security]` lines are what a rule would see. That is what the job
will print when the pull request is good.

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
          curl -fsSL https://submilli.ai/install.sh | sh -s -- --version v0.1.6
          echo "$HOME/.local/bin" >> "$GITHUB_PATH"

      - name: Test the package
        run: submilli build test
```

The job runs on pull requests, to stop a bad change before it merges,
and on pushes to main, to catch what reached it another way. The
installer is pinned to a release, so a new CLI can't change what the
job does until you change the line. `submilli build test` exits 1 when a
test fails, which is what fails the job. Commit the workflow, open a pull
request, and the check runs and passes.

## See it fail

Now break the package the way a careless edit would: drop the comparison
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

Notice that the `check` still passed: the package asked about
`cus_northwind` and was told yes. A blueprint can't catch this; only the
package's own tests can, which is why they gate the merge. Restore the
line.

## Tests that call the service

A package over a real API also has tests that call it, and they need the
service's key. Three steps get it to them in CI, and only to them.

**1. Keep them apart.** Put every test that calls the service in
`tests/network.test.ts`, or `tests/network_<name>.test.ts`, with nothing
else in those files. `--skip-network` leaves those files out by name,
which matters for the one case below where there is no key.

**2. Add the key to the repository's secrets.** On GitHub, open the
repository's **Settings**, then **Secrets and variables**, **Actions**,
and **New repository secret**. Name it `BILLING_API_KEY`, the name the
package reads, and paste the key as its value. With the GitHub CLI,
from a checkout of the repository:

```sh
gh secret set BILLING_API_KEY
```

It prompts for the value, so the key never lands in your shell history.
Use the service's test key, not the live one: the tests will run on every
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
          curl -fsSL https://submilli.ai/install.sh | sh -s -- --version v0.1.6
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
fails: a test that should have reached the service can't, and the job
turns red rather than passing on less than it claims.

Pull requests from forks are the one case to decide on, since GitHub
runs them without your secrets. To test those without the service rather
than fail them, give them a step of their own; pushes to main, and pull
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
          curl -fsSL https://submilli.ai/install.sh | sh -s -- --version v0.1.6
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
1 HTTP test files skipped (--skip-network)
```

Tests see no credential unless the command passes one, so the key
reaches only the run that names it, and the package reads it the way it
would on a server. Keep live tests read-only unless they have a target
that is safe to change, such as the service's test mode; [Write
tests](/docs/next/packages/write-tests) has the details.

You have a job that runs a package's tests on every pull request, and
seen it catch a bug no rule would. Next: [Manage blueprints in
Git](/docs/next/tutorials/manage-blueprints-in-git), the same idea for
the blueprints that grant the package.
