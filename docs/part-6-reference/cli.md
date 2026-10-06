---
title: "CLI"
description: "The submilli command tree: what each command does and where it runs, exit codes, environment variables, the state directory, Package installation and its errors, the GitHub token, and the help text of every command."
slug: reference/cli
sidebar:
  order: 3
authorship:
  label: ai-assisted
  confirmed: true
  contentHash: "37b108fbb93c0f5dcd6943fe72d19e229d1cca5ab4cca058e83e5f1f66d19f77"
  confirmedAt: "2026-10-05T17:36:35.000Z"
---

This page describes `submilli`, the command-line tool. It covers the
command tree, the behaviour every command shares, Package installation and the GitHub
token it uses, and the help text of every command. The help text closes
the page, one section per command from `submilli run` to `submilli server
mcp auth-status`, generated from the binary's `--help`.

## Command tree

`submilli` has fourteen top-level commands. Local commands run on this
machine and need no server. Server commands send HTTP requests to a
running `submilli-server`.

| Command | Does | Runs |
| --- | --- | --- |
| `run` | Compile a program and run its `main`, optionally under a Blueprint | Local |
| `check` | Compile and type-check a program without running it | Local |
| `build` | Scaffold, compile, test, and install the Packages of a project (`submilli.toml`) | Local |
| `install` | Fetch a Package from GitHub, compile it, and put it in the local store, pinned to a commit | Local, fetches from GitHub |
| `docs` | Print a module's, Package's, or built-in's description and declarations | Local |
| `search` | List standard-library modules and installed Packages matching a substring | Local |
| `builtins` | List the language built-ins, or print the declarations of named ones | Local |
| `skill` | Install, check, and update the coding-assistant skill | Local, and `sync` fetches releases |
| `upgrade` | Replace `submilli` and `submilli-server` with a published release | Local, fetches releases |
| `blueprint` | Create and edit a local `blueprint.yaml` | Local |
| `secret` | Manage the local secret store | Local |
| `mcp` | Authenticate a Blueprint's OAuth MCP servers into the local secret store | Local, contacts the MCP server's OAuth host |
| `github` | Manage the GitHub token that `install` and `build` send | Local, contacts GitHub |
| `server` | Run code, manage Blueprints, Packages, secrets, sessions, and MCP credentials on a server | Server |

## Local and server commands

Several local commands have a `submilli server` counterpart that does the
same job against a server's state. A server has its own Blueprints,
Package store, and secret store.

| Local | Server | Difference |
| --- | --- | --- |
| `submilli run --blueprint <file>` | `submilli server run-code --blueprint <name>` or `--session <id>` | The local run reads a Blueprint file, and the server run names a registered Blueprint or an open session |
| `submilli blueprint …` | `submilli server blueprint add`, `apply`, `list`, `show`, `remove` | The local commands edit a file, and the server commands register, list, and remove Blueprints |
| `submilli docs <name> --blueprint <file>` | `submilli server docs <name> --blueprint <name>` | The server reads a registered Blueprint's catalog |
| `submilli install <repo>[@<ref>]` | `submilli server packages install <repo> [--sha <ref>]` | The local install sends your GitHub token. The server fetches with its own (`github_token_file`) and never receives yours |
| `submilli secret put`, `delete`, `list` | `submilli server secret put`, `delete`, `list` | The local store is plaintext owner-only files, and the server's is encrypted |
| `submilli mcp authenticate`, `deauthenticate`, `auth-status` | `submilli server mcp authenticate`, `deauthenticate`, `auth-status` | Locally the Blueprint is a file (`--blueprint <file>`), and on the server it is a registered name (positional) |
| `submilli search` | `submilli server packages list` | `search` lists installed Packages and the standard library, and `packages list` lists the server's stores |

A server on the same machine and with the same `SUBMILLI_HOME` also reads
the local `packages/` directory, as a fallback store it never writes.

`submilli server secret delete` returns HTTP 404 when the secret does not
exist, including after it has already been deleted. Local `submilli secret
delete` succeeds when the secret is already absent.

## Server connection

Every `submilli server` command that sends a request takes two options,
`--server` and `--token-file`:

| Option | Environment variable | Default |
| --- | --- | --- |
| `--server <URL>` | `SUBMILLI_SERVER_URL` | `http://127.0.0.1:8128` |
| `--token-file <PATH>` | `SUBMILLI_SERVER_TOKEN_FILE` | none |

The token is read from the file when one is named, otherwise from
`SUBMILLI_SERVER_TOKEN`. With neither, no token is sent, which only a
server started with `--allow-unauthenticated` accepts. No option takes
the token itself. Surrounding whitespace in the file or variable is
dropped. An empty file is an error, and an empty variable counts as
unset. A token the server does not accept gives:

```text
error: the server did not accept this command's API token. Set `SUBMILLI_SERVER_TOKEN` to the token the server was started with, or `SUBMILLI_SERVER_TOKEN_FILE` to a file holding one
```

A token has the role `user` or `admin`. `run-code`, `session open` and
`close`, and `docs` accept either. `status`, `stop`, `apply`, and the
`blueprint`, `packages`, `secret`, and `mcp` groups need `admin`.
[Connect the CLI](/docs/server/connect-the-cli) shows how to set both up
for a shell.

## Exit codes

| Code | Meaning |
| --- | --- |
| `0` | The command succeeded |
| `1` | The command failed. This includes a program that failed to compile or threw from `run` (except a denial, below), a `blueprint lint` error, `server status` with no server running, `skill status` for a missing, outdated, or modified installation, and `upgrade --check` when a newer release exists |
| `2` | The command line was invalid: an unknown command or option, or a missing argument |
| `3` | `run` only: the program let a permission denial escape, a gated call the Blueprint or the runtime refused (an invariant or a read-only volume) that no `catch` handled. The message is the same text exit code 1 prints |

Errors go to standard error. A command's own errors begin with `error:`.
An error that stopped the command before it could report one begins with
`Error:` and may add a `Caused by:` section. `submilli server stop` exits
0 when no server is running, printing `already stopped (no server at
<URL>)`.

## Environment variables

| Variable | Read by | Effect |
| --- | --- | --- |
| `SUBMILLI_HOME` | Every command | The state directory (see [State directory](#state-directory)) |
| `SUBMILLI_SERVER_URL` | `server …` | The server's base URL. An empty value means the default |
| `SUBMILLI_SERVER_TOKEN` | `server …` | The API token, when no token file is named |
| `SUBMILLI_SERVER_TOKEN_FILE` | `server …` | A file holding the API token. `--token-file` takes precedence |
| `GH_TOKEN`, `GITHUB_TOKEN` | `install`, `build`, `github …` | A GitHub token (see [GitHub token](#github-token)) |
| `SUBMILLI_MAX_EXECUTION_LLM_TOKENS` | `run` | Default for `--max-llm-tokens` |
| `SUBMILLI_MAX_LLM_CONCURRENCY` | `run` | Default for `--max-llm-concurrency` |
| `SUBMILLI_MAX_EXECUTION_EMBEDDING_TOKENS` | `run` | Default for `--max-execution-embedding-tokens` |
| `SUBMILLI_MAX_EXECUTION_EMBEDDING_REQUESTS` | `run` | Default for `--max-execution-embedding-requests` |
| `SUBMILLI_MAX_EMBEDDING_CONCURRENCY` | `run` | Default for `--max-embedding-concurrency` |
| `SUBMILLI_OAUTH_REDIRECT_PORT` | `mcp authenticate`, `server mcp authenticate` | Loopback port of the OAuth redirect listener, `8765` by default |
| `SUBMILLI_SKILL_AUTOUPDATE` | `skill sync` | `0` stops `sync` from contacting the network. It then uses the skill bundled with the CLI |
| `SUBMILLI_SKILL_SOURCE` | `skill sync` | Repository URL skill releases are downloaded from. Must be `https`, or `http` on loopback |
| `SUBMILLI_RELEASE_SOURCE` | `upgrade`, `skill sync` | Repository URL CLI releases are downloaded from. Must be `https`, or `http` on loopback. Default `https://github.com/submilli/submilli-runtime` |
| `SUBMILLI_TELEMETRY` | Every command | `1`, `true`, `yes`, or `on` sends crash reports and per-command counters (command name and which flags were set, never their values). Off by default |

## State directory

The CLI keeps its state under `$SUBMILLI_HOME` when it is set and not
empty, otherwise under `~/.submilli`.

| Path | Holds | Written by |
| --- | --- | --- |
| `packages/` | The local Package store, one directory per Package (`packages/@org/name/`) | `install`, `build publish-local`, `build` for GitHub dependencies |
| `secrets/` | The local secret store, one owner-only (`0600`) plaintext file per key, in an owner-only directory. Holds `store:` secrets for `run --blueprint` and MCP OAuth credentials under `mcp_oauth/<blueprint>/<server>/credential` | `secret put`, `mcp authenticate` |
| `mcp_oauth.yaml` | Local OAuth provider apps: client id, client secret reference, and scopes per OAuth host. Owner-only | `mcp provider add`, `remove` |
| `github_token` | The stored GitHub token. Owner-only (`0600`) | `github authenticate`, and removed by `github deauthenticate` |
| `skill-release.json` | The cached result of the daily skill-release check | `skill sync` |
| `cli-release.json` | The cached result of the daily CLI-release check | `skill sync` |
| `server/` | A local `submilli-server`'s own state. The CLI never reads or writes it | `submilli-server` |

The secret store is not encrypted. A separate `secrets/` and `server/`
keep the CLI's plaintext store and a local server's encrypted store
apart.

## Package projects (`submilli build`)

`submilli build` works on a project, a directory with a `submilli.toml`
holding one `[[package]]` block per Package. Every `build` subcommand
except `init` finds the manifest in the current directory or the nearest
parent that has one. [Package manifest](/docs/reference/package-manifest)
describes the file.

| Subcommand | Does |
| --- | --- |
| `build init [@scope/name] [path]` | Writes `submilli.toml` with a first Package at `path` (default `.`), and scaffolds `src/lib.ts`, `docs/readme.md`, `README.md`, and `tests/lib.test.ts`, plus `tsconfig.json`, `.vscode/`, `.submilli/`, and `.gitignore`. Prompts for the name when it is omitted |
| `build new <@scope/name> <path>` | Adds a Package to `submilli.toml` and scaffolds its folders |
| `build check` | Compiles the Packages in dependency order and writes each one's `capabilities.yaml` beside its source. Installs nothing |
| `build test` | Compiles, then runs `tests/**/*.test.{ts,subm}` and compile-checks the readme's examples |
| `build publish-local` | Compiles, then installs the Packages into the local store |

`check`, `test`, and `publish-local` take `-p <@scope/name>` to compile one
Package and the sibling Packages it depends on. Before compiling, they
fetch the project's GitHub dependencies into the local store, with the
[GitHub token](#github-token), and record the commits in `submilli.lock`
beside `submilli.toml`. A lock that already pins every dependency, with
the store holding them, skips the fetch. A compile error exits 1.

```text
checked @acme/billing v0.1.0
```

```text
installed @acme/billing v0.1.0 -> …/packages/@acme/billing
```

Tests receive no credentials unless `build test` is given them:

| Option | Supplies |
| --- | --- |
| `--env-var <NAME>` | One process variable. Repeatable, or comma-separated. Fails if unset or not Unicode |
| `--env-file <PATH>` | `NAME=value` entries from a file, relative to the current directory. Fails if unreadable |
| `--all-env` | Every Unicode process variable |

When a name is supplied more than once, `--env-var` wins over
`--env-file`, which wins over `--all-env`, whatever the argument order.
`--skip-network` skips `network.test.{ts,subm}` and
`network_*.test.{ts,subm}` anywhere under `tests/`.

## Package installation

`submilli install` installs into the local store and `submilli server
packages install` into a server's store. Both fetch a GitHub repository
at one commit, resolve its GitHub dependencies, compile the Packages its
`submilli.toml` declares, and store them pinned to that commit. The
repository is `org/repo`, `github.com/org/repo`, or a full URL. A second
argument, `@org/name`, installs only that Package. Without it, every
Package the repository declares is installed. A Package must be scoped to
the repository's owner. For example, `submilli/acme` can hold only
`@submilli/…` Packages.

| | `submilli install` | `submilli server packages install` |
| --- | --- | --- |
| Pin to a branch, tag, or commit | `org/repo@<ref>` | `--sha <ref>` |
| Without a pin | The default branch's head | The default branch's head |
| GitHub token | Yours (see [GitHub token](#github-token)) | The server's `github_token_file`. The CLI sends none |
| Replace a Package installed at another commit | `--upgrade` | `--upgrade` |

The first install reports the commit and the Package's location:

```sh
submilli install submilli/acme @submilli/acme-billing
```

```text
fetched github.com/submilli/acme at 88656b81c537
installed @submilli/acme-billing v0.1.0 -> …/packages/@submilli/acme-billing
```

The same command again, with the Package already installed at that
commit, changes nothing and exits 0:

```text
fetched github.com/submilli/acme at 88656b81c537
up to date @submilli/acme-billing
```

A Package installed at another commit is refused, exit 1, until
`--upgrade` is given:

```text
fetched github.com/submilli/acme at 88656b81c537
error: `@submilli/acme-billing` is already installed from github.com/submilli/acme@d7d9fa44e3dc; pass --upgrade to replace it with 88656b81c537
```

On a server the three outcomes are:

```text
installed @submilli/acme-billing @ 88656b81c537
```

```text
up to date @submilli/acme-billing
```

```text
error: @submilli/acme-billing already installed at a different commit; retry with --upgrade to replace with 88656b81c537
```

`submilli install` writes only to the store. It changes no Blueprint and
no manifest. When it finds no public repository and no token was sent,
and both standard input and standard error are a terminal, it offers to
store a GitHub token and, if one is stored, tries once more. [Install
private Packages on a server](/docs/server/install-private-packages)
describes the server's token and its errors.

## GitHub token

`submilli install` and `submilli build` send a GitHub token when one is
available, which lets them read private repositories. The token comes
from the first of these sources that has one:

| Order | Source |
| --- | --- |
| 1 | `GH_TOKEN` |
| 2 | `GITHUB_TOKEN` |
| 3 | The token stored by `submilli github authenticate`, in `$SUBMILLI_HOME/github_token` |
| 4 | The GitHub CLI's token, `gh auth token --hostname github.com`, when `gh` is installed, logged in, and answers within 5 seconds |

An environment variable that is set but empty is skipped. One that holds
something other than a token is skipped with a warning such as
`` warning: ignoring `GH_TOKEN`: … ``. With no source, fetches are
anonymous and reach public repositories only, at GitHub's lower
anonymous rate limit. A token needs Repository permissions → Contents:
Read-only on the Package repositories (a fine-grained token) or the
`repo` scope (a classic token).

| Command | Does |
| --- | --- |
| `github authenticate [--owner <owner>]` | Reads a token (at a terminal with echo off and the token-creation link printed, otherwise from standard input), checks it with GitHub, and stores it, replacing any earlier one. `--owner` fills the owner into the link |
| `github auth-status` | Prints which source is in use and, after asking GitHub, whose token it is and when it expires. Never prints the token |
| `github deauthenticate` | Removes the stored token, and says which source fetches still use, if any |

`authenticate` prints `✓ stored a GitHub token for <login> (<expiry>) in
<path>`, where the expiry is `never expires` or `expires <date>`. With no
token in any source, `auth-status` and `deauthenticate` print:

```text
✓ no GitHub token: packages are fetched from public repositories only; run `submilli github authenticate` to reach private ones
```

```text
✓ no GitHub token was stored
```

`authenticate` adds `` ; `GH_TOKEN` is set and is used instead until you
unset it `` when an environment variable outranks the stored token.

## Install errors

`submilli install`, and `submilli build` when it fetches dependencies,
print these errors and exit 1. Messages name the repository, the token's
source (such as `` the GitHub token in `GH_TOKEN` ``), and, where a token is
missing or wrong, the token-creation link.

| Error begins | Cause | Fix |
| --- | --- | --- |
| `` GitHub has no public repository `org/repo` `` or `no public commit … in` | No token was sent, and the repository is private, misnamed, or lacks the commit | A token that can read the repository, from any source, lets the fetch see it |
| `GitHub has no repository … that <source> can read` or `no commit … that` | A token was sent and GitHub showed nothing, because the name is wrong, the token has no Contents access to the repository, or a fine-grained token awaits the organization's approval | Contents: Read-only on the repository, or the organization's approval, clears it |
| `` resolving org/repo ref `<ref>`: GitHub found no such branch, tag, or commit `` | The `@<ref>` names nothing in the repository | A ref the repository has clears it |
| `GitHub refused <source> for` | GitHub answered 403 to the token because it lacks Contents access, or the organization requires approval or forbids this kind of token or its expiry | Contents: Read-only there, or the organization's approval, clears it |
| `GitHub rejected <source> (expired or revoked)` | The token is expired or revoked. As a warning, fetches went on without it. As an error, the repository was not public | A new token from the same source clears it |
| `` `org/repo` is in an organization that uses SAML single sign-on: authorize `` | The token is not authorized for the organization's single sign-on | Authorizing it, at the link in the message or with Configure SSO on the token, clears it |
| `GitHub's rate limit is used up; try again in about N min` | The rate limit for this token, or for anonymous requests, is exhausted | It clears after the time given. A token has a higher limit than anonymous requests |
| `` `@org/name` is already installed from github.com/org/repo@<sha>; pass --upgrade `` | The Package is in the store at another commit | `--upgrade` replaces it |
| `` package `@org/name` from github.com/org/repo must be scoped `@org/...` `` | The Package's scope is not the repository's owner | — |
| `` package `@org/name` is not declared in submilli.toml `` | The named Package is not in the repository's manifest. The message lists the declared ones | — |
| `… has no submilli.toml at its root` | The repository is not a Submilli Package repository | — |

`submilli github authenticate` adds three more errors:

| Error begins | Cause |
| --- | --- |
| `the GitHub token is empty` | Nothing was entered or piped |
| `that is not a GitHub token: expected one line of visible ASCII` | The input is not a single token |
| `GitHub won't identify this token` | GitHub does not report an owner for the token, so it is not stored. `GH_TOKEN` and `GITHUB_TOKEN` accept it |

## Skill installation (`submilli skill`)

`submilli skill` installs the Submilli skill for a coding assistant, with
a `submilli-verifier` subagent beside it. `--agent` picks the assistant.
`--project <dir>` installs under that directory instead of the user's
home directory (`HOME`, or `USERPROFILE` on Windows).

| `--agent` | Skill directory | Verifier |
| --- | --- | --- |
| `claude` | `.claude/skills/submilli` | `.claude/agents/submilli-verifier.md` |
| `codex` | `.agents/skills/submilli` | `.codex/agents/submilli-verifier.toml` |
| `cursor` | `.cursor/skills/submilli` | `.cursor/agents/submilli-verifier.md` |

```text
…/.claude/skills/submilli: skill v5 from CLI 0.1.6 is installed. Restart your assistant to reload it.
```

The skill directory holds a receipt, `.submilli-skill.json`, with the
installed files' hashes. `install` and `update` write the skill bundled
in the CLI, with no network access. `update` requires an existing
installation. Neither replaces an installation whose files were edited,
added, or removed. Such an installation is reported as `locally modified; preserved`, and
there is no option to overwrite it. A symlink in the installation path is
refused. `status` prints `current`, `not installed`, `outdated; run
submilli skill sync`, or `locally modified; preserved`, and exits 1 for
all but `current`.

`sync` takes no options. It finds every installation with a receipt under
the home directory and in each directory from the current one up to the
enclosing Git repository's root, and updates each unmodified one to the
newest skill release, `skill-v<N>` tags of the release repository. It
checks for a release at most once a day, with a 5-second timeout, and
falls back to the bundled skill when offline or when the bundled one is
newer. A release that needs a newer CLI is not installed. Any other
command run at a terminal prints a reminder when an installation differs
from the CLI's bundle.

## Self-upgrade (`submilli upgrade`)

`submilli upgrade` downloads the latest release, or the tag given with
`--version`, for this platform. It verifies each executable against the
release's SHA-256 checksums and replaces both `submilli` and the
`submilli-server` in the same directory. Both are verified and staged
before either is replaced, so a failure changes nothing. It then runs
`submilli skill sync`. Without `--version` it never moves to an older
release. `--check` prints the latest release and exits 1 if it is newer
than the running CLI, 0 otherwise.

<!-- generated:cli -->

## `submilli run`

```text
Run a Submilli script in-process

Usage: submilli run [OPTIONS] <SCRIPT>

Arguments:
  <SCRIPT>
          Path to the `.ts` or `.subm` script to run

Options:
      --fuel <FUEL>
          Fuel budget. Defaults to the runtime's `RuntimeConfig::default()` value

      --report
          Print fuel, peak accounted memory, and timings to stderr after execution

      --max-stack <MAX_STACK>
          Maximum wasm stack in bytes

      --timeout <TIMEOUT>
          Wall-clock deadline in milliseconds. Omit for no deadline

      --vfs <VFS>
          Directory to expose as the script's VFS root. The future `submilli:fs.*` host functions will resolve paths against it. Omit to allocate a fresh tempdir under the OS temp root for the duration of the run

      --blueprint <BLUEPRINT>
          Apply a blueprint's policy (capability gating, deny-by-default) and `auth_proxy:` secret injection to this local run. Without it, the run is unrestricted (allow-all). `store:` secrets resolve from the local secret store, and authenticated `@mcp/<server>` servers are called in-process — no running server needed

      --var <NAME=VALUE>
          Bind a blueprint variable for this run, `NAME=VALUE` (repeatable), the way an application binds it when it opens a session. Requires `--blueprint`; the blueprint must declare the variable, and its `required` variables must all be bound

      --max-llm-tokens <TOKENS>
          Tokens this run's `submilli:llm` calls may spend in total. A run that asks for more raises a catchable `QuotaExceededError` rather than being billed.
          
          Finite by default, deliberately: unlike `submilli:session`, whose state is memory-only, a blueprint-configured provider spends real money against the operator's credential, and a CLI run has no server-wide ceiling behind it. [default: 1000000] Env: `$SUBMILLI_MAX_EXECUTION_LLM_TOKENS`.

      --max-llm-concurrency <PROMPTS>
          Prompts one `llm.batch` dispatches at once. [default: 4] Env: `$SUBMILLI_MAX_LLM_CONCURRENCY`

      --max-execution-embedding-tokens <TOKENS>
          Tokens this run's `submilli:embedding` calls may spend in total. A call that asks for more raises a catchable `QuotaExceededError`. [default: 2000000] Env: `$SUBMILLI_MAX_EXECUTION_EMBEDDING_TOKENS`, which outranks the config file

      --max-execution-embedding-requests <REQUESTS>
          Outbound provider requests this run's `submilli:embedding` calls may send. [default: 1000] Env: `$SUBMILLI_MAX_EXECUTION_EMBEDDING_REQUESTS`, which outranks the config file

      --max-embedding-concurrency <REQUESTS>
          Provider requests one embedding call sends at once. [default: 4] Env: `$SUBMILLI_MAX_EMBEDDING_CONCURRENCY`, which outranks the config file

  -h, --help
          Print help (see a summary with '-h')
```

## `submilli check`

```text
Typecheck a Submilli script without running it

Usage: submilli check <SCRIPT>

Arguments:
  <SCRIPT>  Path to the `.subm` script to typecheck

Options:
  -h, --help  Print help
```

## `submilli build`

```text
Scaffold, check, and publish a package project (submilli.toml)

Usage: submilli build <COMMAND>

Commands:
  init             Create a submilli.toml with a first package and scaffold its folders
  new              Add a new package to submilli.toml and scaffold its folders
  check            Compile the project's packages in dependency order without installing
  publish-local    Compile the project's packages and install them into the local store
  test             Compile and run the project's `tests/**/*.test.{ts,subm}` files
  security-review  Review package authorization with Codex, Claude Code, or Copilot CLI
  help             Print this message or the help of the given subcommand(s)

Options:
  -h, --help  Print help
```

### `submilli build init`

```text
Create a submilli.toml with a first package and scaffold its folders

Usage: submilli build init [NAME] [PATH]

Arguments:
  [NAME]  First package name (`@scope/name`). Prompted on stdin when omitted
  [PATH]  First package path relative to submilli.toml. Defaults to `.`

Options:
  -h, --help  Print help
```

### `submilli build new`

```text
Add a new package to submilli.toml and scaffold its folders

Usage: submilli build new <NAME> <PATH>

Arguments:
  <NAME>  Package name (`@scope/name`)
  <PATH>  Package path relative to submilli.toml

Options:
  -h, --help  Print help
```

### `submilli build check`

```text
Compile the project's packages in dependency order without installing

Usage: submilli build check [OPTIONS]

Options:
  -p, --package <PACKAGE>  Compile only this package and its sibling dependencies
      --deny-warnings      Fail on code warnings; also enabled by SUBMILLI_DENY_WARNINGS=1
  -h, --help               Print help
```

### `submilli build publish-local`

```text
Compile the project's packages and install them into the local store

Usage: submilli build publish-local [OPTIONS]

Options:
  -p, --package <PACKAGE>  Compile only this package and its sibling dependencies
      --deny-warnings      Fail on code warnings; also enabled by SUBMILLI_DENY_WARNINGS=1
  -h, --help               Print help
```

### `submilli build test`

```text
Compile and run the project's `tests/**/*.test.{ts,subm}` files

Usage: submilli build test [OPTIONS]

Options:
  -p, --package <PACKAGE>  Compile only this package and its sibling dependencies
      --deny-warnings      Fail on code warnings; also enabled by SUBMILLI_DENY_WARNINGS=1
      --all-env            Supply all Unicode process environment variables as test credentials
      --env-var <NAME>     Supply a process variable (repeatable or comma-separated); fail if unset or non-Unicode
      --env-file <PATH>    Read credentials from a file; relative paths use the current directory. Fail if unreadable
      --skip-network       Skip network.test.{ts,subm} and network_*.test.{ts,subm} anywhere under tests/
  -h, --help               Print help

Tests receive no credentials by default. Credential precedence (highest first): --env-var > --env-file > --all-env, regardless of argument order.
```

### `submilli build security-review`

```text
Review package authorization with Codex, Claude Code, or Copilot CLI

Usage: submilli build security-review [OPTIONS] --agent <AGENT> --model <MODEL>

Options:
  -a, --agent <AGENT>      Installed coding agent to invoke; uses its existing authentication [possible values: codex, claude, copilot]
  -m, --model <MODEL>      Provider model ID; "astra" resolves to gpt-6-astra for Codex/Copilot
  -e, --effort <EFFORT>    Reasoning effort; the selected CLI/model must support it [possible values: low, medium, high]
  -p, --package <PACKAGE>  Review this package and its local dependency closure; default: all packages
      --fail-on <FAIL_ON>  Fail on findings at this severity or higher. Incomplete reviews always fail [default: high] [possible values: low, medium, high, critical]
      --output <FILE>      Write a JSON report, including failures. Refuses to replace an existing file
      --timeout <TIMEOUT>  Maximum time for the agent, in seconds (1–3600) [default: 600]
  -h, --help               Print help
```

## `submilli install`

```text
Install a package from GitHub into the local store, pinned to a commit

Usage: submilli install [OPTIONS] <URL> [PACKAGE]

Arguments:
  <URL>      GitHub repo to install from: `org/repo`, `github.com/org/repo`, or a full URL — optionally pinned with `@<ref>` (branch, tag, or commit SHA). A private repository needs a GitHub token: see `submilli github authenticate`
  [PACKAGE]  Install only this package (`@org/name`). Omit to install every package the repo declares

Options:
      --upgrade        Re-install over a package already in the store at a different commit
      --deny-warnings  Fail on code warnings; also enabled by SUBMILLI_DENY_WARNINGS=1
  -h, --help           Print help
```

## `submilli docs`

```text
Print a stdlib package's declarations and description

Usage: submilli docs [OPTIONS] <NAME>

Arguments:
  <NAME>  Package name, e.g. `submilli:http` or an installed `@org/name`. A language built-in (`Temporal`, `Temporal.Instant`) resolves here too

Options:
      --blueprint <BLUEPRINT>  Show only what programs under this blueprint could import. Without it, the whole library is shown. An `@mcp/<server>` package is always read from a blueprint, by default blueprint.yaml
  -h, --help                   Print help
```

## `submilli search`

```text
Search available stdlib packages by name, description, or symbol

Usage: submilli search [OPTIONS] [QUERY]

Arguments:
  [QUERY]  Substring to match against module names, descriptions, and exported symbols. Omit to list every package

Options:
      --blueprint <BLUEPRINT>  List only what programs under this blueprint could import. Its `@mcp/<server>` packages are not listed: read one with `submilli docs @mcp/<server> --blueprint <path>`. Without it, the whole library is listed
  -h, --help                   Print help
```

## `submilli builtins`

```text
Print declarations for language built-ins, or list the catalog

Usage: submilli builtins [NAMES]...

Arguments:
  [NAMES]...  Built-in names to describe, e.g. `Array Map Temporal`. A dotted member path (`Temporal.Instant`) prints just that member. Omit to list the full catalog

Options:
  -h, --help  Print help
```

## `submilli skill`

```text
Install or update the Submilli coding-assistant skill

Usage: submilli skill <COMMAND>

Commands:
  install  Install the skill bundled with this CLI (no network required)
  update   Refresh an installed, unmodified skill from this CLI's bundle
  status   Check installation integrity and freshness; exit 1 if missing or different
  sync     Bring every unmodified installation for this user and project up to the newest released skill. Falls back to this CLI's bundle when offline
  help     Print this message or the help of the given subcommand(s)

Options:
  -h, --help  Print help
```

### `submilli skill install`

```text
Install the skill bundled with this CLI (no network required)

Usage: submilli skill install [OPTIONS] --agent <AGENT>

Options:
      --agent <AGENT>        Assistant whose discovery directory to use [possible values: claude, codex, cursor]
      --project <DIRECTORY>  Install in this project instead of the user home directory
  -h, --help                 Print help
```

### `submilli skill update`

```text
Refresh an installed, unmodified skill from this CLI's bundle

Usage: submilli skill update [OPTIONS] --agent <AGENT>

Options:
      --agent <AGENT>        Assistant whose discovery directory to use [possible values: claude, codex, cursor]
      --project <DIRECTORY>  Install in this project instead of the user home directory
  -h, --help                 Print help
```

### `submilli skill status`

```text
Check installation integrity and freshness; exit 1 if missing or different

Usage: submilli skill status [OPTIONS] --agent <AGENT>

Options:
      --agent <AGENT>        Assistant whose discovery directory to use [possible values: claude, codex, cursor]
      --project <DIRECTORY>  Install in this project instead of the user home directory
  -h, --help                 Print help
```

### `submilli skill sync`

```text
Bring every unmodified installation for this user and project up to the newest released skill. Falls back to this CLI's bundle when offline

Usage: submilli skill sync

Options:
  -h, --help  Print help
```

## `submilli upgrade`

```text
Replace this executable with the latest published release

Usage: submilli upgrade [OPTIONS]

Options:
      --version <TAG>  Release tag to install (for example v0.2.0). Defaults to the latest release
      --check          Report the latest release without installing it; exit 1 if it is newer
  -h, --help           Print help
```

## `submilli blueprint`

```text
Author a blueprint file locally (scaffold, edit)

Usage: submilli blueprint <COMMAND>

Commands:
  init         Scaffold a minimal deny-by-default `blueprint.yaml` in the current directory (`--full` lists every stdlib capability). Offline — no server needed
  git          Configure Git identity and HTTPS username in a local blueprint
  lint         Validate a blueprint file's syntax and references. Offline
  add-mcp      Add an outbound MCP server to a local `blueprint.yaml`
  add-package  Add an already-installed package to a local `blueprint.yaml`
  variable     Manage the session variables declared in a local `blueprint.yaml`'s `variables:` block
  secret       Manage the secrets declared in a local `blueprint.yaml`'s `secrets:` block
  auth-proxy   Manage host-keyed outbound-auth rules in a local `blueprint.yaml`
  capability   Browse gateable capabilities and edit `permissions:` rules in a local `blueprint.yaml`
  prompt       Print the LLM-facing `execute` tool description (the MCP "system prompt") for a blueprint, with placeholders resolved as the server resolves them. Offline — no server needed
  help         Print this message or the help of the given subcommand(s)

Options:
  -h, --help  Print help
```

### `submilli blueprint init`

```text
Scaffold a minimal deny-by-default `blueprint.yaml` in the current directory (`--full` lists every stdlib capability). Offline — no server needed

Usage: submilli blueprint init [OPTIONS] [NAME]

Arguments:
  [NAME]  Blueprint name (the `name:` field). Defaults to the target directory name

Options:
      --blueprint <BLUEPRINT>  Path to write (default: ./blueprint.yaml)
      --full                   Scaffold every stdlib capability as a deny rule with a commented filter example, instead of the minimal empty policy
  -h, --help                   Print help
```

### `submilli blueprint git`

```text
Configure Git identity and HTTPS username in a local blueprint

Usage: submilli blueprint git <COMMAND>

Commands:
  set     Enable or update Git. Values may contain declared ${vars.NAME} references
  show    Show Git templates and token declaration, never secret values
  remove  Disable Git without removing secrets, variables, or permissions
  help    Print this message or the help of the given subcommand(s)

Options:
  -h, --help  Print help
```

#### `submilli blueprint git set`

```text
Enable or update Git. Values may contain declared ${vars.NAME} references

Usage: submilli blueprint git set [OPTIONS] --name <NAME> --email <EMAIL>

Options:
      --name <NAME>            
      --email <EMAIL>          
      --username <USERNAME>    
      --clear-username         
      --blueprint <BLUEPRINT>  
  -h, --help                   Print help
```

#### `submilli blueprint git show`

```text
Show Git templates and token declaration, never secret values

Usage: submilli blueprint git show [OPTIONS]

Options:
      --blueprint <BLUEPRINT>  
  -h, --help                   Print help
```

#### `submilli blueprint git remove`

```text
Disable Git without removing secrets, variables, or permissions

Usage: submilli blueprint git remove [OPTIONS]

Options:
      --blueprint <BLUEPRINT>  
  -h, --help                   Print help
```

### `submilli blueprint lint`

```text
Validate a blueprint file's syntax and references. Offline

Usage: submilli blueprint lint [OPTIONS] <FILE>

Arguments:
  <FILE>  Path to the blueprint YAML file to lint

Options:
      --fix            Add the rules declared packages and their dependencies require for their own calls
      --deny-warnings  Fail on blueprint warnings; also enabled by SUBMILLI_DENY_WARNINGS=1
  -h, --help           Print help
```

### `submilli blueprint add-mcp`

```text
Add an outbound MCP server to a local `blueprint.yaml`

Usage: submilli blueprint add-mcp [OPTIONS] <NAME> <URL>

Arguments:
  <NAME>  Local identifier for the server: its key in the `mcp:` block and the import name `@mcp/<name>`
  <URL>   The MCP endpoint URL

Options:
      --blueprint <BLUEPRINT>
          Blueprint file to edit (default: ./blueprint.yaml)
      --oauth
          Make this an OAuth server (`auth: type: oauth2`). A client id isn't required — the CLI does Dynamic Client Registration at authenticate time
      --client-id <CLIENT_ID>
          OAuth client id, for servers that need a pre-registered client. Implies `--oauth`. A literal or a `${secrets.X}` reference
      --scope <SCOPES>
          OAuth scope to request (repeatable). Implies `--oauth`
      --header <HEADERS>
          Static request header, `"Name: value"` (repeatable). Use `${secrets.X}` for credentials. Conflicts with the OAuth flags
      --authorization-bearer <SECRET_NAME>
          Shorthand for static API-key auth: sets the header `Authorization: Bearer ${secrets.<NAME>}`. The secret must already be declared (`submilli blueprint secret add`). Implies static auth, so the OAuth probe is skipped. Conflicts with the OAuth flags
      --no-probe
          Skip the network probe that auto-detects whether the server needs OAuth
  -h, --help
          Print help
```

### `submilli blueprint add-package`

```text
Add an already-installed package to a local `blueprint.yaml`

Usage: submilli blueprint add-package [OPTIONS] <PACKAGE>

Arguments:
  <PACKAGE>  Package name to add, e.g. `@stripe/sdk`. The packages it depends on get caller rules too

Options:
      --capabilities <NAME,...>  Provided capabilities to allow for `main`, comma-separated
      --all-capabilities         Allow every capability the package provides
      --no-capabilities          Allow none of the provided capabilities (the blueprint's `default:` decides them; under `default: allow` they get explicit `deny` rules)
      --blueprint <BLUEPRINT>    Blueprint file to edit (default: ./blueprint.yaml)
  -h, --help                     Print help
```

### `submilli blueprint variable`

```text
Manage the session variables declared in a local `blueprint.yaml`'s `variables:` block

Usage: submilli blueprint variable <COMMAND>

Commands:
  add     Declare a session variable in the `variables:` block
  list    List the blueprint's declared variables
  remove  Remove a declared variable. Refused while a permission filter still references it
  help    Print this message or the help of the given subcommand(s)

Options:
  -h, --help  Print help
```

#### `submilli blueprint variable add`

```text
Declare a session variable in the `variables:` block

Usage: submilli blueprint variable add [OPTIONS] <NAME>

Arguments:
  <NAME>  Variable name — what `${vars.<NAME>}` references in a filter

Options:
      --required               Reject a session that omits the variable or binds it to an empty string
      --default <VALUE>        Value bound when the caller supplies none. A variable is always a string
      --blueprint <BLUEPRINT>  Blueprint file to edit (default: ./blueprint.yaml)
  -h, --help                   Print help
```

#### `submilli blueprint variable list`

```text
List the blueprint's declared variables

Usage: submilli blueprint variable list [OPTIONS]

Options:
      --blueprint <BLUEPRINT>  Blueprint file to read (default: ./blueprint.yaml)
  -h, --help                   Print help
```

#### `submilli blueprint variable remove`

```text
Remove a declared variable. Refused while a permission filter still references it

Usage: submilli blueprint variable remove [OPTIONS] <NAME>

Arguments:
  <NAME>  The variable to remove

Options:
      --blueprint <BLUEPRINT>  Blueprint file to edit (default: ./blueprint.yaml)
  -h, --help                   Print help
```

### `submilli blueprint secret`

```text
Manage the secrets declared in a local `blueprint.yaml`'s `secrets:` block

Usage: submilli blueprint secret <COMMAND>

Commands:
  add     Declare a secret in the blueprint's `secrets:` block
  list    List the blueprint's declared secrets and their sources (never values)
  remove  Remove a declared secret. Refused while `auth_proxy:`, `mcp:`, `llm:`, or `embedding:` still references it
  help    Print this message or the help of the given subcommand(s)

Options:
  -h, --help  Print help
```

#### `submilli blueprint secret add`

```text
Declare a secret in the blueprint's `secrets:` block

Usage: submilli blueprint secret add [OPTIONS] <--store <KEY>|--harness> <NAME>

Arguments:
  <NAME>  Secret name — what `${secrets.<NAME>}` references

Options:
      --store <KEY>            Read the value from the server's SecretStore under this key
      --harness                Bind the value from the trusted harness separately for each session
      --required               Reject session creation or rebind when the harness value is absent
      --blueprint <BLUEPRINT>  Blueprint file to edit (default: ./blueprint.yaml)
  -h, --help                   Print help
```

#### `submilli blueprint secret list`

```text
List the blueprint's declared secrets and their sources (never values)

Usage: submilli blueprint secret list [OPTIONS]

Options:
      --blueprint <BLUEPRINT>  Blueprint file to read (default: ./blueprint.yaml)
  -h, --help                   Print help
```

#### `submilli blueprint secret remove`

```text
Remove a declared secret. Refused while `auth_proxy:`, `mcp:`, `llm:`, or `embedding:` still references it

Usage: submilli blueprint secret remove [OPTIONS] <NAME>

Arguments:
  <NAME>  The secret to remove

Options:
      --blueprint <BLUEPRINT>  Blueprint file to edit (default: ./blueprint.yaml)
  -h, --help                   Print help
```

### `submilli blueprint auth-proxy`

```text
Manage host-keyed outbound-auth rules in a local `blueprint.yaml`

Usage: submilli blueprint auth-proxy <COMMAND>

Commands:
  add     Add a host-keyed auth-injection rule to the `auth_proxy:` block
  list    List the blueprint's auth-proxy rules (never prints secret values)
  remove  Remove the auth-proxy rule for a host
  help    Print this message or the help of the given subcommand(s)

Options:
  -h, --help  Print help
```

#### `submilli blueprint auth-proxy add`

```text
Add a host-keyed auth-injection rule to the `auth_proxy:` block

Usage: submilli blueprint auth-proxy add [OPTIONS] --host <HOST> <--bearer <SECRET_NAME>|--basic-username <USERNAME>|--header <NAME=VALUE>|--query <KEY=VALUE>>

Options:
      --host <HOST>                   Destination host to match exactly (e.g. `api.github.com`)
      --allow-insecure-http           Permit HTTP for this rule; the blueprint must separately allow_insecure_http
      --bearer <SECRET_NAME>          Bearer-token auth: names a declared secret. Sets `Authorization: Bearer <secret>`
      --basic-username <USERNAME>     Basic-auth username (a literal, not a secret). Requires `--basic-password`
      --basic-password <SECRET_NAME>  Basic-auth password: names a declared secret. Requires `--basic-username`
      --header <NAME=VALUE>           Raw header injection, `Name=value` (repeatable). Value may use `${secrets.X}`
      --query <KEY=VALUE>             Raw query-param injection, `key=value` (repeatable). Value may use `${secrets.X}`
      --blueprint <BLUEPRINT>         Blueprint file to edit (default: ./blueprint.yaml)
  -h, --help                          Print help
```

#### `submilli blueprint auth-proxy list`

```text
List the blueprint's auth-proxy rules (never prints secret values)

Usage: submilli blueprint auth-proxy list [OPTIONS]

Options:
      --blueprint <BLUEPRINT>  Blueprint file to read (default: ./blueprint.yaml)
  -h, --help                   Print help
```

#### `submilli blueprint auth-proxy remove`

```text
Remove the auth-proxy rule for a host

Usage: submilli blueprint auth-proxy remove [OPTIONS] --host <HOST>

Options:
      --host <HOST>            The rule's host to remove
      --blueprint <BLUEPRINT>  Blueprint file to edit (default: ./blueprint.yaml)
  -h, --help                   Print help
```

### `submilli blueprint capability`

```text
Browse gateable capabilities and edit `permissions:` rules in a local `blueprint.yaml`

Usage: submilli blueprint capability <COMMAND>

Commands:
  list    List the capabilities available to the blueprint, annotated with the permission rules already present. Offline — no server needed
  add     Append a permission rule to the `permissions:` block. Rewrites the file; YAML comments are not preserved
  remove  Remove every rule for a capability. Rewrites the file; YAML comments are not preserved
  help    Print this message or the help of the given subcommand(s)

Options:
  -h, --help  Print help
```

#### `submilli blueprint capability list`

```text
List the capabilities available to the blueprint, annotated with the permission rules already present. Offline — no server needed

Usage: submilli blueprint capability list [OPTIONS] [LIBRARY]

Arguments:
  [LIBRARY]  Show one library only: a stdlib module (`submilli:fs`), a declared package or a package one depends on, or a declared MCP server name

Options:
      --blueprint <BLUEPRINT>  Blueprint file to read (default: ./blueprint.yaml). Without a readable blueprint the stdlib catalog is still listed
      --unconfigured           Show only the capabilities declared packages provide that have no rule under `main` (a filtered rule or a `deny` counts as a rule), and the default they fall through to. Requires a readable blueprint
  -h, --help                   Print help
```

#### `submilli blueprint capability add`

```text
Append a permission rule to the `permissions:` block. Rewrites the file; YAML comments are not preserved

Usage: submilli blueprint capability add [OPTIONS] <CAPABILITY>

Arguments:
  <CAPABILITY>  Capability name, e.g. `fs.read` or `mcp.linear`

Options:
      --action <ACTION>        The rule's action. Adding a capability under `default: deny` grants it, so the default is `allow` [default: allow] [possible values: allow, deny, ask-human]
      --filter <FILTER>        Filter expression constraining the rule, e.g. 'path glob "*.csv"'
      --caller <CALLER>        Caller id the rule applies to (`main` is the user script; a package name gates that package's own calls) [default: main]
      --force                  Add the rule even if the capability name isn't known to the stdlib catalog, a declared package, or a declared MCP server
      --blueprint <BLUEPRINT>  Blueprint file to edit (default: ./blueprint.yaml)
  -h, --help                   Print help
```

#### `submilli blueprint capability remove`

```text
Remove every rule for a capability. Rewrites the file; YAML comments are not preserved

Usage: submilli blueprint capability remove [OPTIONS] <CAPABILITY>

Arguments:
  <CAPABILITY>  Capability name whose rules to remove

Options:
      --caller <CALLER>        Caller id to remove the rules from [default: main]
      --blueprint <BLUEPRINT>  Blueprint file to edit (default: ./blueprint.yaml)
  -h, --help                   Print help
```

### `submilli blueprint prompt`

```text
Print the LLM-facing `execute` tool description (the MCP "system prompt") for a blueprint, with placeholders resolved as the server resolves them. Offline — no server needed

Usage: submilli blueprint prompt [OPTIONS]

Options:
      --blueprint <BLUEPRINT>  Blueprint whose policy resolves `{vfs_mode}` / `{http_access}`. Defaults to `./blueprint.yaml` if present, otherwise an empty (deny-all) policy
  -h, --help                   Print help
```

## `submilli secret`

```text
Manage secrets in the local secret store (no running server)

Usage: submilli secret <COMMAND>

Commands:
  put     Store a secret; prompts for the value, or reads it from piped stdin
  delete  Delete a secret by key
  list    List secret keys, optionally filtered by prefix
  help    Print this message or the help of the given subcommand(s)

Options:
  -h, --help  Print help
```

### `submilli secret put`

```text
Store a secret; prompts for the value, or reads it from piped stdin

Usage: submilli secret put <KEY>

Arguments:
  <KEY>  

Options:
  -h, --help  Print help
```

### `submilli secret delete`

```text
Delete a secret by key

Usage: submilli secret delete <KEY>

Arguments:
  <KEY>  

Options:
  -h, --help  Print help
```

### `submilli secret list`

```text
List secret keys, optionally filtered by prefix

Usage: submilli secret list [OPTIONS]

Options:
      --prefix <PREFIX>  Only list keys starting with this prefix
  -h, --help             Print help
```

## `submilli mcp`

```text
Authenticate outbound OAuth MCP servers locally (no running server)

Usage: submilli mcp <COMMAND>

Commands:
  authenticate    Run the OAuth flow for a blueprint's MCP server and store its credential locally. Re-run to re-authenticate (e.g. after changing scopes) — it overwrites the existing credential
  deauthenticate  Remove an MCP server's stored credential (blueprint returns to PENDING)
  auth-status     Show each declared MCP server's authentication state
  provider        Manage local OAuth provider apps (`~/.submilli/mcp_oauth.yaml`) — the client id / secret used to authenticate confidential OAuth servers
  help            Print this message or the help of the given subcommand(s)

Options:
  -h, --help  Print help
```

### `submilli mcp authenticate`

```text
Run the OAuth flow for a blueprint's MCP server and store its credential locally. Re-run to re-authenticate (e.g. after changing scopes) — it overwrites the existing credential

Usage: submilli mcp authenticate --blueprint <BLUEPRINT> <SERVER>

Arguments:
  <SERVER>  The MCP server's local name (its key in the blueprint's `mcp:` block)

Options:
      --blueprint <BLUEPRINT>  Blueprint file that declares the MCP server
  -h, --help                   Print help
```

### `submilli mcp deauthenticate`

```text
Remove an MCP server's stored credential (blueprint returns to PENDING)

Usage: submilli mcp deauthenticate --blueprint <BLUEPRINT> <SERVER>

Arguments:
  <SERVER>  The MCP server's local name (its key in the blueprint's `mcp:` block)

Options:
      --blueprint <BLUEPRINT>  Blueprint file that declares the MCP server
  -h, --help                   Print help
```

### `submilli mcp auth-status`

```text
Show each declared MCP server's authentication state

Usage: submilli mcp auth-status --blueprint <BLUEPRINT>

Options:
      --blueprint <BLUEPRINT>  Blueprint file to report on
  -h, --help                   Print help
```

### `submilli mcp provider`

```text
Manage local OAuth provider apps (`~/.submilli/mcp_oauth.yaml`) — the client id / secret used to authenticate confidential OAuth servers

Usage: submilli mcp provider <COMMAND>

Commands:
  add     Add or replace the provider matching a host
  list    List configured providers (secret references are shown; values are not)
  remove  Remove the provider matching a host
  help    Print this message or the help of the given subcommand(s)

Options:
  -h, --help  Print help
```

#### `submilli mcp provider add`

```text
Add or replace the provider matching a host

Usage: submilli mcp provider add [OPTIONS] --match <MATCH_HOST> --client-id <CLIENT_ID>

Options:
      --match <MATCH_HOST>             OAuth host to match (e.g. `github.com`), compared against the token endpoint's host — not the MCP URL
      --client-id <CLIENT_ID>          OAuth client id. A literal, or a `${secrets.X}` / `${env.X}` reference
      --client-secret <CLIENT_SECRET>  OAuth client secret, as a reference: `${secrets.NAME}` (from the local store) or `${env.VAR}`. Omit for a public (PKCE-only) client
      --scope <SCOPES>                 OAuth scope to request (repeatable)
  -h, --help                           Print help
```

#### `submilli mcp provider list`

```text
List configured providers (secret references are shown; values are not)

Usage: submilli mcp provider list

Options:
  -h, --help  Print help
```

#### `submilli mcp provider remove`

```text
Remove the provider matching a host

Usage: submilli mcp provider remove --match <MATCH_HOST>

Options:
      --match <MATCH_HOST>  The `match` host of the provider to remove
  -h, --help                Print help
```

## `submilli github`

```text
Manage the GitHub token used to install packages from private repositories

Usage: submilli github <COMMAND>

Commands:
  authenticate    Store a GitHub token for installing packages from private repositories. Prompts for it, or reads it from piped stdin. It needs Repository permissions → Contents: Read-only on the package repositories (a fine-grained token), or the `repo` scope (a classic token)
  deauthenticate  Remove the stored GitHub token
  auth-status     Show which GitHub token package fetches use, and whose it is
  help            Print this message or the help of the given subcommand(s)

Options:
  -h, --help  Print help
```

### `submilli github authenticate`

```text
Store a GitHub token for installing packages from private repositories. Prompts for it, or reads it from piped stdin. It needs Repository permissions → Contents: Read-only on the package repositories (a fine-grained token), or the `repo` scope (a classic token)

Usage: submilli github authenticate [OPTIONS]

Options:
      --owner <OWNER>  The user or organization whose repositories the token is for; fills it in on the token-creation link
  -h, --help           Print help
```

### `submilli github deauthenticate`

```text
Remove the stored GitHub token

Usage: submilli github deauthenticate

Options:
  -h, --help  Print help
```

### `submilli github auth-status`

```text
Show which GitHub token package fetches use, and whose it is

Usage: submilli github auth-status

Options:
  -h, --help  Print help
```

## `submilli server`

```text
Interact with a running submilli-server

Usage: submilli server <COMMAND>

Commands:
  apply      Apply blueprint YAML documents to a running submilli-server
  trust      Manage approved HTTPS server public keys
  docs       Read package declarations, including a blueprint's MCP tools
  run-code   Execute a Submilli script on a running submilli-server
  packages   Manage the server's package store and list the packages it can resolve
  status     Report a running server's status (pid, bind, sessions, blueprints)
  stop       Ask a running server to drain and stop
  blueprint  Manage blueprints registered on the server
  secret     Manage secrets in the server's secret store
  session    Open and close sessions, for `run-code --session`
  mcp        Authenticate outbound OAuth MCP servers declared in a blueprint
  help       Print this message or the help of the given subcommand(s)

Options:
  -h, --help  Print help
```

### `submilli server apply`

```text
Apply blueprint YAML documents to a running submilli-server

Usage: submilli server apply [OPTIONS] --file <FILE>

Options:
  -f, --file <FILE>        YAML file (multi-document ok) or directory of .yaml files to apply
      --server <URL>       Base URL of the running submilli-server [env: SUBMILLI_SERVER_URL=] [default: http://127.0.0.1:8128]
      --token-file <PATH>  File holding the API token to send. Without it the token is read from `$SUBMILLI_SERVER_TOKEN`; with neither, no token is sent, which only a server started with `--allow-unauthenticated` accepts. There is no flag taking the token itself, so it never lands in the process list. Env: `$SUBMILLI_SERVER_TOKEN_FILE`
  -h, --help               Print help
```

### `submilli server trust`

```text
Manage approved HTTPS server public keys

Usage: submilli server trust <COMMAND>

Commands:
  add     Inspect and approve a server public key. No token is sent
  list    List approved server public keys in this Submilli home
  remove  Remove a saved public key before approving a verified replacement
  help    Print this message or the help of the given subcommand(s)

Options:
  -h, --help  Print help
```

#### `submilli server trust add`

```text
Inspect and approve a server public key. No token is sent

Usage: submilli server trust add [OPTIONS] --server <URL>

Options:
      --server <URL>          HTTPS server URL whose public key should be approved [env: SUBMILLI_SERVER_URL=]
      --fingerprint <SHA256>  Independently obtained SHA-256 SPKI fingerprint (sha256:<64 hex digits>). Matching it approves trust without an interactive prompt
  -h, --help                  Print help
```

#### `submilli server trust list`

```text
List approved server public keys in this Submilli home

Usage: submilli server trust list

Options:
  -h, --help  Print help
```

#### `submilli server trust remove`

```text
Remove a saved public key before approving a verified replacement

Usage: submilli server trust remove --server <URL>

Options:
      --server <URL>  HTTPS server URL whose saved key should be removed [env: SUBMILLI_SERVER_URL=]
  -h, --help          Print help
```

### `submilli server docs`

```text
Read package declarations, including a blueprint's MCP tools

Usage: submilli server docs [OPTIONS] --blueprint <BLUEPRINT> <NAME>

Arguments:
  <NAME>  Package name, including `@mcp/<server>` virtual packages

Options:
      --blueprint <BLUEPRINT>  Registered blueprint whose package catalog to query
      --server <URL>           Base URL of the running submilli-server [env: SUBMILLI_SERVER_URL=] [default: http://127.0.0.1:8128]
      --token-file <PATH>      File holding the API token to send. Without it the token is read from `$SUBMILLI_SERVER_TOKEN`; with neither, no token is sent, which only a server started with `--allow-unauthenticated` accepts. There is no flag taking the token itself, so it never lands in the process list. Env: `$SUBMILLI_SERVER_TOKEN_FILE`
  -h, --help                   Print help
```

### `submilli server run-code`

```text
Execute a Submilli script on a running submilli-server

Usage: submilli server run-code [OPTIONS] <SCRIPT>

Arguments:
  <SCRIPT>  

Options:
      --blueprint <BLUEPRINT>  Run in a fresh session bound to this registered blueprint
      --session <SESSION>      Run inside a session opened by `submilli server session open`, which keeps its blueprint, variables, files, and session state between runs
      --server <URL>           Base URL of the running submilli-server [env: SUBMILLI_SERVER_URL=] [default: http://127.0.0.1:8128]
      --token-file <PATH>      File holding the API token to send. Without it the token is read from `$SUBMILLI_SERVER_TOKEN`; with neither, no token is sent, which only a server started with `--allow-unauthenticated` accepts. There is no flag taking the token itself, so it never lands in the process list. Env: `$SUBMILLI_SERVER_TOKEN_FILE`
      --var <NAME=VALUE>       Bind a blueprint variable for this run, `NAME=VALUE` (repeatable), the way an application binds it when it opens a session
      --timeout <TIMEOUT>      Omit for no client-side timeout; the server still enforces its own fuel/epoch limits
  -h, --help                   Print help
```

### `submilli server packages`

```text
Manage the server's package store and list the packages it can resolve

Usage: submilli server packages <COMMAND>

Commands:
  install    Install a GitHub package into the server's store
  list       List the packages the server can resolve, marking any read from a fallback store
  uninstall  Remove an installed package from the server's store
  help       Print this message or the help of the given subcommand(s)

Options:
  -h, --help  Print help
```

#### `submilli server packages install`

```text
Install a GitHub package into the server's store

Usage: submilli server packages install [OPTIONS] <URL> [PACKAGE]

Arguments:
  <URL>      GitHub repo: `org/repo`, `github.com/org/repo`, or a full URL. A private repository needs the server's own GitHub token (`github_token_file` in its config file); this command never sends yours
  [PACKAGE]  Install only this package (`@org/name`). Omit to install every package the repo declares

Options:
      --sha <SHA>          Pin to this commit SHA (or ref). Resolved from the default branch when omitted
      --upgrade            Re-install over a package already present at a different commit
      --deny-warnings      Fail on code warnings; also enabled by SUBMILLI_DENY_WARNINGS=1
      --server <URL>       Base URL of the running submilli-server [env: SUBMILLI_SERVER_URL=] [default: http://127.0.0.1:8128]
      --token-file <PATH>  File holding the API token to send. Without it the token is read from `$SUBMILLI_SERVER_TOKEN`; with neither, no token is sent, which only a server started with `--allow-unauthenticated` accepts. There is no flag taking the token itself, so it never lands in the process list. Env: `$SUBMILLI_SERVER_TOKEN_FILE`
  -h, --help               Print help
```

#### `submilli server packages list`

```text
List the packages the server can resolve, marking any read from a fallback store

Usage: submilli server packages list [OPTIONS]

Options:
      --server <URL>       Base URL of the running submilli-server [env: SUBMILLI_SERVER_URL=] [default: http://127.0.0.1:8128]
      --token-file <PATH>  File holding the API token to send. Without it the token is read from `$SUBMILLI_SERVER_TOKEN`; with neither, no token is sent, which only a server started with `--allow-unauthenticated` accepts. There is no flag taking the token itself, so it never lands in the process list. Env: `$SUBMILLI_SERVER_TOKEN_FILE`
  -h, --help               Print help
```

#### `submilli server packages uninstall`

```text
Remove an installed package from the server's store

Usage: submilli server packages uninstall [OPTIONS] <NAME>

Arguments:
  <NAME>  Package to remove, e.g. `@submilli/jina`

Options:
      --server <URL>       Base URL of the running submilli-server [env: SUBMILLI_SERVER_URL=] [default: http://127.0.0.1:8128]
      --token-file <PATH>  File holding the API token to send. Without it the token is read from `$SUBMILLI_SERVER_TOKEN`; with neither, no token is sent, which only a server started with `--allow-unauthenticated` accepts. There is no flag taking the token itself, so it never lands in the process list. Env: `$SUBMILLI_SERVER_TOKEN_FILE`
  -h, --help               Print help
```

### `submilli server status`

```text
Report a running server's status (pid, bind, sessions, blueprints)

Usage: submilli server status [OPTIONS]

Options:
      --server <URL>       Base URL of the running submilli-server [env: SUBMILLI_SERVER_URL=] [default: http://127.0.0.1:8128]
      --token-file <PATH>  File holding the API token to send. Without it the token is read from `$SUBMILLI_SERVER_TOKEN`; with neither, no token is sent, which only a server started with `--allow-unauthenticated` accepts. There is no flag taking the token itself, so it never lands in the process list. Env: `$SUBMILLI_SERVER_TOKEN_FILE`
  -h, --help               Print help
```

### `submilli server stop`

```text
Ask a running server to drain and stop

Usage: submilli server stop [OPTIONS]

Options:
      --server <URL>       Base URL of the running submilli-server [env: SUBMILLI_SERVER_URL=] [default: http://127.0.0.1:8128]
      --token-file <PATH>  File holding the API token to send. Without it the token is read from `$SUBMILLI_SERVER_TOKEN`; with neither, no token is sent, which only a server started with `--allow-unauthenticated` accepts. There is no flag taking the token itself, so it never lands in the process list. Env: `$SUBMILLI_SERVER_TOKEN_FILE`
  -h, --help               Print help
```

### `submilli server blueprint`

```text
Manage blueprints registered on the server

Usage: submilli server blueprint <COMMAND>

Commands:
  add     Register a new blueprint file with the server; fails if the name is taken
  apply   Register a blueprint file, replacing any existing one with the same name
  list    List blueprints registered on the server
  show    Print a registered blueprint's YAML
  remove  Unregister a blueprint by name
  help    Print this message or the help of the given subcommand(s)

Options:
  -h, --help  Print help
```

#### `submilli server blueprint add`

```text
Register a new blueprint file with the server; fails if the name is taken

Usage: submilli server blueprint add [OPTIONS] <FILE>

Arguments:
  <FILE>  

Options:
      --server <URL>       Base URL of the running submilli-server [env: SUBMILLI_SERVER_URL=] [default: http://127.0.0.1:8128]
      --token-file <PATH>  File holding the API token to send. Without it the token is read from `$SUBMILLI_SERVER_TOKEN`; with neither, no token is sent, which only a server started with `--allow-unauthenticated` accepts. There is no flag taking the token itself, so it never lands in the process list. Env: `$SUBMILLI_SERVER_TOKEN_FILE`
  -h, --help               Print help
```

#### `submilli server blueprint apply`

```text
Register a blueprint file, replacing any existing one with the same name

Usage: submilli server blueprint apply [OPTIONS] <FILE>

Arguments:
  <FILE>  

Options:
      --server <URL>       Base URL of the running submilli-server [env: SUBMILLI_SERVER_URL=] [default: http://127.0.0.1:8128]
      --token-file <PATH>  File holding the API token to send. Without it the token is read from `$SUBMILLI_SERVER_TOKEN`; with neither, no token is sent, which only a server started with `--allow-unauthenticated` accepts. There is no flag taking the token itself, so it never lands in the process list. Env: `$SUBMILLI_SERVER_TOKEN_FILE`
  -h, --help               Print help
```

#### `submilli server blueprint list`

```text
List blueprints registered on the server

Usage: submilli server blueprint list [OPTIONS]

Options:
      --server <URL>       Base URL of the running submilli-server [env: SUBMILLI_SERVER_URL=] [default: http://127.0.0.1:8128]
      --token-file <PATH>  File holding the API token to send. Without it the token is read from `$SUBMILLI_SERVER_TOKEN`; with neither, no token is sent, which only a server started with `--allow-unauthenticated` accepts. There is no flag taking the token itself, so it never lands in the process list. Env: `$SUBMILLI_SERVER_TOKEN_FILE`
  -h, --help               Print help
```

#### `submilli server blueprint show`

```text
Print a registered blueprint's YAML

Usage: submilli server blueprint show [OPTIONS] <NAME>

Arguments:
  <NAME>  

Options:
      --server <URL>       Base URL of the running submilli-server [env: SUBMILLI_SERVER_URL=] [default: http://127.0.0.1:8128]
      --token-file <PATH>  File holding the API token to send. Without it the token is read from `$SUBMILLI_SERVER_TOKEN`; with neither, no token is sent, which only a server started with `--allow-unauthenticated` accepts. There is no flag taking the token itself, so it never lands in the process list. Env: `$SUBMILLI_SERVER_TOKEN_FILE`
  -h, --help               Print help
```

#### `submilli server blueprint remove`

```text
Unregister a blueprint by name

Usage: submilli server blueprint remove [OPTIONS] <NAME>

Arguments:
  <NAME>  

Options:
      --server <URL>       Base URL of the running submilli-server [env: SUBMILLI_SERVER_URL=] [default: http://127.0.0.1:8128]
      --token-file <PATH>  File holding the API token to send. Without it the token is read from `$SUBMILLI_SERVER_TOKEN`; with neither, no token is sent, which only a server started with `--allow-unauthenticated` accepts. There is no flag taking the token itself, so it never lands in the process list. Env: `$SUBMILLI_SERVER_TOKEN_FILE`
  -h, --help               Print help
```

### `submilli server secret`

```text
Manage secrets in the server's secret store

Usage: submilli server secret <COMMAND>

Commands:
  put     Store a secret; prompts for the value, or reads it from piped stdin
  delete  Delete a secret by key
  list    List secret keys, optionally filtered by prefix
  help    Print this message or the help of the given subcommand(s)

Options:
  -h, --help  Print help
```

#### `submilli server secret put`

```text
Store a secret; prompts for the value, or reads it from piped stdin

Usage: submilli server secret put [OPTIONS] <KEY>

Arguments:
  <KEY>  

Options:
      --server <URL>       Base URL of the running submilli-server [env: SUBMILLI_SERVER_URL=] [default: http://127.0.0.1:8128]
      --token-file <PATH>  File holding the API token to send. Without it the token is read from `$SUBMILLI_SERVER_TOKEN`; with neither, no token is sent, which only a server started with `--allow-unauthenticated` accepts. There is no flag taking the token itself, so it never lands in the process list. Env: `$SUBMILLI_SERVER_TOKEN_FILE`
  -h, --help               Print help
```

#### `submilli server secret delete`

```text
Delete a secret by key

Usage: submilli server secret delete [OPTIONS] <KEY>

Arguments:
  <KEY>  

Options:
      --server <URL>       Base URL of the running submilli-server [env: SUBMILLI_SERVER_URL=] [default: http://127.0.0.1:8128]
      --token-file <PATH>  File holding the API token to send. Without it the token is read from `$SUBMILLI_SERVER_TOKEN`; with neither, no token is sent, which only a server started with `--allow-unauthenticated` accepts. There is no flag taking the token itself, so it never lands in the process list. Env: `$SUBMILLI_SERVER_TOKEN_FILE`
  -h, --help               Print help
```

#### `submilli server secret list`

```text
List secret keys, optionally filtered by prefix

Usage: submilli server secret list [OPTIONS]

Options:
      --prefix <PREFIX>    Only list keys starting with this prefix
      --server <URL>       Base URL of the running submilli-server [env: SUBMILLI_SERVER_URL=] [default: http://127.0.0.1:8128]
      --token-file <PATH>  File holding the API token to send. Without it the token is read from `$SUBMILLI_SERVER_TOKEN`; with neither, no token is sent, which only a server started with `--allow-unauthenticated` accepts. There is no flag taking the token itself, so it never lands in the process list. Env: `$SUBMILLI_SERVER_TOKEN_FILE`
  -h, --help               Print help
```

### `submilli server session`

```text
Open and close sessions, for `run-code --session`

Usage: submilli server session <COMMAND>

Commands:
  open   Open a session bound to a registered blueprint and print its id
  close  Close a session, discarding its files and session state
  help   Print this message or the help of the given subcommand(s)

Options:
  -h, --help  Print help
```

#### `submilli server session open`

```text
Open a session bound to a registered blueprint and print its id

Usage: submilli server session open [OPTIONS] --blueprint <BLUEPRINT>

Options:
      --blueprint <BLUEPRINT>  Name of a blueprint registered on the server
      --var <NAME=VALUE>       Bind a blueprint variable for the whole session, `NAME=VALUE` (repeatable)
      --server <URL>           Base URL of the running submilli-server [env: SUBMILLI_SERVER_URL=] [default: http://127.0.0.1:8128]
      --token-file <PATH>      File holding the API token to send. Without it the token is read from `$SUBMILLI_SERVER_TOKEN`; with neither, no token is sent, which only a server started with `--allow-unauthenticated` accepts. There is no flag taking the token itself, so it never lands in the process list. Env: `$SUBMILLI_SERVER_TOKEN_FILE`
  -h, --help                   Print help
```

#### `submilli server session close`

```text
Close a session, discarding its files and session state

Usage: submilli server session close [OPTIONS] <SESSION>

Arguments:
  <SESSION>  Id printed by `submilli server session open`

Options:
      --server <URL>       Base URL of the running submilli-server [env: SUBMILLI_SERVER_URL=] [default: http://127.0.0.1:8128]
      --token-file <PATH>  File holding the API token to send. Without it the token is read from `$SUBMILLI_SERVER_TOKEN`; with neither, no token is sent, which only a server started with `--allow-unauthenticated` accepts. There is no flag taking the token itself, so it never lands in the process list. Env: `$SUBMILLI_SERVER_TOKEN_FILE`
  -h, --help               Print help
```

### `submilli server mcp`

```text
Authenticate outbound OAuth MCP servers declared in a blueprint

Usage: submilli server mcp <COMMAND>

Commands:
  authenticate    Run the OAuth flow for a blueprint's MCP server and store its refresh token. Re-run to re-authenticate (e.g. after changing scopes) — it overwrites the existing credential
  deauthenticate  Remove an MCP server's stored refresh token (blueprint returns to PENDING)
  auth-status     Show each declared MCP server's authentication state
  help            Print this message or the help of the given subcommand(s)

Options:
  -h, --help  Print help
```

#### `submilli server mcp authenticate`

```text
Run the OAuth flow for a blueprint's MCP server and store its refresh token. Re-run to re-authenticate (e.g. after changing scopes) — it overwrites the existing credential

Usage: submilli server mcp authenticate [OPTIONS] <BLUEPRINT> <SERVER>

Arguments:
  <BLUEPRINT>  Blueprint that declares the MCP server
  <SERVER>     The MCP server's local name (its key in the `mcp:` block)

Options:
      --server <URL>       Base URL of the running submilli-server [env: SUBMILLI_SERVER_URL=] [default: http://127.0.0.1:8128]
      --token-file <PATH>  File holding the API token to send. Without it the token is read from `$SUBMILLI_SERVER_TOKEN`; with neither, no token is sent, which only a server started with `--allow-unauthenticated` accepts. There is no flag taking the token itself, so it never lands in the process list. Env: `$SUBMILLI_SERVER_TOKEN_FILE`
  -h, --help               Print help
```

#### `submilli server mcp deauthenticate`

```text
Remove an MCP server's stored refresh token (blueprint returns to PENDING)

Usage: submilli server mcp deauthenticate [OPTIONS] <BLUEPRINT> <SERVER>

Arguments:
  <BLUEPRINT>  Blueprint that declares the MCP server
  <SERVER>     The MCP server's local name (its key in the `mcp:` block)

Options:
      --server <URL>       Base URL of the running submilli-server [env: SUBMILLI_SERVER_URL=] [default: http://127.0.0.1:8128]
      --token-file <PATH>  File holding the API token to send. Without it the token is read from `$SUBMILLI_SERVER_TOKEN`; with neither, no token is sent, which only a server started with `--allow-unauthenticated` accepts. There is no flag taking the token itself, so it never lands in the process list. Env: `$SUBMILLI_SERVER_TOKEN_FILE`
  -h, --help               Print help
```

#### `submilli server mcp auth-status`

```text
Show each declared MCP server's authentication state

Usage: submilli server mcp auth-status [OPTIONS] <BLUEPRINT>

Arguments:
  <BLUEPRINT>  Blueprint to report on

Options:
      --server <URL>       Base URL of the running submilli-server [env: SUBMILLI_SERVER_URL=] [default: http://127.0.0.1:8128]
      --token-file <PATH>  File holding the API token to send. Without it the token is read from `$SUBMILLI_SERVER_TOKEN`; with neither, no token is sent, which only a server started with `--allow-unauthenticated` accepts. There is no flag taking the token itself, so it never lands in the process list. Env: `$SUBMILLI_SERVER_TOKEN_FILE`
  -h, --help               Print help
```

<!-- /generated:cli -->
