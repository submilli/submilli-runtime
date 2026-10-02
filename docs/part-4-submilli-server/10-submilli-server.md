---
title: "Submilli server"
description: "Operating submilli-server: starting it, who can reach it, where it keeps state, how it is configured, and how blueprints, packages, and secrets get onto it."
slug: server
sidebar:
  order: 10
---

The quickstart started `submilli-server` and left it running; the chapters
since worked without it. `submilli-server` is the service your harness or
application uses to run the agent's programs. It is how Submilli runs in
production: one long-lived process that holds your blueprints, packages, and
secrets, runs each session's programs in isolation, and answers every gated
call from the blueprint the session was opened against. An agent framework connects
to it over MCP and gets Submilli as a set of tools; an application calls its
HTTP API. Either way, the server keeps each session's files and state and
enforces the limits that keep one runaway program from touching the others.

Programs run concurrently inside that one process. Each run is a fresh
WebAssembly instance with its own memory and the view of the filesystem its
blueprint allows. An instance costs a few megabytes and no CPU while it waits
on a request, so one server carries many sessions at once, each run within the
[resource limits](/docs/resource-limits) you set.

## Start it

The server checks a token on every request, so it needs one to start. Put it
in `SUBMILLI_SERVER_TOKEN`:

```sh
export SUBMILLI_SERVER_TOKEN=$(openssl rand -hex 32)
submilli-server
```

```text
INFO submilli_server::auth: inbound authentication enabled tokens="SUBMILLI_SERVER_TOKEN (admin)"
INFO submilli_server::serve: submilli-server listening addr=127.0.0.1:8128
```

Apart from the token, everything the server needs it creates on first use.
The `submilli` command drives a running server through its `server`
subcommands, which are HTTP clients for the address above. They send the token
in `SUBMILLI_SERVER_TOKEN`, so in the same shell they work as they are;
`--server http://host:port` points them elsewhere, and `--token-file <path>`
reads the token from a file instead.

```sh
submilli server status
```

```text
status:          running
bind:            127.0.0.1:8128
pid:             16882
active sessions: 0
blueprints:      (none)
```

| Command | Does |
| --- | --- |
| `submilli server status` | Print the bind address, pid, live session count, and registered blueprints |
| `submilli server blueprint add\|apply\|list\|show\|remove` | Manage registered blueprints |
| `submilli server packages install\|list\|uninstall` | Manage the server's package store |
| `submilli server secret put\|list\|delete` | Manage the server's secret store |
| `submilli server mcp authenticate\|auth-status\|deauthenticate` | Authorize the OAuth MCP servers a blueprint declares |
| `submilli server run-code <file> --blueprint <name> [--var NAME=VALUE]` | Run a program once, the way an application would |
| `submilli server session open --blueprint <name> [--var NAME=VALUE]` | Open a session and print its id |
| `submilli server run-code <file> --session <id>` | Run a program inside that session |
| `submilli server session close <id>` | Close the session, discarding its files and state |
| `submilli server stop` | Ask the server to finish in-flight requests and exit |

`run-code` is the quickest check that a blueprint does what you meant: it
sends the file to `/v1/execute`, prints the program's console output on
standard error and the result on standard output, and exits 1 if the program
failed.

## Who can reach it

The server is a backend for your application, not a public service. Every
request carries the token as `Authorization: Bearer <token>`; without it the
answer is `401`, on every endpoint except `GET /healthz`. The server speaks
plain HTTP, so a token that crosses a network can be read on the way: keep the
port where only your application can reach it, and put a reverse proxy with
TLS in front when it can't be. That is why it listens on `127.0.0.1` by
default, and why the container and cluster setups in
[deploying](/docs/deploying) keep it private too. Run one server per
application.

The token in `SUBMILLI_SERVER_TOKEN` is an admin token: it can call the whole
API, which is what lets one variable serve the CLI and your application alike.
That is fine while both are yours, on one machine or one private network.
Before an agent runs somewhere you don't fully trust, give the application a
`user` token instead. A `user` token runs programs, uses sessions and MCP, and
reads what a blueprint offers; everything else, such as replacing the
blueprint that constrains the agent, answers `403`. Extra tokens are declared
in the config file, each read from a file of its own:

```yaml title="server.yaml"
api_tokens:
  - name: app
    role: user            # or admin
    token_file: /etc/submilli/app.token
```

A token is at least 32 characters, and the file must be readable by the user
the server runs as. To rotate one, add a second entry with the same role,
move the callers over, and remove the old one. A token identifies an
application, not a person: your application says who a session is for by
binding variables when it opens one, as [connecting to your
harness](/docs/harness) shows.

`--allow-unauthenticated` starts the server with no token at all, for an
experiment on your own machine or a network that already admits exactly one
caller. It can't be combined with tokens.

Agents can also connect to the server directly, over MCP, sending the token
like any other caller. The MCP endpoint also accepts only requests whose
`Host` header is a loopback name, which stops a web page open in a user's
browser from reaching it through a host name that resolves to `127.0.0.1`.
Behind a reverse proxy or a platform host name, add that name with
`--mcp-allowed-host`.

## Where it keeps state

Most of what the server knows has to outlive the process: the blueprints you
registered, the sessions your users are partway through, and the secrets and
packages you gave it. On a laptop this takes care of itself. Everything lands
under `~/.submilli` (or `$SUBMILLI_HOME` when set), and a restart picks up
where it left off. In a container it doesn't: anything not on a persistent
volume is gone after the next deploy. So the question is what has to be kept,
and what losing each piece would cost you.

| What | Where, under `$SUBMILLI_HOME/server` | If it's lost |
| --- | --- | --- |
| Registered blueprints | `blueprints/` | Every program is refused until you register them again. That's quick if they live in source control and your deploy job applies them ([register blueprints](#register-blueprints)). |
| Open sessions | `sessions/`, `vfs/sessions/` | Your users' sessions end: reconnecting clients get `404 unknown session`, and files the agent wrote in them are gone. There's nothing to rebuild them from. |
| Secrets | `secrets/` | Blueprints that read `store:` secrets fail until every value is put back. Keep the key file safe too: without it the store can't be read. |
| Packages | `packages/` | Imports fail until you reinstall. The same install commands bring them back. |
| Managed volumes | `volumes/`, or `volume_dir` | Whatever programs kept in [named volumes](#volumes): agent memory, shared notes. Like session files, nothing can rebuild them. |
| Per-run scratch space | the OS temp dir, or `vfs_ephemeral_dir` | Nothing. Each run gets its own directory, deleted when the run ends. |

The simplest setup is one persistent volume for `$SUBMILLI_HOME`. Back up the
sessions, the managed volumes and the secrets, and keep the key somewhere
separate from the store.
Blueprints and packages can be rebuilt from source. If you need to split
things up, for example to put sessions on faster disk, each directory has its
own setting (`session_store_dir` and its siblings in the template below).

The server keeps its state under `server/`, apart from the CLI's own
`packages/`, `secrets/`, and `mcp_oauth.yaml`. It does read the CLI's
`packages/` as a read-only fallback, which is why a package installed with
`submilli install` or `submilli build publish-local` is visible to a server on
the same machine, and how the quickstart worked.

## Configure it

On your laptop, flags are all you need. A deployment usually wants more
structure: a config file in your repository, reviewed like code and the same
everywhere, plus environment variables for the few things that differ between
environments, such as the port or where the store key lives. The server reads
three sources: flags, `SUBMILLI_*` environment variables, and a YAML file named
by `--config` or `$SUBMILLI_CONFIG`. When they disagree, a flag beats a
variable and a variable beats the file. Two settings break that order. A file
that says `telemetry: false` wins over `SUBMILLI_TELEMETRY`. And the plain
`PORT` variable that hosting platforms inject ranks below the file: on its own,
`PORT` makes the server listen on every interface, so a file that says
`bind: 127.0.0.1` keeps it private.

Here is a file with every setting spelled out. Apart from the paths and the
token entry, each value is the default, so treat it as a template and delete what
you don't change:

```yaml title="server.yaml"
bind: 127.0.0.1
port: 8128

api_tokens:                           # beside the one in SUBMILLI_SERVER_TOKEN
  - name: app
    role: user                        # admin | user
    token_file: /etc/submilli/app.token
# allow_unauthenticated: true         # instead of tokens, never with them
# github_token_file: /etc/submilli/github.token   # for private packages

blueprint_dir: /srv/submilli/blueprints
session_store_dir: /srv/submilli/sessions
vfs_session_dir: /srv/submilli/vfs
package_store_dir: /srv/submilli/packages
vfs_ephemeral_dir: /tmp/submilli
volume_dir: /srv/submilli/volumes

secret_store:
  dir: /srv/submilli/secrets
  key_file: /etc/submilli/store.key   # or key_env: SUBMILLI_SECRET_KEY

network:
  allow_localhost: false
  allow_private: false
  allow_ip: []

volumes: {}
mcp_allowed_hosts: []

max_execution_memory: 50
max_execution_time: 0
max_execution_fuel: 1T
max_execution_stack: 512
max_session_state_memory: 1024
max_llm_tokens: 20M
max_execution_llm_tokens: 1M
max_llm_concurrency: 4
shutdown_grace: 5
telemetry: false
telemetry_include_source: false
```

```sh
submilli-server --config server.yaml
```

```text
INFO submilli_server::serve: submilli-server listening addr=127.0.0.1:8128
```

The names line up across the three sources: `max_execution_memory` in the
file is `--max-execution-memory` as a flag and `SUBMILLI_MAX_EXECUTION_MEMORY`
in the environment. The settings grouped under a heading in the file are the
exception and use flat flag names: `secret_store.dir` is
`--secret-store-dir`, `network.allow_private` is `--allow-private`, and the
`mcp_allowed_hosts` list is a repeatable `--mcp-allowed-host`.

Fuel and token counts accept decimal suffixes in all three sources: `K` for
1,000, `M` for 1,000,000, `B` for 1,000,000,000, and `T` for
1,000,000,000,000. For example, use `--max-execution-fuel 1T` or
`SUBMILLI_MAX_LLM_TOKENS=20M`. Suffixes are case-insensitive, with no space
before them. Whole numbers may contain underscores between digits, such as
`10_000_000_000`; fractions and scientific notation are not accepted.
These forms apply to `max_execution_fuel`, `max_llm_tokens`, and
`max_execution_llm_tokens`. Memory and stack settings retain their existing units.

Three settings exist only in the file. `api_tokens`, `volumes`, and
`github_token_file` have no flag and no variable, so who else may call the
server, which volumes programs can name, and the server's GitHub
credential are each decided in one reviewable place.
`SUBMILLI_SERVER_TOKEN` is the reverse: it exists only in the environment.

A misspelled key stops the server from starting, rather than being quietly
ignored. A limit you think you set but didn't is worse than a failed boot:

```text
Error: parsing config file `server.yaml`

Caused by:
    unknown field `prot`, expected one of `bind`, `port`, `blueprint_dir`, …
```

### Telemetry

`telemetry` is off unless the file or `SUBMILLI_TELEMETRY` turns it on. When
on, the server reports to the Submilli maintainers' Sentry project:

- **Always:** crashes; usage counters (sessions opened, programs run, lookups
  made); and for each failed program, the kind of failure and the first line
  of its message.
- **Only with `telemetry_include_source`** (or
  `SUBMILLI_TELEMETRY_INCLUDE_SOURCE`): the failed program's source, and its
  full error, including the backtrace or diagnostics that quote the failing
  lines.
- **Never:** client IP addresses, request headers, or the host names of the
  programs' outbound calls.

## Secrets

The server's secret store is what a blueprint's `store:` secrets read from,
and where `submilli server mcp authenticate` keeps the OAuth tokens it
obtains. It is encrypted at rest and off until it has a key:

```sh
head -c 32 /dev/urandom | base64 > store.key
submilli-server --config server.yaml --secret-store-key-file store.key
```

The key can also come from the `SUBMILLI_SECRET_KEY` environment variable;
without one the server boots normally and every secret command answers `no
secret store is configured on this server`.

```sh
submilli server secret put billing_api_key
```

```text
Value for 'billing_api_key': [hidden]
Stored secret 'billing_api_key'
```

`put` prompts for the value with echo off, `list` prints keys, and `delete`
removes one. Nothing reads a value back over the API: the value is decrypted
inside the server process when a package calls `secrets.get` and never leaves
that process.

A credential that belongs to the session rather than the server, a customer's
own API token for instance, is a `harness:` secret that the application
supplies when it opens a session; the server keeps it in memory only.
[Connecting to your harness](/docs/harness) covers it.

## Install packages

```sh
submilli server packages install acme/billing-package @acme/billing
```

```text
installed @acme/billing @ 3f9c2a1b7e40
```

The server fetches the repository from GitHub, builds the package (or every
package the repository declares when none is named), and puts it in its
package store pinned to the commit it resolved. `--sha <ref>` pins a commit,
tag, or branch. A package already installed at the same commit is reported
`up to date` and left alone; one installed at a different commit is refused
unless `--upgrade` is given, which replaces it. `list` prints what is
installed and `uninstall <name>` removes one. Packages are compiled
at install time, so nothing is built per request.

The server fetches with its own GitHub token, never the caller's, so out of
the box it reaches public repositories only. For private ones, put [a GitHub
token](/docs/cli#install-a-package) that can read the package repositories in
a file, and name it in the config file:

```yaml title="server.yaml"
github_token_file: /etc/submilli/github.token
```

The server refuses to start if the file is missing or empty, and reads it
again on every install, so replacing the file rotates the token. An install
of a repository the token can't read fails with `github_access` and says
what the token needs; one that hits GitHub's rate limit fails with
`github_rate_limited`.

## Register blueprints

A blueprint reaches the server as a file you register; the server keeps its
own copy and never reads the file again. `apply` registers or replaces,
`add` refuses a name already taken:

```sh
submilli server blueprint apply blueprint.yaml
```

```text
Added blueprint 'support'
```

Run it again after an edit and the answer is `Updated blueprint 'support'`.
`list` prints the registered names and `show <name>` prints the YAML the
server holds. `remove <name>` unregisters a blueprint and ends every open
session bound to it; on a live server that cuts off the people using it.

With the blueprint registered, the secrets in the store, and the package
installed, the program from the CLI chapter runs on the server the way an
application would run it:

```sh
submilli server run-code credit.ts --blueprint support --var customerId=cus_northwind
```

```text
credited 1500 cents
```

Registration checks the blueprint, so a mistake fails here rather than on the
first program: the YAML and every filter must parse, every `store:` secret
must exist in the server's store, and every named volume, as the root or
under `mounts`, must be one the server declares, with no more access than it
allows.

Blueprint secrets use `store:` for values provisioned in the server's secret
store or `harness:` for values supplied per session. Provision store values
before applying the blueprint.

Registration also checks that every package in `packages:` and its dependencies
can be loaded from the server's package store or the CLI fallback store. A missing
package is refused with an install command. Required capabilities need caller
rules, just as with `submilli blueprint lint`; an explicit deny or a narrower rule
remains an operator choice. This is a registration-time check: uninstalling a
package later can still make a program's import fail.


Keep blueprints in source control and run `submilli server blueprint apply`
from your deploy job with an admin token.

## Volumes

A named volume is storage that outlives sessions. A blueprint names it as its
filesystem root (`vfs: {mode: named, volume: <name>}`) or mounts it at a path
beside the session's files (`vfs.mounts`), and the server decides where it
lives, who may write it, and how big it may grow. The declarations live in
the config file only:

```yaml title="server.yaml (fragment)"
volumes:
  project-memory:
    kind: managed-local
    size_limit: 1GiB
  company-handbook:
    kind: local-path
    path: /srv/company-handbook
    access: read_only
    size_limit: unlimited
```

There are two kinds:

| Kind | Where its files live |
| --- | --- |
| `managed-local` | `<volume_dir>/<name>`, created the first time a program uses it. `volume_dir` defaults to `$SUBMILLI_HOME/server/volumes`; `--volume-dir` and `SUBMILLI_VOLUME_DIR` set it too. |
| `local-path` | The absolute `path` you give. The server never creates or deletes it. |

`size_limit` is required: a size such as `10GB`, or `unlimited`. One limit
covers the volume however many sessions and blueprints use it at once, so a
blueprint can't escape it by mounting the volume at a second path. The count
starts from what the directory holds the first time a program that may write
opens it, and files changed outside Submilli aren't counted until the next
restart. `access` is `read_write` unless you say `read_only`, and a blueprint
can only narrow it.

Every session of every blueprint that names a volume shares that one
directory; within the access you allow, the blueprint's filesystem rules are
the only thing separating one program's files from another's.

Nothing the server does deletes a volume's files. Ending a session or deleting
a blueprint leaves them, and removing a volume from the config only stops
blueprints from naming it: declare it again and its files are still there. To
remove a managed volume for good, delete its directory under `volume_dir`
while the server is stopped.

Two checks guard the declarations. Registration refuses a blueprint that names
a volume the server doesn't declare, or asks for `read_write` on one declared
`read_only`. Boot refuses declarations that would let a program reach the
server's own state: a relative path, a volume that contains or sits inside a
directory the server owns, a `volume_dir` that overlaps one, two names for one
directory, one volume nested in another, or a managed volume name that isn't a
plain directory name. Each refusal says what collided and why.

Earlier versions mapped a name straight to a directory (`shared:
/srv/submilli/volumes/shared`) for blueprints with `vfs: {mode: persistent}`.
Both forms are now refused with the edit that fixes them: the declaration
becomes `shared: {kind: local-path, path: /srv/submilli/volumes/shared,
size_limit: unlimited}`, which keeps using the same directory, and the
blueprint's `mode: persistent` becomes `mode: named`. A blueprint stored with
the old mode stays registered but can't run until you apply the new form.

## Outbound network

The server usually runs inside your network, next to things no agent should
touch: your database, internal admin tools, the cloud metadata endpoint that
hands out credentials. So whatever the blueprint allows, the server blocks
private addresses on its own: loopback, the private ranges, and link-local,
which covers the metadata endpoint. The block covers every connection the
server makes on a program's behalf, including the model endpoints and MCP
servers a blueprint declares. It checks the address a host name resolves to, so
a public name that points at an internal address is caught too. The local CLI
has no such block. A program that fetched a local URL under `submilli run`
fails on the server with an error naming the policy and the flag that would
open it:

```text
blocked by network policy: localhost resolves only to private/loopback IP space; allow-list it on the server with --allow-ip / --allow-localhost / --allow-private
```

When a package needs an internal service, open the smallest hole that works;
in production that is `--allow-ip` for the one address:

| Setting | Opens | Typical use |
| --- | --- | --- |
| `--allow-ip <ip\|cidr>` | One address or range; repeatable | A package that calls one internal API |
| `--allow-localhost` | IPv4 and IPv6 loopback | Development, against a service on your own machine |
| `--allow-private` | Every private range | Not in production; it opens your whole internal network |

Grants add up across flags, environment, and file, and no source can revoke
another's, so a stray `SUBMILLI_ALLOW_PRIVATE=1` overrides a file that says
`allow_private: false`; the server warns at startup whenever one of those
variables is set.

## MCP servers

The MCP servers a blueprint declares are reached from the server once the
blueprint is registered, under the server's network rules and with logins
kept in its secret store. [MCP servers on the server](/docs/server-mcp) shows
how to operate them.

## Limits

Some of the programs an agent writes will be wrong: a loop that never stops, a
list that never stops growing, a batch that asks a model a million questions.
The `max_*` settings in the template make such a program fail on its own
instead of taking the server down or running up your model provider's bill.
They are the operator's; a blueprint can't raise them. [Resource
limits](/docs/resource-limits) describes each one, what a program sees when it
passes it, and how to size a container for them. Set `max_execution_time` in a
deployment whose callers have timeouts of their own: without it, a runaway loop
runs until its fuel is gone, which takes far longer than any caller waits.

## Health, logs, and stopping

`GET /healthz` is the health endpoint: it answers `200` with an empty body,
and it is the one endpoint that needs no token, so a load balancer or a
kubelet can call it without holding a credential. In a container with no shell
or `curl`, use `submilli-server --health-check` as the probe. It calls
`/healthz` and exits 0 when the server answers. Answering means the process is
serving, not that any blueprint is registered or that the secret store has a
key. The probe is a separate process and does not see the server's flags: it
finds the port through `SUBMILLI_PORT` or a config file, so if you changed the
port with a flag, set it one of those ways as well.

`GET /v1/status` is what `submilli server status` prints: the bind address,
pid, session count, and registered blueprints. It needs an `admin` token.

Logs go to standard output, one line per event, at `info` and above;
`RUST_LOG=submilli_server=debug` raises the level.

To stop, send SIGTERM or SIGINT, run `submilli server stop`, or `POST
/v1/shutdown` with an `admin` token. The server lets running requests finish for up to
`shutdown_grace` seconds (5 by default), then exits; a program still running at
that point is cut off and its caller gets no response. Keep the grace period a
few seconds under the container runtime's own stop timeout.

## With a coding agent

A coding agent with the [Submilli skill](/docs/skill) drives the server
through the same `submilli server` commands. These prompts were run with
Claude Code, a local server started with a secret store, the server token in
`SUBMILLI_SERVER_TOKEN` in the agent's shell, and a project
holding the research blueprint from [connecting to your
harness](/docs/harness#the-example-a-research-agent-with-a-notebook).

**Put a blueprint on the server and prove it.**

```text
Put this blueprint on my local server with Jina's key, and show me it works.
```

The agent checks the server's status, its secret store, and its packages
before changing anything. The key isn't there, so it stops and asks for it,
and suggests you store it yourself with `submilli server secret put
jina_api_key`, so the value never passes through the conversation. Told the
key is stored, it applies the blueprint and runs programs with `run-code` as
two users. As `alice`, a search and a page read return real results, and a
note written in one run is read back in the next. Writing to `bob`'s
directory, reading `bob`'s notes, calling Jina's API directly, and running
with no `userId` are each refused.

In the run for this book, it also found a hole. The blueprint's rules
confined the program to the user's directory, but not the package: through
`@submilli/jina`'s download function, `alice` saved a file into `bob`'s
directory. The agent reported it with a fix and left the change to you. The
example blueprint now has that fix.

**Find out why the server behaves differently.**

```text
stock.ts works with submilli run, but on my server it fails. Why, and what should I change?
```

`stock.ts` reads an inventory service on `localhost`. The agent reads the
program, the blueprint, and the service's log, which shows a request from
the local run and none from the server. It explains that the blueprint's
rule matched, and the [outbound network](#outbound-network) block stopped
the connection. It recommends `network.allow_ip: ["127.0.0.1"]` rather than
opening every private range, and warns that `localhost` means the server's
own machine or container, so a server in a container needs the service's
real address instead.

Next: [MCP servers on the server](/docs/server-mcp), for blueprints that
declare them; then [connecting to your harness](/docs/harness), where an
application or an agent framework opens sessions against a registered
blueprint.
