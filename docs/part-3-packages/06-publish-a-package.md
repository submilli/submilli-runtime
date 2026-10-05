---
title: "Publish a Package"
description: "How to publish a Package: install it into your local store, see what programs see, add it to a Blueprint and run a program under it, make it installable from your repository, and put it on a server."
slug: packages/publish-a-package
sidebar:
  order: 6
authorship:
  label: ai-assisted
  confirmed: true
  contentHash: "8253af93e547b61383b60367c60cce57831bb86ad8f100bf13824f03c3aaa528"
  confirmedAt: "2026-10-05T10:59:51.489Z"
---

The Package compiles and its tests pass, but nothing can import it yet.
Programs import from a Package store, yours or a server's, and you test
the policy by running a program under a Blueprint.

This guide shows you how to publish a Package. The example is Acme's
billing Package on Stripe. Substitute your Package.

## Install it locally

```sh
submilli build publish-local -p @acme/billing
```

```text
installed @acme/billing v0.1.0 -> ~/.submilli/packages/@acme/billing
```

`publish-local` compiles and installs into the local store, where
`submilli run` and a server on the same machine find it. Nothing is
uploaded. Without `-p`, it installs every Package in the project.

## See what programs see

The agent finds a Package by searching, then reads its declarations. Do
the same:

```sh
submilli search billing
```

```text
@acme/billing — Goodwill credits for one customer of Acme's billing service.
```

`submilli docs @acme/billing` prints the declarations and doc comments,
as [Document the Package](/docs/packages/document-the-package)
shows. The description in `submilli.toml` and the doc comments are all the
model knows about the Package.

## Add it to a Blueprint

In a new directory, start a Blueprint and add the Package. The
Blueprint commands read the `capabilities.yaml` the build derived on
[Export a function](/docs/packages/export-a-function):

```sh
submilli blueprint init support
submilli blueprint add-package @acme/billing --no-capabilities
```

```text
✓ created blueprint.yaml (name: support)
warning: blueprint.yaml: package `@acme/billing` requires secret `BILLING_API_KEY`, but `secrets:` does not declare it
✓ added @acme/billing to blueprint.yaml
  1 provided capabilities not selected; `default: deny` denies calls to them
  added 3 rules to caller `@acme/billing` (default allow):
    allow http.get (filter: host == "api.stripe.com")
    allow http.post (filter: host == "api.stripe.com")
    allow secrets.get (filter: name == "BILLING_API_KEY")
```

The three rules under the Package's caller are `requires`, written as
grants, with the host from the constant and the secret by its name. The
warning is the secret the Package reads, which the Blueprint must
declare. The capability not selected comes from `provides`, with the
fields the payload reports:

```sh
submilli blueprint capability list @acme/billing
```

```text
@acme/billing
  acme.com/credits.apply — Add a goodwill credit to a customer's account.
      fields: amount: number, customerClass: string, customerId: string
```

## Try it under a Blueprint

Finish the Blueprint as [Start a
Blueprint](/docs/blueprints/start-a-blueprint) does. Declare the key,
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

The program reached Stripe through the Package and came back with the
account's balance. Bound to another customer, the same program is refused
at the `check`, as [Start a
Blueprint](/docs/blueprints/start-a-blueprint) shows.

## Make it installable

Other machines install from source. Push the project to GitHub, and a
developer installs the Package from the repository, built there and
pinned to the commit it resolved:

```sh
submilli install acme/billing-package @acme/billing
```

The first argument is the repository, `owner/repo`, with `@<ref>` to pin
a branch, tag, or commit. The second is the Package, since one repository
can hold several. A private repository installs the same way once the CLI
has a GitHub token that can read it, as [Start a
Blueprint](/docs/blueprints/start-a-blueprint) shows:

```sh
submilli github authenticate
submilli install acme/billing-package @acme/billing
```

## Put it on a server

A server on the same machine reads the local store, so it already has the
Package. Any other server installs it from the repository with
`submilli server packages install`, the same way. For a private
repository, refer to
[Install private Packages](/docs/server/install-private-packages).
Then put the Stripe key in the server's store, register the Blueprint,
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
