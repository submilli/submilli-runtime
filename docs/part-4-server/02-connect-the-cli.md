---
title: "Connect the CLI"
description: "How to point the submilli server commands at a server: its token from a file, its address, the first check, keeping both in your shell, switching between servers by name, and which commands need the admin token."
slug: server/connect-the-cli
sidebar:
  order: 2
---

The `submilli server` commands talk to a running server over HTTP or HTTPS. Out
of the box they call `http://127.0.0.1:8128` and send the token from
`SUBMILLI_SERVER_TOKEN`, which is why they need no setup in the shell
that started the server. From another machine, or from a deploy job, the
address is different and the token shouldn't be typed.

This guide shows you how to point the `submilli server` commands at a
server: give them its token from a file, name its address, make the
first check, keep both in your shell, switch between servers by name,
and know which commands need the admin token.

## Give it the token

Without a token the server answers `401`, and the commands say so:

```text
error: the server did not accept this command's API token. Set `SUBMILLI_SERVER_TOKEN` to the token the server was started with, or `SUBMILLI_SERVER_TOKEN_FILE` to a file holding one
```

Set `SUBMILLI_SERVER_TOKEN` in the shell, or point `--token-file`, or
`SUBMILLI_SERVER_TOKEN_FILE`, at a file holding it. No flag takes the
token itself, so it never lands in the process list or in shell history.

On Kubernetes the chart keeps the tokens in a Secret; read the admin one
into a file first:

```sh
kubectl get secret submilli-auth -o jsonpath='{.data.admin-token}' | base64 -d > admin.token
```

## Name the address

`--server` names the server, on every `submilli server` command, and
`SUBMILLI_SERVER_URL` sets it for a whole shell. `status` is the first
check; it needs the admin token:

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

With HTTPS disabled, reach the server the way your application does,
over the private network it sits on, and not across the open internet.
Under Compose the port is published on the host's loopback, so the
commands work from the host without `--server`, and `docker compose exec
submilli submilli server status` works from inside the container. On
Kubernetes, `kubectl port-forward svc/submilli 8128:8128` brings the
server to your loopback for the length of a command.

## Connect over HTTPS

For a server with HTTPS enabled, use its certificate's hostname in `--server`
or `SUBMILLI_SERVER_URL`:

```sh
submilli server status --server https://runtime.example.com:8128 --token-file admin.token
```

Publicly trusted certificates need no extra setup.

For Kubernetes port forwarding, temporarily map the certificate's hostname to
`127.0.0.1` in `/etc/hosts` and keep that hostname in the HTTPS URL.

### Trust a self-signed server

For a self-signed certificate or an unknown issuer, the CLI shows the public-key
fingerprint and asks whether to trust it. Approval defaults to No. Verify the
fingerprint with the operator before accepting. On the server machine, obtain
it from the certificate file:

```sh
openssl x509 -in server.crt -pubkey -noout \
  | openssl pkey -pubin -outform DER \
  | openssl dgst -sha256 \
  | awk '{print "sha256:" $NF}'
```

Without an interactive terminal, register that verified fingerprint first.
Replace the placeholder with the command's `sha256:` value:

```sh
submilli server trust add --server https://runtime.example.com:8128 --fingerprint 'sha256:<64 hexadecimal digits>'
```

Trust is saved by hostname and port in `~/.submilli/server-trust.json`
(or `$SUBMILLI_HOME/server-trust.json`). Inspect or remove it with:

```sh
submilli server trust list
submilli server trust remove --server https://runtime.example.com:8128
```

Renewing with the same key keeps trust. For a changed key, verify the replacement,
remove the old entry, and approve it again. Expired certificates and hostname
mismatches still fail. Application and MCP clients configure trust separately.

## Keep them in your shell

To stop passing the two on every command, put them in your shell's
startup file, `~/.zshrc` or `~/.bashrc`, with the token in a file only
you can read:

```sh title="~/.zshrc"
export SUBMILLI_SERVER_URL=http://10.0.12.7:8128
export SUBMILLI_SERVER_TOKEN_FILE=$HOME/.submilli/servers/prod/token
```

From then on every new shell is connected, and `submilli server status`
needs no flags.

## Switch between servers

With more than one server, staging and production say, keep each one's
address and token in a directory of its own:

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

The switch lasts for the shell it runs in. To start every shell on one
server, call the function as the last line of `~/.zshrc` or
`~/.bashrc`: `submilli-use staging`.

## Which commands need the admin token

A `user` token, the kind an application holds, runs programs and reads;
everything that changes the server needs `admin`:

| Token | Commands |
| --- | --- |
| `user` or `admin` | `run-code`, `session open` and `close`, `docs` |
| `admin` only | `status`, `blueprint …`, `packages …`, `secret …`, `mcp …`, `stop` |

A `user` token on an admin command is refused by role, not by value:

```text
error: this endpoint needs a token with the `admin` role; the token sent has the `user` role
```

[Run the server](/docs/server/run-the-server) shows how to declare a
`user` token. Refer to the [CLI reference](/docs/reference/cli) for
every `submilli server` command and its options.
