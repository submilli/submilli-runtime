---
title: "Submilli server"
description: "Operating submilli-server: starting it, who can reach it, where it keeps state, how it is configured, and how blueprints, packages, and secrets get onto it."
slug: server
sidebar:
  order: 8
---

The quickstart started `submilli-server` and left it running; the last two
chapters worked without it. `submilli-server` is the service your harness or
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
on a request, so one server carries many sessions at once. There is no process
or container boundary between programs: a fault in the runtime itself would
reach every session.

## Start it

```sh
submilli-server
```

```text
INFO submilli_server::serve: submilli-server listening addr=127.0.0.1:8128
```

Everything the server needs it creates on first use, so a bare start on a
fresh machine works. The `submilli` command drives a running server through
its `server` subcommands, which are HTTP clients for the address above;
`--server http://host:port` points them elsewhere.

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
| `submilli server stop` | Ask the server to finish in-flight requests and exit |

`run-code` is the quickest check that a blueprint does what you meant: it
sends the file to `/v1/execute`, prints the program's console output on
standard error and the result on standard output, and exits 1 if the program
failed.

## Who can reach it

The server is a backend for your application, not a public service. Your
application is its one client: it registers blueprints, opens sessions, and
sends programs, and the server treats whatever arrives on its port as coming
from your application. That is why it listens on `127.0.0.1` by default and
logs a warning when bound anywhere else, and why the container and cluster
setups in [deploying](/docs/deploying) keep it private too.

The server does not authenticate callers: no token, no client list, on any
endpoint, `POST /v1/shutdown` included. Anything that can open a connection to
the port can run programs, register blueprints and through them use every
stored secret, install packages, and stop the server. It speaks plain HTTP; a
reverse proxy in front of it is where TLS goes. So run one server per
application, and keep its port where only that application can reach it: where
you put it on the network is the whole access control.

Agents can also connect to the server directly, over MCP. The MCP endpoint
accepts only requests whose `Host` header is a loopback name, which stops a web
page open in a user's browser from reaching it through a host name that
resolves to `127.0.0.1`. Behind a reverse proxy or a platform host name, add
that name with `--mcp-allowed-host`. This is a check on one header, on the MCP
endpoint only; it is not authentication, and the HTTP API has no equivalent.

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

Here is a file with every setting spelled out. Apart from the paths, each
value is the default, so treat it as a template and delete what you don't
change:

```yaml title="server.yaml"
bind: 127.0.0.1
port: 8128

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
INFO submilli_server::serve: submilli-server listening addr=127.0.0.1:8128
```

The names line up across the three sources: `max_execution_memory` in the
file is `--max-execution-memory` as a flag and `SUBMILLI_MAX_EXECUTION_MEMORY`
in the environment. The settings grouped under a heading in the file are the
exception and use flat flag names: `secret_store.dir` is
`--secret-store-dir`, `network.allow_private` is `--allow-private`, and the
`mcp_allowed_hosts` list is a repeatable `--mcp-allowed-host`.

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
submilli-server --secret-store-key-file store.key
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
that name to a directory on its host. The map lives in the config file only,
so which host directories programs can ever touch is decided in one place:

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

## Limits

Some of the programs an agent writes will be wrong: a loop that never stops
growing a list, a batch that asks a model a million questions. The limits make
one bad program fail on its own instead of taking the server down or running
up your model provider's bill. They are the operator's settings; a blueprint
can't raise them.

| Setting | Default | What it stops, and how |
| --- | --- | --- |
| `max_execution_memory` | 50 MB | One program holding too much memory; the run ends with an `out of memory` error |
| `max_execution_time` | off | One program running too long; the run ends with `timeout exceeded` |
| `max_execution_llm_tokens` | 1,000,000 | One program spending too many model tokens; the next model call throws a `RangeError` naming the budget |
| `max_llm_tokens` | 20,000,000 | All running programs together, your ceiling on the provider credential; the next model call throws a `RangeError` naming the budget |
| `max_session_state_memory` | 1024 MB | `submilli:session` state across every open session; a `set` past it throws |
| `max_llm_concurrency` | 4 | One `llm.batch` sending too many prompts at once; the extra prompts wait their turn |
| `idle_timeout` (blueprint) | 24 h | A session nobody has run a program in; a sweep every 30 seconds closes it and deletes its `per_session` files, across restarts too |

Strings are stored as UTF-16, so text costs two bytes per character: a 25 MB
document needs about 50 MB of the execution limit. To size a container, budget
`max_execution_memory` times the programs running at once, plus
`max_session_state_memory`.

`max_execution_time` is whole seconds, `0` disables it, and it counts from the
moment `main` starts, so compiling the program doesn't eat into it. The check
runs once a second, so a program may run up to a second past the limit. A host
call already in flight, an HTTP request for instance, is not interrupted; the
program is stopped when it returns. Without a time limit, a runaway loop runs
until it exhausts a fixed budget of a trillion instructions, which takes far
longer than any caller will wait, so a deployment whose callers have their own
timeouts should set one.

## Health, logs, and stopping

`GET /v1/status` is the health endpoint and what `submilli server status`
prints. In a container with no shell or `curl`, use `submilli-server
--health-check` as the probe. It exits 0 when the server answers. Answering
means the process is serving, not that any blueprint is registered or that the
secret store has a key. The probe is a separate process and does not see the
server's flags: it finds the port through `SUBMILLI_PORT` or a config file, so
if you changed the port with a flag, set it one of those ways as well.

Logs go to standard output, one line per event, at `info` and above;
`RUST_LOG=submilli_server=debug` raises the level.

To stop, send SIGTERM or SIGINT, run `submilli server stop`, or `POST
/v1/shutdown`. The server lets running requests finish for up to
`shutdown_grace` seconds (5 by default), then exits; a program still running at
that point is cut off and its caller gets no response. Keep the grace period a
few seconds under the container runtime's own stop timeout.

Next: [connecting to your harness](/docs/harness), where an application or
an agent framework opens sessions against a registered blueprint; then
[deploying](/docs/deploying), which puts all of this in a container.
