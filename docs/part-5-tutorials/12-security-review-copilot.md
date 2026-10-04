---
title: "Review package security with GitHub Copilot"
description: "Use copilot to review a package's authorization and require the result before merging."
slug: tutorials/security-review-copilot
# Verification: GPT-6.1 Sol found the flawed fixture and passed its correction on GitHub-hosted CI (run 37188559097), using the unreleased source-built CLI. Local personal-account Copilot access was unavailable; the same CLI review used the workflow token.
# Requires a released Submilli CLI containing build security-review; pin that release before publication.
sidebar:
  order: 14
---

A package can check permission for one customer and still return another
customer's data. An agent review can identify that mismatch before the package
reaches a server.

In this tutorial we will build a required GitHub Actions check using GitHub Copilot.
We will review a deliberately incorrect charge lookup, correct its result, and
keep the review report with the commit it inspected.

You need a private GitHub repository you can administer, GitHub CLI (`gh`)
authenticated to it, and a Submilli CLI release containing
`submilli build security-review`. Work in a checkout of that repository. This
workflow is for trusted contributors; the first job rejects fork pull requests.

## Authenticate the reviewer

Use a repository whose owner or organization has Copilot CLI access and permits
the selected model. For an organization-owned repository, an organization owner
configures these settings:

1. Open the organization on GitHub, select **Settings**, then **Copilot → Policies**.
2. Enable **Copilot CLI** and select **Allow use of Copilot CLI billed to the organization**.
3. Open **Copilot → Models** and allow the model selected by the workflow.

An enforced enterprise restriction must be changed by an enterprise owner.
See [GitHub's policy setup](https://docs.github.com/en/copilot/how-tos/copilot-cli/use-copilot-cli-in-actions).

The workflow grants `copilot-requests: write` and authenticates with its temporary
`GITHUB_TOKEN`; no personal token needs to be saved as a secret. Usage is billed
to the organization, or to the repository owner's seat for a personal repository.
See [GitHub's authentication and billing guide](https://docs.github.com/en/copilot/concepts/agents/copilot-cli/copilot-cli-in-github-actions).

For the local steps, your GitHub account needs Copilot access to the selected
model. Organization workflow billing does not grant that access to a personal
token. Install Copilot CLI 1.0.91 locally. For the isolated review process, use a
GitHub CLI login with Copilot access or a supported `COPILOT_GITHUB_TOKEN`.
The command does not load Copilot's personal configuration directory.

```sh
npm install -g @github/copilot@1.0.91
gh auth login
```

If your GitHub CLI token lacks Copilot access, configure a supported fine-grained
personal token with Copilot Requests permission as `COPILOT_GITHUB_TOKEN` in your
local environment. Do not commit it. The CI workflow uses `GITHUB_TOKEN` instead.

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
submilli build security-review -a copilot -m gpt-6.1-sol -e high --fail-on high --output review-before.json
```

`-a` selects the installed agent, `-m` its model, and `-e` the reasoning effort.
`--fail-on high` makes a high or critical finding fail the command.
`--output` saves the report to a new file; use a different name for each run.

Read the result:

```sh
jq '{status, findings, coverage_gaps, error}' review-before.json
```

Look for a finding that `listCharges("cus_northwind")` checks only Northwind but
returns Initech's charge as well. This real GitHub-hosted run produced:

```sh
jq '{status, findings: [.findings[] | {severity, title}]}' review-before.json
```

```json
{
  "status": "complete",
  "findings": [
    {
      "severity": "high",
      "title": "Customer-scoped authorization returns every customer's charges"
    }
  ]
}
```

Notice that the review completed but found a high-severity defect; the command
exited 1. Model wording and severity can vary between runs.
A review that misses the bug is not proof the package is safe; the
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
submilli build security-review -a copilot -m gpt-6.1-sol -e high --fail-on high --output review-after.json
```

The corrected fixture in the same GitHub-hosted run exited 0:

```sh
jq '{status, findings}' review-after.json
```

```json
{
  "status": "complete",
  "findings": []
}
```

Inspect any remaining findings. A complete report with no high or critical
findings exits 0. An incomplete review, including failed authentication, exits 2.

## Add the workflow

Set the repository variable `SUBMILLI_VERSION` to the exact released CLI version
you tested locally, including its `v` prefix. That release must contain
`build security-review`; the workflow never selects a moving latest release.

Save this complete workflow:

```yaml title=".github/workflows/security-review-copilot.yml"
name: Security review (copilot)

on:
  pull_request:
  push:
    branches: [main]
  workflow_dispatch:

permissions:
  contents: read
  copilot-requests: write

concurrency:
  group: security-review-copilot
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
      - name: Install Copilot CLI
        run: npm install -g @github/copilot@1.0.91
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
          GITHUB_TOKEN: ${{ github.token }}
        run: >-
          submilli build security-review -a copilot
          -m gpt-6.1-sol -e high --fail-on high
          --output "$RUNNER_TEMP/security-review-${{ github.run_id }}-${{ github.run_attempt }}.json"
      - name: Keep the report
        if: always()
        uses: actions/upload-artifact@v4
        with:
          name: security-review-copilot-${{ github.sha }}
          path: ${{ runner.temp }}/security-review-${{ github.run_id }}-${{ github.run_attempt }}.json
          if-no-files-found: error
          retention-days: 14
```

The report upload runs even when the review fails. A missing report also fails
the job. Credentials belong only to the review step; this workflow does not
receive a Submilli server token.

Commit the files, open a pull request, and inspect the **Security review (copilot)**
workflow. Download its report artifact and check the source hashes and findings.
Temporarily restore the incorrect `return charges;` to see the review fail, then
restore the correction. Require both `trusted-source` and `review` checks in the
repository's branch protection or ruleset before merging. Do not add
`continue-on-error` to the review step.

You have a package review recorded with the source it inspected. Next, connect
that reviewed revision to deployment in
[Manage blueprints in Git](/docs/tutorials/manage-blueprints-in-git#require-a-package-security-review).
