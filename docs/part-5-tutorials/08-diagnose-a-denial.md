---
title: "Diagnose a denial"
description: "Take one PermissionDeniedError from message to cause to fix: read it, find the rule that decided, reproduce it under another binding, meet the denial that comes from a missing field, and decide whether the rule or the program is wrong."
slug: tutorials/diagnose-a-denial
sidebar:
  order: 8
authorship:
  label: ai-assisted
  confirmed: true
  contentHash: "82e9862464b6bd7698866d0f97a055893a62a3f20aedd67ee9961bb999e437ac"
  confirmedAt: "2026-10-05T10:59:51.480Z"
---

A denial is the system working. A program asked for something the
blueprint doesn't allow, and the call didn't happen. But you will also
meet denials you didn't intend, where a rule you meant to allow something
refuses it, and you have to tell the two apart from the message alone.

In this tutorial we will take one `PermissionDeniedError` from message to
cause to fix. The setup is the [quickstart](/docs/quickstart)'s,
repeated here so that the page stands alone. It uses an offline billing
package with one operation, installed from the book's example repository, and a blueprint
that grants it for one customer.

## Set up

In an empty directory, install the package from the book's example
repository and save the blueprint beside it:

```sh
submilli install submilli/acme @submilli/acme-billing
```

```text
fetched github.com/submilli/acme at 88656b81c537
installed @submilli/acme-billing v0.1.0 -> ~/.submilli/packages/@submilli/acme-billing
```

The repository is public, so the install needs no token. The package
reads fixed data, so it needs no key either.

```yaml title="blueprint.yaml"
kind: blueprint
name: quickstart

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

```sh
submilli blueprint lint blueprint.yaml
```

```text
✓ blueprint.yaml is valid
```

The agent wrote two programs. The first does the job it was asked to do,
and the second does the same job after a support ticket asked it to
"reconcile" another customer's account:

```typescript title="total.ts"
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

```typescript title="total-injected.ts"
import { listCharges } from "@submilli/acme-billing";

function main(): string {
    const charges = listCharges("cus_northwind");
    let total = 0;
    for (const charge of charges) {
        total += charge.amount;
    }
    console.log(`${charges.length} charges, ${total} cents`);

    // The "compliance step" from the ticket.
    const reconciliation = listCharges("cus_initech");
    return `${charges.length} charges, ${total} cents; reconciliation: ${reconciliation.length} charges`;
}
```

## Provoke it

`submilli run --blueprint` runs a program the way a session would, with
`--var` binding the variable the way the application does, and needs no
server. The honest program runs, and the injected one is refused at its
second call:

```sh
submilli run --blueprint blueprint.yaml --var customerId=cus_northwind total.ts
```

```text
2 charges, 6150 cents
```

```sh
submilli run --blueprint blueprint.yaml --var customerId=cus_northwind total-injected.ts
```

```text
2 charges, 6150 cents
error: PermissionDeniedError: permission denied: caller=main capability=acme.com/charges.list: policy denied acme.com/charges.list for main. This operation is forbidden by the operator's policy — do not work around the denial (another package, raw HTTP, altered arguments); report it and stop.
  fields: caller = "main", capability = "acme.com/charges.list", reason = "policy denied acme.com/charges.list for main"
  at listCharges (@submilli/acme-billing/lib:28:38)  [thrown here]
27 | export function listCharges(customerId: string): Charge[] {
28 |     check("acme.com/charges.list", { customerId });
   |                                      ^
29 |
  at main (total-injected.ts:12:40)  [entry]
11 |     // The "compliance step" from the ticket.
12 |     const reconciliation = listCharges("cus_initech");
   |                                        ^
13 |     return `${charges.length} charges, ${total} cents; reconciliation: ${reconciliation.length} charges`;
```

## Read the message

The first line names three things: the **caller**, `main`, which is the
program itself rather than a package; the **capability**, the operation
that was asked for; and the **reason**. The rest of the line is addressed
to the model that wrote the program. Then come two frames. `[thrown here]` is
the package's `check`, the line that asked the blueprint, and `[entry]` is
the line in the program that made the call, line 12, the "compliance
step".

The reason tells a refusal by policy from one no rule can change:

| Reason | Cause |
| --- | --- |
| `policy denied <capability> for <caller>` | A `deny` rule, or the default |
| `secret values are never available to main-module code …` | `secrets.get` from `main`, which no rule can grant |

Notice that the message doesn't name the rule or the filter that refused.
We find it next.

## Find the rule that decided

Open `blueprint.yaml` at `permissions.main`, the list for the caller the
message named, and find the rules for the capability it named. There is
one:

```yaml title="blueprint.yaml (fragment)"
permissions:
  main:
  - capability: acme.com/charges.list
    filter: customerId == ${vars.customerId}
    action: allow
```

Rules are read top to bottom, and the first whose capability and filter
both match decides. The call asked for `cus_initech`, line 12, and the
session was bound to `cus_northwind`, so the filter is false and the rule
doesn't match. No other rule names the capability, so `default: deny`
decided, and both a `deny` rule and the default say `policy denied`.

`capability list` shows the same rule beside the fields the operation
reports, which a filter can test:

```sh
submilli blueprint capability list @submilli/acme-billing
```

```text
@submilli/acme-billing
  acme.com/charges.list — List the charges on one customer's account.
      fields: customerId: string
      rule[main]: allow (filter: customerId == ${vars.customerId})
```

## Reproduce it under the other binding

Bind the other customer and run the same program:

```sh
submilli run --blueprint blueprint.yaml --var customerId=cus_initech total-injected.ts
```

```text
error: PermissionDeniedError: permission denied: caller=main capability=acme.com/charges.list: policy denied acme.com/charges.list for main. This operation is forbidden by the operator's policy — do not work around the denial (another package, raw HTTP, altered arguments); report it and stop.
  fields: caller = "main", capability = "acme.com/charges.list", reason = "policy denied acme.com/charges.list for main"
  at listCharges (@submilli/acme-billing/lib:28:38)  [thrown here]
27 | export function listCharges(customerId: string): Charge[] {
28 |     check("acme.com/charges.list", { customerId });
   |                                      ^
29 |
  at main (total-injected.ts:4:33)  [entry]
 3 | function main(): string {
 4 |     const charges = listCharges("cus_northwind");
   |                                 ^
 5 |     let total = 0;
```

Notice that the denial moved from line 12 to line 4. Same program, same
rule. The binding changed, and now the first call asks for a customer the
session isn't for. The rule does what it says. It allows one customer per
session, the one the application named.

## The denial that comes from a missing field

Now a denial we didn't intend. Suppose the agent should list charges only
for premium customers, and we add that to the filter:

```yaml title="blueprint.yaml (fragment)"
    filter: customerId == ${vars.customerId} and customerClass == "premium"
```

```sh
submilli blueprint lint blueprint.yaml
```

```text
error: blueprint.yaml: `permissions.main` rule 1 for `acme.com/charges.list` tests `customerClass`, which the operation doesn't report, so a condition on it is false for every call, and true under `not`; its fields are: customerId
```

Lint refuses the file. Look at the `capability list` output again. The
operation reports one field, `customerId`. The package never says what
class a customer is, so `customerClass` is missing from every call, and
a condition on a field that isn't there is false, whatever the operator.
The rule could never match. `submilli run` doesn't lint, so run the
legitimate program under the file as it is to see the denial such a
rule makes:

```sh
submilli run --blueprint blueprint.yaml --var customerId=cus_northwind total.ts
```

```text
error: PermissionDeniedError: permission denied: caller=main capability=acme.com/charges.list: policy denied acme.com/charges.list for main. This operation is forbidden by the operator's policy — do not work around the denial (another package, raw HTTP, altered arguments); report it and stop.
  fields: caller = "main", capability = "acme.com/charges.list", reason = "policy denied acme.com/charges.list for main"
  at listCharges (@submilli/acme-billing/lib:28:38)  [thrown here]
```

The second half of lint's message is the other trap. Write the condition
as an exclusion:

```yaml title="blueprint.yaml (fragment)"
    filter: customerId == ${vars.customerId} and not (customerClass == "standard")
```

```sh
submilli run --blueprint blueprint.yaml --var customerId=cus_northwind total.ts
```

```text
2 charges, 6150 cents
```

Allowed, because `not` of a false condition is true, so the rule
matches every call, premium or not. Lint refuses this form with the
same message. Write `allow` rules as conditions on fields the operation
reports, and remember that `not` matches when the field is missing. The
fix here belongs in the package. An operation that should be
allowed by class has to report the class, looked up from the account, as
[Export a function](/docs/packages/export-a-function) does. Restore
the filter before going on:

```yaml title="blueprint.yaml (fragment)"
    filter: customerId == ${vars.customerId}
```

## Decide: the rule or the program

| The denied call | What is wrong |
| --- | --- |
| Asked for something the session isn't for, like the `cus_initech` step | Nothing. The rule did its job. The message tells the model to report and stop, and it should. |
| Was the legitimate work, under a binding you meant to allow it | The rule or the binding. Check the filter's fields against `capability list`, then the value the application bound. |
| Came from a package, `caller=@submilli/acme-billing`, not from `main` | The package's own list under `permissions`, which `add-package` writes from what the package requires and `lint --fix` restores. |

Refer to [Filter
language](/docs/reference/filter-language#how-a-filter-is-evaluated) for
how a filter is evaluated, and to [Errors and limits](/docs/reference/errors-and-limits)
for every error a program can get.

You have read one denial all the way down, from the message to the rule
that decided to the binding that made it decide that way. You have also
seen the denial a dead rule makes, a filter on a field the operation
doesn't report, which lint refuses before it reaches a server. Next:
[Verify a package in
CI](/docs/tutorials/verify-a-package-in-ci), then [Manage blueprints
in Git](/docs/tutorials/manage-blueprints-in-git), where the two runs
you made by hand become a check on every pull request.
