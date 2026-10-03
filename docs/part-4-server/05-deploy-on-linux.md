---
title: "Deploy on Linux"
description: "How to run the server on a Linux machine of its own under systemd: the binaries for the system, a user and directories of its own, the config and token files under /etc/submilli, the unit, upgrades, and backups."
slug: server/deploy-on-linux
sidebar:
  order: 5
---

In production, `submilli-server` runs on a machine of its own, and your
application calls it over the network. On a Linux machine that means
running it like any other service: under systemd, as a dedicated user,
with the binaries in `/usr/local/bin`, the configuration in
`/etc/submilli`, and the state in `/var/lib/submilli`.

This guide shows you how to set that up: install the binaries for the
system, create the user and the directories, place the config file and
the token files, write the unit and start it, and upgrade and back it
up. The config file is the
one [Run the server](/docs/server/run-the-server) builds; this page
only decides where everything lives. If your application runs in
containers, refer to [Deploy with
Compose](/docs/server/deploy-with-compose); on Kubernetes, to
[Deploy on Kubernetes](/docs/server/deploy-on-kubernetes).

## Install for the system

The install script puts the binaries under your home directory unless
told otherwise. For a service, put them where every user finds them:

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
store's key into `/etc/submilli`, readable by the server's user alone:

```sh
sudo sh -c 'openssl rand -hex 32 > /etc/submilli/admin.token'
sudo sh -c 'openssl rand -hex 32 > /etc/submilli/app.token'
sudo sh -c 'head -c 32 /dev/urandom | base64 > /etc/submilli/store.key'
sudo chown submilli:submilli /etc/submilli/admin.token /etc/submilli/app.token /etc/submilli/store.key
sudo chmod 0400 /etc/submilli/admin.token /etc/submilli/app.token /etc/submilli/store.key
```

Then write the config file, with your editor or with `tee` as below,
with the paths of those three files in it. Your application is on
another machine, so the server listens on every interface, and an
application that connects over MCP needs the name it will use for this
machine under `mcp_allowed_hosts`; `hostname -f` gives the machine's
own, which is that name unless you point a DNS alias at it. The state
directories aren't in the file: they
keep their defaults under the state directory, which the server takes
from the `SUBMILLI_HOME` environment variable (`~/.submilli` when it is
unset), and the unit below sets it to `/var/lib/submilli`:

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

On stop, systemd sends SIGTERM and the server lets running requests
finish for five seconds before it exits; `TimeoutStopSec` gives it that
and a margin, so a program still running isn't killed mid-request.

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
its other secrets. From your own machine, [Connect the
CLI](/docs/server/connect-the-cli) reaches the server with the
admin token. Register blueprints, store their secrets, and install their
packages as [Register a blueprint](/docs/server/register-a-blueprint)
shows; a deploy job does the same with the admin token, which [Manage
blueprints in Git](/docs/tutorials/manage-blueprints-in-git) builds.

## Reach an internal service

The server blocks programs from calling private addresses, whatever a
blueprint allows, so a package that calls a service on your private
network fails until you allow that address in the config file:

```yaml title="/etc/submilli/server.yaml (fragment)"
network:
  allow_ip:
    - 10.0.12.7
```

Allow the one address rather than `allow_private`, which opens your whole
internal network. Refer to the [server
settings](/docs/reference/server-settings) reference for the block
and its settings.

## Upgrade and back up

To upgrade, run the installer again and restart the service:

```sh
curl -fsSL https://submilli.ai/install.sh | sudo sh -s -- --install-dir /usr/local/bin
sudo systemctl restart submilli
```

Back up `/var/lib/submilli` with the rest of the machine. `/etc/submilli`
holds the store's key and the tokens: leave it out of that backup, and
keep its contents in your secrets manager instead, since keeping the key
apart from the store is what makes the encryption worth having.
