---
title: "Start a project"
description: "How to create a Package project with submilli build: scaffold it, fill in submilli.toml, check that it builds, add a second Package, and open it in your editor."
slug: packages/start-a-project
sidebar:
  order: 1
authorship:
  label: ai-assisted
  confirmed: true
  contentHash: "fb4166e58d31788571a955d63c16168815c1e076eae4e183c1f924e2ba08939f"
  confirmedAt: "2026-10-05T13:01:53.009Z"
---

The agent needs to reach a system of yours, such as a billing API, a CRM,
or an internal tool. There are two ways in without writing a Package, and
neither gives you [semantic permissions](/docs/blueprints/#semantic-permission-model):

- The program calls the system's HTTP API itself, with the credential
  added by the
  [authorization proxy](/docs/blueprints/http-and-credentials). A
  rule then sees a host and a path. It can't tell what the call means.
- The Blueprint declares the system's MCP server, if it has one, and
  [each tool becomes a function](/docs/blueprints/add-an-mcp-server).
  A rule then sees the tool's name but not its payload, so it can't rule
  on what the call does, and you have another server to run.

A Package does. It is a library whose functions say what each operation
means and ask the Blueprint before they act, so a rule can say which
customer and how much. A Package lives in a project. `submilli build`
scaffolds the project, compiles it, derives what a Blueprint can grant,
runs its tests, and installs it where programs can import it. There is
nothing else to install.

This guide shows you how to create a Package project. The example is
Acme's billing Package, `@acme/billing`. Substitute your scope and name.

## Scaffold it

In the directory that will hold the project:

```sh
submilli build init @acme/billing packages/billing
```

```text
created …/acme/submilli.toml
created …/acme/packages/billing/src/lib.ts
created …/acme/packages/billing/docs/readme.md
created …/acme/packages/billing/README.md
created …/acme/packages/billing/tests/lib.test.ts
add packages with `submilli build new <@scope/name> <path>`; compile and install with `submilli build publish-local`; run tests with `submilli build test`
```

The first argument is the Package's name, `@scope/name`, the name
programs will import. The scope must match your GitHub organization,
since that is where other machines install the Package from. The name
after it is yours to choose. The second argument is the Package's
directory, relative to the project. The project is the current
directory, and it holds one Package or several.

| Path | Holds |
| --- | --- |
| `submilli.toml` | The manifest: one `[[package]]` block per Package |
| `packages/billing/src/lib.ts` | The entry point. What it exports is the Package's API. |
| `packages/billing/src/*.ts` | Other source files, imported with a relative path |
| `packages/billing/docs/readme.md` | The documentation the model reads before it writes a program |
| `packages/billing/README.md` | The readme for the person who installs and grants the Package |
| `packages/billing/tests/*.test.ts` | Tests, run by `submilli build test` |
| `packages/billing/capabilities.yaml` | Written by the build: what the Package provides and requires |
| `tsconfig.json`, `.vscode/`, `.submilli/` | Editor files, [below](#open-it-in-your-editor) |

The scaffold's `hello()` function, its test, and its two one-line
readmes are placeholders. The next pages replace them.

## Fill in submilli.toml

```toml title="submilli.toml"
[[package]]
name = "@acme/billing"
version = "0.1.0"
description = "Package @acme/billing."
keywords = []
path = "packages/billing"
```

Write the description and keywords for the model. `submilli search`
matches them, and the agent sees them when it looks for a Package to do
a job.

```toml title="submilli.toml"
[[package]]
name = "@acme/billing"
version = "0.1.0"
description = "Goodwill credits for one customer of Acme's billing service."
keywords = ["billing", "credits"]
path = "packages/billing"
```

Refer to the [Package manifest reference](/docs/reference/package-manifest)
for every field. Dependencies are declared here too, as
[Add a dependency](/docs/packages/add-a-dependency) shows.

## Check that it builds

```sh
submilli build check
```

```text
warning: exported symbol `hello` has no doc comment
 --> packages/billing/src/lib.ts:1:1
  |
1 | export function hello(): string {
  | ^^^^^^
2 |     return "hello from @acme/billing";
checked @acme/billing v0.1.0
```

`check` compiles every Package in the project, in dependency order, and
installs nothing. Every `build` command finds `submilli.toml` in the current
directory or a parent of it, so run them from anywhere in the project.
The warning is about the placeholder. Doc comments are part of a
Package's API, because `submilli docs` prints them and the model reads
them, so the build warns about an export without one. A compile
error stops the build and exits 1:

```text
error: expected `number`, got `string`
  --> packages/support/src/lib.ts:12:43
   |
12 | export function broken(): number { return "x"; }
   |                                           ^^^
```

## Add a second Package

If one project should hold several Packages, such as a Package per
service, add the next one with `build new`:

```sh
submilli build new @acme/support packages/support
```

```text
added @acme/support to …/acme/submilli.toml
created …/acme/packages/support/src/lib.ts
created …/acme/packages/support/docs/readme.md
created …/acme/packages/support/README.md
created …/acme/packages/support/tests/lib.test.ts
```

`check`, `test`, and `publish-local` then work on every Package. Add
`-p @acme/billing` to work on one and the siblings it depends on.

## Open it in your editor

Package source is TypeScript, so any editor with TypeScript support gives
you completion, hover documentation, and go to definition on it. `init`
wrote the files that make that work:

| Path | Holds | Yours to edit |
| --- | --- | --- |
| `tsconfig.json` | One line that extends the generated configuration | Yes |
| `.vscode/tasks.json` | A build task that runs `submilli build check` | Yes |
| `.gitignore` | An entry for `.submilli/` | Yes |
| `.submilli/` | The generated configuration and the type declarations of the standard library, your Packages, and your dependencies | No |

Open the project directory, the one holding `submilli.toml`. Everything
under `.submilli/` is written again by every `submilli build check` and
`publish-local`, so it follows `submilli.toml` as you add Packages and
dependencies, and it follows the `submilli` you have installed. So Git
ignores it. After a clone, run `submilli build check` once and the
editor has its types. If the editor stops resolving imports after you add
a Package or upgrade `submilli`, do the same and restart its TypeScript
server.

The editor helps you write, but `submilli build check` decides what
compiles. Where they differ, the compiler is right. The types are
Submilli's, not Node's or the browser's, so `fetch`, `process`, and `Date`
are missing and `Temporal` is there. Null checking is off in the editor on purpose, because
TypeScript reads an absent optional field as `undefined` and Submilli
reads it as `null`. The editor accepts `any`, `undefined`, and `async`,
which the compiler refuses, and only the compiler sees a `@capability` tag
that disagrees with its `check`.

In VS Code, run the build task, **Terminal → Run Build Task** or
Ctrl+Shift+B (Cmd+Shift+B on a Mac). It runs `submilli build check` and
puts each error and warning in the Problems panel, on the line the
compiler named. In another editor, run it in a terminal. Its errors have
the form `--> path:line:column`, which most editors can follow.
