---
title: "Run the server"
description: "How to run submilli-server for an application: start it, configure its admin and user credentials, keep the server private, put its state on a persistent disk, turn on the secret store, and keep its audit trail."
slug: server/run-the-server
sidebar:
  order: 1
authorship:
  label: ai-assisted
  confirmed: true
  contentHash: "e1a7cf98f4a97f20e4f46afd3716000e8620350a7c18e6ba8c8dea3d1051ea2a"
  confirmedAt: "2026-10-05T13:01:53.009Z"
---

This guide shows you how to run `submilli-server` for an application.

You can configure `submilli-server` through a config file or through
command-line arguments. This page uses the file, named by `--config` or
`SUBMILLI_CONFIG`. The [server settings](/docs/reference/server-settings)
reference covers the full file and the arguments.

## Start it

The install script from [Install](/docs/install) puts both binaries
on the machine, `submilli` and `submilli-server`, so a machine with the
CLI already has the server. On a machine with neither, run it there
first. macOS and Linux:

```sh
curl -fsSL https://submilli.ai/install.sh | sh
```

Windows (PowerShell):

```powershell
irm https://submilli.ai/install.ps1 | iex
```

The server requires an API token on every request. The token is a secret
string you generate, at least 32 characters long, and each caller sends
it with its requests. Put it in `SUBMILLI_SERVER_TOKEN` and start the server, which
refuses to start without one:

```sh
export SUBMILLI_SERVER_TOKEN=$(openssl rand -hex 32)
submilli-server
```

```text
ts=2026-10-03T17:05:10.750Z level=info stream=log target=submilli_server::auth msg="inbound authentication enabled" tokens="SUBMILLI_SERVER_TOKEN (admin)"
ts=2026-10-03T17:05:10.758Z level=info stream=log target=submilli_server::serve msg="submilli-server listening" addr=127.0.0.1:8128 protocol=http
```

Apart from the API token, everything the server needs it creates on
first use. The `submilli server` commands read the API token from the
same variable, so in this shell they need no further setup:

```sh
submilli server status
```

```text
status:          running
bind:            127.0.0.1:8128
pid:             17832
active sessions: 0
blueprints:      (none)
```

`submilli server stop` asks the server to finish the requests in flight
and exit, and so does SIGTERM. To reach a server from another shell or
another machine, refer to [Connect the
CLI](/docs/server/connect-the-cli).

Under a process manager or in a container, declare the token in the
config file. Read it from a file that only the server's user can read,
and start the server with no variable. The file and the paths in it can
be wherever suits the machine. The paths here are a Linux deployment's:

```yaml title="server.yaml"
api_tokens:
  - name: admin
    role: admin
    token_file: /etc/submilli/admin.token
```

```sh
openssl rand -hex 32 > /etc/submilli/admin.token
submilli-server --config server.yaml
```

```text
ts=2026-10-03T17:05:12.272Z level=info stream=log target=submilli_server::auth msg="inbound authentication enabled" tokens="admin (admin)"
ts=2026-10-03T17:05:12.327Z level=info stream=log target=submilli_server::serve msg="submilli-server listening" addr=127.0.0.1:8128 protocol=http
```

The `submilli server` commands then read the token from the file too,
with `--token-file`, or with `SUBMILLI_SERVER_TOKEN_FILE` for a shell:

```sh
submilli server status --token-file /etc/submilli/admin.token
```

For an experiment on your own machine, `allow_unauthenticated: true` in
the config file (or `--allow-unauthenticated`) starts the server with no
API token. It can't be combined with one. If the variable is still set,
the server refuses to start, so it never has to guess which you meant.

## Give the application its own API token

The token so far is an admin token. It can call the entire API, so one
token serves the CLI and your application alike. That is fine while both
are yours, on one machine or one private network. Before an agent runs
somewhere you don't fully trust, give the application a `user` token. A
`user` token runs programs, opens sessions, and reads what a Blueprint
offers. The server refuses anything else, such as replacing the Blueprint
that constrains the agent. Add the token as a second entry, with its own
file:

```yaml title="server.yaml"
api_tokens:
  - name: admin
    role: admin
    token_file: /etc/submilli/admin.token
  - name: app
    role: user
    token_file: /etc/submilli/app.token
```

```sh
openssl rand -hex 32 > /etc/submilli/app.token
submilli-server --config server.yaml
```

```text
ts=2026-10-03T17:05:13.799Z level=info stream=log target=submilli_server::auth msg="inbound authentication enabled" tokens="admin (admin), app (user)"
ts=2026-10-03T17:05:13.813Z level=info stream=log target=submilli_server::serve msg="submilli-server listening" addr=127.0.0.1:8128 protocol=http
```

To rotate a token, add another entry with the same role, move the
callers over, and remove the old one. An API token identifies an
application, not a person. Your application says who a session is for by
binding variables when it opens one.

## Keep it reachable only by your application

The server is a backend for your application, not a public service.
Every request carries the API token as `Authorization: Bearer <token>`.
Without it the answer is `401`, on every endpoint except `GET /healthz`.
Keep the port where only your application can reach it. The server
listens on `127.0.0.1` unless `bind` says otherwise, and the
[Compose](/docs/server/deploy-with-compose) and
[Kubernetes](/docs/server/deploy-on-kubernetes) setups keep it
private too. Run one server per application.

An agent framework that connects over MCP sends the API token like any
other caller. The MCP endpoint also accepts only requests whose `Host`
header is a loopback name, which stops a web page open in a user's
browser from reaching it through a host name that resolves to
`127.0.0.1`. The server refuses a client that reaches it by any other
name, such as a reverse proxy's, a platform's, or a service name under
Compose or Kubernetes:

```text
403 Forbidden: Host header is not allowed
```

List that name, with its port, in the config file. The Compose file and
the Helm chart do this for their service names:

```yaml title="server.yaml (fragment)"
mcp_allowed_hosts:
  - submilli:8128
```

## Put its state on a persistent disk

Most of what the server knows has to outlive the process. That includes
the Blueprints you registered, the sessions your users are partway
through, and the secrets and Packages you gave it. On a laptop this takes
care of itself. Everything lands under `~/.submilli` (or `$SUBMILLI_HOME`
when set), and a restart picks up where it left off. In a container,
anything not on a persistent volume is gone after the next deploy. So
decide what has to be kept, and what losing each piece would cost you.

| What | Setting | Default | If it's lost |
| --- | --- | --- | --- |
| Registered Blueprints | `blueprint_dir` | `$SUBMILLI_HOME/server/blueprints` | Every program is refused until you register them again. That's quick if they live in source control and a deploy job applies them ([Manage Blueprints in Git](/docs/tutorials/manage-blueprints-in-git)). |
| Open sessions | `session_store_dir` | `$SUBMILLI_HOME/server/sessions` | Your users' sessions end, and reconnecting clients get `404 unknown session`. There's nothing to rebuild them from. |
| Sessions' files | `vfs_session_dir` | `$SUBMILLI_HOME/server/vfs/sessions` | Files the agent wrote in a session are gone. |
| Secrets | `secret_store.dir` | `$SUBMILLI_HOME/server/secrets` | Blueprints that read `store:` secrets fail until every value is put back. Keep the key file safe too, because without it the store can't be read. |
| Packages | `package_store_dir` | `$SUBMILLI_HOME/server/packages` | Imports fail until you reinstall. The same install commands bring them back. |
| Managed volumes | `volume_dir` | `$SUBMILLI_HOME/server/volumes` | Whatever programs kept in [named volumes](/docs/server/mount-a-shared-volume), such as agent memory and shared notes. Like sessions' files, nothing can rebuild them. |
| Per-run scratch space | `vfs_ephemeral_dir` | The OS temp directory, `/tmp` on Linux (not under `$SUBMILLI_HOME`) | Nothing. Each run gets its own directory, deleted when the run ends. |

The first six belong on a persistent disk. The scratch space belongs on
a volatile one, so it never fills the disk the state is on. The simplest
setup keeps the six together, either by pointing `SUBMILLI_HOME` at the
persistent disk or by naming each directory in the file:

```yaml title="server.yaml"
blueprint_dir: /var/lib/submilli/blueprints
session_store_dir: /var/lib/submilli/sessions
vfs_session_dir: /var/lib/submilli/vfs
package_store_dir: /var/lib/submilli/packages
volume_dir: /var/lib/submilli/volumes
secret_store:
  dir: /var/lib/submilli/secrets
vfs_ephemeral_dir: /tmp/submilli
```

```sh
submilli-server --config server.yaml
```

```text
ts=2026-10-03T17:05:26.518Z level=info stream=log target=submilli_server::serve msg="submilli-server listening" addr=127.0.0.1:8128 protocol=http
```

Back up the sessions, the managed volumes, and the secrets, and keep the
store's key somewhere separate from the store. Blueprints and Packages
can be rebuilt from source.

The server keeps its state under `server/`, apart from the CLI's own
`packages/`, `secrets/`, and `mcp_oauth.yaml`. It does read the CLI's
`packages/` as a read-only fallback. So a Package installed with
`submilli install` or `submilli build publish-local` is visible to a
server on the same machine, and the quickstart relied on this.

## Turn on the secret store

A Blueprint's `store:` secrets read from the server's secret store, and
`submilli server mcp authenticate` keeps the OAuth tokens it obtains
there. The store is encrypted at rest and off until it has a key.
Generate one, and name the file in the config file:

```sh
head -c 32 /dev/urandom | base64 > /etc/submilli/store.key
```

```yaml title="server.yaml (fragment)"
secret_store:
  key_file: /etc/submilli/store.key
```

The key can also come from the `SUBMILLI_SECRET_KEY` environment
variable. Without one the server boots normally and every secret command
answers `no secret store is configured on this server`.

```sh
submilli server secret put billing_api_key
```

```text
Value for 'billing_api_key': [hidden]
Stored secret 'billing_api_key'
```

`put` prompts for the value with echo off, `list` prints keys, and
`delete` removes one. Nothing reads a value back over the API. The server
decrypts the value inside its process when a Package calls `secrets.get`,
and the value never leaves that process.

A credential that belongs to the session, such as a customer's API token,
is a `harness:` secret. The application supplies it when it opens the
session, and the server keeps it in memory only. The [Connect a harness](/docs/tutorials/connect-a-harness)
tutorials show how an application supplies it.

## Keep an audit trail

The server records what it decided and what changed. That covers each
operation a program was refused, the operations it was allowed, each run
and session, each change an admin made, and each request it turned away
for a bad token. The records are on from the start. They go to the same
output as the log, marked `stream=audit`, so a log collector can route
them apart. Here is a refused write from a program that tried to write
outside its notes:

```text
ts=2026-10-03T20:02:39.695Z level=info stream=audit target=submilli_server::audit msg=decision blueprint=support blueprint_hash=5a2a1e61c440e6b36c43c60b2eff7ecad0ed7989bf5dee30d486678206a480d0 caller=main capability=fs.write context.length=1 context.path=/etc/passwd decision=deny event_id=4760971b-3dee-412c-9dea-b1d77bcb1420 execution_id=0dfa846a-4483-4b8a-b271-45339daecbaf principal=SUBMILLI_SERVER_TOKEN reason="policy denied the capability" rule=default schema=submilli.audit/1 source=policy type=decision
```

The record names the run, the Blueprint and the version of it in force,
who called, what the program asked for and with which values, and the
rule that decided (here the Blueprint's `default`). To keep the trail in
a separate file:

```yaml title="server.yaml (fragment)"
logging:
  audit:
    file: /var/log/submilli/audit.log
```

Allowed operations are summarized, one record per rule a run used, with
a count. With `allows: all`, the server records each one. [Audit
trail](/docs/reference/audit-trail) lists the records and their fields,
and [Server settings](/docs/reference/server-settings#audit-trail) the
settings.
