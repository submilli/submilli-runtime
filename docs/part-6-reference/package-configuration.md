---
title: "Package configuration"
description: "The src/config.ts build configuration contract, accepted declarations, command inputs, upgrade behavior, dependency errors, and server API fields."
slug: reference/package-configuration
sidebar:
  order: 11.5
authorship:
  label: ai-generated
  confirmed: true
  contentHash: "9f62bbd390990fcb8bfa7ccb22ba7c278034836021b20fc52dcbdb573dd323b6"
  confirmedAt: "2026-10-17T11:43:08.504042+00:00"
---

## File and imports

| Property | Contract |
| --- | --- |
| Location | `<package path>/src/config.ts` (legacy `config.subm` is also accepted) |
| Declaration source | The file itself. No configuration declaration in `submilli.toml` |
| Imports | Ordinary relative imports, such as `import { baseUrl } from "./config"` |
| Scope | One configuration per installed Package name in a package store |
| Lifetime | Values are resolved before typechecking and compiled into the Package |
| Blueprint/session overrides | None |
| Credentials | Use secrets. Configuration is inspectable in artifacts and administrative responses |
| Existing files | The root configuration module is reserved. Rename an existing module that contains executable configuration code and update its imports |

## Declarations

```ts title="src/config.ts"
export declare const baseUrl: string;
export const timeoutMs = 5000;
export const debug: boolean = false;
```

| Declaration | Meaning |
| --- | --- |
| `export declare const name: string;` | Required string, supplied at build/install time |
| `export declare const name: number;` | Required finite number |
| `export declare const name: boolean;` | Required boolean |
| `export const name = "value";` | String parameter with a default |
| `export const name = 5000;` | Number parameter with a default. Negative literals are accepted |
| `export const name = false;` | Boolean parameter with a default |
| `export const name: string = "value";` | An optional primitive annotation must match the default's type |

Comments, single/double-quoted strings, no-substitution template literals, and
omitted semicolons follow normal lexical rules. Imports, re-exports, functions,
mutable bindings, destructuring, objects, arrays, union types, and computed
initializers are refused. Names must be unique. Every parameter must have a
default or a required declaration with a primitive type.

A required declaration has a Submilli build-time meaning. In ordinary TypeScript,
`declare` emits no runtime value. The Submilli build supplies the initializer.

## Command inputs

```sh
submilli build check -p @acme/search --config baseUrl=https://search.test
submilli build test -p @acme/search --config baseUrl=https://search.test
submilli build publish-local -p @acme/search --config baseUrl=https://search.test
submilli install acme/search @acme/search --config baseUrl=https://search.internal
submilli server packages install acme/search @acme/search --config baseUrl=https://search.internal
```

| Input | Contract |
| --- | --- |
| `--config NAME=VALUE` | Repeatable. Only short parameter names of the explicitly selected Package |
| Package selection | `-p @scope/name` for build commands; the Package positional argument for install commands |
| Build commands | `check`, `test`, `publish-local`, `authority-map`, `security-review` |
| Strings | Literal command argument text after the first `=`. Empty strings are valid |
| Numbers | Finite decimal floating-point input, including exponent notation |
| Booleans | Exactly `true` or `false` |
| Invalid inputs | Unknown/duplicate names, malformed assignments, incompatible types, and missing required parameters fail the command |
| Local builds | Source defaults plus explicit inputs. The selected Package's installed overrides are not inherited |
| Local dependencies | Their own installed overrides may satisfy required parameters |
| Reset | Install commands accept repeatable `--reset-config NAME` to remove a saved override and restore the source default |
| Reset errors | Unknown saved overrides, duplicate resets, overlap with `--config`, and required values left without a default are refused |

## Installation and upgrades

| Operation | Behavior |
| --- | --- |
| First install | Resolve source defaults and supplied values |
| Reinstall/upgrade | Keep saved explicit overrides; unspecified parameters follow the new source defaults |
| Replace source/configuration | Requires `--upgrade`, including a configuration change at the same source commit |
| Unchanged installation | Same source commit and configuration are reported as up to date |
| Removed/retyped parameters | Incompatible saved overrides fail validation. Reset removed overrides or supply compatible values |
| Dependency with defaults | May install automatically |
| Dependency missing required values | Stop before publishing prepared Packages. Install the dependency explicitly with its own `--config` values, then retry the original command |
| Dependency inputs | A parent's inputs are never forwarded. A compatible installed dependency supplies its own saved overrides |
| Repository-wide install | Defaults and saved overrides are allowed; configuration flags require an explicitly selected Package |
| Playground | Automatic rebuilds retain saved explicit overrides. For a first required value, use `build publish-local -p … --config …` |
| Artifacts | Metadata records explicit overrides and resolved values; embedded source remains the original source |
| Source locks | `submilli.lock` continues to pin source provenance, without deployment configuration |
| Compatibility | Artifact schema 11 is retained. Existing artifacts without configuration metadata load with empty configuration |

Configuration does not grant capabilities. Network requests still require the
Blueprint's permissions. Capability derivation analyzes the resolved constants.

## Administrative HTTP API

`POST /v1/packages/install` accepts these additional optional fields:

| Field | Type | Default |
| --- | --- | --- |
| `config` | Object mapping short parameter names to string input values | `{}` |
| `reset_config` | Array of saved override names to remove | `[]` |

The `package` field is required when either input is non-empty. Validation runs
on the server. The CLI sends input text, including for numeric/boolean parameters.

A missing required parameter returns HTTP 400 with `error:
"configuration_required"`. The `configuration` object contains:

| Field | Value |
| --- | --- |
| `package` | Package needing configuration |
| `parameters` | Object mapping missing names to primitive type names |
| `source` | GitHub source provenance, including the resolved commit SHA |
| `command` | `submilli server packages install` command pinned to that source, with value placeholders |

The CLI includes the original server target and token-file flag in the suggested
command. Replace the quoted value placeholders, run the dependency command, then
retry the original installation. Configuration inputs without an explicitly selected Package and invalid reset combinations use `invalid_configuration`; unknown names and incompatible parameter values use `compile_failed` with a source diagnostic.

Lone-surrogate string defaults appear in resolved metadata as `{ "utf16": [55296] }`, preserving their code units. Other strings use ordinary JSON strings.

`GET /v1/packages` includes `config.overrides` (string inputs) and `config.values`
(resolved primitive values) for each installed Package. The CLI's concise package
listing remains unchanged.
