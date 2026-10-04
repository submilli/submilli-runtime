---
title: "Deploy with Compose"
description: "How to run the server as a container beside your application with the published compose file: a port only the host's loopback and your application's container can reach, state on a volume, the store key as a file, HTTPS, and upgrades by release."
slug: server/deploy-with-compose
# The Compose ps output is from the earlier documented run; its image pin
# was updated for 0.2.0. It could not be recaptured during release preparation
# because the local Docker engine did not respond.
# Turn on HTTPS was not run under Docker (no engine was available); the
# SUBMILLI_TLS_* variables and --health-check over HTTPS were run with the
# release binaries on main 51ce450b.
sidebar:
  order: 6
---

If your application runs in containers on one host, the server runs as
one more container. Docker changes two things about keeping it private:
a published port goes around the host's firewall, and other containers
reach a container over Docker's networks, not through the published
port. The published `compose.yaml` handles both, and keeps the state on a
volume so it survives a redeploy.

This guide shows you how to run the server with Docker Compose: start it
from the published file, put your application on its network, give it
the store key as a file, enable HTTPS, allow a service on your Docker
network, and upgrade and back it up.

## Start it

The published file is the stack's `compose.yaml`; your application joins
it as another service. The file and the image come from the same
release, so download the file at the release's tag and pin the image to
the same version in `.env`, beside the token. Into a new directory:

```sh
curl -fsSLO https://raw.githubusercontent.com/submilli/submilli-runtime/v0.2.0/compose.yaml
printf 'SUBMILLI_IMAGE=ghcr.io/submilli/submilli-runtime:0.2.0\nSUBMILLI_SERVER_TOKEN=%s\n' "$(openssl rand -hex 32)" > .env
docker compose up -d
docker compose ps
```

```text
NAME                   IMAGE                                     COMMAND                  SERVICE    CREATED          STATUS                    PORTS
myproject-submilli-1   ghcr.io/submilli/submilli-runtime:0.2.0   "/usr/local/bin/subm…"   submilli   12 seconds ago   Up 12 seconds (healthy)   127.0.0.1:8128->8128/tcp
```

Compose reads the token from `.env` and refuses to start without it. The
port is published on the host's loopback, so the `submilli server`
commands work from the host once the same token is in your shell:

```sh
set -a; . ./.env; set +a
submilli server status
```

The file also sets up:

- **State on a named volume.** `submilli-state` holds `$SUBMILLI_HOME`,
  so blueprints, sessions, packages, and secrets survive `docker compose
  down` and image upgrades. `docker compose down -v` deletes it.
- **Scratch space in memory.** Each run's scratch directory lives in a
  256 MB `tmpfs` at `/tmp`, so it never grows the container's disk.
- **A locked-down container.** The server runs as a non-root user on a
  read-only filesystem with every Linux capability dropped. The image has
  no shell. These are extra layers beneath the sandbox itself.
- **A health check** using `submilli-server --health-check`, which is why
  `docker compose ps` can say `healthy`.
- **Ten seconds to stop.** Docker waits that long before forcing the
  container to stop, which covers the server's own five-second drain. If
  you raise `--shutdown-grace`, raise `stop_grace_period` with it.

## Put your application on its network

The file publishes the server's port as `127.0.0.1:8128`, not `8128`.
That difference matters more with Docker than it looks: Docker routes
published ports around the host's firewall, so a plain `8128:8128` is
reachable from your whole network even on a host where `ufw` says the
port is closed.

Loopback publishing only covers the host, though. Other containers reach
the server over Docker's networks, not the published port. So the server
sits on its own network, `submilli-net`, and only containers you put on
that network can reach it. Add your application to it and call the
server by its service name:

```yaml title="compose.yaml (your service, added to the published file)"
services:
  app:
    image: your-application
    environment:
      SUBMILLI_URL: http://submilli:8128
      SUBMILLI_SERVER_TOKEN: ${SUBMILLI_SERVER_TOKEN}
    networks:
      - submilli-net
```

The two variables are your application's own; name them as it expects.
This hands it the admin token. To give it a `user` token instead,
declare one in a config file as [Run the
server](/docs/server/run-the-server) shows, and mount the file and
the token the way the store key is mounted below.

Any container on `submilli-net` can reach the API, so don't put anything
else there. A container on Docker's default network can't connect at
all; its requests time out.

An application that connects over MCP also needs the server to accept
the service name as a `Host` header, or its requests are refused with
`403 Forbidden: Host header is not allowed`. The shipped file takes care
of it: it sets `SUBMILLI_MCP_ALLOWED_HOSTS: submilli:8128` on the
service. Change that value if you rename the service or its port.

## Register blueprints and install packages

The port is published on the host, so blueprints, secrets, and packages
reach the server from the host with the same commands as anywhere,
through [Register a blueprint](/docs/server/register-a-blueprint);
a deploy job does the same with the token from `.env`. For a package in
a private repository the server needs a GitHub token of its own; refer
to [Install private packages](/docs/server/install-private-packages).

## Give it the store key as a file

The secret store stays off until the server has a key. Give it one as a
file, not an environment variable: environment variables show up in
`docker inspect` and `docker compose config`.

```sh
head -c 32 /dev/urandom | base64 > submilli-store-key
chmod 0444 submilli-store-key
```

Additions to the stack go in `compose.override.yaml`, which Compose
reads automatically, so the shipped file stays untouched:

```yaml title="compose.override.yaml"
services:
  submilli:
    environment:
      SUBMILLI_SECRET_STORE_KEY_FILE: /run/secrets/submilli-store-key
    secrets:
      - submilli-store-key

secrets:
  submilli-store-key:
    file: ./submilli-store-key
```

The `0444` matters on a Linux host. Outside Docker Swarm, Compose mounts
the file with its original owner and mode, and the server runs as user
65532, so a `0600` file owned by you is unreadable and the server refuses
to start:

```text
Error: opening secret store: secret store key: reading key file `/run/secrets/submilli-store-key`: Permission denied (os error 13)
```

Docker Desktop on macOS and Windows hides file ownership, so a `0600` key
works there and then fails on the Linux server you deploy to. Set `0444`
everywhere. Keep the key out of source control and out of your volume
backups.

## Enable HTTPS

In this setup the API token never leaves the host: the port is published
on the loopback, and your application reaches the server over a Docker
network on the same machine. Plain HTTP is enough there. If you publish
the port beyond the loopback, so that callers on other machines reach
it, turn HTTPS on first, or the token crosses the network readable.

Mount the certificate chain and its private key, in PEM, the way the
store key is mounted, and name them in the two variables. The
certificate must cover the names callers use: `submilli` for your
application's container, and the host's name for callers elsewhere.

```yaml title="compose.override.yaml (fragment)"
services:
  submilli:
    environment:
      SUBMILLI_TLS_CERT_FILE: /run/secrets/submilli-tls-cert
      SUBMILLI_TLS_KEY_FILE: /run/secrets/submilli-tls-key
    secrets:
      - submilli-tls-cert
      - submilli-tls-key

secrets:
  submilli-tls-cert:
    file: ./server.crt
  submilli-tls-key:
    file: ./server.key
```

The key needs the same `0444` as the store key, for the same reason. The
health check switches to HTTPS on its own.

Your application then calls `https://submilli:8128`. For a self-signed
certificate it must also be told to trust it, so give its container the
certificate:

```yaml title="compose.override.yaml (fragment)"
services:
  app:
    environment:
      SUBMILLI_URL: https://submilli:8128
      NODE_EXTRA_CA_CERTS: /run/secrets/submilli-tls-cert
    secrets:
      - submilli-tls-cert
```

`NODE_EXTRA_CA_CERTS` is for a Node application; it adds the file to the
authorities Node already trusts. Python's `SSL_CERT_FILE` replaces those
authorities instead, so a Python application that also calls its model
provider over HTTPS needs a bundle that holds both, built when the
container starts:

```sh
cat "$(python -m certifi)" /run/secrets/submilli-tls-cert > /tmp/ca-bundle.pem
export SSL_CERT_FILE=/tmp/ca-bundle.pem
```

## Allow a service on your Docker network

The server blocks programs from calling private addresses, which
includes other containers on your Docker networks. So a package that
calls one of your own services, say an inventory API in another
container, fails with a generic network error until you allow that
service's address:

```yaml title="compose.override.yaml (fragment)"
services:
  submilli:
    environment:
      SUBMILLI_ALLOW_IP: 172.21.0.3
```

`SUBMILLI_ALLOW_IP` takes an address or a range, comma-separated for more
than one. Allow the narrowest thing that works: a container's address can
change when it's recreated, so for a service that moves, give its network
a fixed subnet and allow that. Expect this line in the log once you do;
it's there so that a setting like this never goes unnoticed:

```text
ts=2026-10-03T17:04:41.940Z level=warn stream=log target=submilli_server msg="the outbound egress guard was widened by environment variables; the config file cannot revoke these" vars=SUBMILLI_ALLOW_IP
```

## Upgrade and back up

Both the file and the image are pinned to a release. Version 0.2.0 is the
first published release compatible with this guide's token authentication
and health check. An older installation needs its configuration and
blueprints migrated before starting the new server; read the
[0.2.0 release notes](https://github.com/submilli/submilli-runtime/releases/tag/v0.2.0)
and back up its state first.

For a later upgrade, download `compose.yaml` from that published release's
tag, set `SUBMILLI_IMAGE` in `.env` to the same version, and run
`docker compose up -d`. Keep the API token and the store key. The volume
carries the state across; check the release's migration instructions before
reusing it.

To back up, copy the volume while the server is stopped. Compose prefixes
the volume name with the project name, usually the directory name;
`docker volume ls` shows it:

```sh
docker compose stop submilli
docker run --rm -v myproject_submilli-state:/state:ro -v "$PWD":/backup busybox \
  tar czf /backup/submilli-state.tgz -C /state .
docker compose start submilli
```
