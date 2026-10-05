---
title: "Start a Blueprint"
description: "How to create a Blueprint with the CLI: start from nothing allowed, check the file, add a Package, grant operations, declare secrets and variables, test it, and register it on a server."
slug: blueprints/start-a-blueprint
sidebar:
  order: 1
authorship:
  label: ai-assisted
  confirmed: true
  contentHash: "bb1415caa827cac20a5dd90f8c2304e65d53e3656374084a5e9733d08a370580"
  confirmedAt: "2026-10-05T10:59:51.490Z"
---

This guide shows you how to build a Blueprint block by block with the
`submilli blueprint` commands, test it, and register it on a server. The
examples use the billing Package from [Packages](/docs/packages).
Substitute your own Package, operation, and fields.

The commands edit `blueprint.yaml` in the current directory and rewrite it
each time, so comments you add by hand don't survive them. Refer to the
[Blueprint file reference](/docs/reference/blueprint-file) for every
block and field.

## Install the Package

A Blueprint can only list a Package that is in your local store. If the
Package is your own, `submilli build publish-local` from its project puts
it there. If someone else published it, install it from its repository:

```sh
submilli install acme/billing-package @acme/billing
```

The first argument is the GitHub repository, `owner/repo`. The second is
the Package to build from it, since one repository can hold several. Leave
the Package out to install all the Packages the repository declares. `install`
fetches the repository, builds the Package, and puts it in the local store,
pinned to the commit it resolved. To pin a branch, tag, or commit yourself,
append `@<ref>` to the repository name. This installs the curated Jina
Package at the runtime's `v0.2.0` tag:

```sh
submilli install submilli/submilli-runtime@v0.2.0 @submilli/jina
```

Add `--upgrade` to replace a Package already installed at another commit.
The [curated Packages](/docs/reference/curated-packages) all come from
that repository.

### From a private repository

A private repository needs a GitHub token that can read it. Without
one, `install` reports that it found no public repository by that name.
Store yours once, and `install` and `build` send it from then on:

```sh
submilli github authenticate
```

```text
✓ stored a GitHub token for octocat (never expires) in ~/.submilli/github_token
```

`authenticate` prompts for the token, or reads it from piped standard
input, and checks it with GitHub. In CI, put the token in `GH_TOKEN`.
`submilli github auth-status` says which token applies. Refer
to [Install private Packages on a
server](/docs/server/install-private-packages#create-the-token) for
the token's settings, and to the [CLI
reference](/docs/reference/cli) for the errors an install can
give.

### On a server

A server has its own Package store, so a Blueprint that will run there
needs the Package installed there too. The `submilli server` commands talk
to a running server, and [Connect the CLI](/docs/server/connect-the-cli)
shows how they reach it:

```sh
submilli server packages install acme/billing-package @acme/billing
```

```text
installed @acme/billing @ 3f9c2a1b7e40
```

The server fetches, builds, and pins the Package the same way. `--sha <ref>`
pins a commit, tag, or branch, and `--upgrade` replaces an installed one.

## Read the Package's capabilities and docs

Before granting anything, read what the Package provides:

```sh
submilli blueprint capability list @acme/billing
```

```text
@acme/billing
  acme.com/credits.apply — Add a goodwill credit to a customer's account.
      fields: amount: number, customerClass: string, customerId: string
```

`submilli docs @acme/billing` prints the declarations the model will read.

## Start from nothing allowed

```sh
submilli blueprint init support
```

```text
✓ created blueprint.yaml (name: support)
```

The file it writes, minus its comments, permits nothing:

```yaml title="blueprint.yaml"
kind: blueprint
name: support
default: deny
permissions:
  main: []
```

The application and the server refer to the Blueprint by `name`.
`default: deny` means anything without a rule is refused. Leaving `default`
out means the same.

## Check the file

```sh
submilli blueprint lint blueprint.yaml
```

```text
✓ blueprint.yaml is valid
```

`lint` validates the file against the installed Packages and exits 1 on an
error and 0 on warnings, so it can gate a commit. `lint --fix` adds missing
Package rules. Run it after each step below, because its warnings say what
the file still lacks.

## Add the Package

```sh
submilli blueprint add-package @acme/billing --no-capabilities
```

```text
warning: blueprint.yaml: package `@acme/billing` requires secret `BILLING_API_KEY`, but `secrets:` does not declare it
✓ added @acme/billing to blueprint.yaml
  1 provided capabilities not selected; `default: deny` denies calls to them
  added 2 rules to caller `@acme/billing` (default allow):
    allow http.post (filter: host == "billing.internal.example.com")
    allow secrets.get (filter: name == "BILLING_API_KEY")
```

```yaml title="blueprint.yaml (fragment)"
packages:
- '@acme/billing'
permissions:
  '@acme/billing':
  - capability: http.post
    filter: host == "billing.internal.example.com"
    action: allow
  - capability: secrets.get
    filter: name == "BILLING_API_KEY"
    action: allow
  main: []
```

The Package is listed, so the import resolves. It also got its own caller
list, written from what it declares it requires. `--no-capabilities`
leaves `main` empty so that you grant operations one by one below.
`--all-capabilities` or `--capabilities a,b` grants the Package's operations
in the same command. The warning is about the key, declared in
[Declare the secret](#declare-the-secret).

## Grant an operation

`main` is still empty, so a program that calls `applyCredit` is refused.
Grant the operation, narrowed by a filter over the fields it reports:

```sh
submilli blueprint capability add acme.com/credits.apply --filter 'customerClass == "premium"'
```

```text
✓ added allow acme.com/credits.apply (filter: customerClass == "premium") to caller 'main' in blueprint.yaml
```

```yaml title="blueprint.yaml (fragment)"
permissions:
  main:
  - capability: acme.com/credits.apply
    filter: customerClass == "premium"
    action: allow
```

A rule names a capability, an optional filter over the fields the
operation reports, and an action. Rules are read top to bottom and the
first match wins. Names match exactly, so allowing `fs.write` doesn't allow
`fs.mkdir`. `capability add` refuses a name nothing provides. Refer to
the [filter language](/docs/reference/filter-language) reference for what
a filter can test.

## Declare the secret

The Package reads `BILLING_API_KEY` by name, and `add-package` warned that
the Blueprint doesn't declare it. When a Package reads a secret, the
Blueprint declares it by that name and says where its value comes from.
The value itself never enters the file:

```sh
submilli blueprint secret add BILLING_API_KEY --store billing_api_key
```

```text
✓ declared secret 'BILLING_API_KEY' (store: billing_api_key) in blueprint.yaml
```

```yaml title="blueprint.yaml (fragment)"
secrets:
  BILLING_API_KEY:
    store: billing_api_key
```

Lint again, and the warning is gone:

```sh
submilli blueprint lint blueprint.yaml
```

```text
✓ blueprint.yaml is valid
```

`--store` names a key in a **secret store**, which keeps your credentials
outside the Blueprint. There are two. The local store is a directory under
`~/.submilli`, readable only by your user, and `submilli run` reads it. A
server has its own store, encrypted at rest, and the Blueprints registered
on that server read it. Put the value in the local store:

```sh
submilli secret put billing_api_key
```

```text
Value for 'billing_api_key': [hidden]
Stored secret 'billing_api_key'
```

The command prompts with echo off. In a script, pipe the value in with
`submilli secret put billing_api_key < key.txt`. On a server,
`submilli server secret put` does the same.

If the credential changes with the context, such as the access token a
user granted your application for their Google Calendar, declare it with
`--harness`. The application supplies the value when it opens the
session, and `--required` refuses a session that doesn't:

```sh
submilli blueprint secret add GOOGLE_ACCESS_TOKEN --harness --required
```

```text
✓ declared secret 'GOOGLE_ACCESS_TOKEN' (harness, required: true) in blueprint.yaml
```

```yaml title="blueprint.yaml (fragment)"
secrets:
  GOOGLE_ACCESS_TOKEN:
    harness:
      required: true
```

[Connect a harness](/docs/tutorials/connect-a-harness) shows how each
harness supplies it.

## Declare a variable

To limit a permission by the session's context, such as the customer the
agent is serving, declare a **variable** for that context. The application
binds its value when it opens the session:

```sh
submilli blueprint variable add customerId --required
```

```text
✓ declared variable 'customerId' (required) in blueprint.yaml
```

```yaml title="blueprint.yaml (fragment)"
variables:
  customerId:
    required: true
```

A variable is a string, either `--required` or with a `--default`. A
session that omits a required variable is refused before any program runs:

```text
error: invalid variables: required variable 'customerId' was not supplied
```

A filter refers to the variable as `${vars.customerId}`. Replace the grant
above with one that also requires the customer to be the session's, so one
Blueprint serves all customers:

```sh
submilli blueprint capability remove acme.com/credits.apply
submilli blueprint capability add acme.com/credits.apply \
  --filter 'customerId == ${vars.customerId} and customerClass == "premium"'
```

```text
✓ removed 1 rule(s) for 'acme.com/credits.apply' from caller 'main' in blueprint.yaml
✓ added allow acme.com/credits.apply (filter: customerId == ${vars.customerId} and customerClass == "premium") to caller 'main' in blueprint.yaml
```

```yaml title="blueprint.yaml (fragment)"
permissions:
  main:
  - capability: acme.com/credits.apply
    filter: customerId == ${vars.customerId} and customerClass == "premium"
    action: allow
```

## Test it

Create `credit.ts`, a program like one a model would write under this
Blueprint. It imports the Package and calls its operation for the premium
customer:

```typescript title="credit.ts"
import { applyCredit } from "@acme/billing";

function main(): string {
    const credit = applyCredit("cus_northwind", 1500);
    return `credited ${credit.amount} cents`;
}
```

`submilli run --blueprint` runs it the way a session would, with `--var`
binding the variable the way the application does. Binding the premium
customer lets the call through, and binding another customer produces the
denial a session would see:

```sh
submilli run --blueprint blueprint.yaml --var customerId=cus_northwind credit.ts
```

```text
credited 1500 cents
```

```sh
submilli run --blueprint blueprint.yaml --var customerId=cus_initech credit.ts
```

```text
error: PermissionDeniedError: permission denied: caller=main capability=acme.com/credits.apply: policy denied acme.com/credits.apply for main. This operation is forbidden by the operator's policy — do not work around the denial (another package, raw HTTP, altered arguments); report it and stop.
  fields: caller = "main", capability = "acme.com/credits.apply", reason = "policy denied acme.com/credits.apply for main"
  at applyCredit (lib:28:66)  [thrown here]
```

Each time you change a rule, test the case it should allow and the case
it should refuse.

## The result

```yaml title="blueprint.yaml"
kind: blueprint
name: support
secrets:
  BILLING_API_KEY:
    store: billing_api_key
variables:
  customerId:
    required: true
packages:
- '@acme/billing'
default: deny
permissions:
  '@acme/billing':
  - capability: http.post
    filter: host == "billing.internal.example.com"
    action: allow
  - capability: secrets.get
    filter: name == "BILLING_API_KEY"
    action: allow
  main:
  - capability: acme.com/credits.apply
    filter: customerId == ${vars.customerId} and customerClass == "premium"
    action: allow
```

## See the prompt

The Blueprint also shapes what the model is told. The description of the
execute tool is assembled from it. It says which modules a program may
import, what its filesystem is, and which hosts it may reach. Print it as the
model receives it:

```sh
submilli blueprint prompt
```

```text
…
You do NOT have access to Node.js APIs, browser globals, or NPM
packages. Submilli ships its own standard library — modules are:
`submilli:url`, `submilli:crypto`, `submilli:uuid`, `submilli:session`. Submilli native
packages and discovered `@mcp/<server>` packages may also be available;
…
```

Nothing here grants `fs.read` or `http.get`, so `submilli:fs` and
`submilli:http` are not listed. A Blueprint that grants them gets the
modules, a `Sandbox:` line naming its filesystem, and a `Network:` line
listing its hosts. The rest of the text, on how to write a program and what to do with a
denial, is the same for all Blueprints.

## Register it on a server

You register a Blueprint file on a server. The server keeps its own copy
and never reads the file again. Put the secret's value in the
server's store first, since registration checks that every `store:` secret
exists there:

```sh
submilli server secret put billing_api_key
submilli server blueprint apply blueprint.yaml
```

```text
Value for 'billing_api_key': [hidden]
Stored secret 'billing_api_key'
Added blueprint 'support'
```

Run `apply` again after an edit, and the answer is `Updated blueprint
'support'`. Then run the program the way an application would, naming the
Blueprint by its name:

```sh
submilli server run-code credit.ts --blueprint support --var customerId=cus_northwind
```

```text
credited 1500 cents
```

Applications name the Blueprint the same way when they open a session.
Refer to [Register a Blueprint](/docs/server/register-a-blueprint) for
applying Blueprints, replacing them, and removing them.
