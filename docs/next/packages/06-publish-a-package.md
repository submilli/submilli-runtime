---
title: "Publish a package"
description: "How to publish a package: install it into your local store, see what programs see, add it to a blueprint and run a program under it, make it installable from your repository, and put it on a server."
slug: next/packages/publish-a-package
pagefind: false
sidebar:
  order: 6
  hidden: true
---

The package compiles and its tests pass, but nothing can import it yet.
Programs import from a package store, yours or a server's, and the only
test of the policy is a program running under a blueprint.

This guide shows you how to publish a package: install it into your
local store, see what programs will see, add it to a blueprint and run a
program under it, make it installable from your repository, and put it
on a server. The example is Acme's billing package on Stripe; substitute your
package.

## Install it locally

```sh
submilli build publish-local -p @acme/billing
```

```text
installed @acme/billing v0.1.0 -> ~/.submilli/packages/@acme/billing
```

`publish-local` compiles and installs into the local store, where
`submilli run` and a server on the same machine find it. Nothing is
uploaded. Without `-p`, it installs every package in the project.

## See what programs see

The agent finds a package by searching, then reads its declarations. Do
the same:

```sh
submilli search billing
```

```text
@acme/billing — Goodwill credits for one customer of Acme's billing service.
```

`submilli docs @acme/billing` prints the declarations and doc comments,
as [Document the package](/docs/next/packages/document-the-package)
shows; what the description in `submilli.toml` and the doc comments say is all
the model knows about the package.

## Add it to a blueprint

In a directory of its own, start a blueprint and add the package. The
blueprint commands read the `capabilities.yaml` the build derived on
[Export a function](/docs/next/packages/export-a-function):

```sh
submilli blueprint init support
submilli blueprint add-package @acme/billing --no-capabilities
```

```text
✓ created blueprint.yaml (name: support)
warning: blueprint.yaml: package `@acme/billing` requires secret `BILLING_API_KEY`, but `secrets:` does not declare it
✓ added @acme/billing to blueprint.yaml
  1 provided capabilities not selected — denied by `default: deny`
  added 3 rules to caller `@acme/billing` (default allow):
    allow http.get (filter: host == "api.stripe.com")
    allow http.post (filter: host == "api.stripe.com")
    allow secrets.get (filter: name == "BILLING_API_KEY")
```

The three rules under the package's own caller are `requires`, written
as grants: the host from the constant, the secret by its name. The
warning is the secret the package reads, which the blueprint must
declare. The one capability not selected is `provides`, with the fields
the payload reports:

```sh
submilli blueprint capability list @acme/billing
```

```text
@acme/billing
  acme.com/credits.apply — Add a goodwill credit to a customer's account.
      fields: amount: number, customerClass: string, customerId: string
```

## Try it under a blueprint

Finish the blueprint as [Start a
blueprint](/docs/next/blueprints/start-a-blueprint) does: declare the key,
the customer the session is for, and one rule over the payload's fields:

```sh
submilli blueprint secret add BILLING_API_KEY --store billing_api_key
submilli secret put billing_api_key
submilli blueprint variable add customerId --required
submilli blueprint capability add acme.com/credits.apply \
  --filter 'customerId == ${vars.customerId} and customerClass == "premium"'
```

```text
✓ declared secret 'BILLING_API_KEY' (store: billing_api_key) in blueprint.yaml
Value for 'billing_api_key': [hidden]
Stored secret 'billing_api_key'
✓ declared variable 'customerId' (required) in blueprint.yaml
✓ added allow acme.com/credits.apply (filter: customerId == ${vars.customerId} and customerClass == "premium") to caller 'main' in blueprint.yaml
```

Create `credit.ts`, a program like one the model would write. It credits
Northwind, a premium customer in Stripe's test mode:

```typescript title="credit.ts"
import { applyCredit } from "@acme/billing";

function main(): string {
    const credit = applyCredit("cus_VMQR3azuTWVAWs", 1500);
    return `credited ${credit.amount} cents, balance ${credit.balance}`;
}
```

Run it bound to Northwind:

```sh
submilli run --blueprint blueprint.yaml --var customerId=cus_VMQR3azuTWVAWs credit.ts
```

```text
credited 1500 cents, balance -1600
```

The program reached Stripe through the package and came back with the
account's balance. Bound to another customer, the same program is refused
at the `check`, as [Start a
blueprint](/docs/next/blueprints/start-a-blueprint) shows.

## Make it installable

Other machines install from source. Push the project to GitHub, and a
developer installs the package from the repository, built there and
pinned to the commit it resolved:

```sh
submilli install acme/billing-package @acme/billing
```

The first argument is the repository, `owner/repo`, with `@<ref>` to pin
a branch, tag, or commit; the second is the package, since one repository
can hold several. A private repository is fetched over SSH, as you, with
the keys in your ssh-agent or under `~/.ssh`:

```sh
submilli install git@github.com:acme/billing-package.git @acme/billing
```

## Put it on a server

A server on the same machine reads the local store, so it already has the
package. Any other server installs it from the repository with
`submilli server packages install`, the same way; for a private
repository, refer to
[Install private packages](/docs/next/server/install-private-packages).
Then put the Stripe key in the server's store, register the blueprint,
and run the program there, the way an application would:

```sh
submilli server secret put billing_api_key
submilli server blueprint apply blueprint.yaml
submilli server run-code credit.ts --blueprint support --var customerId=cus_VMQR3azuTWVAWs
```

```text
Value for 'billing_api_key': [hidden]
Stored secret 'billing_api_key'
Added blueprint 'support'
credited 1500 cents, balance -6100
```
