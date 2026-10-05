---
title: "Connect the CLI"
description: "How to point the submilli server commands at a server: its token from a file, its address, the first check, trusting its HTTPS certificate, keeping both in your shell, switching between servers by name, and which commands need the admin token."
slug: server/connect-the-cli
sidebar:
  order: 2
authorship:
  label: ai-assisted
  confirmed: true
  contentHash: "4454eb41f05507ae24295c0a81cb337844e4083b490941cac9b2909a90bd0668"
  confirmedAt: "2026-10-05T10:59:51.485Z"
---

The `submilli server` commands talk to a running server over HTTP or
HTTPS. Out of the box they call `http://127.0.0.1:8128` and send the token from
`SUBMILLI_SERVER_TOKEN`, so they need no setup in the shell that started
the server. From another machine, or from a deploy job, the address is
different and the token shouldn't be typed.

This guide shows you how to point the `submilli server` commands at a
server.

## Give it the token

Without a token the server answers `401`, and the commands say so:

```text
error: the server did not accept this command's API token. Set `SUBMILLI_SERVER_TOKEN` to the token the server was started with, or `SUBMILLI_SERVER_TOKEN_FILE` to a file holding one
```

Set `SUBMILLI_SERVER_TOKEN` in the shell, or point `--token-file`, or
`SUBMILLI_SERVER_TOKEN_FILE`, at a file holding it. No flag takes the
token itself, so it never lands in the process list or in shell history.

On Kubernetes the chart keeps the tokens in a Secret. Read the admin one
into a file first:

```sh
kubectl get secret submilli-auth -o jsonpath='{.data.admin-token}' | base64 -d > admin.token
```

## Name the address

`--server` names the server, on every `submilli server` command, and
`SUBMILLI_SERVER_URL` sets it for a shell. Check the connection with
`status`, which needs the admin token:

```sh
export SUBMILLI_SERVER_URL=http://10.0.12.7:8128
submilli server status --token-file admin.token
```

```text
status:          running
bind:            0.0.0.0:8128
pid:             39225
active sessions: 0
blueprints:      support
```

Under Compose the port is published on the host's loopback, so the
commands work from the host without `--server`, and `docker compose exec
submilli submilli server status` works from inside the container. On
Kubernetes, `kubectl port-forward svc/submilli 8128:8128` brings the
server to your loopback for the length of a command.

## Connect over HTTPS

A server with HTTPS turned on is named with `https://` and the host name
on its certificate:

```sh
submilli server status --server https://localhost:8128
```

A certificate from a public authority needs nothing more, and the
command prints the status as before.

### Trust a self-signed certificate

The CLI can't check a self-signed certificate, or one from your
organization's authority, against the authorities it knows. So the
first time, it shows the certificate's public-key fingerprint and asks
whether to trust it:

```text
Server: localhost:8128
Certificate names: localhost
Valid: Oct  3 17:03:03 2026 +00:00 to Oct  3 17:03:03 2027 +00:00
Public-key fingerprint: sha256:5ee140a80e2613385387daa41b22fb98dddf3a6ad328d734fc369d6212dcce29
Verify this fingerprint with the server operator before approving.
Trust this server public key? yes
status:          running
bind:            127.0.0.1:8128
pid:             48439
active sessions: 0
blueprints:      (none)
```

Answer yes only after checking the fingerprint with whoever runs the
server. Otherwise answer no, which is also the default. The operator
reads the fingerprint from the certificate file:

```sh
openssl x509 -in server.crt -pubkey -noout \
  | openssl pkey -pubin -outform DER \
  | openssl dgst -sha256 \
  | awk '{print "sha256:" $NF}'
```

```text
sha256:5ee140a80e2613385387daa41b22fb98dddf3a6ad328d734fc369d6212dcce29
```

A deploy job has no terminal to answer in, so there the command refuses
and names the next step:

```text
Error: server certificate is not approved; verify its public-key fingerprint independently, then run `submilli server trust add --server https://localhost:8128/ --fingerprint sha256:5ee140a80e2613385387daa41b22fb98dddf3a6ad328d734fc369d6212dcce29`
```

Register the fingerprint you checked before the job runs:

```sh
submilli server trust add --server https://localhost:8128 --fingerprint sha256:5ee140a80e2613385387daa41b22fb98dddf3a6ad328d734fc369d6212dcce29
```

```text
Trusted localhost:8128 sha256:5ee140a80e2613385387daa41b22fb98dddf3a6ad328d734fc369d6212dcce29
```

The command refuses a fingerprint that isn't the server's. A mistyped
one, or a server other than the one you checked, never gets trusted:

```text
Error: server fingerprint mismatch: expected sha256:0000000000000000000000000000000000000000000000000000000000000000, received sha256:5ee140a80e2613385387daa41b22fb98dddf3a6ad328d734fc369d6212dcce29
```

Trust is kept per host and port, in `~/.submilli/server-trust.json` (or
`$SUBMILLI_HOME/server-trust.json`):

```sh
submilli server trust list
```

```text
localhost:8128 sha256:5ee140a80e2613385387daa41b22fb98dddf3a6ad328d734fc369d6212dcce29
```

A certificate renewed with the same key stays trusted. When the key
changes, check the new fingerprint, remove the old entry with
`submilli server trust remove --server https://localhost:8128`, and
trust the new one. An expired certificate, or one whose names don't
include the host in `--server`, fails whatever you trusted.

## Keep them in your shell

To stop passing the two on each command, put them in your shell's
startup file (`~/.zshrc` or `~/.bashrc`), with the token in a file that
only you can read:

```sh title="~/.zshrc"
export SUBMILLI_SERVER_URL=http://10.0.12.7:8128
export SUBMILLI_SERVER_TOKEN_FILE=$HOME/.submilli/servers/prod/token
```

From then on each new shell is connected, and `submilli server status`
needs no flags.

## Switch between servers

With more than one server, say staging and production, keep each one's
address and token in a separate directory:

```text
~/.submilli/servers/staging/url      http://10.0.12.7:8128
~/.submilli/servers/staging/token
~/.submilli/servers/prod/url         http://10.0.20.4:8128
~/.submilli/servers/prod/token
```

Then a small function in `~/.zshrc` or `~/.bashrc` switches the two
variables by name, and refuses a name with no directory:

```sh title="~/.zshrc"
submilli-use() {
    local dir=~/.submilli/servers/$1
    [ -d "$dir" ] || { echo "no server named '$1' under ~/.submilli/servers" >&2; return 1; }
    export SUBMILLI_SERVER_URL=$(cat "$dir/url")
    export SUBMILLI_SERVER_TOKEN_FILE=$dir/token
}
```

```sh
submilli-use staging
submilli server status
```

```text
status:          running
bind:            0.0.0.0:8128
pid:             39225
active sessions: 0
blueprints:      support
```

The switch lasts for the shell it runs in. To start each shell on one
server, make `submilli-use staging` the last line of `~/.zshrc` or
`~/.bashrc`.

## Which commands need the admin token

A `user` token, the kind an application holds, runs programs and reads.
Anything that changes the server needs `admin`:

| Token | Commands |
| --- | --- |
| `user` or `admin` | `run-code`, `session open` and `close`, `docs` |
| `admin` only | `status`, `blueprint …`, `packages …`, `secret …`, `mcp …`, `stop` |

The server refuses a `user` token on an admin command because of its
role, not its value:

```text
error: this endpoint needs a token with the `admin` role; the token sent has the `user` role
```

[Run the server](/docs/server/run-the-server) shows how to declare a
`user` token. The [CLI reference](/docs/reference/cli) lists each
`submilli server` command and its options.
