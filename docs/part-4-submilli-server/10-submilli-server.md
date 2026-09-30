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

The server checks a token on every request, and it won't start until it has at
least one to check:

```sh
submilli-server
```

```text
Error: no API tokens are configured, so nothing could call this server. Declare at least one under `api_tokens:` in the config file (`--config`), or serve without authentication by setting `allow_unauthenticated: true` (`--allow-unauthenticated`, `$SUBMILLI_ALLOW_UNAUTHENTICATED=1`)
```

Tokens are declared in a config file. The file names where each token comes
from and never holds the token itself, so it is safe to commit:

```yaml title="server.yaml"
api_tokens:
  - name: ops
    role: admin
    token_env: SUBMILLI_ADMIN_TOKEN
  - name: app
    role: user
    token_env: SUBMILLI_USER_TOKEN
```

Generate the two tokens and start the server with the file:

```sh
export SUBMILLI_ADMIN_TOKEN=$(openssl rand -hex 32)
export SUBMILLI_USER_TOKEN=$(openssl rand -hex 32)
submilli-server --config server.yaml
```

```text
INFO submilli_server::auth: inbound authentication enabled tokens="ops (admin), app (user)"
INFO submilli_server::serve: submilli-server listening addr=127.0.0.1:8128
```

The admin token is yours, for operating the server. The user token is the one
your application sends; [who can reach it](#who-can-reach-it) explains the
split. Apart from the tokens, everything the server needs it creates on first
use.

The `submilli` command drives a running server through its `server`
subcommands, which are HTTP clients for the address above. They send the token
in `SUBMILLI_ADMIN_TOKEN`, so in the shell that exported it they work as they
are:

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

In another shell, or on another machine, give the command the token and the
address. Both are options on every `submilli server` command:

| Option | Environment variable | Default |
| --- | --- | --- |
| `--server <url>` | `SUBMILLI_SERVER_URL` | `http://127.0.0.1:8128` |
| `--token-file <path>` | `SUBMILLI_SERVER_TOKEN_FILE` | the token in `SUBMILLI_ADMIN_TOKEN` |

`submilli apply` has neither option and reads the same environment variables;
it has no default address, so `SUBMILLI_SERVER_URL` must be set for it.

There is no option that takes the token itself, so it never appears in your
shell history or the process list. A command run without a token the server
accepts says so:

```text
error: the server did not accept this command's API token. Set `SUBMILLI_ADMIN_TOKEN` to a token from the server's `api_tokens`, or `SUBMILLI_SERVER_TOKEN_FILE` to a file holding one
```

| Command | Does |
| --- | --- |
| `submilli server status` | Print the bind address, pid, live session count, and registered blueprints |
| `submilli server blueprint add\|apply\|list\|show\|remove` | Manage registered blueprints |
| `submilli server packages install\|list\|uninstall` | Manage the server's package store |
| `submilli server secret put\|list\|delete` | Manage the server's secret store |
| `submilli server mcp authenticate\|auth-status\|deauthenticate` | Authorize the OAuth MCP servers a blueprint declares |
| `submilli server run-code <file> --blueprint <name> [--var NAME=VALUE]` | Run a program once, the way an application would |
| `submilli server stop` | Ask the server to finish in-flight requests and exit |

`run-code` is the quickest check that a blueprint does what you meant: it
sends the file to `/v1/execute`, prints the program's console output on
standard error and the result on standard output, and exits 1 if the program
failed.

## Who can reach it

The server is a backend for your application, not a public service. Two things
decide who can use it: a token on every request, and where the port is
reachable from. Use both.

### Tokens and roles

Every request carries `Authorization: Bearer <token>`, and the token's role
decides what it may call:

| Role | May call |
| --- | --- |
| `user` | `POST /v1/execute`; everything under `/v1/sessions`; the MCP endpoint `/mcp/{blueprint}`; and the reads that describe one blueprint: `GET /v1/blueprints/{name}/prompt`, `…/packages/search`, `…/packages/docs`, `…/builtins`, and `…/builtins/docs` |
| `admin` | Everything a `user` token may, plus registering, reading, and removing blueprints (`/v1/blueprints`), `/v1/secrets`, `/v1/packages`, `/v1/capabilities`, `/v1/volumes`, the MCP login routes under `/v1/mcp/`, `GET /v1/status`, and `POST /v1/shutdown` |
| no token | `GET /healthz` only |

The split follows from what a blueprint is: the limit on what an agent's
programs can do. The `user` token is the one that ends up in your application
and in every agent process that connects over MCP, so it can run programs
against the blueprints you registered and nothing more. It can't replace a
blueprint, read the list of secrets, install a package, or stop the server.
Those need the `admin` token, which belongs to whoever deploys: you, your CI,
the `submilli server` commands. Never hand the admin token to an agent; it
could rewrite the policy that constrains it.

A request with no token, or one the server doesn't know, is refused:

```sh
curl -i http://127.0.0.1:8128/v1/status
```

```text
HTTP/1.1 401 Unauthorized
content-type: application/json
www-authenticate: Bearer

{"error":"unauthorized","message":"missing or unrecognised API token; send `Authorization: Bearer <token>` with one of the tokens in the server's `api_tokens`"}
```

A `user` token on an `admin` endpoint gets `403`:

```text
{"error":"forbidden","message":"this endpoint needs a token with the `admin` role; the token sent has the `user` role"}
```

A token identifies an application, not a person. The server has no notion of
your end users: your application tells it who a session is for by binding
variables when it opens one, as [connecting to your
harness](/docs/harness) shows, and the blueprint's rules take it from there.
Everyone holding the same token has the same access.

### Where tokens come from

Each `api_tokens` entry has a `name`, which the startup log prints, a `role`,
and exactly one source for the token:

- `token_env` names an environment variable of the server process. It needs
  nothing on disk, which makes it the simple choice on a laptop and in
  Compose.
- `token_file` names a file holding the token. It is the better choice where
  a secret manager or Kubernetes mounts secrets as files: the token stays out
  of the process environment and out of anything that prints it.

Either way the token is read once, at startup, with surrounding whitespace
trimmed, and the server keeps only a hash of it. A token must be at least 32
characters of letters, digits, and `-._~+/`; `openssl rand -hex 32` produces a
good one. Names must be unique, and so must tokens. Anything else stops the
server at startup with a message naming the entry, never the token:

```text
Error: `api_tokens` entry `ops`: environment variable `SUBMILLI_ADMIN_TOKEN` is not set
```

To rotate a token, add a second entry with the same role and a new token,
restart, move the callers over, then remove the old entry and restart again.
Both tokens work in between, so nothing has to switch at the same moment.

### Serving without tokens

`allow_unauthenticated: true` in the config file, `--allow-unauthenticated`,
or `SUBMILLI_ALLOW_UNAUTHENTICATED=1` starts the server with no tokens at all.
Every caller that can reach the port then has the whole API, shutdown
included, and the log says so at every start:

```text
WARN submilli_server::auth: inbound authentication is disabled: every process on this machine can run code, manage blueprints, and stop the server. Configure `api_tokens` to require a token
```

That is reasonable for an experiment on your own machine, and for a server
whose network already admits exactly one caller. It is the wrong default for
anything else, which is why it has to be asked for. Setting it together with
`api_tokens` is refused, so a stray environment variable can't switch off
tokens the file declares. Nothing is exempt by address: with tokens
configured, a request from `127.0.0.1` needs one like any other.

### The network still matters

The server speaks plain HTTP. A token sent over a network in the clear can be
read by anything on the path, so keep the port where only your application
can reach it, and put a reverse proxy with TLS in front whenever a request
has to cross a network you don't control. That is why the server listens on
`127.0.0.1` by default, and why the container and cluster setups in
[deploying](/docs/deploying) keep it private as well as requiring tokens. Run
one server per application.

Agents can also connect to the server directly, over MCP, sending the `user`
token like any other caller. The MCP endpoint applies one more check of its
own: it accepts only requests whose `Host` header is a loopback name, which
stops a web page open in a user's browser from reaching it through a host
name that resolves to `127.0.0.1`. Behind a reverse proxy or a platform host
name, add that name with `--mcp-allowed-host`. This guards against DNS
rebinding and is separate from the token: a request needs both.

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
| Registered blueprints | `blueprints/` | Every program is refused until you register them again. That's quick if they live in source control and the server seeds from them ([register blueprints](#register-blueprints)). |
| Open sessions | `sessions/`, `vfs/sessions/` | Your users' sessions end: reconnecting clients get `404 unknown session`, and files the agent wrote in them are gone. There's nothing to rebuild them from. |
| Secrets | `secrets/` | Blueprints that read `store:` secrets fail until every value is put back. Keep the key file safe too: without it the store can't be read. |
| Packages | `packages/` | Imports fail until you reinstall. The same install commands bring them back. |
| Per-run scratch space | the OS temp dir, or `vfs_ephemeral_dir` | Nothing. Each run gets its own directory, deleted when the run ends. |

The simplest setup is one persistent volume for `$SUBMILLI_HOME`. Back up the
sessions and the secrets, and keep the key somewhere separate from the store.
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
tokens, each value is the default, so treat it as a template and delete what
you don't change:

```yaml title="server.yaml"
bind: 127.0.0.1
port: 8128

api_tokens:
  - name: ops
    role: admin                       # admin | user
    token_env: SUBMILLI_ADMIN_TOKEN   # or token_file: /etc/submilli/admin.token
  - name: app
    role: user
    token_env: SUBMILLI_USER_TOKEN
# allow_unauthenticated: true         # instead of api_tokens, never with it

blueprint_dir: /srv/submilli/blueprints
blueprint_seed_dir: /etc/submilli/blueprints
session_store_dir: /srv/submilli/sessions
vfs_session_dir: /srv/submilli/vfs
package_store_dir: /srv/submilli/packages
vfs_ephemeral_dir: /tmp/submilli

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
max_execution_fuel: 1000000000000
max_execution_stack: 512
max_session_state_memory: 1024
max_llm_tokens: 20000000
max_execution_llm_tokens: 1000000
max_llm_concurrency: 4
shutdown_grace: 5
telemetry: false
telemetry_include_source: false
```

```sh
submilli-server --config server.yaml
```

```text
INFO submilli_server::auth: inbound authentication enabled tokens="ops (admin), app (user)"
INFO submilli_server::serve: submilli-server listening addr=127.0.0.1:8128
```

The names line up across the three sources: `max_execution_memory` in the
file is `--max-execution-memory` as a flag and `SUBMILLI_MAX_EXECUTION_MEMORY`
in the environment. The settings grouped under a heading in the file are the
exception and use flat flag names: `secret_store.dir` is
`--secret-store-dir`, `network.allow_private` is `--allow-private`, and the
`mcp_allowed_hosts` list is a repeatable `--mcp-allowed-host`.

Two settings exist only in the file. `api_tokens` and `volumes` have no flag
and no variable, so who may call the server and which host directories
programs can touch are each decided in one reviewable place.
`allow_unauthenticated` does have a flag and a variable
(`--allow-unauthenticated`, `SUBMILLI_ALLOW_UNAUTHENTICATED`); any one of the
three turns it on, and the server refuses to start if it is on while
`api_tokens` has entries. [Who can reach it](#who-can-reach-it) covers tokens
and the opt-out.

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
must exist in the server's store, and a `persistent` filesystem must name a
volume the server declares.

Over the API, `env:` and `file:` secret sources are refused: a caller who can
register a blueprint could otherwise read the server's environment and files.
Use `store:` or `harness:` instead, or seed the blueprint from a directory, as
described next. Registration does not check that the packages in `packages:`
are installed. A missing package fails the first program that imports it, and
the error names the directories it searched.

For a deployment that keeps blueprints in source control, `--blueprint-seed-dir`
names a read-only directory of blueprint YAML that the server registers on
every start, so nobody runs `apply` after a deploy. The directory is the source
of truth: a seeded blueprint edited or removed over the API returns to its
seeded form at the next start, and blueprints the directory doesn't name are
left alone. Because the operator controls the directory, seeded blueprints may
use `env:` and `file:` secrets. A seeded blueprint whose `store:` secret
doesn't exist yet is still registered, so you can add secrets after the first
deploy; programs that need the missing one fail until you do. A file that fails
to register is logged and counted, not fatal, so read the `blueprint seed
reconcile complete` line after a deploy: a nonzero `failed=` means a blueprint
you think is registered isn't.

## Volumes

A blueprint whose `vfs` is `persistent` names a volume, and the server maps
that name to a directory on its host. The map lives in the config file only:

```yaml title="server.yaml (fragment)"
volumes:
  shared: /srv/submilli/volumes/shared
```

Every session of every blueprint that names a volume shares that one
directory, read and write; the blueprint's filesystem rules are the only thing
separating one program's files from another's.

Two checks guard the map. Registration refuses a blueprint that names a volume
the map doesn't have. Boot refuses a map that would let a program reach the
server's own state: a relative path, a volume that contains or sits inside a
directory the server owns, two names for one directory, or one volume nested
in another. Each refusal says what collided and why.

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
Claude Code, a local server started with a secret store, the admin token in
`SUBMILLI_ADMIN_TOKEN` in the agent's shell, and a project
holding the research blueprint from [connecting to your
harness](/docs/harness#the-example-a-research-agent-with-a-notebook).
The coding agent here is acting as you, the operator, which is why it holds
the admin token. The rule against giving an agent that token is about the
agent your application runs, whose programs the blueprint constrains.

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
