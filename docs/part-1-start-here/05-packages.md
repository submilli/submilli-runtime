---
title: "Packages"
description: "What a package is in Submilli: a library built for agents, where every operation asks the blueprint before it acts; how a program uses one, and the tools for building one."
slug: packages
sidebar:
  order: 5
---

A package in Submilli is what a package is in npm or pip: a library you
install and import. The language is TypeScript, but npm packages can't be
used. Submilli resets the ecosystem, with packages built for AI agents:
every function that reaches outside names its operation and asks the
blueprint before it acts. Why the reset is worth it comes later in this
chapter. In the quickstart you wrote one with a single function.

Packages are also Submilli's answer to MCP servers. Other code-execution
platforms take the MCP servers you run and turn them into an interface the
agent's code can call. Submilli can do that too: declare a server in the
blueprint and it becomes a package, each tool a function the blueprint can
allow or deny. But MCP was designed for tool calling, not for code. Most
MCP servers publish no output schema, so a program can't know the shape of
what a tool returns, and a rule over an MCP tool can see only the tool's
name. A package needs no server to deploy or maintain, calls the API
directly, returns typed values, and tells the runtime what each call means.

A package can be one you write for an internal system, one you write for a
third-party service you consume, or one someone else published: any package
in a Git repository
[installs straight from it](/docs/blueprints/start-a-blueprint).
Submilli publishes
[curated packages](/docs/reference/curated-packages) that way for
common services, GitHub, Slack, Google Drive, Linear, Notion, and others.

## Why not npm

Generated code can't import npm packages or Node.js modules. An npm package
is written for Node, and Node gives it the whole operating system: files,
sockets, processes, anything a system call can reach. Submilli is designed
for agents, and the ways a program can reach the outside world are designed
for that: a small set of operations, each named, each checked. An npm
package also has no semantic security (no `check` calls), so a blueprint
would have nothing to govern. You pay by wrapping your systems as packages.

## A package, from the inside

Here is one operation of the billing package, the one the previous
chapter's rules were about:

```typescript title="package/src/lib.ts (fragment)"
import { check } from "submilli:security";
import { post } from "submilli:http";
import secrets from "submilli:secrets";

const BASE = "https://billing.internal.example.com/v1";

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

    const key = secrets.get("BILLING_API_KEY");
    if (key === null) {
        throw new Error("BILLING_API_KEY is not configured for this blueprint");
    }
    const headers = new Map<string, string>();
    headers.set("Authorization", "Bearer " + key);
    const response = post(BASE + "/customers/" + customerId + "/credits", { amount }, headers);
    if (!response.ok) {
        throw new Error("billing API failed: HTTP " + response.status.toString());
    }
    return JSON.parse(response.body) as Credit;
}
```

Two lines make it an operation. The `@capability` tag **declares** it: a
name, and the fields a rule may test. The `check` call, from
`submilli:security`, **enforces** it: it takes the capability's name and
those fields, asks the blueprint whether the caller may do this with these
values, and throws `PermissionDeniedError` if not.

:::tip[Did you know?]
The compiler keeps the `@capability` tag and the `check` call in step. When
you build a package, it compares the fields the tag declares with the
fields the call passes. An undeclared payload field is an error; other
disagreements warn. `submilli build check --deny-warnings` also fails on
those warnings.


:::

This is semantic security from the package's side. The package decides
what the operation means and which facts describe it, and hands them to
the runtime typed: the customer, the amount, and the customer's class,
which the call didn't carry and the package looked up. A blueprint can then
say "premium customers only", and nobody had to read a payload.

## What the agent's program sees

To the model, a package is an import. Before it writes a program, the agent
searches for packages and reads a package's documentation. You can do the
same from the CLI:

```
submilli search billing
```

```text
@acme/billing — Credits for one customer of Acme's billing service.
```

```
submilli docs @acme/billing
```

```typescript
// @acme/billing — Credits for one customer of Acme's billing service.

/**
 * Add a goodwill credit to a customer's account.
 * @param customerId The customer's id in the billing system, such as `cus_northwind`.
 * @param amount The credit, in cents; must be positive.
 * @capability acme.com/credits.apply { customerId: string, customerClass: string, amount: number }
 * @returns The credit as recorded.
 */
function applyCredit(customerId: string, amount: number): Credit;

/**
 * A credit applied to a customer's account.
 */
interface Credit {
  /**
   * Amount in cents.
   */
  amount: number;
  /**
   * The customer credited.
   */
  customerId: string;
}
```

The agent's documentation tool returns the same declarations, doc comments
included, together with the package's readme. Then it writes the program:

```typescript title="credit.ts"
import { applyCredit } from "@acme/billing";

function main(): string {
    const credit = applyCredit("cus_northwind", 1500);
    return `credited ${credit.amount} cents`;
}
```

Calls are synchronous: no `await`, the program gets the value back. Run
under the previous chapter's blueprint, bound to `cus_northwind`:

```text
credited 1500 cents
```

The program never sees the billing API, its URL, or the key that
authenticates the request; the package holds all three. When the blueprint
refuses the call, the program gets the error you saw in the quickstart, and
the model reads it.

## The tools to build one

`submilli build` is to a package what `npm` is to a Node project: it
scaffolds the project, compiles it, derives what it can be granted from the
`@capability` tags, runs its tests, and installs it where programs can
import it. And with the skill installed, your coding assistant does the
writing: give it a service's API documentation and what the agent may do,
and it writes the package, the readme the model reads, and the tests, then
tests it under a blueprint.

The how-to pages on packages take each step in turn, starting with
[start a project](/docs/packages/start-a-project), and the
[build a package](/docs/tutorials/build-a-package) tutorial walks
through doing it with your assistant.

Next: [the server](/docs/server), the process that compiles the
program and answers every check.
