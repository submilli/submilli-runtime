---
title: "Permissions"
description: "Reference for blueprint permissions: how a call is decided, callers, actions, the standard library's capabilities and their fields, the filter language, and the errors a rule produces."
slug: permissions
sidebar:
  order: 9
---

This chapter is the reference for the `permissions` block of a blueprint:
how a call is decided, what each capability reports, and the language filters
are written in. [How Submilli works](/docs/how-submilli-works) explains why
permissions are written this way, and [crafting a blueprint](/docs/blueprints)
builds one step by step with the CLI.

## How a call is decided

```yaml
default: deny

permissions:
  main:
  - capability: fs.write
    filter: path glob "/notes/*"
    action: allow
  - capability: fs.read
    action: allow

  '@acme/billing':
  - capability: http.get
    filter: host == "billing.internal.example.com"
    action: allow
```

When code performs a gated operation, the runtime:

1. Names the **caller**: `main` for the program the model wrote, or the name
   of the package whose code is running.
2. Takes the caller's list under `permissions`.
3. Reads the list from the top and stops at the first rule whose
   `capability` equals the capability being checked and whose `filter`, if it
   has one, matches the fields the operation reports, its **context**.
4. Applies that rule's `action`.
5. Applies the blueprint's `default` if no rule matched or the caller has no
   list.

A capability name is matched exactly; names have no wildcards. A rule without
a filter matches every use of its capability.

| Field | Meaning |
| --- | --- |
| `capability` | The operation's name, such as `fs.write` or `acme.com/credits.apply`. Required. |
| `filter` | A condition on the operation's fields. Optional. |
| `action` | `allow`, `deny`, or `ask-human`. Required. |

## Actions and the default

| Action | Effect |
| --- | --- |
| `allow` | The operation proceeds. |
| `deny` | The operation throws `PermissionDeniedError`. |
| `ask-human` | Reserved for approval by a person. It is accepted in a blueprint and behaves as `deny`. |

`default` takes the same three values and is `deny` when the blueprint
doesn't set it. Under `default: allow`, the rules become a list of
exceptions and anything they don't name is permitted, including any operation
a package added later provides.

## Callers

Each list under `permissions` belongs to a **caller**: whose code asked for
the operation.

| Caller | Its rules apply to |
| --- | --- |
| `main` | The program the model wrote |
| A package's name, such as `'@acme/billing'` | What that package's own code does |

Take a program that calls `listCharges` from `@acme/billing`. The package
checks `acme.com/charges.list`, and that check is judged by the rules under
`main`, because the program asked for it. The package then sends its request
to the billing service, and that request is judged by the rules under
`'@acme/billing'`.

Submilli works out the caller itself, and a program can't claim to be a
package. A function the program hands to a package remains the program's
code, so what it does is judged under `main`.

A package with no list of its own may do only what `default` allows.
`submilli blueprint add-package` writes the list for you.

## Refusals no rule can change

Two refusals are made before the rules are read:

- `secrets.get` from `main`. A secret's value is available only to packages.
  `submilli blueprint capability add secrets.get` refuses to write the rule,
  and the runtime refuses the call even under `default: allow`.
- An operation Submilli can't assign to `main` or to a package.

## Capabilities

A capability comes from one of three places:

| Source | Names | Fields |
| --- | --- | --- |
| The standard library | `fs.*`, `git.*`, `http.*`, `llm.call`, `secrets.get`, `session.*` | Listed below |
| A package | Chosen by its author, such as `acme.com/credits.apply` | What the package passes to `check` |
| An MCP server in the blueprint | `mcp.<server>` | `tool`, `transport` |

`submilli blueprint capability list` prints every capability the blueprint
can use, from the standard library, its packages, and its MCP servers, with
their fields and the rules already written for each.

### The standard library

| Capability | Checked by | Fields |
| --- | --- | --- |
| `fs.read` | `read`, `readText`, `readBytes`, `lines`, `bytes` | `path` |
| `fs.write` | `write`, `writeText`, `append`, `appendText`, `writer`, and `download` for its destination | `path` |
| `fs.stat` | `stat`, `exists`, `size`, `peek` | `path` |
| `fs.list` | `list` | `path`, `recursive` |
| `fs.mkdir` | `mkdir` | `path`, `recursive` |
| `fs.remove` | `remove` | `path`, `recursive` |
| `fs.move` | `move` | `from`, `to` |
| `fs.copy` | `copy` | `from`, `to`, `recursive` |
| `http.get`, `http.post`, `http.put`, `http.patch`, `http.delete`, `http.head`, `http.options` | The request of that method | `host`, `path`, `body_size`, `timeout_ms` |
| `http.download` | `download` | `host`, `url_path`, `vfs_path`, `max_bytes`, `overwrite`, `decompress` |
| `llm.call` | `call`, `batch`, `models` | `model`, `prompt_count` |
| `secrets.get` | `get` | `name` |
| `session.read` | `get` and `has`; `list` checks it once for each key it would return, and leaves out the keys refused | `key` |
| `session.write` | `set` | `key` |
| `session.remove` | `remove` | `key` |
| `session.list` | `list` | `prefix` |
| `git.init`, `git.clone`, `git.fetch`, `git.commit` | See [Git capabilities](#git-capabilities) | |

A rule sees a path after `.` and `..` are resolved: a write to
`/notes/../secrets` is checked as `/secrets`. The same holds for both paths
of `move` and `copy`, and for the destination of `download`.

`llm.call` with a filter on `model` also limits what `models()` lists, so a
program is never shown a model it would be refused.

When the agent framework reads or lists the program's files directly, with
the tools [connecting to your harness](/docs/harness) describes, the request
is checked under `main`'s `fs.read` and `fs.list` rules, with the session's
variables.

### A package's capabilities

A package author declares an operation with `check` and a `@capability`
annotation that names its fields:

```typescript title="package/src/lib.ts (fragment)"
/**
 * Add a goodwill credit to a customer's account.
 * @capability acme.com/credits.apply { customerId: string, customerClass: string, amount: number }
 */
export function applyCredit(customerId: string, amount: number): Credit {
    const customerClass = lookUpClass(customerId);
    check("acme.com/credits.apply", { customerId, customerClass, amount });
    …
}
```

`submilli build` generates the package's `capabilities.yaml` automatically
from its source. `provides` lists
the operations the package declares. `requires` lists what the package's own
code calls: standard-library operations, and operations of packages it
imports. When an argument is fixed in the source, such as
`get("https://billing.internal.example.com/charges")`, the entry gets the
filter `host == "billing.internal.example.com" and path == "/charges"`.
`add-package` turns `requires` into the package's own list.

## Filters

A filter is a condition on the operation's context.

```text
amount < 500
customerId == ${vars.customerId} and customerClass == "premium"
not (host glob "*.internal.example.com")
path == "/${vars.userId}" or path glob "/${vars.userId}/*"
```

| Operator | True when |
| --- | --- |
| `==`, `!=` | The field equals, or differs from, a string, number, `true`, `false`, or `null` |
| `<`, `<=`, `>`, `>=` | The field is a number and compares so |
| `glob` | The field is a string and the whole of it fits the pattern |
| `matches` | The field is a string and the regular expression matches part of it |
| `contains` | The field is an array and one of its elements equals the value |

In a `glob` pattern, `*` stands for any run of characters, `?` for exactly
one, and `\` makes the next character literal. `*` crosses `/`, so
`path glob "/notes/*"` also matches `/notes/2026/a.md`.

`matches` succeeds when the pattern appears anywhere in the field:
`host matches "internal"` matches `api.internal.example.com`. Write `^` and
`$` around the pattern to match the whole field.

Conditions combine with `and`, `or`, and `not`. `not` binds tightest, then
`and`, then `or`; parentheses group. Strings are written in double quotes.

A field inside an object is named with a dot, `order.total`, and an element
of an array by its position, `items.0.sku`.

### Variables

`${vars.NAME}` stands for the value of a variable the blueprint declares.
It can stand alone, as in `customerId == ${vars.customerId}`, or sit inside a
quoted string, as in `path glob "/users/${vars.userId}/*"`. Quoted strings
take variables with `==`, `!=`, `glob`, and `contains`; `matches` doesn't
take them.

- A variable's value is a string. Beside a number field it is read as a
  number, and beside a boolean field as `true` or `false`.
- Inside a `glob` pattern a variable's value is taken literally. A value of
  `*` matches an asterisk and can't widen the pattern.
- A program can't read or set a variable.

### How a filter is evaluated

Evaluating a filter never fails. A condition that can't be decided is false.

| Situation | Result |
| --- | --- |
| The field is missing from the context | False, for every operator, `!=` included |
| The field's type doesn't suit the operator, such as `<` on a string | False |
| The variable has no value | False |
| `not` around a condition that is false for one of the reasons above | True |
| `field == null` | True only when the field is present and null |

Because of the fourth row, `not (owner == "ops")` is true for a context with
no `owner`. Write `allow` rules as conditions on what is present, because
`not` turns a missing field into a match.

## Errors when the blueprint is read

A blueprint is checked when it is linted and when it is registered, so a
mistake stops it before any program runs.

| Mistake | Error |
| --- | --- |
| A malformed filter | ``invalid filter `customerId = ${vars.customerId}`: expected `==`; a single `=` is not an operator``, with a caret under the fault |
| A variable the blueprint doesn't declare | `filter references undeclared variable '${vars.customer}'` |
| A regular expression that doesn't compile | `invalid regex:` and the reason |
| `matches` with a variable | `` `matches` takes a literal regex; `${vars.NAME}` interpolation isn't supported inside a regex pattern `` |
| An MCP capability with the tool in its name, `mcp.linear/save_issue` | Refused. Write `capability: mcp.linear` with `filter: tool == "save_issue"`. |
| A rule for an MCP server the blueprint doesn't declare | `permission rule 'mcp.x' references undeclared mcp server 'x'` |

`submilli blueprint lint` also compares the blueprint with its packages:

| Finding | Level |
| --- | --- |
| A package provides an operation that `main` has no rule for | Warning |
| A package requires a permission that its own list has no rule for | Error |
| A package requires a permission that its own list grants with a different filter, or denies | Warning |
| `default: allow` | Warning |
| A `secrets.get` rule under `main`, which can have no effect | Warning |
| A list for a caller that isn't among the blueprint's packages | Warning |
| A package among the blueprint's packages with no list of its own | Warning |

## Denials at run time

A refused operation throws `PermissionDeniedError`, which a program can
catch:

```text
error: PermissionDeniedError: permission denied: caller=main capability=acme.com/charges.list: policy denied acme.com/charges.list for main. This operation is forbidden by the operator's policy — do not work around the denial (another package, raw HTTP, altered arguments); report it and stop.
  fields: caller = "main", capability = "acme.com/charges.list", reason = "policy denied acme.com/charges.list for main"
  at listCharges (lib:26:38)  [thrown here]
```

The message names the caller, the capability, and a reason, followed by an
instruction addressed to the model that wrote the program. It doesn't name
the rule or the filter that refused. The reasons are:

| Reason | Cause |
| --- | --- |
| `policy denied <capability> for <caller>` | A `deny` rule, or the default |
| `policy requires human approval for <capability> (caller <caller>); ask-human is deferred and treated as deny` | An `ask-human` rule |
| `secret values are never available to main-module code, and no policy can grant this. …` | `secrets.get` from `main` |

## Git capabilities

The blueprint's [`git` block](/docs/blueprints#let-the-program-commit) turns
`submilli:git` on. The rules below decide what it may do. As with any
operation, the caller decides which list applies; a repository a package
hands the program is used under `main`.

Every Git capability reports `path`, the repository's directory.

| Capability | Used by | Other fields |
| --- | --- | --- |
| `git.init` | `init` | None |
| `git.clone` | `clone` | `remoteName`, `remote`, `branch` |
| `git.fetch` | `fetch`, `pull` | `remoteName`, `remote`, `branch` |
| `git.commit` | `commit` | `branch` |

Each operation needs only its own rule. `git.clone` creates and fills its
directory without `git.init`, `git.fetch`, or any `fs.*` rule, and `git.init`
creates its directory. `git.fetch` also covers updating the files when a
`pull` fast-forwards. `git.commit` checks the branch currently checked out.

Reading history, staging, creating or switching branches, and adding remotes
need no rule. They touch only the program's files.

If a rule filters on `branch`, name the branch in the call, as in
`Repository.clone(url, "/repo", { branch: "main" })`. Without it the
`branch` field is missing and the rule can't match. Every branch fetched is
checked before any of its data arrives.

`remote` is the full HTTPS URL, not a host:
`remote == "https://github.com/acme/project.git"`. It is compared after
`.` and `..` segments, the host's case, and a default port are normalized,
so `https://GitHub.com:443/acme/./project.git` is checked as the same URL.
URLs with credentials or a query string are refused, and so are redirects.
A named remote is checked by its current URL on every fetch. On a server,
connections also follow its [outbound network rules](/docs/server#outbound-network).

If the remote asks for credentials, Submilli sends the `GIT_TOKEN` secret
with the configured username. The program never sees the token, and nothing
else can supply Git credentials.

`fs.*` rules never let a program change `.git` directly, or move or remove a
directory that contains it; only `git.*` operations change a repository. Git
hooks and filters never run. The [standard library
chapter](/docs/standard-library#git-repositories) lists the repository
formats Submilli supports.

## With a coding agent

A coding agent with the [Submilli skill](/docs/skill) writes and reads
permissions for you. It uses the CLI commands this chapter describes, and
you review the blueprint it produces. These prompts were run with Claude
Code in a project with the skill installed and `@submilli/jina` in the
package store.

**Find out what a package lets you control.**

```text
What capabilities does the @submilli/jina package offer, and what fields can a rule filter on?
```

The agent runs `submilli docs @submilli/jina` and answers with the two
capabilities, `jina.ai/read` and `jina.ai/search`, the functions each one
covers, and the one field a rule can test: `host` on `jina.ai/read`. It
points out that `jina.ai/search` has no fields, so a rule can only allow or
deny search as a whole.

**Grant something narrow.**

```text
Add Jina to my blueprint so the agent can read pages from docs.python.org and nothing else.
```

The agent adds the package and writes the rules:

```yaml
permissions:
  main:
  - capability: jina.ai/read
    filter: host == "docs.python.org"
    action: allow
  - capability: jina.ai/search
    action: deny
```

It declares the `JINA_API_KEY` secret the package needs, runs
`submilli blueprint lint`, and tests the result with `submilli run`: a page
on docs.python.org is read, and another site, a look-alike host such as
`docs.python.org.evil.com`, and a search are each refused. The skill then
hands the change to its verifier, which reads the blueprint and the package
looking for a way around the rule, and the agent reports what it found and
what you still need to do, such as storing the key on the server.

Expect the second request to take several minutes. Testing the rule and
reviewing it is most of the work.

Next: [Submilli server](/docs/server), where the blueprint and packages you
have locally are registered with a running server.
