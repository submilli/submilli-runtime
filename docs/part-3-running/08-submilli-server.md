---
title: "Submilli server"
description: "Operating submilli-server: starting it, who can reach it, where it keeps state, how it is configured, and how blueprints, packages, and secrets get onto it."
slug: server
sidebar:
  order: 8
---

`submilli-server` is the process that runs the agent's programs. Your
application sends it a program and the name of a blueprint; the server
compiles the program, runs it under that blueprint's rules, and returns the
result ([how Submilli works](/docs/how-submilli-works)).

This chapter is about operating that process: starting it, where it keeps
things, how it is configured, and how your blueprints, packages, and secrets
get onto it.

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
| `submilli server run-code <file> --blueprint <name>` | Run a program once, the way an application would |
| `submilli server stop` | Ask the server to finish in-flight requests and exit |

`run-code` is the quickest check that a blueprint does what you meant: it
sends the file to `/v1/execute`, prints the program's console output, then the
result, and exits 1 if the program failed.

## Who can reach it

The server is a backend for your application, not a public service. Your
application is its one client: it registers blueprints, opens sessions, and
sends programs, and the server treats whatever arrives on its port as coming
from your application. That is why it listens on `127.0.0.1` by default and
logs a warning when bound anywhere else, and why the container and cluster
setups in [deploying](/docs/deploying) keep it private too.

So run one server per application, and keep its port where only that
application can reach it. The server doesn't yet authenticate callers
itself; until it does, where you put it on the network is how you control
who uses it.

## Where it keeps state

Most of what the server knows has to outlive the process: the blueprints you
registered, the sessions your users are partway through, and the secrets and
packages you gave it. On a laptop this takes care of itself. Everything lands
under `~/.submilli` (or `$SUBMILLI_HOME` when set), and a restart picks up
where it left off. In a container it doesn't: anything not on a persistent
volume is gone after the next deploy. So the question is what has to be kept,
and what losing each piece would cost you.

| What | Where, under `$SUBMILLI_HOME` | If it's lost |
| --- | --- | --- |
| Registered blueprints | `blueprints/` | Every program is refused until you register them again. That's quick if they live in source control ([seed directory](#blueprints-from-a-directory)). |
| Open sessions | `sessions/`, `vfs/sessions/` | Your users' sessions end: reconnecting clients get `404 unknown session`, and files the agent wrote in them are gone. There's nothing to rebuild them from. |
| Secrets | `secrets/` | Blueprints that read `store:` secrets fail until every value is put back. Keep the key file safe too: without it the store can't be read. |
| Packages | `packages/` | Imports fail until you reinstall. The same install commands bring them back. |
| Per-run scratch space | the OS temp dir | Nothing. Each run gets its own directory, deleted when the run ends. |

The simplest setup is one persistent volume for `$SUBMILLI_HOME`. Back up the
sessions and the secrets, and keep the key somewhere separate from the store.
Blueprints and packages can be rebuilt from source. If you need to split
things up, for example to put sessions on faster disk, each directory has its
own flag, environment variable, and config key (`--session-store-dir`,
`SUBMILLI_SESSION_STORE_DIR`, `session_store_dir`).

**Running the CLI and a server on one machine.** With default paths, the two
share some of these directories. For packages that's convenient: a package
built with `submilli install` is already visible to the server, which is how
the quickstart worked. For secrets it's a trap. The CLI stores plain values
and the server stores encrypted ones in the same folder, so the server lists
the CLI's secrets and then fails to read them (`secret store crypto: sealed
blob too short`). Give the server its own `--secret-store-dir`.

## Configure it

On your laptop, flags are all you need. A deployment usually wants more
structure: a config file in your repository, reviewed like code and the same
everywhere, plus environment variables for the few things that differ between
environments, such as the port or where the store key lives. The server reads
flags, `SUBMILLI_*` environment variables, and a YAML file named by
`--config` or `$SUBMILLI_CONFIG`, so you can mix them that way.

Here is a file with every setting spelled out. Apart from the paths, each
value is the default, so treat it as a template and delete what you don't
change:

```yaml title="server.yaml"
bind: 127.0.0.1
port: 8128

blueprint_dir: /srv/submilli/blueprints
session_store_dir: /srv/submilli/sessions
vfs_session_dir: /srv/submilli/vfs
package_store_dir: /srv/submilli/packages
vfs_ephemeral_dir: /tmp/submilli

secret_store:
  dir: /srv/submilli/secrets
  key_file: /etc/submilli/store.key

network:
  allow_localhost: false
  allow_private: false
  allow_ip: []

volumes: {}
mcp_allowed_hosts: []

max_execution_memory: 50
max_session_state_memory: 1024
max_llm_tokens: 20000000
max_execution_llm_tokens: 1000000
max_llm_concurrency: 4
shutdown_grace: 5
telemetry: false
```

```sh
submilli-server --config server.yaml
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

### When sources disagree

The most specific source wins: a flag, then a `SUBMILLI_*` variable, then the
config file. Below the file come the plain `HOST` and `PORT` variables, and
then the built-in default.

`HOST` and `PORT` are there for hosting platforms such as Heroku, Railway, and
Cloud Run, which set `PORT` and expect the process to accept traffic from
outside. So `PORT` on its own also makes the server bind `0.0.0.0`. That is
why the file outranks them: if your file says `bind: 127.0.0.1`, a platform
that happens to set `PORT` can't quietly open the server up.

### Settings only the file can hold

`volumes` and `mcp_oauth` can't be set by flag or environment variable.
`volumes` decides which host directories programs can ever reach, and
`mcp_oauth` holds the OAuth client registrations for MCP servers that need
one. Both widen what a program can touch, so they live in the one file that
gets reviewed.

### Telemetry

`telemetry` is off unless the file or `SUBMILLI_TELEMETRY` turns it on. When
on, the server sends errors and crashes to the Submilli maintainers' Sentry
project, including request details such as client IP addresses and headers.
Leave it off if that data must not leave your infrastructure.

## Register blueprints

A blueprint reaches the server as a file you register; the server keeps its
own copy and never reads the file again. `apply` registers or replaces,
`add` refuses a name already taken:

```sh
submilli server blueprint apply blueprint.yaml
```

```text
Added blueprint 'notes'
```

Run it again after an edit and the answer is `Updated blueprint 'notes'`.
`list` prints the registered names, `show <name>` prints the YAML the server
holds, and `remove <name>` unregisters it. Under the hood the store is a
revision log, so every version ever applied is on disk; `show` and
executions use the latest.

`remove` also ends every open session bound to that blueprint and deletes
its records, so a client reconnecting to one gets `404 unknown session` rather
than its old session back. Re-registering the name later does not bring them
back. On a live server, removing a blueprint cuts off the people using it.

Registration is where a blueprint is checked, so a mistake fails here rather
than on the first program:

- The YAML must parse and every rule's filter must parse; the error points
  at the line:

  ```text
  error: blueprint parse error: permissions.main.[0]: invalid filter `path glob`: expected an operand after `glob`
    path glob
             ^ at line 5 column 7
  ```

- Every declared secret must be resolvable now. A `store:` secret must exist
  in the server's store, and be readable with the server's key (`secret
  check failed: missing secret 'A'`).
- A `persistent` filesystem must name a volume the server declares (`volume
  'shared' is not declared on this server`).
- `env:` and `file:` secret sources are refused over the API, because a
  caller who can register a blueprint would otherwise be able to read the
  server's environment and files, the store key included. Use `store:` or
  `harness:`; the seed directory below is the one way to register a
  blueprint that reads the server's environment.

One thing registration does not check is that the packages in `packages:`
are installed. A blueprint naming a package the server doesn't have
registers fine and runs fine until a program imports it, which fails with
``package `@acme/billing` was not found in …/packages``.

### Blueprints from a directory

A deployment usually keeps blueprints in source control and wants the server
to pick them up, rather than an operator running `apply` after every
deploy. `--blueprint-seed-dir` names a read-only directory of blueprint
YAML that the server reconciles into its store on every start:

```sh
submilli-server --blueprint-seed-dir /etc/submilli/blueprints
```

```text
INFO submilli_server::blueprint_seed: blueprint seed reconcile complete dir=/etc/submilli/blueprints seeded=2 skipped=0 failed=0 unresolved_secrets=0
```

The files win. A seeded blueprint that is edited or removed over the API
comes back in its seeded form at the next start, so a blueprint is owned
either by the directory or by the API, never both. Blueprints the directory
doesn't name are left alone, which means removing a file doesn't remove the
blueprint; do that with `remove`. The store keys on the `name:` inside each
file, not the file name. Because the directory is the operator's, blueprints
seeded from it may use `env:` and `file:` secrets.

Watch the reconcile line after each deploy. A file that fails to register is
logged and counted in `failed` without stopping the server, so a seed
directory the process can't read looks like a healthy server that knows no
blueprints.

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
tag, or branch; `--upgrade` replaces a package already installed at another
commit, without which the command reports it `up to date`. `list` prints
what is installed and `uninstall <name>` removes one. Packages are compiled
at install time, so nothing is built per request.

## Secrets

The server's secret store is what a blueprint's `store:` secrets read from,
and where `submilli server mcp authenticate` keeps the OAuth tokens it
obtains. It is encrypted at rest and off until it has a key:

```sh
head -c 32 /dev/urandom | base64 > store.key
submilli-server --secret-store-key-file store.key
```

The key is 32 random bytes, base64-encoded, read from the file named by
`--secret-store-key-file` or from the environment variable
`SUBMILLI_SECRET_KEY` (a different variable name with
`--secret-store-key-env`). The file wins when both are set, and the key
never appears on a command line. With no key the store stays off, the
server boots normally, and every secret command answers `no secret store is
configured on this server`.

```sh
submilli server secret put billing_api_key
```

```text
Value for 'billing_api_key': [hidden]
Stored secret 'billing_api_key'
```

`put` prompts with echo off or reads a pipe, `list` prints keys, `delete`
removes one. There is no way to read a value back over the API: values are
written in, decrypted inside the server process when a package calls
`secrets.get`, and never leave it. Each secret is one sealed file
(XChaCha20-Poly1305), opened on demand and not held decrypted in memory.

A credential that belongs to the session rather than the server, a customer's
own API token for instance, is a `harness:` secret: the application supplies
it when it opens a session and the server keeps it only in memory for that
session. [Connecting to your harness](/docs/harness) covers it.

## Volumes

A blueprint whose `vfs` is `persistent` names a volume, and the server maps
that name to a directory on its host. The map lives in the config file only,
so which host directories programs can ever touch is decided in one place:

```yaml title="server.yaml (fragment)"
volumes:
  shared: /srv/submilli/volumes/shared
```

`GET /v1/volumes` lists the names, never the paths. Registration refuses a
blueprint naming a volume that isn't in the table, and boot refuses a table
that would let a program reach the server's own state: a relative path, a
volume containing or inside any of the six directories above or the config
file itself, two names for one directory, or one volume nested in another.
The refusal says what it collided with and why:

```text
Error: volume 'oops' (/srv/submilli) contains the blueprint store (/srv/submilli/blueprints): a guest write would reach the blueprint index, letting a program grant itself capabilities. Point the volume at a directory outside it, or move the blueprint store elsewhere
```

## Outbound network

The server usually runs inside your own network, next to things no agent
should touch: your database, internal admin tools, and the cloud metadata
endpoint that hands out credentials to anything that asks. A blueprint that
lets programs call `http.request` is a door to those if a program can be
talked into fetching the wrong URL, for instance by instructions hidden in a
web page the agent read.

So the server blocks private addresses on its own, whatever the blueprint
allows: loopback, the private ranges (RFC 1918, carrier-grade NAT, IPv6
unique local), and link-local, which covers the metadata endpoint. It checks
where a host name actually resolves, so a public name pointing at an internal
address is caught too. The local CLI doesn't block these addresses, which means a
program that fetched a local URL fine under `submilli run` fails on the
server. The error doesn't yet say why; it looks like any other connection
failure:

```text
error: Error: http GET http://localhost:8128/v1/status: network error: error sending request for url (http://localhost:8128/v1/status)
```

If a program fails like this against an internal or local address that you
know is up, the block is the likely cause.

When a package legitimately needs an internal service, open the smallest
hole that works:

| Setting | Opens | Typical use |
| --- | --- | --- |
| `--allow-ip <ip\|cidr>` | One address or range; repeatable | A package that calls one internal API. The right choice in production. |
| `--allow-localhost` | IPv4 and IPv6 loopback | Development, against a service on your own machine |
| `--allow-private` | Every private range | Rarely; it opens your whole internal network |

Grants add up across sources: one made by a flag, an environment variable, or
the file stands, and no other source can take it back. A stray
`SUBMILLI_ALLOW_PRIVATE=1` left in a deployment's environment would therefore
override a file that says `allow_private: false`, so the server calls it out
at startup:

```text
the outbound egress guard was widened by environment variables; the config file cannot revoke these
```

One more network setting is about inbound traffic from agents. The server
also has an MCP endpoint that agents can connect to directly, and it only
answers requests addressed to a loopback host name. That stops a malicious
web page in the user's browser from reaching it through a DNS trick. If
agents reach it through a reverse proxy or a platform host name, add that
name with `--mcp-allowed-host`.

## Limits

An agent writes its own programs, and some of them will be wrong: a loop
that never stops growing a list, a prompt that reads a whole data dump into
memory, a batch that asks a model a million questions. The limits make sure
one bad program fails on its own instead of taking the server down with it
or running up your model provider's bill. They are the operator's settings;
a blueprint can't raise them.

| Setting | Default | What it stops |
| --- | --- | --- |
| `max_execution_memory` | 50 MB | One program holding too much memory. The program gets an error it can catch, and the server carries on. |
| `max_execution_llm_tokens` | 1,000,000 | One program spending too many model tokens. Its next model call fails, telling it to use fewer or shorter calls. |
| `max_llm_tokens` | 20,000,000 | All running programs together spending too much. This is your ceiling on the provider credential at any moment. |
| `max_session_state_memory` | 1024 MB | `submilli:session` state piling up across every open session. A `set` that would pass it is refused. |
| `max_llm_concurrency` | 4 | One `llm.batch` sending too many prompts at once and running into the provider's rate limits. |

Raise a limit when legitimate work hits it, not before. The two token limits
fail with different messages, and the difference matters: the per-program
one tells the agent to use fewer or shorter calls, while the server-wide one
says the program's own spend isn't the problem and the operator needs to
raise `--max-llm-tokens`.

Text costs more memory than its file size suggests. The runtime stores
strings as UTF-16, two bytes per character, so reading a 25 MB document needs
about 50 MB of the execution limit.

To size a container's memory, budget at least:

```text
max_execution_memory × programs running at once + max_session_state_memory
```

Sessions don't last forever either. Each ends after its blueprint's
`idle_timeout` without use (24 hours unless the blueprint says otherwise). A
sweep every 30 seconds closes idle sessions and deletes their `per_session`
files, and because it works from the session store on disk, sessions still
expire on time across a restart.

## Health, logs, and stopping

`GET /v1/status` is the health endpoint and what `submilli server status`
prints:

```sh
curl -s http://127.0.0.1:8128/v1/status
```

```json
{"status":"running","bind_addr":"127.0.0.1:8128","pid":16882,"active_sessions":0,"blueprints":["notes"]}
```

For a container with no shell or `curl`, the binary probes itself:
`submilli-server --health-check` exits 0 when the server answers and 1 with
the reason when it doesn't. The probe is a separate process and can't see
the flags given to the serving one. It resolves the address the same way the
server does, from its own flags, `SUBMILLI_BIND`/`SUBMILLI_PORT`, or the
config file. So a server started with `--port 9000` needs
`--health-check --port 9000`; setting the port through the environment or
the config file instead covers both processes at once.

Logs go to standard error, one line per event, at `info` and above;
`RUST_LOG=submilli_server=debug` raises the level for the server's own
modules.

To stop, send SIGTERM or SIGINT, run `submilli server stop`, or `POST
/v1/shutdown`. The server stops accepting connections, lets requests already
running finish for up to `shutdown_grace` seconds (5 by default), then drops
whatever remains and exits; a second signal skips the wait. Keep the grace
period a few seconds under the container runtime's own stop timeout (Docker
gives 10), or the runtime kills the process mid-drain.

Next: [connecting to your harness](/docs/harness), where an application or
an agent framework opens sessions against a registered blueprint and runs
programs in them; then [deploying](/docs/deploying), which puts everything
above in a container and a cluster.
