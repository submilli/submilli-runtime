---
title: "Deploy on Linux"
description: "How to run the server on a Linux machine of its own under systemd: the binaries for the system, a user and directories of its own, the config and token files under /etc/submilli, the unit, HTTPS, upgrades, and backups."
slug: server/deploy-on-linux
sidebar:
  order: 5
authorship:
  label: ai-assisted
  confirmed: true
  contentHash: "e53a07678fa78836f55e0fc7d3dbaede8829212460186b52af3af952e322d1e8"
  confirmedAt: "2026-10-05T16:46:31.966Z"
---

In production, `submilli-server` runs on a dedicated machine, and your
application calls it over the network. On Linux, run it like any other
service. It runs under systemd as a dedicated user, with the binaries in
`/usr/local/bin`, the configuration in `/etc/submilli`, and the state in
`/var/lib/submilli`.

This guide shows you how to set that up on Linux. The config file is the
one [Run the server](/docs/server/run-the-server) builds, and this page
decides where everything lives. If your application runs in containers,
use [Deploy with Compose](/docs/server/deploy-with-compose). On
Kubernetes, use [Deploy on Kubernetes](/docs/server/deploy-on-kubernetes).

## Install for the system

The install script puts the binaries under your home directory unless
told otherwise. The published Linux binaries require x86-64. For a
service, put them where all users find them:

```sh
curl -fsSL https://submilli.ai/install.sh | sudo sh -s -- --install-dir /usr/local/bin
```

## Create the user and the directories

The server needs no privileges, so give it a system user that can't log
in, with the state directory as its home, and a configuration directory
it can read:

```sh
sudo useradd --system --home-dir /var/lib/submilli --create-home --shell /usr/sbin/nologin submilli
sudo install -d -o submilli -g submilli -m 0750 /etc/submilli
```

## Place the config and token files

Generate the admin token, the application's token, and the secret
store's key into `/etc/submilli`, readable only by the server's user:

```sh
sudo sh -c 'openssl rand -hex 32 > /etc/submilli/admin.token'
sudo sh -c 'openssl rand -hex 32 > /etc/submilli/app.token'
sudo sh -c 'head -c 32 /dev/urandom | base64 > /etc/submilli/store.key'
sudo chown submilli:submilli /etc/submilli/admin.token /etc/submilli/app.token /etc/submilli/store.key
sudo chmod 0400 /etc/submilli/admin.token /etc/submilli/app.token /etc/submilli/store.key
```

Then write the config file, with your editor or with `tee` as below,
with the paths of those three files in it. Your application is on
another machine, so the server listens on all interfaces. An application
that connects over MCP needs the name it will use for this machine under
`mcp_allowed_hosts`. `hostname -f` gives the machine's name, which is
that name unless you point a DNS alias at it. The state directories
aren't in the file. They keep their defaults under the state directory,
which the server takes from the `SUBMILLI_HOME` environment variable
(`~/.submilli` when it is unset). The unit below sets it to
`/var/lib/submilli`:

```sh
sudo tee /etc/submilli/server.yaml >/dev/null <<EOF
bind: 0.0.0.0
api_tokens:
  - name: admin
    role: admin
    token_file: /etc/submilli/admin.token
  - name: app
    role: user
    token_file: /etc/submilli/app.token
secret_store:
  key_file: /etc/submilli/store.key
mcp_allowed_hosts:
  - $(hostname -f):8128
EOF
```

## Run it under systemd

Write the unit the same way:

```sh
sudo tee /etc/systemd/system/submilli.service >/dev/null <<'EOF'
[Unit]
Description=Submilli server
After=network-online.target
Wants=network-online.target

[Service]
User=submilli
Group=submilli
Environment=SUBMILLI_HOME=/var/lib/submilli
ExecStart=/usr/local/bin/submilli-server --config /etc/submilli/server.yaml
Restart=on-failure
TimeoutStopSec=10

[Install]
WantedBy=multi-user.target
EOF
```

On stop, systemd sends SIGTERM and the server waits up to five seconds
for running requests to finish. Requests still running after that grace
period are cancelled and their connections dropped. `TimeoutStopSec`
gives the server that grace period and a margin to exit before systemd
forces it to stop.

```sh
sudo systemctl daemon-reload
sudo systemctl enable --now submilli
sudo journalctl -u submilli
```

The journal holds the server's log, starting with the two lines [Run
the server](/docs/server/run-the-server) shows at start-up. Check
it from your shell with the admin token:

```sh
sudo submilli server status --token-file /etc/submilli/admin.token
```

Your application connects to `http://<the machine's name>:8128` with
the value in `/etc/submilli/app.token`, given to it through whatever keeps
its other secrets. From your machine, [Connect the
CLI](/docs/server/connect-the-cli) reaches the server with the
admin token. Register Blueprints, store their secrets, and install their
Packages as [Register a Blueprint](/docs/server/register-a-blueprint)
shows. A deploy job does the same with the admin token, and [Manage
Blueprints in Git](/docs/tutorials/manage-blueprints-in-git) builds one.

## Enable HTTPS

Your application reaches this machine over the network, so its API token
crosses that network with every request. Over plain HTTP anyone on the
way can read it. Turn HTTPS on unless you control the network between
the two end to end.

Put the certificate chain and its private key, in PEM, at
`/etc/submilli/server.crt` and `/etc/submilli/server.key`. Use a
certificate from your certificate authority, issued for the host name
your application connects to. To try it first, generate a self-signed
one for this machine's name:

```sh
sudo openssl req -x509 -newkey rsa:2048 -nodes -days 365 \
  -keyout /etc/submilli/server.key -out /etc/submilli/server.crt \
  -subj "/CN=$(hostname -f)" \
  -addext "subjectAltName=DNS:$(hostname -f)" \
  -addext 'basicConstraints=critical,CA:FALSE'
```

Give the server read access:

```sh
sudo chown submilli:submilli /etc/submilli/server.crt /etc/submilli/server.key
sudo chmod 0400 /etc/submilli/server.crt /etc/submilli/server.key
```

Add the TLS block to `/etc/submilli/server.yaml`:

```yaml title="/etc/submilli/server.yaml (fragment)"
tls:
  cert_file: /etc/submilli/server.crt
  key_file: /etc/submilli/server.key
```

Restart and watch the journal:

```sh
sudo systemctl restart submilli
sudo journalctl -u submilli -f
```

Wait for a new `submilli-server listening` line with `protocol=https`,
then press Ctrl+C to stop following the journal. `systemctl restart`
can return before the server is ready to accept connections. Connect
using HTTPS on the same port:

```sh
sudo submilli server status --server "https://$(hostname -f):8128" --token-file /etc/submilli/admin.token
```

Update your application's URL to `https://<hostname>:8128`, and restart
the service after you replace the certificate. For a self-signed
certificate, the CLI asks you to check and trust its fingerprint, as
[Connect the
CLI](/docs/server/connect-the-cli#trust-a-self-signed-certificate)
shows. Your application trusts a self-signed certificate the way its language
does. Node adds the file to the authorities it already trusts with
`NODE_EXTRA_CA_CERTS`:

```sh
export NODE_EXTRA_CA_CERTS=$PWD/server.crt
```

Python's `SSL_CERT_FILE` replaces those authorities, so a
Python application that also calls its model provider over HTTPS needs a
bundle that holds both:

```sh
cat "$(python -m certifi)" server.crt > ca-bundle.pem
export SSL_CERT_FILE=$PWD/ca-bundle.pem
```

## Reach an internal service

The server blocks programs from calling private addresses, whatever a
Blueprint allows, so a Package that calls a service on your private
network fails until you allow that address in the config file:

```yaml title="/etc/submilli/server.yaml (fragment)"
network:
  allow_ip:
    - 10.0.12.7
```

Allow the single address. `allow_private` opens your entire internal
network. The [server settings](/docs/reference/server-settings)
reference covers the block and its settings.

## Upgrade and back up

Before upgrading from 0.2.0 to 0.3.0, stop the service and back up
`/var/lib/submilli`. The new server imports Blueprint revisions into
`server/db/submilli.db` and archives the old files. Later revisions live
only in SQLite. A rollback to 0.2.0 requires restoring the pre-upgrade
backup. Persist the whole database directory on local or block-backed
storage.

After the new release is published, run the installer and start the service:

```sh
sudo systemctl stop submilli
# Back up /var/lib/submilli before continuing.
curl -fsSL https://submilli.ai/install.sh | sudo sh -s -- --version v0.3.0 --install-dir /usr/local/bin
sudo systemctl start submilli
```

Back up `/var/lib/submilli` with the rest of the machine. `/etc/submilli`
holds the store's key and the tokens. Leave it out of that backup and
keep its contents in your secrets manager. The encryption is only worth
having while the key stays apart from the store.
