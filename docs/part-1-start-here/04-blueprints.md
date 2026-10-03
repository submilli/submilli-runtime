---
title: "Blueprints"
description: "The blueprint in full: default deny, rules per caller, the variable the application binds, secrets, files, and models, and what no rule can grant."
slug: blueprints
sidebar:
  order: 4
---

Your agent's program has to run somewhere. That somewhere is an environment
the server builds for each run: the packages the program can import, the
files it can see, the secrets its packages may use, and the rules for all
of it. A blueprint is the plan for that environment.
It is to the environment what a blueprint is to a house, or an image to a
container: not the thing itself, but the definition it is built from. The
server keeps the plan under a name. Each time your application opens a
session and names it, the server builds an environment to that plan and
runs the agent's programs inside. What the plan doesn't include, the
environment doesn't have.

The plan is written once, by you (or your coding assistant, with our skill),
outside the model, and the runtime reads it on every call. In the quickstart you wrote one with a single rule and
watched the environment refuse one call. This chapter is the plan in full:
what it says, how a call is decided against it, and where its power stops.



## What the plan says

Before the details, the shopping list. A blueprint has these parts:

1. **Packages.** The Submilli packages the program may import. They are like
   npm packages, but written for Submilli, so every operation in them can
   be checked against the rules.
2. **Permissions.** What the program may do, and what each package may do
   on its behalf: the rules, per caller, for every operation that reaches
   outside, and the default when no rule matches.
3. **Secrets and variables.** Secrets are credentials, declared by name,
   with their values kept outside the file. Variables are values your
   application binds when it opens a session, for the rules to test.
4. **Auth proxy.** When the program uses HTTP directly, this adds the
   credential on the way out, without exposing it to the model.
5. **MCP servers.** Tool servers you already run, declared here so they
   become packages the program can import.
6. **Files.** What filesystem the program sees: nothing, a scratch directory
   for the run, a directory that lasts the session, or a volume the
   server's operator declared. How long an idle session lives is set here
   too.

The rest of this chapter is about the rules, because that is where the plan
does its work. The other parts each get a page in the how-to part on
blueprints.

## Nothing is allowed until you say so

The smallest blueprint permits nothing:

```yaml
kind: blueprint
name: support
default: deny
permissions:
  main: []
```

`name` is what the application and the server call it. `permissions` is a
map from *caller* to a list of rules; `main` is the generated program.
`default` is the answer when no rule matches. Leaving `default` out means
`deny` as well, so a blueprint containing only a name denies everything.

Under this file, a program that computes and returns a value runs fine.
Anything that reaches outside the instance — a file, a request, a package
operation — fails with a permission error. Every rule you add widens that.

## Rules

A rule has three fields: `capability`, the name the package or the standard
library gave the operation; an optional `filter` over the fields the
operation reports; and `action`, `allow` or `deny`.

```yaml
permissions:
  main:
  - capability: acme.com/credits.apply
    filter: customerId == ${vars.customerId} and customerClass == "premium"
    action: allow
```

When a program calls a gated operation, the runtime finds the caller's list,
walks it top to bottom, and takes the first rule whose capability matches by
name and whose filter is absent or true. If none matches, `default` decides.
Names match exactly: there is no `fs.*`, so allowing `fs.write` doesn't
allow `fs.mkdir`.

Filters compare a field with `==`, `!=`, `<`, `<=`, `>`, `>=`, test a
pattern with `glob` or `matches`, and combine with `and`, `or`, `not`. A
field the operation didn't report never matches.

## Code you trust and code you don't

Two kinds of code run inside one program: the packages you reviewed and
installed, and the code the model wrote a moment ago. Submilli keeps them
apart, and the blueprint gives each its own list of rules: `main` for the
generated code, and each package under its own name. Permissions are per
caller, not per program.

A package declares its **capabilities**: what it *provides*, the operations
a program can be granted, such as `acme.com/credits.apply`; and what it
*requires*, what its own code needs to do its job, such as an HTTP request
to the billing host and the secret that authenticates it.

```yaml
permissions:
  # What generated code may do.
  main:
  - capability: acme.com/credits.apply
    filter: customerId == ${vars.customerId} and customerClass == "premium"
    action: allow

  # What the package itself may do.
  '@acme/billing':
  - capability: http.post
    filter: host == "billing.internal.example.com"
    action: allow
  - capability: secrets.get
    filter: name == "BILLING_API_KEY"
    action: allow
```

Under this blueprint, generated code can't send a request to the billing
host or read the key. It can call `applyCredit`, and the package sends the
request, authenticated with the key. The
package's function is the only form in which generated code can use the
billing API. The CLI writes a package's own list from what it requires when
you add it. What it provides, you grant to `main`, narrowed with filters
and variables.

## Rules are about what an operation means

Consider the alternative: run the agent in a secure sandbox, or route all of
its traffic through a gateway, and write rules over what it sends. To write
a rule such as "may credit the customer it is serving, and only a premium
one", you would have to reverse-engineer the traffic: find the request that
means a credit among everything posted to `billing.internal.example.com`,
work out which field is the customer and which the amount, and do it again
for every service the agent uses, and again when a service changes its API.
And the fact the rule most needs, the customer's class, isn't in the
request at all.

Submilli inverts that. The package that performs the operation says what it
means: `acme.com/credits.apply` means applying a credit, and it hands the
runtime the customer, the amount, and the customer's class, typed, before
anything is sent. The rule is written against those, not against a
payload. Nobody guesses what a request does, and the model is never asked
to judge its own intent. Submilli calls this **semantic security**, and the
[next chapter](/docs/packages) shows where the meaning comes from.

## Context: who the session is for

The other thing a sandbox or a gateway can't see is context: which customer
this conversation is about. The request doesn't carry it, and the model
can't be trusted to state it. A **variable** brings that context into the
rules. Your application binds it when it opens a session, from what it
knows, and a rule can test against it. You write one blueprint and bind a
different customer for each session:

```yaml
variables:
  customerId:
    required: true

default: deny

permissions:
  main:
  - capability: acme.com/credits.apply
    filter: customerId == ${vars.customerId}
    action: allow
```

The filter compares two values. `customerId` is the customer the program is
asking to credit, supplied by the package in the permission check.
`${vars.customerId}` is the customer the application authorized for this
session, taken from trusted context such as the signed-in account and
supplied outside the generated program. With `cus_northwind` bound, a
credit for `cus_initech` fails the rule even though applying credits is
allowed, and nothing the program does can change the binding.

A **session** is what the application or harness opens for one conversation
with the agent, and then runs each program inside. The variables are bound
when it opens and last as long as it does.

## Secrets stay on the trusted side

Anything generated code can read, the model can be talked into repeating.
That holds for whatever an allowed operation returns; the blueprint decides
what a program may fetch, not what the model says afterwards. So credentials
must be unreadable altogether.

The blueprint declares each **secret** by name and says where the runtime
finds the value: a secret store, or the application when it opens the
session. The value never enters the file.

- A package reads a secret by name with `secrets.get`. The runtime refuses
  the same call from `main` whatever the blueprint says, even one that
  allows it.
- When a blueprint lets generated code call an HTTP endpoint directly,
  Submilli's **auth proxy** adds the credential outside the program. The
  program sees the response, never the header.

Next: [packages](/docs/packages), where the operations a blueprint
rules on come from.
