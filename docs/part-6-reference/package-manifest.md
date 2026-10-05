---
title: "Package manifest"
description: "A Package project: its layout, every key of submilli.toml and the dependency forms, submilli.lock, the doc-comment tags the build reads, the derived capabilities.yaml, docs/readme.md, and the test API of submilli build test."
slug: reference/package-manifest
sidebar:
  order: 11
authorship:
  label: ai-assisted
  confirmed: true
  contentHash: "c79ae1075feef5fdf61d7bd8eff0301abd89f4b9f3b52a3df12f06a073bfa862"
  confirmedAt: "2026-10-05T13:01:53.009Z"
---

This page describes a Package project as `submilli build` reads and writes
it. It covers the files in the project, `submilli.toml`, `submilli.lock`, the
doc-comment tags in a Package's source, the derived `capabilities.yaml`,
`docs/readme.md`, and the test files and test API. The commands themselves
are in the [CLI reference](/docs/reference/cli). The how-to pages start at
[Start a project](/docs/packages/start-a-project).

## Project layout

A project is a directory that holds a `submilli.toml`. `submilli build check`,
`test`, and `publish-local` use the first `submilli.toml` found in the
current directory or a parent of it. `init` and `new` use the one in the
current directory.

`submilli build init @acme/billing packages/billing` creates:

```text
created …/submilli.toml
created …/packages/billing/src/lib.ts
created …/packages/billing/docs/readme.md
created …/packages/billing/README.md
created …/packages/billing/tests/lib.test.ts
```

`init` takes the Package name (prompted on stdin when omitted) and the
Package's path relative to `submilli.toml` (default `.`).
`submilli build new <@scope/name> <path>` appends a `[[package]]` table to the
`submilli.toml` in the current directory and creates the same four Package
files.

| Path | Created by | Holds |
| --- | --- | --- |
| `submilli.toml` | `init`, and `new` appends | The [manifest](#submillitoml) |
| `<path>/src/lib.ts` | `init`, `new` | The entry point, whose exports are the Package's API |
| `<path>/src/**/*.ts` | You | Other modules, imported by relative path |
| `<path>/docs/readme.md` | `init`, `new` | [The readme the model reads](#docsreadmemd). Required |
| `<path>/README.md` | `init`, `new` | The readme for the person who installs and grants the Package. Not read by the build |
| `<path>/tests/**/*.test.ts` | `init`, `new` (`lib.test.ts`) | [Test files](#test-files) |
| `<path>/capabilities.yaml` | Every `check`, `test`, `publish-local` | [What the Package provides and requires](#capabilitiesyaml). Rewritten on every build |
| `submilli.lock` | A build with a GitHub dependency | [The resolved GitHub dependencies](#submillilock) |
| `tsconfig.json` | `init` | One line extending `.submilli/tsconfig.submilli.json` |
| `.vscode/tasks.json` | `init` | A build task that runs `submilli build check` |
| `.gitignore` | `init` | The entry `.submilli/` |
| `.submilli/` | `init`, and refreshed on every build | Editor configuration and type declarations of the standard library, the project's Packages, and their dependencies |

The `.subm` extension is accepted wherever `.ts` is, in `src/lib.subm`, other
modules, and `*.test.subm`. A Package with both `src/lib.ts` and
`src/lib.subm` is an error, and so is one with neither.

## submilli.toml

The manifest has one `[[package]]` table per Package and an optional
top-level `[dependencies]` table. Keys the build doesn't know are ignored.

```toml title="submilli.toml"
[dependencies]
"@submilli/jina" = { github = "github.com/submilli/submilli-runtime", rev = "b3e33dfd1f7d7cdbb63bcd3843c2b7902ecffdf1" }

[[package]]
name = "@acme/billing"
version = "0.1.0"
description = "Credits and invoices for one customer of Acme's billing service."
keywords = ["billing", "credits", "invoices"]
path = "packages/billing"

[[package]]
name = "@acme/support"
version = "0.1.0"
description = "Support replies that cite the status page and apply goodwill credits."
path = "packages/support"
dependencies = ["@acme/billing", "@submilli/jina"]
```

### [[Package]]

At least one `[[package]]` table is required.

| Key | Type | Required | Default | Constraints |
| --- | --- | --- | --- | --- |
| `name` | string | Yes | — | `@org/name`. `org`: 1–39 ASCII letters, digits, and single internal hyphens. `name`: ASCII letters, digits, `.`, `_`, `-`. Unique in the manifest. |
| `version` | string | Yes | — | Any string. Not parsed as a semantic version. |
| `description` | string | Yes | — | Not empty or whitespace. |
| `keywords` | array of strings | No | `[]` | Each not empty or whitespace. |
| `path` | string | When the manifest has two or more Packages | `.` | Relative to `submilli.toml`. May not be absolute or climb above the manifest's directory. |
| `dependencies` | array of strings | No | `[]` | Each the `name` of another `[[package]]`, or a key of `[dependencies]`. |

`name` is the name programs import. `submilli search` and the agent's
Package search match `name`, `description`, `keywords`, and the names of
the Package's exports, case-insensitively, as substrings.
`description` is the line `submilli search` and `submilli docs` print after
the name.

### [dependencies]

A key is a Package name, and its value takes one of two forms.

| Form | Example | Resolved from |
| --- | --- | --- |
| Version string | `"@submilli/jina" = "0.1.0"` | The local Package store. The stored Package's version must equal the string exactly. |
| GitHub table | `"@submilli/jina" = { github = "github.com/org/repo", rev = "<sha>" }` | The repository at that commit, fetched into the local Package store by the build. |

In the GitHub table, `github` must name a `github.com/` repository
(`https://` and `http://` prefixes are accepted) and `rev` must be a full
40-character hexadecimal commit SHA. Branches and tags are refused. A GitHub
dependency takes its version from the fetched Package's own manifest. A
private repository needs a GitHub token that can read it (see
[Add a dependency](/docs/packages/add-a-dependency)).

A Package's `dependencies` entry resolves to a sibling `[[package]]` first,
then to `[dependencies]`.

### Manifest errors

Each error stops the build with exit code 1 and points at the manifest line.

| Cause | Message |
| --- | --- |
| No `[[package]]` | `add a [[package]] table to submilli.toml` |
| `name` not scoped | ``package `acme/demo` must use scoped form `@org/name` (GitHub org as the scope)`` |
| Two Packages with one `name` | `rename duplicate package "@acme/demo"; package names must be unique` |
| `description` missing | `add description = "..." for package "@acme/demo"` |
| `description` empty | `replace empty package description with a one-line summary` |
| Empty keyword | `remove empty package keyword or replace it with a search term` |
| `path` missing in a multi-Package manifest | `add path = "..." for package "@acme/demo"; path is required in a monorepo` |
| `path` absolute | `replace path "/x" with a path relative to submilli.toml` |
| `path` above the manifest | `replace path "../x"; package paths may not escape the manifest directory` |
| No entry point | `create packages/billing/src/lib.ts; package entrypoints are conventional` |
| Undeclared dependency | `declare dependency "@acme/missing" as a sibling [[package]] or in top-level [dependencies]` |
| `rev` not a commit SHA | `pin rev "main" to a full 40-character commit SHA; branches and tags are not reproducible` |
| `github` not on github.com | `point github "gitlab.com/x/y" at a github.com repository (e.g. "github.com/org/repo")` |
| Versioned dependency not in the store | ``failed to load external dependency `@submilli/jina`: package `@submilli/jina` was not found in …`` |
| Stored version differs | ``external dependency `@submilli/jina` has version 0.2.0 in the store, but submilli.toml requires 0.1.0; …`` |
| `docs/readme.md` missing | ``package `@acme/demo` is missing documentation at …/docs/readme.md; create docs/readme.md: …`` |

## submilli.lock

The build writes `submilli.lock` beside `submilli.toml` when the project has
GitHub dependencies, and deletes it when it has none. It records every
GitHub dependency in the closure, transitive ones included, and later builds
reuse the recorded commits.

```toml title="submilli.lock"
[[package]]
name = "@submilli/jina"
version = "0.1.0"
github = "github.com/submilli/submilli-runtime"
sha = "b3e33dfd1f7d7cdbb63bcd3843c2b7902ecffdf1"
source_hash = "sha256:7a91f7149c8eb353896386cfe938c1c9b8cb9d9bce076571e791d0b2e8914e6f"
```

| Key | Holds |
| --- | --- |
| `name` | The Package's name |
| `version` | The version in the fetched Package's manifest |
| `github` | The repository |
| `sha` | The commit |
| `source_hash` | `sha256:` and the SHA-256 of the downloaded source archive |

Entries are sorted by `name`.

## Doc comments

A doc comment is a `/** … */` comment directly before a declaration. The
text before the first tag is the summary. A tag starts with `@` at the
beginning of a line (after the optional leading `*`) and runs to the next
tag or the end of the comment. A `{type}` right after a tag name, as in
JSDoc, is skipped.

| Tag | Form | Read by the build | Produces |
| --- | --- | --- | --- |
| Summary | Text before the first tag | Yes | The declaration's description in `submilli docs`, and the `description` of a capability in `capabilities.yaml` |
| `@param` | `@param <name> <description>` | Yes | The parameter's description in `submilli docs`, and the `description` of a `capabilities.yaml` field bound to that parameter |
| `@returns`, `@return` | `@returns <description>` | Yes | The return value's description in `submilli docs` |
| `@capability` | `@capability <name> [{ <bindings> }] [<description>]` | Yes | A capability the function provides (see [@capability](#capability)) |
| `@throws`, `@throw` | `@throws <text>` | Accepted | Not shown by `submilli docs` |
| `@deprecated` | `@deprecated <text>` | Accepted | Not shown by `submilli docs` |
| `@example` | `@example <text>` | Accepted | Not shown by `submilli docs` |
| Any other | — | — | Warning ``unknown JSDoc tag `@since` `` |

`submilli docs` and the agent's documentation tool show each export's
summary, `@param`, `@capability`, and `@returns` lines.

The build checks a documented function or method against its signature and
warns:

| Warning | Cause |
| --- | --- |
| ``exported symbol `hello` has no doc comment`` | An export with no doc comment |
| ``parameter `a` is undocumented (missing `@param a`)`` | A parameter with no `@param` |
| ``destructured parameter 1 is undocumented (add a `@param` in its position)`` | A destructured parameter with no `@param` at its position |
| ``duplicate `@param a` `` | Two `@param` tags for one parameter |
| ``` `@param x` does not match any parameter of this function``` | A `@param` naming no parameter |
| ``` `@param b` is out of order — appears before earlier params in the signature``` | `@param` tags in a different order from the parameters |
| ``missing `@returns` on a non-`void`-returning function`` | A function returning a value, with no `@returns` |
| ``` `@returns` on a `void`-returning function``` | `@returns` on a `void` function |
| ``unknown JSDoc tag `@parm` `` | A tag not in the table above |

On a property, `@param` and `@returns` are warned about too.

### @capability

```text
@capability <name> [{ <field>, <field>: <value>, … }] [<description>]
```

`<name>` runs to the first space or `{`. The braces list the payload's
fields. `{}` is a capability with no fields. Text after the closing brace is
the tag's description, with a leading `-` or `—` removed. `submilli docs`
shows it on the tag's line, and `capabilities.yaml` doesn't contain it. A
function may carry several `@capability` tags.

A field is written one of these ways:

| Form | Example | The field is | Its type in `capabilities.yaml` |
| --- | --- | --- | --- |
| `<field>` | `customerId` | The parameter of that name | The parameter's type |
| `<field>: $<param>` | `orderId: $id` | The parameter `<param>` | The parameter's type |
| `<field>: $<param>.<path>` | `team: $input.teamId` | A property inside the parameter, by a dotted path | The property's type |
| `<field>: <type>` | `amount: number`, `tags: string[]` | A value the Package computes | The type as written |
| `<field>: "<text>"` | `kind: "goodwill"` | A fixed string | `string` |
| `<field>: <number>` | `version: 2` | A fixed number | `number` |
| `<field>: true`, `false` | `live: true` | A fixed boolean | `boolean` |
| `<field>: null` | `parent: null` | A fixed `null` | `null` |

A parameter or path the build can't resolve gets the type `unknown`. Type
aliases are reduced to the type they name.

The build compares each tag with the `check` calls (from
`submilli:security`) in the same function:

| Severity | Message | Cause |
| --- | --- | --- |
| Error | ``payload key `extra` missing from `@capability` binding`` | A `check` payload field the tag doesn't list |
| Warning | ``` `@capability` binding key `team` is missing from `check()` payload``` | A tag field the `check` payload doesn't contain |
| Warning | ``missing `@capability acme.com/tickets.reopen` for `check()` call`` | A `check` with no tag of that name |
| Warning | ``extra `@capability acme.com/tickets.open` has no matching `check()` call`` | A tag with no `check` of that name |
| Warning | ``unknown parameter binding `$team` in `@capability` `` | A field naming no parameter |
| Warning | ``unknown field `idd` in `@capability` binding `$ticket` `` | A path naming no property of the parameter's type |
| Warning | ``dynamic capability string in `check()`; use a string literal`` | A `check` whose capability name isn't a string literal |

A warning doesn't stop the build. With `--deny-warnings`, or
`SUBMILLI_DENY_WARNINGS=1`, any code warning, these and the
documentation warnings alike, fails `submilli build check`, `build test`,
`build publish-local`, `submilli install`, and `submilli server packages
install`, which then installs nothing.

Where a `check` may be called, and how each value reaching it must be read,
is in [Export a function](/docs/packages/export-a-function).

## capabilities.yaml

`capabilities.yaml` is derived from the Package's source and rewritten in
the Package's directory by every `submilli build check`, `test`, and
`publish-local`. The same file is part of the installed Package.
`submilli blueprint add-package` reads it.

```yaml title="packages/billing/capabilities.yaml"
namespace: acme
provides:
- name: acme.com/credits.apply
  description: Add a goodwill credit to a customer's account.
  fields:
    amount:
      type: number
      description: The credit, in cents; must be positive.
    customerClass:
      type: string
    customerId:
      type: string
      description: The customer's id in the billing system, such as `cus_northwind`.
    kind:
      type: string
- name: acme.com/invoices.list
  description: List a customer's invoices from the billing service.
  fields:
    customer:
      type: string
      description: The customer and how many invoices to return.
requires:
- capability: http.get
  filter: host == "billing.acme.com"
- capability: secrets.get
  filter: name == "BILLING_API_KEY"
```

The tags that produced it:

```typescript title="packages/billing/src/lib.ts (fragment)"
/**
 * Add a goodwill credit to a customer's account.
 * @param customerId The customer's id in the billing system, such as `cus_northwind`.
 * @param amount The credit, in cents; must be positive.
 * @returns The credit as recorded.
 * @capability acme.com/credits.apply { customerId, customerClass: string, amount, kind: "goodwill" }
 */
export function applyCredit(customerId: string, amount: number): Credit {

/**
 * List a customer's invoices from the billing service.
 * @param query The customer and how many invoices to return.
 * @returns The invoices' ids.
 * @capability acme.com/invoices.list { customer: $query.customerId }
 */
export function listInvoices(query: InvoiceQuery): string[] {
```

| Key | Holds |
| --- | --- |
| `namespace` | The Package name's scope, without `@` (`acme` for `@acme/billing`) |
| `provides` | One entry per capability name declared by an `@capability` tag, sorted by name |
| `provides[].name` | The capability's name |
| `provides[].description` | The summary of the first documented callable that declares it. Omitted when empty |
| `provides[].fields` | Each field, sorted by name, with its `type` and, for a field bound to a parameter, that parameter's `@param` description |
| `requires` | One entry per distinct capability and filter the Package's code calls, sorted |
| `requires[].capability` | The capability of a called standard-library function or function of a dependency |
| `requires[].filter` | The filter derived from the call. Omitted when nothing was derived |

For `provides`, the build reads the exported functions, in name order, then
the public static and instance methods of each exported class,
including instance methods inherited from a class of the same Package.
When several callables declare one capability, their fields are merged into
one entry.

For each call to a function that carries a `@capability` tag, the build
writes the tag's capability to `requires`, and a filter with one
`<field> == <value>` term per field whose value is known at build time,
joined with `and`:

| The tag's field is | The term is derived when |
| --- | --- |
| A fixed value (`kind: "goodwill"`) | Always |
| A parameter (`name`, `name: $param`, `name: $param.path`) | The argument is a literal, a top-level constant, or a `+` concatenation of string literals and constants. For a path, an object literal holding one at that path |
| `$url.host` on an `http.*` function | The URL is built only from string literals and top-level string constants joined with `+`, or begins with such a part holding the scheme, the host, and the `/` after it |
| `$url.path` on an `http.*` function | The URL is built only from string literals and top-level string constants joined with `+` |
| A computed value (`host: string`) | Never |

A field the build can't derive is left out of the filter, with a warning:

```text
warning: non-literal argument for `path`; no static capability filter for `path`
```

For an HTTP host the warning is ``cannot statically resolve the host in the
URL passed to `http.get`; no host capability filter was derived``. A
relative path given to a filesystem function gets no path filter, since it
depends on the session's working directory. A Package that calls
`read` from `@submilli/jina`, whose tag binds `host: string`, requires
`jina.ai/read` with no filter.

## docs/readme.md

`<path>/docs/readme.md` is required, and a Package without it fails to build. It
is copied into the installed Package. The server's documentation tool and
HTTP API return it, followed by a `## Declarations` section with
the Package's declarations. `submilli docs` prints only the declarations.

`submilli build test` compiles every fenced block in it whose info string is
exactly `ts` or `typescript`, against the Package and its dependencies, and
counts each as a test, named `example <n> (compile)`. The blocks are
compiled, not run. A block with any other info string, such as
`ts ignore`, `text`, or `yaml`, is skipped. A compile error is reported at
the readme's line.

`<path>/README.md`, at the Package's root, is not read by the build.

## Test files

`submilli build test` runs, for each Package, every file named `*.test.ts`
or `*.test.subm` anywhere under `<path>/tests/`, in path order, then
compiles the Package's readme examples. `-p <@scope/name>` limits the run to
one Package.

A test file is a program. It imports the Package by name, as a program does,
and defines `function main(): void`. Each file runs on its own, with a
fresh, empty filesystem, and with no Blueprint, so every `check` is allowed
and printed on a `[security]` line. Calls made by the test's `main` have the
caller `main`, and calls made inside a Package have the Package's name as the
caller.

A file named `network.test.ts` or `network_<anything>.test.ts` (or the
`.subm` equivalents), anywhere under `tests/`, is a network test file.
`--skip-network` leaves those files out before they are compiled. Selection
is by file name only.

### Segments and output

`label` divides a file into segments. A segment runs from one `label` to the
next, or to the end of `main`. A file with no `label` is one segment, named
by its path. The first uncaught error ends the file. The segments before it
pass, the one it happened in fails, and the ones after it don't run and
aren't counted. An error in a Package's or the file's top-level statements
fails the first segment.

```text
[security] caller=main capability=acme.com/credits.apply context={"amount":5,"customerClass":"standard","customerId":"cus_northwind","kind":"goodwill"}
ok   packages/billing/tests/fail.test.ts :: credits
FAIL packages/billing/tests/fail.test.ts :: refuses a negative amount with a TypeError
error: Error: expectException: expected a TypeError error, but caught RangeError
  at main (packages/billing/tests/fail.test.ts:8:66)  [thrown here]
 7 |     label("refuses a negative amount with a TypeError");
 8 |     expectException(() => { applyCredit("cus_northwind", -1); }, "TypeError");
   |                                                                  ^
 9 |     label("never runs");
[security] caller=main capability=acme.com/credits.apply context={"amount":1500,"customerClass":"standard","customerId":"cus_northwind","kind":"goodwill"}
ok   packages/billing/tests/lib.test.ts :: credits the customer named
ok   packages/billing/tests/lib.test.ts :: refuses a zero amount
skip packages/billing/tests/network.test.ts (--skip-network)
ok   packages/billing/docs/readme.md :: example 1 (compile)

4 passed, 1 failed across 3 files
1 network test files skipped (--skip-network)
```

| Line | Meaning |
| --- | --- |
| `ok   <file> :: <label>` | A segment passed |
| `FAIL <file> :: <label>` | A segment failed. The error and its trace follow on standard error |
| `FAIL <file>  (compile error)` | The file didn't compile. The diagnostics precede it |
| `skip <file> (--skip-network)` | A network test file left out. The path is relative to `submilli.toml` |
| `ok   <path>/docs/readme.md :: example <n> (compile)` | A readme example compiled |
| `<p> passed, <f> failed across <n> files` | The totals. The readme counts as one file when it has examples. |
| `<n> network test files skipped (--skip-network)` | Printed when files were skipped |

The run exits 0 when nothing failed and 1 otherwise. With no test files and
no readme examples it prints
`no test files found (looked for tests/**/*.test.{ts,subm})` and exits 0.

### Test API

| Name | Signature | Behavior |
| --- | --- | --- |
| `assert` | `assert(condition: boolean, message: string): void` | In scope in every program, without an import. Throws `Error(message)` when `condition` is false. |
| `label` | `label(description: string): void` | From `submilli:test`. Starts a segment named `description`. |
| `expectException` | `expectException(fn: () => void, errorType: string = ""): Error` | From `submilli:test`. Calls `fn` and returns the error it throws. Fails the segment if `fn` returns. When `errorType` is not empty, also fails unless the error's `name` equals it. |

`submilli:test` is available only to test files run by `submilli build
test`. A program run with `submilli run` that imports it fails to compile:

```text
error: package `submilli:test` not found
 --> t.ts:1:23
  |
1 | import { label } from "submilli:test";
  |                       ^^^^^^^^^^^^^^^
2 | function main(): void { label("x"); }
  |
help: `submilli:test` is only available to test files run via `submilli build test`; it is not importable from a program run with `submilli run`
```

### Test credentials

Tests receive no credentials by default, so `secrets.get` returns `null` for
every name. These options supply them:

| Option | Supplies |
| --- | --- |
| `--env-var <NAME>` | The process environment variable `NAME`. Repeatable, or comma-separated names. Fails if the variable is unset or not valid Unicode. |
| `--env-file <PATH>` | The `NAME=value` lines of a file. A relative path is resolved from the current directory. Fails if the file can't be read. |
| `--all-env` | Every process environment variable whose name and value are valid Unicode |

When the same name comes from more than one option, `--env-var` wins over
`--env-file`, which wins over `--all-env`, whatever their order on the
command line. No `.env` file is read unless `--env-file` names it.

In an `--env-file` file, blank lines and lines starting with `#` are
ignored, a leading `export ` is removed, whitespace around the name and the
value is trimmed, and one pair of surrounding `"` or `'` quotes is removed
from the value. A line without `=` is ignored.

```text
$ submilli build test --env-var BILLING_API_KEY_NOPE
Error: --env-var BILLING_API_KEY_NOPE: process variable is not set
```

```text
$ submilli build test --env-file nope.env
Error: reading credential file nope.env

Caused by:
    No such file or directory (os error 2)
```

Both exit 1 before any test runs.
