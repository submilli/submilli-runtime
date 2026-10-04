---
title: "Manage blueprints in Git"
description: "Keep the blueprints in a repository: on every pull request, lint them and test each one as two sessions, one it must allow and one it must refuse; on every merge to main or every release, register them on the server, with the packages pinned in the same commit and a rollback that is a revert."
slug: tutorials/manage-blueprints-in-git
# The workflows have not run on GitHub: the installer is not public yet.
# SUB-1309 runs them. Every command inside them was run locally.
sidebar:
  order: 10
---

A blueprint is policy, and `apply` from a laptop leaves no record of who
changed it, when, or why; the server holds whatever was applied last. Kept
in a repository, every change is a reviewed commit, the pipeline proves
each rule refuses what it should before the merge, and the server holds
exactly what the main branch says.

In this tutorial we will put a blueprint in a repository and give it a
pipeline: on every pull request, lint it and test it as two sessions; on
every merge to main, or every release, register it on the server, with
the package it lists pinned in the same commit, and a rollback that is a
revert. The blueprint grants the book's example billing package, which
reads fixed data, so nothing here needs a key. You need a server, as
[Run the server](/docs/server/run-the-server) shows, with its admin
token in your shell.

## The repository

```text
.github/workflows/blueprints.yml   # written below
packages.txt
blueprints/
└── support/
    ├── blueprint.yaml
    ├── README.md
    ├── total.ts
    └── test.sh
```

Each blueprint gets a folder of its own, with the file named
`blueprint.yaml`. The `submilli blueprint` commands read that file from
the current directory, so in the folder `capability add`, `secret add`,
and the rest work on it without naming it. Everything that belongs to
the blueprint sits beside it: a `README.md` saying what the agent is
for and who owns the policy, and the program and script that test it.

The blueprint lets the agent list the charges of one customer, the one
the session is for:

```yaml title="blueprints/support/blueprint.yaml"
kind: blueprint
name: support

# Bound once per request by the application, never by the program.
variables:
  customerId:
    required: true

packages:
- '@submilli/acme-billing'

default: deny

permissions:
  # What generated code may do.
  main:
  - capability: acme.com/charges.list
    filter: customerId == ${vars.customerId}
    action: allow

  # What the package itself may do. Nothing: it reads a fixture.
  '@submilli/acme-billing': []
```

`packages.txt` names every package the blueprints list, one per line: the
GitHub repository it is installed from, the package, and the commit to
pin:

```text title="packages.txt"
submilli/acme @submilli/acme-billing 88656b81c537
```

## Test the policy as two sessions

A passing lint says the file is well formed. Whether the rule refuses what
it should is tested by running a program under it, bound to the customer
the rule should allow and then to one it should refuse:

```typescript title="blueprints/support/total.ts"
import { listCharges } from "@submilli/acme-billing";

function main(): string {
    const charges = listCharges("cus_northwind");
    let total = 0;
    for (const charge of charges) {
        total += charge.amount;
    }
    return `${charges.length} charges, ${total} cents`;
}
```

```sh title="blueprints/support/test.sh"
#!/usr/bin/env bash
# Runs total.ts under this blueprint as two sessions: the customer it
# should allow, and one it should refuse.
set -eu
submilli run --blueprint blueprint.yaml --var customerId=cus_northwind total.ts
submilli run --blueprint blueprint.yaml --var customerId=cus_initech total.ts 2>&1 | grep PermissionDeniedError
```

Install the pinned package, lint, and run the test, the three steps the
pull-request job will run:

```sh
submilli install submilli/acme@88656b81c537 @submilli/acme-billing
cd blueprints/support
submilli blueprint lint blueprint.yaml
chmod +x test.sh
./test.sh
```

```text
installed @submilli/acme-billing v0.1.0 -> ~/.submilli/packages/@submilli/acme-billing
✓ blueprint.yaml is valid
2 charges, 6150 cents
error: PermissionDeniedError: permission denied: caller=main capability=acme.com/charges.list: policy denied acme.com/charges.list for main. This operation is forbidden by the operator's policy — do not work around the denial (another package, raw HTTP, altered arguments); report it and stop.
```

The first line of the test is the program's result for the customer the
session is for. The second is the test: bound to Initech, the same
program asks for Northwind's charges and is refused, and the `grep`
passes only when that denial appears. A run that fails for another
reason, or that is allowed, fails the script.

## Check every pull request

```yaml title=".github/workflows/blueprints.yml"
name: Blueprints

on:
  pull_request:
  push:
    branches: [main]

jobs:
  check:
    runs-on: ubuntu-latest
    env:
      SUBMILLI_DENY_WARNINGS: "1"
    steps:
      - uses: actions/checkout@v4

      - name: Install Submilli
        run: |
          curl -fsSL https://submilli.ai/install.sh | sh -s -- --version v0.2.0
          echo "$HOME/.local/bin" >> "$GITHUB_PATH"

      - name: Install the packages the blueprints list
        run: while read repo package sha; do submilli install "$repo@$sha" "$package"; done < packages.txt

      - name: Lint and test each blueprint
        run: |
          for dir in blueprints/*/; do
            (cd "$dir" && submilli blueprint lint blueprint.yaml && ./test.sh)
          done
```

`SUBMILLI_DENY_WARNINGS` makes every warning fail the job: a package
whose `check` and `@capability` tag disagree fails `submilli install`,
and a blueprint that lint warns about, such as one with `default: allow`,
fails `submilli blueprint lint`.

Now break the rule the way a careless edit would: drop the filter, so
that any customer's charges are allowed. Lint still passes, since the
file is well formed. The policy test doesn't:

```text
✓ blueprint.yaml is valid
2 charges, 6150 cents
```

The second run was allowed, so `grep` found no denial, the script exits
1, and the pull request's check turns red with that line in its log.

A package in `packages.txt` can also be held to an agent's security review
before its commit is pinned there; [Review a package's
security](/docs/packages/review-package-security#make-deployment-wait-for-it)
shows how.

## Register on every merge

The second job runs only on a push to main, after the check, and talks
to the server. It needs the server's address and an admin token, since
registering a blueprint is an admin operation. Store them in the
repository with the GitHub CLI, from a checkout of it: the address as a
variable, since it isn't secret, and the token as a secret, which
`gh` prompts for so it never lands in your shell history:

```sh
gh variable set SUBMILLI_SERVER_URL --body http://submilli.internal:8128
gh secret set SUBMILLI_SERVER_TOKEN
```

The job reads both into its environment, where the `submilli server`
commands look for them:

```yaml title=".github/workflows/blueprints.yml (continued)"
  deploy:
    if: github.event_name == 'push'
    needs: check
    runs-on: ubuntu-latest
    env:
      SUBMILLI_SERVER_URL: ${{ vars.SUBMILLI_SERVER_URL }}
      SUBMILLI_SERVER_TOKEN: ${{ secrets.SUBMILLI_SERVER_TOKEN }}
    steps:
      - uses: actions/checkout@v4

      - name: Install Submilli
        run: |
          curl -fsSL https://submilli.ai/install.sh | sh -s -- --version v0.2.0
          echo "$HOME/.local/bin" >> "$GITHUB_PATH"

      - name: Install the packages on the server
        run: while read repo package sha; do submilli server packages install "$repo" "$package" --sha "$sha"; done < packages.txt

      - name: Register the blueprints
        run: for dir in blueprints/*/; do submilli server blueprint apply "${dir}blueprint.yaml"; done
```

The server has to be reachable from the runner: a server inside your
network takes a self-hosted runner in the same network. Merge the pull
request, and the job's log shows the server taking the package and the
blueprint:

```text
installed @submilli/acme-billing @ 88656b81c537
Added blueprint 'support'
```

Merge a change to the file and the same job prints `Updated blueprint
'support'`. `apply` registers or replaces, so running the job again is
safe. To register on every release instead of every merge, change the
trigger to `release: types: [published]` and the job's condition to
`github.event_name == 'release'`.

## Check what the server holds, and roll back

```sh
submilli server blueprint list
submilli server run-code blueprints/support/total.ts --blueprint support --var customerId=cus_northwind
```

```text
support
2 charges, 6150 cents
```

`submilli server blueprint show support` prints the file the server
holds, which is the main branch's; when it isn't, someone ran `apply` by
hand, and the next merge puts the repository's version back. A change
that turns out wrong is reverted like any other, `git revert HEAD` and a
push, and the job registers the previous file. One thing the job doesn't
do is remove: a blueprint whose folder is deleted stays registered
until `submilli server blueprint remove` is run, which ends its
sessions, so make that call part of the same change.

You have a blueprint that reaches the server only from main, linted and
tested from both sides on the way, with the package it needs pinned
beside it and a history of every change. Next: [Add the GitHub MCP
server](/docs/tutorials/add-the-github-mcp-server).
