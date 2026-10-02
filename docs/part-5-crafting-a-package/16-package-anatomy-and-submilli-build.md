---
title: "Package anatomy and submilli build"
description: "What a Submilli package is made of and how submilli build turns it into something a blueprint can grant: the manifest, the source, capabilities and check, the derived capabilities.yaml, credentials, dependencies, and publishing."
slug: package-anatomy
sidebar:
  order: 16
---

A package is how an agent's programs reach a service of yours. The program
imports a function, the function checks the blueprint, and only then does
the package call the service, with a credential the program never sees. The
[quickstart](/docs/quickstart) built one in a few lines; this chapter covers
everything a package is made of.

Packages are made with `submilli build`, the part of the CLI for package
projects. It is to a package what `cargo` is to a Rust crate or `npm` to a
Node project: one tool that scaffolds the project, compiles its packages,
derives what they can be granted, runs their tests, and installs them where
programs can import them. There is nothing else to install.

| Command | Does |
| --- | --- |
| `submilli build init <@scope/name> [path]` | Create a project with its first package |
| `submilli build new <@scope/name> <path>` | Add a package to the project |
| `submilli build check` | Compile every package and write its `capabilities.yaml` |
| `submilli build test` | Compile, then run the tests and check the readme examples |
| `submilli build publish-local` | Compile, then install into the local package store |

`check`, `test`, and `publish-local` take `-p <@scope/name>` to work on one
package and the siblings it depends on.

The example is `@acme/billing`, the package the earlier chapters granted and
called.

## Start a project

```sh
submilli build init @acme/billing packages/billing
```

```text
created …/submilli.toml
created …/packages/billing/src/lib.ts
created …/packages/billing/tests/lib.test.ts
```

A project is a directory with a `submilli.toml`, and it holds one package or
several. `submilli build new <@scope/name> <path>` adds another. Every
`build` command finds the manifest in the current directory or a parent of
it, so you can run them from anywhere in the project.

| Path | Holds |
| --- | --- |
| `submilli.toml` | The manifest: one `[[package]]` block per package |
| `packages/billing/src/lib.ts` | The entry point. What it exports is the package's API. |
| `packages/billing/src/*.ts` | Other source files, imported with a relative path |
| `packages/billing/docs/readme.md` | The documentation the model reads before it writes a program |
| `packages/billing/tests/*.test.ts` | Tests, covered in [testing packages](/docs/testing-packages) |
| `packages/billing/capabilities.yaml` | Written by the build: what the package provides and requires |
| `tsconfig.json`, `.vscode/`, `.submilli/` | Editor files, covered in [editor setup](/docs/editor-setup) |

The scaffold's `hello()` function and one-line readme are placeholders to
replace.

## The manifest

```toml title="submilli.toml"
[[package]]
name = "@acme/billing"
version = "0.1.0"
description = "Credits and invoices for one customer of Acme's billing service."
keywords = ["billing", "credits", "invoices"]
path = "packages/billing"
```

| Field | Meaning |
| --- | --- |
| `name` | `@scope/name`, the name programs import |
| `version` | The package's version |
| `description`, `keywords` | What `submilli search` matches, and what the model sees when it looks for a package. Write them for that reader. |
| `path` | The package's directory, relative to the manifest |
| `dependencies` | Other packages this one imports; see [dependencies](#dependencies) |

## The source

A package is written in [the language](/docs/the-language) programs are
written in, with the same [standard library](/docs/standard-library). What
`src/lib.ts` exports is the package's API. A function in another file is
internal unless `lib.ts` exports it:

```typescript title="packages/billing/src/lib.ts (fragment)"
import { check } from "submilli:security";
import { lookUpClass } from "./classes";

/** A credit applied to a customer's account. */
export interface Credit {
    /** The customer credited. */
    customerId: string;
    /** Amount in cents. */
    amount: number;
}

/**
 * Add a goodwill credit to a customer's account.
 * @capability acme.com/credits.apply { customerId: string, customerClass: string, amount: number }
 */
export function applyCredit(customerId: string, amount: number): Credit {
    if (amount <= 0) {
        throw new RangeError("amount must be positive");
    }
    const customerClass = lookUpClass(customerId);
    check("acme.com/credits.apply", { customerId, customerClass, amount });

    // In production, this would call the billing API.
    return { customerId, amount };
}
```

The doc comments are part of the API. They are what `submilli docs` prints
and what the model reads, and the build warns about an export without one:

```text
warning: exported symbol `hello` has no doc comment
```

## Capabilities and `check`

Two lines make an operation something a blueprint can rule on.

The `@capability` tag **declares** it: a name, and the fields a rule may
test. Name a capability `<domain>/<resource>.<verb>`, one per operation, so a
blueprint can allow reading without allowing writing.

The `check` call **enforces** it. It asks the blueprint whether the caller
may do this, with these values, and throws `PermissionDeniedError` if not.
The tag alone enforces nothing, so put `check` before the effect it guards:
nothing protected may run before it passes.

You write both, and the compiler keeps them in step. `submilli build check`
compares each tag with the `check` in its function and warns when they
disagree: a `check` with no tag, a tag with no `check`, a field in one and
not the other, or a tag that names a parameter the function doesn't have.

```text
warning: payload key `customerId` missing from `@capability` binding
  --> packages/billing/src/lib.ts:73:36
   |
73 |     check("acme.com/refunds.make", { customerId });
   |                                    ^^^^^^^^^^^^^^
```

Put the `check` directly in the body of the exported function it guards.
The compiler warns about a `check` anywhere else: in a function the package
doesn't export, where it runs only if some exported function happens to call
it, or in a nested function, which may run later, more than once, or never.

Read each property of an object the program passes in once, into a `const`,
and use that `const` for both the `check` and the effect. A property can
return a different value each time it is read, so a function that reads
`input.customerId` twice can check one customer and act on another. The
compiler warns about a second read, and about the object being passed on to
another function, for any value that reaches a `check` or decides whether or
which `check` runs. A value that reaches no `check` can be read and passed on
freely: the program could pass anything in its place anyway. Strings,
numbers, and booleans can't change this way. When the compiler can't follow
what reaches a `check` in a function, it examines every value the program
passed in, and the warning says why.

A field in the tag is written one of four ways:

| Form | Meaning |
| --- | --- |
| `customerId` | A parameter of that name, with the parameter's type |
| `orderId: $id`, `team: $input.teamId` | A parameter, or a path inside one, under another name |
| `amount: number`, `tags: string[]` | A value the package computes, with its type |
| `kind: "order"` | A fixed value |

### The payload says what the operation means

A rule never sees the request the package sends to the service. It sees two
things: the capability's name, and the payload the package passes to
`check`. That is what [semantic
security](/docs/how-submilli-works#semantic-security-rules-about-what-an-operation-means)
means. The blueprint rules on what an operation *does*, such as "credit this
customer this much", instead of on URLs and request bodies it would have to
interpret. The meaning is whatever the package writes into the payload.

That makes the payload the most important decision in a package. A policy can
only be as precise as the payload it is given: a fact that isn't in the
payload can never appear in a rule. So build the payload for the rules people
will want to write, not from the arguments the function happens to take. Ask
what an operator would want to limit: which customer, how much, sent where,
from which state to which.

**Facts the caller didn't pass.** Some of those facts aren't arguments.
`applyCredit` takes a customer and an amount. Whether the customer is premium
is a fact about the account, so the package looks it up and puts
`customerClass` in the payload, and a blueprint can then allow credits for
premium customers only. Don't take such a fact from the caller: the caller is
the program you are guarding against.

**The scope a session works in.** The field that matters most is often one
no call mentions: the scope the application authorizes a session to work
in. In a multi-tenant system that is the tenant. In Linear it is the team or
the project. An agent working for one team should touch only that team's
issues, so every operation needs the team in its payload, including those
that take only an issue id, such as updating an issue or commenting on it.
The package fetches the issue, reads its team, and checks
`{ teamId, issueId }`. One rule, `teamId == ${vars.teamId}`, then holds
across every operation, and the application binds the team for each session
the way it binds a customer. An operation that leaves the scope out of its
payload is the way around that rule.

## Build it

```sh
submilli build check
```

```text
checked @acme/billing v0.1.0
```

`build check` compiles every package in the project, in dependency order,
and installs nothing. A compile error stops the build and exits 1:

```text
error: expected `number`, got `string`
  --> packages/support/src/lib.ts:9:43
   |
 9 | export function broken(): number { return "x"; }
   |                                           ^^^
```

The build also writes `capabilities.yaml` beside the source:

```yaml title="packages/billing/capabilities.yaml"
namespace: acme
provides:
- name: acme.com/credits.apply
  description: Add a goodwill credit to a customer's account.
  fields:
    amount:
      type: number
    customerClass:
      type: string
    customerId:
      type: string
- name: acme.com/invoices.latest
  description: Fetch a customer's latest invoice from the billing service.
  fields:
    customerId:
      type: string
requires:
- capability: http.get
  filter: host == "billing.acme.com"
- capability: secrets.get
  filter: name == "BILLING_API_KEY"
```

`provides` is what the package offers: the capabilities a blueprint grants to
programs, and the fields their rules may test. `requires` is what the package
itself needs from the standard library. Both are derived from the source, so
don't edit the file. `submilli blueprint add-package` reads it and writes the
package's own grants into the blueprint, as [crafting a
blueprint](/docs/blueprints) showed.

## Calling a service

`latestInvoice` is where `requires` came from:

```typescript title="packages/billing/src/lib.ts (fragment)"
import { get } from "submilli:http";
import secrets from "submilli:secrets";

const BASE = "https://billing.acme.com/v1";

/**
 * Fetch a customer's latest invoice from the billing service.
 * @capability acme.com/invoices.latest { customerId: string }
 */
export function latestInvoice(customerId: string): Invoice | null {
    check("acme.com/invoices.latest", { customerId });
    const key = secrets.get("BILLING_API_KEY");
    if (key === null) {
        throw new Error("BILLING_API_KEY is not configured for this blueprint");
    }
    const headers = new Map<string, string>();
    headers.set("Authorization", "Bearer " + key);
    const response = get(BASE + invoicePath(customerId), headers);
    if (response.status === 404) {
        return null;
    }
    if (!response.ok) {
        throw new Error("billing API failed: HTTP " + response.status.toString());
    }
    return JSON.parse(response.body) as Invoice;
}
```

Three habits keep the grants narrow and the credential inside the package:

- **Keep the host in a constant.** The build reads the host out of `BASE`
  and writes `host == "billing.acme.com"` into `requires`. A host that
  arrives in a parameter can't be derived, and the build warns that the
  filter has lost it.
- **Read the secret by its literal name.** `secrets.get("BILLING_API_KEY")`
  becomes `name == "BILLING_API_KEY"`. The blueprint says where the value
  comes from; the package only names it.
- **Never return the credential.** Don't export a function that returns the
  key, accept a destination that will carry it, or log the headers. A
  program can't call `secrets.get` itself, so the package is the only place
  the value exists.

## The readme

`docs/readme.md` is written for the model. It is what an agent reads, through
its documentation tool, before it writes a program that imports the package.
Say what the package is for, what each operation takes and returns, what
comes back as `null`, and what a denial means. End with one complete `main`.

Its `ts` examples are compiled against the package by `submilli build test`,
so an example can't drift from the API it shows.

## Dependencies

A package imports another by name once the manifest declares it:

```toml title="submilli.toml (fragment)"
[dependencies]
"@submilli/jina" = "0.1.0"

[[package]]
name = "@acme/support"
version = "0.1.0"
path = "packages/support"
dependencies = ["@acme/billing", "@submilli/jina"]
```

| The dependency is | Declare it |
| --- | --- |
| Another package in the project | In the package's `dependencies` |
| A package in the local store | There, and in `[dependencies]` with its version |
| A package in a GitHub repository | There, and in `[dependencies]` as `{ github = "github.com/org/repo", rev = "<commit>" }` |

A GitHub dependency is fetched into the local store by the build, which
records the commits it used in `submilli.lock`. A name that isn't declared
stops the build:

```text
error: declare dependency "@acme/missing" as a sibling [[package]] or in top-level [dependencies]
```

A dependency in a private repository is declared the same way. Whoever builds
or installs it needs a GitHub token that can read it: [on a
machine](/docs/cli#install-a-package), or
[`github_token_file`](/docs/server#install-packages) on a server.

What a package uses of another shows up in its `requires`. `@acme/support`
calls `applyCredit`, so it requires `acme.com/credits.apply`, and a blueprint
grants that to `@acme/support` as it would to a program.

## Publish it

```sh
submilli build publish-local
```

```text
installed @acme/billing v0.1.0 -> ~/.submilli/packages/@acme/billing
```

`publish-local` compiles and installs into the local store, where `submilli
run` and a server on the same machine find it. Nothing is uploaded. Check
what programs will see:

```sh
submilli docs @acme/billing
```

Then try it under a blueprint, which is the only test of the policy:

```sh
submilli blueprint init support
submilli blueprint add-package @acme/billing
submilli run --blueprint blueprint.yaml credit.ts
```

```text
error: PermissionDeniedError: permission denied: caller=main capability=acme.com/credits.apply: policy denied acme.com/credits.apply for main. …
```

```sh
submilli blueprint capability add acme.com/credits.apply --filter 'customerClass == "premium"'
submilli run --blueprint blueprint.yaml credit.ts
```

```text
credited 1500 cents
```

Other machines install from source. Push the project to GitHub, then
`submilli install org/repo` on a developer's machine or [`submilli server
packages install`](/docs/server#install-packages) on a server builds it
there, pinned to a commit. A private repository works the same, given a
GitHub token that can read it.

## With a coding agent

A coding agent with the [Submilli skill](/docs/skill) builds a package from
a real service's API documentation. It writes the package, the readme the
model reads, the tests, and a blueprint, and it tests all of them against
the service. This run used Claude Code, the skill, and an
[Attio](https://attio.com) workspace: a CRM, the kind of service a support
agent needs.

The agent needs the service's API key to test against it. Put the key in an
environment variable, or in a `.env` file beside `submilli.toml`, and tell
the agent which one. The key never has to appear in the conversation:

```sh title=".env"
ATTIO_API_KEY=…
```

```text
Build a read-only Submilli package over the Attio REST API (https://docs.attio.com/rest-api/overview) for our support agent. The agent may look up the company it is serving and read that company's people, notes, and tasks. It must never read any other company's records. The Attio API key is in the ATTIO_API_KEY environment variable.
```

The agent read Attio's reference pages for the four endpoints it needed and
sampled the workspace to see the real response shapes. Then it scaffolded
the project and produced:

| File | Holds |
| --- | --- |
| `src/lib.ts` | `getCompany`, `listPeople`, `listNotes`, `listTasks`, each checking `{ companyId }` |
| `docs/readme.md` | What the model reads: each operation, paging, and that a denial means the record is forbidden, so don't retry with other ids |
| `tests/lib.test.ts` | Unit tests of the request builders, and live reads that skip without the key |
| `blueprint.yaml` | A required `companyId` variable, and one rule per operation |
| `verify.sh` | Programs run under the blueprint: the allowed reads and each way the rule should refuse |

`src/lib.ts` is about 500 lines. Most of it describes Attio's response
shapes and turns them into the four small types the model sees. Here is one
of those types and the operation that returns it:

```typescript title="src/lib.ts (fragment)"
/** A note attached directly to the company record. */
export interface Note {
    /** Note id (UUID). */
    id: string;
    /** Note title. */
    title: string;
    /** Body as Markdown. */
    content: string;
    /** ISO 8601 creation time. */
    createdAt: string;
}

/**
 * List notes attached directly to this company, as the Attio API orders them.
 * Notes attached to the company's people are not included.
 * @capability attio.com/notes.list { companyId: string }
 */
export function listNotes(companyId: string, page: PageOptions | null = null): Page<Note> {
    const limit = page === null ? null : page.limit;
    const offset = page === null ? null : page.offset;
    const id = normalizeRecordId(companyId);
    check("attio.com/notes.list", { companyId: id });
    const paging = resolvePage(limit, offset);
    const response = request("GET", "/notes" + buildNotesQuery(id, paging), null);
    if (response.status === 404) {
        // Attio answers 404 when the parent record does not exist.
        return { items: [], nextOffset: null };
    }
    const notes = (JSON.parse(response.body) as ListEnvelope<RawNote>).data;
    const items: Note[] = [];
    for (const note of notes) {
        if (note.parent_object === "companies" && note.parent_record_id === id) {
            items.push({
                id: note.id.note_id,
                title: note.title,
                content: note.content_markdown,
                createdAt: note.created_at,
            });
        }
    }
    return { items: items, nextOffset: nextOffset(paging, notes.length) };
}
```

The id is normalized before the check, and the normalized id is the one
checked and the one sent, so the rule sees exactly what Attio receives. Each
paging field is read once, before the check, and the helper takes the fields
and not the object the program passed. The
loop keeps only notes whose parent is that company, so a response that
somehow names another company's note doesn't reach the program. `Note` is
the package's own type: the program never sees Attio's field names.

The rules are the ones the [payload](#the-payload-says-what-the-operation-means)
section describes. Every operation carries the company, so one filter covers
all four:

```yaml title="blueprint.yaml (fragment)"
permissions:
  main:
  - capability: attio.com/notes.list
    filter: companyId == ${vars.companyId}
    action: allow
```

Two details weren't in the prompt. The first came from the service:
Attio's notes and tasks endpoints return every record in the workspace when
their company filter is missing, so `normalizeRecordId` refuses anything but
a record id, and each result's owner is checked before it is returned, as
`listNotes` does above. The second was the agent's own choice: it narrowed
the package's grant from all of `api.attio.com` to the paths it calls.

It tested against the live workspace. `submilli build test` passed, live
reads included. Under the blueprint, a session bound to one company read
that company, its people, notes, and tasks; all four operations were refused
for another company; and calling Attio directly, reading the key, and a
session with no company were refused too. Two controls showed the rule was
the reason: bound to the other company, the results reversed, and with the
filter removed, the other company's data came through.

The skill has a second agent review the work, and in this run it did so the
way an attacker would: with ids in other forms, look-alike characters, and a
program that catches a denial and carries on. It found no way to another
company's records, and its findings led to four fixes, among them leaving
out a task linked to two companies.

The agent's final report ended with decisions for you: whether tasks shared
with another company should show, that notes on the company's people aren't
included, and that the application must bind `companyId` as the company's
record id. The workspace had no notes or tasks yet, so it said that
filtering them was tested only on sample data. The run took about twelve minutes.

### Packages that write

This example is read-only on purpose. A package that writes gets tested by
writing, so where the agent does that matters. If the service has a sandbox
or test mode, give the agent a key for it. If it doesn't, tell the agent
which records it may change, such as one test company, and that it must not
touch anything else. Without that, a careful agent tests writes only with
unit tests and says so; a less careful one writes to your real data.

Next: [editor setup](/docs/editor-setup).
