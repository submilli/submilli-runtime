---
title: "Review package security with Claude Code"
description: "Use claude to review a package's authorization and require the result before merging."
slug: tutorials/security-review-claude
# Verification: Live local and GitHub Actions reviews detected the vulnerable fixture and passed the correction; reports verified in test-github-package run 37185669929. CI used a source build because the command is unreleased.
# Requires a released Submilli CLI containing build security-review; pin that release before publication.
sidebar:
  order: 13
---

A package can check permission for one customer and still return another
customer's data. An agent review can identify that mismatch before the package
reaches a server.

In this tutorial we will build a required GitHub Actions check using Claude Code.
We will review a deliberately incorrect charge lookup, correct its result, and
keep the review report with the commit it inspected.

You need a private GitHub repository you can administer, GitHub CLI (`gh`)
authenticated to it, and a Submilli CLI release containing
`submilli build security-review`. Work in a checkout of that repository. This
workflow is for trusted contributors; the first job rejects fork pull requests.

## Authenticate the reviewer

Use a Claude Pro, Max, Team, or Enterprise account with Claude Code access.
Install Claude Code 2.1.288 locally and sign in:

```sh
npm install -g @anthropic-ai/claude-code@2.1.288
claude auth login
claude setup-token
```

The last command creates a long-lived token tied to your subscription. Save it
as an Actions secret; `gh` prompts for the value so it is not in shell history:

```sh
gh secret set CLAUDE_CODE_OAUTH_TOKEN
```

The review step receives this secret as an environment variable. Its requests
consume your subscription allowance. Renew the token when it expires or is
revoked. Do not also supply `ANTHROPIC_API_KEY`: Claude gives that key precedence
and would use API billing. See [Claude authentication](https://code.claude.com/docs/en/authentication#generate-a-long-lived-token).

## Create the package

Save these files. The fixed ledger makes the example reproducible without a
billing service or production credentials.

```toml title="submilli.toml"
[[package]]
name = "@acme/billing"
version = "0.1.0"
description = "Customer charge lookup."
```

```typescript title="src/lib.ts"
import { check } from "submilli:security";

/** A charge owned by one customer. */
export interface Charge {
    /** Service charge identifier. */
    id: string;
    /** Customer who owns the charge. */
    customerId: string;
    /** Amount in cents. */
    amount: number;
}

const charges: Charge[] = [
    { id: "ch_1", customerId: "cus_northwind", amount: 4900 },
    { id: "ch_2", customerId: "cus_initech", amount: 9900 },
];

/**
 * List the requested customer's charges.
 * @param customerId Customer whose charges to return.
 * @returns Charges belonging to that customer.
 * @capability acme.com/charges.list { customerId: string }
 */
export function listCharges(customerId: string): Charge[] {
    check("acme.com/charges.list", { customerId });
    return charges;
}
```

```markdown title="docs/readme.md"
# Billing

List the charges belonging to the requested customer.
```

```typescript title="tests/lib.test.ts"
import { listCharges } from "@acme/billing";

function main(): void {
    const charges = listCharges("cus_northwind");
    assert(charges.length > 0, "the customer has charges");
}
```

Run the ordinary checks:

```sh
submilli build check --deny-warnings
submilli build test --deny-warnings --skip-network
```

The test passes:

```text
[security] caller=main capability=acme.com/charges.list context={"customerId":"cus_northwind"}
ok   tests/lib.test.ts

1 passed, 0 failed across 1 files
```

The test checks that a charge exists. It does not establish that every returned
charge belongs to the authorized customer. The package's capability declaration
and `check()` agree, so that compiler check also cannot detect this mistake.

## Review the authorization

```sh
submilli build security-review -a claude -m claude-opus-5-5 -e high --fail-on high --output review-before.json
```

`-a` selects the installed agent, `-m` its model, and `-e` the reasoning effort.
`--fail-on high` makes a high or critical finding fail the command.
`--output` saves the report to a new file; use a different name for each run.

Read the result:

```sh
jq '{status, findings, coverage_gaps, error}' review-before.json
```

The run used for this tutorial exited 1. Its output began:

```text
Security review: complete (2 finding(s))
High src/lib.ts:26 — listCharges authorizes one customer but returns every customer's charges
```

Look for a finding that `listCharges("cus_northwind")` checks only Northwind but
returns Initech's charge as well. Model wording and severity can vary between
runs. A review that misses the bug is not proof the package is safe; the
[report reference](/docs/reference/security-review) explains the limits and
incomplete-review status.

## Correct the result

Replace `return charges;` with:

```typescript title="src/lib.ts (fragment)"
    const result: Charge[] = [];
    for (const charge of charges) {
        if (charge.customerId === customerId) {
            result.push({
                id: charge.id,
                customerId: charge.customerId,
                amount: charge.amount,
            });
        }
    }
    return result;
```

The function now returns copies of only the authorized customer's charges.
Replace the test with a regression check that detects the original mistake:

```typescript title="tests/lib.test.ts"
import { listCharges } from "@acme/billing";

function main(): void {
    const charges = listCharges("cus_northwind");
    assert(charges.length === 1, "only Northwind's charge is returned");
    for (const charge of charges) {
        assert(charge.customerId === "cus_northwind", "customer scope is preserved");
    }
}
```

Run the test and review again:

```sh
submilli build test --deny-warnings --skip-network
submilli build security-review -a claude -m claude-opus-5-5 -e high --fail-on high --output review-after.json
```

The corrected package passed the test, and the review exited 0:

```text
Security review: complete (0 finding(s))
```

Inspect any remaining findings. A complete report with no high or critical
findings exits 0. An incomplete review, including failed authentication, exits 2.

## Add the workflow

Set the repository variable `SUBMILLI_VERSION` to the exact released CLI version
you tested locally, including its `v` prefix. That release must contain
`build security-review`; the workflow never selects a moving latest release.

Save this complete workflow:

```yaml title=".github/workflows/security-review-claude.yml"
name: Security review (claude)

on:
  pull_request:
  push:
    branches: [main]
  workflow_dispatch:

permissions:
  contents: read

concurrency:
  group: security-review-claude
  cancel-in-progress: false

jobs:
  trusted-source:
    runs-on: ubuntu-latest
    steps:
      - name: Require a branch in this private repository
        env:
          IS_PRIVATE: ${{ github.event.repository.private }}
          HEAD_REPOSITORY: ${{ github.event.pull_request.head.repo.full_name || github.repository }}
          REPOSITORY: ${{ github.repository }}
        run: test "$IS_PRIVATE" = true && test "$HEAD_REPOSITORY" = "$REPOSITORY"
  review:
    needs: trusted-source
    runs-on: ubuntu-latest
    timeout-minutes: 15
    steps:
      - uses: actions/checkout@v4
        with:
          persist-credentials: false
      - uses: actions/setup-node@v4
        with:
          node-version: '22'
      - name: Install Claude Code
        run: npm install -g @anthropic-ai/claude-code@2.1.288
      - name: Install Submilli
        env:
          SUBMILLI_VERSION: ${{ vars.SUBMILLI_VERSION }}
        run: |
          test -n "$SUBMILLI_VERSION"
          curl -fsSL https://submilli.ai/install.sh | sh -s -- --version "$SUBMILLI_VERSION"
          echo "$HOME/.local/bin" >> "$GITHUB_PATH"
      - name: Check and test the package
        run: |
          submilli build check --deny-warnings
          submilli build test --deny-warnings --skip-network
      - name: Review authorization
        env:
          CLAUDE_CODE_OAUTH_TOKEN: ${{ secrets.CLAUDE_CODE_OAUTH_TOKEN }}
        run: >-
          submilli build security-review -a claude
          -m claude-opus-5-5 -e high --fail-on high
          --output "$RUNNER_TEMP/security-review-${{ github.run_id }}-${{ github.run_attempt }}.json"
      - name: Keep the report
        if: always()
        uses: actions/upload-artifact@v4
        with:
          name: security-review-claude-${{ github.sha }}
          path: ${{ runner.temp }}/security-review-${{ github.run_id }}-${{ github.run_attempt }}.json
          if-no-files-found: error
          retention-days: 14
```

The report upload runs even when the review fails. A missing report also fails
the job. Credentials belong only to the review step; this workflow does not
receive a Submilli server token.

Commit the files, open a pull request, and inspect the **Security review (claude)**
workflow. Download its report artifact and check the source hashes and findings.
Temporarily restore the incorrect `return charges;` to see the review fail, then
restore the correction. Require both `trusted-source` and `review` checks in the
repository's branch protection or ruleset before merging. Do not add
`continue-on-error` to the review step.

You have a package review recorded with the source it inspected. Next, connect
that reviewed revision to deployment in
[Manage blueprints in Git](/docs/tutorials/manage-blueprints-in-git#require-a-package-security-review).
