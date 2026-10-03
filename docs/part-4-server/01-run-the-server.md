---
title: "Run the server"
description: "How to run submilli-server for an application: start it, configure its admin and user credentials, keep the server private, put its state on a persistent disk, and turn on the secret store."
slug: server/run-the-server
sidebar:
  order: 1
---

This guide shows you how to run `submilli-server` for an application:
start it, configure its admin and user credentials, keep the server
reachable only by your application, put its state on a persistent disk,
and turn on the secret store.

You can configure `submilli-server` through a config file or through
command-line arguments. This page configures it through the file, named
by `--config` or `SUBMILLI_CONFIG`; refer to the [server
settings](/docs/reference/server-settings) reference for the full
file and the arguments.

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

The server requires an API token on every request: a secret string you
generate, at least 32 characters, which each caller sends with its
requests. Put it in `SUBMILLI_SERVER_TOKEN` and start the server, which
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
and exit; so does SIGTERM. To reach a server from another shell or
another machine, refer to [Connect the
CLI](/docs/server/connect-the-cli).

Under a process manager or in a container, declare the token in the
config file instead, read from a file only the server's user can read,
and start the server with no variable at all. The file and the paths in
it can be wherever suits the machine; the paths here are a Linux
deployment's:

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
with `--token-file`, or `SUBMILLI_SERVER_TOKEN_FILE` for a whole shell:

```sh
submilli server status --token-file /etc/submilli/admin.token
```

For an experiment on your own machine, `allow_unauthenticated: true` in
the config file (or `--allow-unauthenticated`) starts the server with no
API token at all. It can't be combined with one: with the variable still
set, the server refuses to start rather than guess which you meant.

## Give the application its own API token

The token so far is an admin token: it can call the whole API, which is
what lets one token serve the CLI and your application alike. That is
fine while both are yours, on one machine or one private network. Before
an agent runs somewhere you don't fully trust, give the application a
`user` token instead. A `user` token runs programs, opens sessions, and
reads what a blueprint offers; everything else, such as replacing the
blueprint that constrains the agent, is refused. Add it as a second
entry, with a file of its own:

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
callers over, and remove the old one. An API token identifies an application, not a
person: your application says who a session is for by binding variables
when it opens one.

## Keep it reachable only by your application

The server is a backend for your application, not a public service.
Every request carries the API token as `Authorization: Bearer <token>`;
without it the answer is `401`, on every endpoint except `GET /healthz`.
Keep the port where only your application can reach it. That is why it
listens on `127.0.0.1` unless `bind` says otherwise, and why the
[Compose](/docs/server/deploy-with-compose) and
[Kubernetes](/docs/server/deploy-on-kubernetes) setups keep it
private too. Run one server per application.

An agent framework that connects over MCP sends the API token like any
other caller. The MCP endpoint also accepts only requests whose `Host`
header is a loopback name, which stops a web page open in a user's
browser from reaching it through a host name that resolves to
`127.0.0.1`. A client that reaches the server by any other name, a
reverse proxy's, a platform's, or a service name under Compose or
Kubernetes, is refused:

```text
403 Forbidden: Host header is not allowed
```

List that name, with its port, in the config file; the Compose file and
the Helm chart do this for their own service names:

```yaml title="server.yaml (fragment)"
mcp_allowed_hosts:
  - submilli:8128
```

## Put its state on a persistent disk

Most of what the server knows has to outlive the process: the blueprints
you registered, the sessions your users are partway through, and the
secrets and packages you gave it. On a laptop this takes care of itself.
Everything lands under `~/.submilli` (or `$SUBMILLI_HOME` when set), and
a restart picks up where it left off. In a container it doesn't: anything
not on a persistent volume is gone after the next deploy. So the question
is what has to be kept, and what losing each piece would cost you.

| What | Setting | Default | If it's lost |
| --- | --- | --- | --- |
| Registered blueprints | `blueprint_dir` | `$SUBMILLI_HOME/server/blueprints` | Every program is refused until you register them again. That's quick if they live in source control and a deploy job applies them ([Manage blueprints in Git](/docs/tutorials/manage-blueprints-in-git)). |
| Open sessions | `session_store_dir` | `$SUBMILLI_HOME/server/sessions` | Your users' sessions end: reconnecting clients get `404 unknown session`. There's nothing to rebuild them from. |
| Sessions' files | `vfs_session_dir` | `$SUBMILLI_HOME/server/vfs/sessions` | Files the agent wrote in a session are gone. |
| Secrets | `secret_store.dir` | `$SUBMILLI_HOME/server/secrets` | Blueprints that read `store:` secrets fail until every value is put back. Keep the key file safe too: without it the store can't be read. |
| Packages | `package_store_dir` | `$SUBMILLI_HOME/server/packages` | Imports fail until you reinstall. The same install commands bring them back. |
| Managed volumes | `volume_dir` | `$SUBMILLI_HOME/server/volumes` | Whatever programs kept in [named volumes](/docs/server/mount-a-shared-volume): agent memory, shared notes. Like sessions' files, nothing can rebuild them. |
| Per-run scratch space | `vfs_ephemeral_dir` | The OS temp directory, `/tmp` on Linux; not under `$SUBMILLI_HOME` | Nothing. Each run gets its own directory, deleted when the run ends. |

The first six belong on a persistent disk; the scratch space belongs on
a volatile one, so it never fills the disk the state is on. The simplest
setup keeps the six together, by pointing `SUBMILLI_HOME` at the
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
store's key somewhere separate from the store. Blueprints and packages
can be rebuilt from source.

The server keeps its state under `server/`, apart from the CLI's own
`packages/`, `secrets/`, and `mcp_oauth.yaml`. It does read the CLI's
`packages/` as a read-only fallback, which is why a package installed
with `submilli install` or `submilli build publish-local` is visible to a
server on the same machine, and how the quickstart worked.

## Turn on the secret store

The server's secret store is what a blueprint's `store:` secrets read
from, and where `submilli server mcp authenticate` keeps the OAuth tokens
it obtains. It is encrypted at rest and off until it has a key. Generate one, and
name the file in the config file:

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
`delete` removes one. Nothing reads a value back over the API: the value
is decrypted inside the server process when a package calls
`secrets.get` and never leaves that process.

A credential that belongs to the session rather than the server, a
customer's own API token for instance, is a `harness:` secret that the
application supplies when it opens the session; the server keeps it in
memory only. The [Connect a harness](/docs/tutorials/connect-a-harness)
tutorials show how an application supplies it.
