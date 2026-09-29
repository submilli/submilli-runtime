---
title: "Editor setup"
description: "What submilli build init sets up for your editor: TypeScript types for the standard library and your packages, a VS Code build task that reports compile errors, and what the editor does and doesn't check."
slug: editor-setup
sidebar:
  order: 17
---

Package source is TypeScript files, so your editor's TypeScript support
works on it: completion, hover documentation, and go to definition.
`submilli build init` writes the files that make that work. This chapter says
what they are, and where the editor's view of your code stops and the
compiler's begins.

## What init writes

| Path | Holds | Yours to edit |
| --- | --- | --- |
| `tsconfig.json` | One line that extends the generated configuration | Yes |
| `.vscode/tasks.json` | A build task that runs `submilli build check` | Yes |
| `.gitignore` | An entry for `.submilli/` | Yes |
| `.submilli/tsconfig.submilli.json` | The generated configuration: where each package lives, and the types below | No |
| `.submilli/types/` | Type declarations for the language's built-ins, the standard library, and the packages you depend on | No |

The first three are written once and then left alone. Everything under
`.submilli/` is written again by every `submilli build check` and `publish-local`,
so it follows the manifest as you add packages, and it follows the
`submilli` you have installed. That is why it is ignored by Git: after a
clone, run `submilli build check` once and the editor has its types.

## What the editor gives you

Open the project directory, the one holding `submilli.toml`, in any editor
with TypeScript support. There is no extension to install.

- **The standard library.** `import { get } from "submilli:http"` completes,
  and hovering a function shows the documentation `submilli docs` prints.
- **Your packages.** An import of `@acme/billing` from a test or from a
  sibling package goes to its `src/lib.ts`.
- **Your dependencies.** A package from the local store or from GitHub
  completes from the declarations it was built with.
- **The language's built-ins.** The types are Submilli's, not the browser's
  or Node's. `fetch`, `process`, and `Date` aren't there because programs
  don't have them, and `Temporal` is.

## The compiler is the checker

The editor helps you write; `submilli build check` decides what compiles.
The two differ in a few places, and where they do, the compiler is right.

| | The editor | `submilli build check` |
| --- | --- | --- |
| An optional field that is absent | Not checked: null checking is off | Reads as `null` |
| `any`, `undefined`, `async` | Accepted | Refused |
| A `@capability` tag that disagrees with its `check` | Not seen | Warned about |

Null checking is off on purpose. TypeScript reads an absent optional field as
`undefined` and Submilli reads it as `null`, so with checking on, the editor
would flag correct code.

## Compile errors in VS Code

Run the build task, **Terminal → Run Build Task** or Ctrl+Shift+B (Cmd+Shift+B
on a Mac). It runs `submilli build check` and puts each error and warning in
the Problems panel, on the line the compiler named.

In another editor, run `submilli build check` in a terminal. Its errors have
the form `--> path:line:column`, which most editors can follow.

## Changing the configuration

Add your own settings to `tsconfig.json`, after the `extends` line. Leave
`.submilli/` alone: the next build overwrites it.

If the editor stops resolving imports after you add a package or a
dependency, or upgrade `submilli`, run `submilli build check` and restart the editor's TypeScript
server.

Next: [testing packages](/docs/testing-packages).
