---
title: "Add a dependency"
description: "How to make a package import another, from the same project, your local store, or a GitHub repository, and what the dependency adds to what the package requires and to the blueprint."
slug: packages/add-a-dependency
sidebar:
  order: 4
---

A package often builds on another. Acme's support package apologizes to
a customer with a credit, so it imports the billing package rather than
calling Stripe again; a package that reads a web page imports the curated
Jina package. The build has to know where each import comes from, and the
blueprint needs rules for every package in the chain, not only the one
the program imports; `add-package` writes them.

This guide shows you how to make a package import another: from the same
project, from your local store, or from a GitHub repository, and what the
dependency adds to what the package requires and to the blueprint. The
example is `@acme/support`, which imports `@acme/billing`; substitute
your packages.

## Import it

`@acme/support` is the second package of the project, added with
`submilli build new`. Its one operation credits the customer through the
billing package:

```typescript title="packages/support/src/lib.ts"
import { applyCredit } from "@acme/billing";

/**
 * Apologize to a customer with a goodwill credit, and say so in dollars.
 * @param customerId The customer's id in the billing system.
 * @returns A sentence saying how much was credited.
 */
export function apologize(customerId: string): string {
    const credit = applyCredit(customerId, 1500);
    return `credited $${(credit.amount / 100).toString()}`;
}
```

An import `submilli.toml` doesn't declare stops the build:

```sh
submilli build check -p @acme/support
```

```text
error: package `@acme/billing` not found
 --> packages/support/src/lib.ts:1:29
  |
1 | import { applyCredit } from "@acme/billing";
  |                             ^^^^^^^^^^^^^^^
```

## Declare it

| The dependency is | Declare it |
| --- | --- |
| Another package in the project | In the package's `dependencies` |
| A package in the local store | There, and in `[dependencies]` with its version |
| A package in a GitHub repository | There, and in `[dependencies]` as `{ github = "github.com/org/repo", rev = "<commit>" }` |
| A package in a private GitHub repository | The same; whoever builds needs a GitHub token that can read it |

The billing package is a sibling, so one line in the support package's
block declares it:

```toml title="submilli.toml (fragment)"
[[package]]
name = "@acme/support"
version = "0.1.0"
path = "packages/support"
dependencies = ["@acme/billing"]
```

```sh
submilli build check -p @acme/support
```

```text
checked @acme/billing v0.1.0
checked @acme/support v0.1.0
```

`-p` builds the package and the siblings it depends on, in order. A
package from the local store or from GitHub is declared at the top of
`submilli.toml` as well, and named in the package's list the same way:

```toml title="submilli.toml (fragment)"
[dependencies]
"@submilli/jina" = "0.1.0"
"@acme/crm" = { github = "github.com/acme/crm-package", rev = "9c1f2e4a7d3b06e15a8c4f2d7b9e1a3c5f7d9b2e" }

[[package]]
name = "@acme/support"
version = "0.1.0"
path = "packages/support"
dependencies = ["@acme/billing", "@submilli/jina", "@acme/crm"]
```

A GitHub dependency is fetched into the local store by the build, which
records the commits it used in `submilli.lock`. `rev` is the full
40-character commit SHA; a branch, tag, or short SHA is refused. A private
one, such as `@acme/crm` above, is fetched with the GitHub token of whoever
builds or installs: yours on your machine (`submilli github authenticate`),
the server's own on a server; refer to [Install private
packages](/docs/server/install-private-packages). The token needs
Contents: Read-only on every private repository in the dependencies,
including the ones your dependencies depend on.

## What it adds to the blueprint

What a package uses of another shows up in what it requires. The build
derived this for the support package:

```yaml title="packages/support/capabilities.yaml"
namespace: acme
provides: []
requires:
- capability: acme.com/credits.apply
```

A blueprint grants that to `@acme/support` as it would to a program. The
billing package makes calls of its own, as the caller `@acme/billing`,
so it needs rules too, and so does the secret it reads. `add-package`
adds the whole chain:

```sh
submilli blueprint init support
submilli blueprint add-package @acme/support --no-capabilities
```

```text
✓ created blueprint.yaml (name: support)
warning: blueprint.yaml: package `@acme/billing` requires secret `BILLING_API_KEY`, but `secrets:` does not declare it
✓ added @acme/support to blueprint.yaml
  added 1 rules to caller `@acme/support` (default allow):
    allow acme.com/credits.apply
✓ added caller rules for @acme/billing, a dependency of @acme/support
  added 3 rules to caller `@acme/billing` (default allow):
    allow http.get (filter: host == "api.stripe.com")
    allow http.post (filter: host == "api.stripe.com")
    allow secrets.get (filter: name == "BILLING_API_KEY")
```

Each package in the chain gets its own caller list from what it
requires, but only the package you named is listed under `packages:`,
the packages a program may import. A program can credit a customer only
through `apologize`, which fixes the amount. Declare the secret the
warning names, and put its value in the store if it isn't there yet:

```sh
submilli blueprint secret add BILLING_API_KEY --store billing_api_key
submilli secret put billing_api_key
```

```text
✓ declared secret 'BILLING_API_KEY' (store: billing_api_key) in blueprint.yaml
Value for 'billing_api_key': [hidden]
Stored secret 'billing_api_key'
```

Create `apology.ts` and run it:

```typescript title="apology.ts"
import { apologize } from "@acme/support";

function main(): string {
    return apologize("cus_VMQR3azuTWVAWs");
}
```

```sh
submilli run --blueprint blueprint.yaml apology.ts
```

```text
credited $15
```

The program called the support package, the support package called the
billing package, and the billing package called Stripe, each under its
own rules. The whole file:

```yaml title="blueprint.yaml"
kind: blueprint
name: support
secrets:
  BILLING_API_KEY:
    store: billing_api_key
packages:
- '@acme/support'
default: deny
permissions:
  '@acme/billing':
  - capability: http.get
    filter: host == "api.stripe.com"
    action: allow
  - capability: http.post
    filter: host == "api.stripe.com"
    action: allow
  - capability: secrets.get
    filter: name == "BILLING_API_KEY"
    action: allow
  '@acme/support':
  - capability: acme.com/credits.apply
    action: allow
  main: []
```
