---
title: "Deploy on Linux"
description: "Install and run the server under systemd, enable HTTPS, and maintain its state and secrets."
slug: server/deploy-on-linux
sidebar:
  order: 5
---

Run `submilli-server` as a systemd service with a dedicated user. This setup
keeps configuration in `/etc/submilli` and state in `/var/lib/submilli`.

## Install for the system

```sh
curl -fsSL https://submilli.ai/install.sh | sudo sh -s -- --install-dir /usr/local/bin
```

## Create the user and the directories

```sh
sudo useradd --system --home-dir /var/lib/submilli --create-home --shell /usr/sbin/nologin submilli
sudo install -d -o submilli -g submilli -m 0750 /etc/submilli
```

## Place the config and token files

Generate admin and application tokens and the secret store's key:

```sh
sudo sh -c 'openssl rand -hex 32 > /etc/submilli/admin.token'
sudo sh -c 'openssl rand -hex 32 > /etc/submilli/app.token'
sudo sh -c 'head -c 32 /dev/urandom | base64 > /etc/submilli/store.key'
sudo chown submilli:submilli /etc/submilli/admin.token /etc/submilli/app.token /etc/submilli/store.key
sudo chmod 0400 /etc/submilli/admin.token /etc/submilli/app.token /etc/submilli/store.key
```

Write the configuration. If clients use a DNS alias, add that name and port to
`mcp_allowed_hosts`:

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

`SUBMILLI_HOME` sets the state directory. `TimeoutStopSec=10` allows the
server's default five-second graceful shutdown.

```sh
sudo systemctl daemon-reload
sudo systemctl enable --now submilli
sudo journalctl -u submilli
sudo submilli server status --token-file /etc/submilli/admin.token
```

Applications use `http://<hostname>:8128` and the token in `app.token`.
See [Connect the CLI](/docs/server/connect-the-cli) for remote administration
and [Register a blueprint](/docs/server/register-a-blueprint) for deployment.

## Enable HTTPS

HTTPS is off by default. Supply a PEM certificate chain and matching private
key, or generate a self-signed certificate for the hostname clients use:

```sh
sudo openssl req -x509 -newkey rsa:2048 -nodes -days 365 \
  -keyout /etc/submilli/server.key -out /etc/submilli/server.crt \
  -subj "/CN=$(hostname -f)" \
  -addext "subjectAltName=DNS:$(hostname -f)" \
  -addext 'basicConstraints=critical,CA:FALSE'
sudo chown submilli:submilli /etc/submilli/server.crt /etc/submilli/server.key
sudo chmod 0400 /etc/submilli/server.crt /etc/submilli/server.key
```

Add to `/etc/submilli/server.yaml`:

```yaml title="/etc/submilli/server.yaml (fragment)"
tls:
  cert_file: /etc/submilli/server.crt
  key_file: /etc/submilli/server.key
```

```sh
sudo systemctl restart submilli
sudo submilli server status --server "https://$(hostname -f):8128" --token-file /etc/submilli/admin.token
```

Use `https://<hostname>:8128` for clients. Self-signed certificates need
[verified CLI trust](/docs/server/connect-the-cli#trust-a-self-signed-server);
other clients configure trust separately. Restart after replacing certificates.
Invalid or incomplete TLS configuration stops startup. See
[TLS settings](/docs/reference/server-settings#tls) for details.

## Reach an internal service

To allow programs to call a private service, add its address:

```yaml title="/etc/submilli/server.yaml (fragment)"
network:
  allow_ip:
    - 10.0.12.7
```

`allow_ip` permits that address; `allow_private` permits all private addresses.
See [server settings](/docs/reference/server-settings#network) for options.

## Upgrade and back up

```sh
curl -fsSL https://submilli.ai/install.sh | sudo sh -s -- --install-dir /usr/local/bin
sudo systemctl restart submilli
```

Back up `/var/lib/submilli`. Keep `/etc/submilli`'s tokens, private key, and
store key separately in your secrets manager.
