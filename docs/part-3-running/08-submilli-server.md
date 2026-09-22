---
title: "Submilli server"
description: "Operating submilli-server: the trust boundary it assumes, where it keeps state, how it is configured, and how blueprints, packages, and secrets get onto it."
slug: server
sidebar:
  order: 8
---

`submilli-server` is the process that runs the agent's programs. Your
application sends it a program and the name of a blueprint; the server
compiles the program, runs it under that blueprint's rules, and returns the
result ([how Submilli works](/docs/how-submilli-works)). This chapter is about
operating that process: starting it, where it keeps things, how it is
configured, and how the blueprint from [crafting a
blueprint](/docs/blueprints) and the packages from [using the CLI](/docs/cli)
get onto it. Calling it from an application or an agent harness is the [next
chapter](/docs/harness); running it in a container or a cluster is
[deploying](/docs/deploying).

## One server per application

Start with the rule the rest of the chapter assumes. The server has no
authentication of its own: anything that can open a connection to its port
can run programs under any registered blueprint, register or replace
blueprints, and stop the process. The design treats your application as the
trusted party and expects it to be the only thing that can reach the server.
The boundary is the network, not a credential.

So run one server per application, and let nothing else reach it. The server
enforces its half of that: it binds `127.0.0.1` unless told otherwise and
logs a warning when bound to any other address. The container and cluster
shapes in [deploying](/docs/deploying) keep the same rule with a loopback-only
port publish and a network policy. Inbound authentication is planned; until
it ships, this rule is what stands in for it.

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

## Where it keeps state

The server owns six directories. Five sit under one root, `~/.submilli` by
default or `$SUBMILLI_HOME` when set; the sixth is the operating system's
temporary directory. Every one has a flag, an environment variable, and a
config-file key, so a deployment can put each where it belongs.

| Directory | Default | Holds | Survives a restart? |
| --- | --- | --- | --- |
| `--blueprint-dir` | `~/.submilli/blueprints` | Registered blueprints, as a revision log: `index.json` plus one `<name>.<revision>.yaml` per version | Must |
| `--session-store-dir` | `~/.submilli/sessions` | Session bookkeeping: which blueprint and variables each open session is bound to, its idle timer, and the idempotency ledger | Must, for sessions to resume |
| `--vfs-session-dir` | `~/.submilli/vfs/sessions` | The files of every `per_session` blueprint's sessions | Must, for sessions to resume |
| `--secret-store-dir` | `~/.submilli/secrets` | The encrypted secret store | Must |
| `--package-store-dir` | `~/.submilli/packages` | Installed packages | Must |
| `--vfs-ephemeral-dir` | the OS temp dir | One scratch directory per run, deleted when the run returns | Need not |

Two of these are the same directories the local CLI uses. `submilli install`
and `submilli server packages install` write to one package store when both
run on the same machine with default paths, which is why the quickstart's
locally built package was visible to its server without a further step. The
secret directory is shared too, and that one is a trap: the CLI's local store
keeps plain files while the server's store keeps encrypted ones, so a server
with its store enabled lists the CLI's entries but fails to read them
(`secret store crypto: sealed blob too short`). On a machine that runs both,
give the server its own `--secret-store-dir`.

## Configure it

Settings come from three places: flags on the command line, `SUBMILLI_*`
environment variables, and a YAML file named by `--config` or
`$SUBMILLI_CONFIG`. Each key has the same name in all three, with dashes in
the flag (`--max-execution-memory`), underscores in the file
(`max_execution_memory`), and an upper-case prefix in the environment
(`SUBMILLI_MAX_EXECUTION_MEMORY`).

When the same setting is given more than once, the most specific source
wins: a flag, then a `SUBMILLI_*` variable, then the config file, then the
ambient `HOST` and `PORT` variables that platforms inject, then the built-in
default. The file sitting above `HOST`/`PORT` is deliberate. `PORT` on its own
implies binding `0.0.0.0`, because a platform that sets it routes traffic in
from outside; a config file that says `bind: 127.0.0.1` must not be flipped
open by a platform that merely happens to set `PORT`.

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

Every value above is the default apart from the paths, so the file is a
template to trim. A key the server doesn't know is a boot failure, not a
warning:

```text
Error: parsing config file `server.yaml`

Caused by:
    unknown field `prot`, expected one of `bind`, `port`, `blueprint_dir`, …
```

Two settings exist only in the file: `volumes`, and `mcp_oauth`, the OAuth
client registrations for MCP servers that need one. Both map a name a
blueprint can write to something on the host, and keeping them in one
reviewable file is the point. `telemetry` opts in to crash reporting; it is
off unless the file or `SUBMILLI_TELEMETRY` turns it on.

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
holds, `remove <name>` unregisters it. Under the hood the store is a
revision log, so every version ever applied is on disk; `show` and
executions use the latest.

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
seeded from it may use `env:` and `file:` secrets. A file that fails to
register is logged and counted in `failed` without stopping the server, so
watch that line: a seed directory the process can't read looks like a
healthy server that knows no blueprints.

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

A blueprint decides which hosts a program may call; the server adds a floor
underneath that decision. Outbound HTTP to loopback addresses and to private
ranges (RFC 1918, carrier-grade NAT, IPv6 unique local) is blocked by
default, so an allowed `http.request` still can't be pointed at a service on
the server's own host or network. Three settings widen it:

| Setting | Permits |
| --- | --- |
| `--allow-localhost` | IPv4 and IPv6 loopback |
| `--allow-private` | Every private range |
| `--allow-ip <ip\|cidr>` | One address or range; repeatable |

These are additive across the three sources: a grant from a flag, the
environment, or the file stands, and no source can revoke another's. The
server says so at startup when the environment granted anything (`the
outbound egress guard was widened by environment variables; the config file
cannot revoke these`), because a stray `SUBMILLI_ALLOW_PRIVATE=1` in a
deployment's environment is the way this goes wrong.

`--mcp-allowed-host` is the other network setting. The agent-facing MCP
endpoint accepts requests whose `Host` header is loopback, as a guard
against DNS rebinding; behind a reverse proxy or a platform hostname, add the
host clients actually target.

## Limits

The server bounds what one program, and all programs together, can consume.
These are the operator's knobs; a blueprint can't raise them.

| Setting | Default | Bounds |
| --- | --- | --- |
| `max_execution_memory` | 50 MB | Memory one execution may hold live; a program that asks for more gets a catchable error |
| `max_session_state_memory` | 1024 MB | `submilli:session` state across every live session; a `set` that would exceed it is refused |
| `max_execution_llm_tokens` | 1,000,000 | Tokens one execution's model calls may spend |
| `max_llm_tokens` | 20,000,000 | Tokens every live execution's model calls may spend together, so the process has a ceiling against your provider credential |
| `max_llm_concurrency` | 4 | Prompts one `llm.batch` sends at once |

Strings are UTF-16 inside the runtime, so text costs two bytes per character
against the execution memory limit: a 25 MB document needs about 50 MB. To
size a container's memory, budget the execution limit times the number of
programs you expect to run at once, plus the session-state limit.

Sessions themselves are bounded by the blueprint's `idle_timeout` (24 hours
unless the blueprint says otherwise); a background sweep closes idle sessions
every 30 seconds and deletes their `per_session` files. The bookkeeping in
the session store is what lets that sweep, and a client's reconnect, survive
a restart.

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
the reason when it doesn't. The probe is a separate process, so it finds the
address from `SUBMILLI_BIND`/`SUBMILLI_PORT` or the config file, not from
flags given to the serving process; a container that moves the port should
do so through one of those.

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
