---
title: "Connect the CLI"
description: "Set the server address and token, trust a self-signed certificate, and switch between servers."
slug: server/connect-the-cli
sidebar:
  order: 2
---

Use `submilli server` commands to manage a running server. The default address
is `http://127.0.0.1:8128`.

## Give it the token

Set `SUBMILLI_SERVER_TOKEN`, or supply a token file with `--token-file` or
`SUBMILLI_SERVER_TOKEN_FILE`. Tokens passed through a file stay out of shell
history and the process list.

For Kubernetes, read the chart's admin token into a file:

```sh
kubectl get secret submilli-auth -o jsonpath='{.data.admin-token}' | base64 -d > admin.token
```

## Name the address

Use `--server` for one command or `SUBMILLI_SERVER_URL` for the shell:

```sh
export SUBMILLI_SERVER_URL=http://10.0.12.7:8128
submilli server status --token-file admin.token
```

Use plain HTTP only on a private network. Compose publishes the port on the
host's loopback. To reach Kubernetes locally:

```sh
kubectl port-forward svc/submilli 8128:8128
```

## Trust a self-signed server

Use an HTTPS URL whose hostname or IP address is covered by the certificate:

```sh
submilli server status --server https://runtime.example.com:8128 --token-file admin.token
```

Publicly trusted certificates work automatically. For an unknown issuer, the
CLI asks you to approve the public-key fingerprint; approval defaults to No.
Verify it with the operator before accepting. On the server machine, obtain it
from the certificate file:

```sh
openssl x509 -in server.crt -pubkey -noout \
  | openssl pkey -pubin -outform DER \
  | openssl dgst -sha256 \
  | awk '{print "sha256:" $NF}'
```

For automation, register that independently verified fingerprint first:

```sh
submilli server trust add --server https://runtime.example.com:8128 --fingerprint 'sha256:<64 hexadecimal digits>'
submilli server status --server https://runtime.example.com:8128 --token-file admin.token
```

Inspect or remove saved trust:

```sh
submilli server trust list
submilli server trust remove --server https://runtime.example.com:8128
```

Trust is stored by hostname and port in `$SUBMILLI_HOME/server-trust.json`
(default `~/.submilli/server-trust.json`). Same-key renewal keeps working.
For a changed key, verify the replacement, remove the old entry, and approve it
again. Expiry and hostname checks still apply. Application and MCP clients
configure their own trust separately.

### Connect through a Kubernetes port forward

Keep the certificate's hostname in the URL. For a certificate covering
`submilli.default.svc`, add a temporary hosts entry and start the tunnel:

```sh
printf '127.0.0.1 submilli.default.svc\n' | sudo tee -a /etc/hosts
kubectl port-forward svc/submilli 8128:8128
```

In another shell:

```sh
submilli server status --server https://submilli.default.svc:8128 --token-file admin.token
```

Remove the temporary hosts entry when finished.

## Keep them in your shell

Put these in `~/.zshrc` or `~/.bashrc`, with the token file readable only by you:

```sh title="~/.zshrc"
export SUBMILLI_SERVER_URL=http://10.0.12.7:8128
export SUBMILLI_SERVER_TOKEN_FILE=$HOME/.submilli/servers/prod/token
```

## Switch between servers

Keep each server's address and token in its own directory:

```text
~/.submilli/servers/staging/url      http://10.0.12.7:8128
~/.submilli/servers/staging/token
~/.submilli/servers/prod/url         http://10.0.20.4:8128
~/.submilli/servers/prod/token
```

Add this function to your shell's startup file:

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

The selection applies to the current shell.

## Which commands need the admin token

| Token | Commands |
| --- | --- |
| `user` or `admin` | `run-code`, `session open` and `close`, `docs` |
| `admin` only | `status`, `blueprint …`, `packages …`, `secret …`, `mcp …`, `stop` |

[Run the server](/docs/server/run-the-server) covers token configuration.
See the [CLI reference](/docs/reference/cli) for all commands and options.
