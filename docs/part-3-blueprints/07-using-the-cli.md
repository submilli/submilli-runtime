---
title: "Using the CLI"
description: "The submilli command, by task: run and check programs under a blueprint, look up packages and built-ins, author blueprints, keep local secrets, and install packages."
slug: cli
sidebar:
  order: 7
---

The `submilli` command is the local half of Submilli: everything in this
chapter runs on your machine, with no server. It compiles and runs programs,
looks things up, edits blueprints, and keeps a local store of packages and
secrets. The other half, `submilli server` and the `submilli-server` binary,
is in [Submilli server](/docs/server). Those commands talk to a running
server and send its token, read from `SUBMILLI_SERVER_TOKEN`; [start
it](/docs/server#start-it) shows how. Nothing in this chapter needs one.

This chapter is a reference, grouped by task. Every command prints its own
help with `--help`; the text here says what each one is for and what to expect
back.

## Run a program

```sh
submilli run hello.ts
```

`run` compiles the file, runs `main`, and prints the returned value on standard
output, encoded the way the agent would receive it: a string as itself, a
number or boolean as its text, an object or array as JSON. `console.log` goes
to standard error, so a pipe or a redirect gets the result and nothing else.
The exit status is 0 when `main` returned and 1 for a compile error or an
uncaught exception, printed to standard error with source context.

Without a blueprint the run is unrestricted: every capability is allowed, but
only the standard library is importable, since packages arrive through a
blueprint. `submilli:git` additionally needs [Git configuration](/docs/blueprints#let-the-program-commit).
With a blueprint, the run behaves as a session would:

```sh
submilli run --blueprint blueprint.yaml --var customerId=cus_northwind credit.ts
```

```text
credited 1500 cents
```

`--blueprint` applies the policy, resolves `store:` secrets from the local
store, adds `auth_proxy` credentials, connects declared MCP servers, and calls
declared models. `--var NAME=VALUE` binds a blueprint variable the way an
application does when it opens a session, and is checked the same way: a
missing required variable or an undeclared name refuses the run before the
program starts. Bind a customer the rule doesn't cover and the denial is the
same one a session would see:

```sh
submilli run --blueprint blueprint.yaml --var customerId=cus_initech credit.ts
```

```text
error: PermissionDeniedError: permission denied: caller=main capability=acme.com/credits.apply: policy denied acme.com/credits.apply for main.
```

One thing a local run doesn't have is session state: `submilli:session` needs
a session store, which only [a server](/docs/server) provides, so `session.set` throws a
`TypeError` saying so.

| Flag | Effect |
| --- | --- |
| `--blueprint <file>` | Apply a blueprint; required for packages, secrets, MCP servers, models, and Git |
| `--var NAME=VALUE` | Bind a blueprint variable; repeatable, requires `--blueprint` |
| `--vfs <dir>` | Use this directory as the program's `/`; the default is a temporary directory deleted after the run |
| `--timeout <ms>` | Stop the program after this long with `timeout exceeded`; there is no deadline by default |
| `--fuel <n>` | Instruction budget; exceeding it stops the program with `fuel exhausted` |
| `--max-stack <bytes>` | Call-stack limit |
| `--max-llm-tokens <n>` | Tokens the run's model calls may spend in total; default 1,000,000 |
| `--max-llm-concurrency <n>` | Prompts one `llm.batch` sends at once; default 4 |

`--vfs` is how you look at what a program wrote: point it at a directory and
the files are there afterwards. `--timeout` and `--fuel` are the local
counterparts of the limits a server enforces ([resource
limits](/docs/resource-limits)).

## Check a program

```sh
submilli check credit.ts
```

`check` compiles and type-checks without running, and prints nothing when the
program is fine. Errors are the same diagnostics `run` would print, with exit
status 1:

```text
error: expected `number`, got `string`
 --> bad.ts:1:34
  |
1 | function main(): number { return "three"; }
  |                                  ^^^^^^^
```

`check` doesn't take a blueprint, so a program that imports a package can't be
checked on its own; `run --blueprint` compiles it first and stops at the first
error before anything executes.

## Look things up

Three commands read the same declarations the agent's lookup tools read, so
what they print is what the model sees.

| Command | Prints |
| --- | --- |
| `submilli search [query]` | Standard-library modules and installed packages whose name, description, or exported symbol contains the query; all of them with no query |
| `submilli docs <name>` | One module's or package's description and declarations: `submilli:fs`, `@acme/billing`; a built-in such as `Temporal.Instant` resolves here too |
| `submilli search` or `submilli docs` with `--blueprint <file>` | Only what programs under that blueprint can import: `submilli:git` needs its Git identity, gated modules need a rule, installed packages need a declaration. Without `--blueprint`, both show everything, even when `blueprint.yaml` is in the directory |
| `submilli docs @mcp/<server> --blueprint <file>` | Discover one MCP server using local credentials and print its tool signatures; the file defaults to `blueprint.yaml` |
| `submilli builtins [names…]` | The built-in catalog with no argument; one or more built-ins' declarations with names, down to a member such as `Temporal.Instant` |

```sh
submilli docs @acme/billing
```

```text
@acme/billing — Package @acme/billing.

/**
 * Add a goodwill credit to a customer's account.
 * @capability acme.com/credits.apply { customerId: string, customerClass: string, amount: number }
 */
function applyCredit(customerId: string, amount: number): Credit;
…
```

The `@capability` line is what the blueprint chapter's `capability list` reads;
`docs` is the place to read a package before granting anything to it.

MCP tools retain representable argument types; unsupported fields appear as
`unknown` and are validated by the MCP server. Calls have a 60-second deadline,
including authentication and connection. Use `mcp.<server>` permission rules
with a filter such as `tool == "save_issue"`; the legacy `/tool` suffix is rejected.

## Author a blueprint

[Crafting a blueprint](/docs/blueprints) builds one with these commands. They
all edit `blueprint.yaml` in the current directory unless `--blueprint <file>`
says otherwise, and they rewrite the file when they change it, so comments
don't survive.

| Command | Does |
| --- | --- |
| `blueprint init [name] [--full]` | Write a deny-everything blueprint; `--full` lists every standard-library capability as a commented rule |
| `blueprint lint <file> [--fix]` | Validate the file and check it against the installed packages; `--fix` adds the rules packages require for their own calls |
| `blueprint add-package <name> [--capabilities a,b \| --all-capabilities \| --no-capabilities]` | List an installed package and write its own rules; optionally grant its operations to `main` |
| `blueprint add-mcp <name> <url> [--oauth …]` | Declare an MCP server, importable as `@mcp/<name>` |
| `blueprint variable add\|list\|remove` | The `variables` block |
| `blueprint secret add\|list\|remove` | The `secrets` block: `--store`, `--harness`, `--env`, or `--file` |
| `blueprint git set\|show\|remove` | Manage the blueprint's [Git configuration](#configure-git) |
| `blueprint auth-proxy add\|list\|remove` | Credentials the runtime adds by host; `add --allow-insecure-http` opts that rule into HTTP, requiring a separate top-level YAML opt-in |
| `blueprint capability list [library] [--unconfigured]` | Every capability a rule can name, with its filter fields and any rules already present; `--unconfigured` shows only what the blueprint's packages provide that `main` has no rule for |
| `blueprint capability add <name> [--filter …] [--action allow\|deny] [--caller <id>]` | Append a rule; refuses a capability name nothing provides unless `--force` |
| `blueprint capability remove <name> [--caller <id>]` | Drop every rule for a capability |
| `blueprint prompt` | Print the tool description the agent receives, with this blueprint's policy filled in |

`lint` exits 1 on an error, such as a package that requires an operation its
own rules don't mention, and 0 on warnings, so it can gate a commit. A package
rule you narrowed on purpose is a warning, and `--fix` leaves it as it is.

### Configure Git

[Let the program commit](/docs/blueprints#let-the-program-commit) walks through
the identity, variables, secrets, and grants. These commands manage that
configuration:

| Command | Behavior |
| --- | --- |
| `blueprint git set --name <name> --email <email> [--username <username>]` | Requires both identity fields; omitting `--username` preserves its previous value. Use `--clear-username` to remove it. |
| `blueprint git show` | Prints configuration templates and whether `GIT_TOKEN` is declared, without resolving or displaying credentials. |
| `blueprint git remove` | Removes only the Git block; succeeds if it is already absent. Secrets, variables, and permission rules remain. |

Single-quote variable templates to prevent shell expansion. The [`run --var`
bindings](#run-a-program) apply to both Git identity and capability filters.

## Keep secrets locally

```sh
submilli secret put billing_api_key
submilli secret list
submilli secret delete billing_api_key
```

The local secret store is what `run --blueprint` reads for `store:` secrets
and where `mcp authenticate` keeps OAuth credentials. `put` prompts for the
value with echo off, or reads it from a pipe; `list` prints keys only. The
store is a directory of files readable by your user and nobody else, not
encrypted: a key kept on the same machine would protect nothing.

For a blueprint that declares MCP servers with OAuth, `submilli mcp
authenticate --blueprint blueprint.yaml <server>` runs the login flow in your
browser and stores the credential; `auth-status` shows each declared server's
state and `deauthenticate` forgets one. `mcp provider` holds the OAuth client
registrations for servers that need a pre-registered app. [Using MCP
servers](/docs/mcp-servers) covers all of it.

## Install a package

```sh
submilli install acme/billing-package @acme/billing
```

`install` fetches a repository from GitHub, builds the package it declares
(or one named package), and puts it in the local store, pinned to the commit
it resolved. `org/repo@ref` pins a branch, tag, or commit instead of the
default branch, and `--upgrade` replaces a package already installed at another
commit. `search` and `docs` see the package as soon as it is installed;
`blueprint add-package` is how a blueprint gets it.

## Where the CLI keeps things

Everything above lives under one directory, `~/.submilli` by default, or
`$SUBMILLI_HOME` when set:

| Path | Holds |
| --- | --- |
| `packages/` | Installed packages, one directory per `@org/name` |
| `secrets/` | The local secret store |
| `mcp_oauth.yaml` | OAuth provider registrations |
| `server/` | A local `submilli-server`'s own state; the CLI never touches it |

A server on the same machine reads the packages in `packages/` too, so a
package you publish locally is available to it without a second install
([where it keeps state](/docs/server#where-it-keeps-state)). Its secrets are separate:
`submilli secret put` fills the local store that `run --blueprint` reads, and
`submilli server secret put` fills the server's.

Pointing `SUBMILLI_HOME` at an empty directory gives you a clean slate for
trying something out.

Next: [using MCP servers](/docs/mcp-servers), which turns tool servers you
already have into packages.
