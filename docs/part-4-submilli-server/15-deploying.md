---
title: "Deploying"
description: "Running submilli-server next to your application in production: on one machine, with Docker Compose, or on Kubernetes with the Helm chart."
slug: deploying
sidebar:
  order: 15
---

On your laptop, `submilli-server` runs in a terminal and keeps its state in
`~/.submilli`. In production it has to live somewhere your application can
call it, keep its state through restarts and redeploys, and get its
blueprints and secrets without someone typing commands after every release.
This chapter shows how to do that in the three places applications usually
run:

| Your application runs… | Run the server… |
| --- | --- |
| Directly on a machine, as a process | As a second process on the same machine |
| In containers on one host | As a container beside it, with Docker Compose |
| On Kubernetes | As a pod in the same cluster, with the Helm chart |

Pick the section that matches. The server itself is the same program in all
three, configured the way [Submilli server](/docs/server) describes; what
changes is how each setup handles the same four jobs.

## What every deployment has to get right

**Every caller holds the right token.** The server checks a bearer token on
every request and won't start without one. The token it is started with, in
`SUBMILLI_SERVER_TOKEN`, is an `admin` token: it runs programs and also
manages blueprints, secrets, and packages. Handing that same token to your
application is the quickest way to get going, and fine while the application
and the server are both yours on one machine. Before an agent runs anywhere
you don't fully trust, add a `user` token and give the application that
instead: it runs programs against registered blueprints and can't change
them. [Who can reach it](/docs/server#who-can-reach-it) has the full split.
Each setup below shows both steps.

**Only your application can reach the server.** The token is one layer; where
the server sits on the network is the other. The server speaks plain HTTP, so
a token that crosses a network unencrypted can be read on the way, and a
leaked token is only useful to someone who can reach the port. Each setup
below places the server where your application can reach it and nothing else
can: loopback on a single machine, a private network in Compose, a network
policy in Kubernetes. Each section shows how, and how to confirm it's working.

**State lives on storage that outlasts the process.** Registered blueprints,
open sessions, installed packages, and secrets all live under
`$SUBMILLI_HOME`. [Where it keeps state](/docs/server#where-it-keeps-state)
explains what losing each one costs. On a machine that's a directory; in a
container it has to be a volume, or it's gone at the next deploy.

**Blueprints and secrets arrive without manual steps.** Keep blueprints in
source control and let the server load them at startup from a [seed
directory](/docs/server#register-blueprints). Keep secret values out
of source control and out of anywhere they'd be printed, such as command
lines and environment dumps.

## On one machine

If your application runs as a process on a server you manage, run
`submilli-server` beside it. Install it the same way as on your laptop:

```sh
curl -fsSL https://submilli.ai/install.sh | sh
```

Then start it under whatever already keeps your application running
(systemd, a process manager, a supervisor), with a config file that puts its
state somewhere durable:

```yaml title="/etc/submilli/server.yaml"
bind: 127.0.0.1
blueprint_seed_dir: /etc/submilli/blueprints
secret_store:
  key_file: /etc/submilli/store.key
api_tokens:
  - name: app
    role: user
    token_file: /etc/submilli/app.token
```

The server takes its own token, the admin one, from `SUBMILLI_SERVER_TOKEN`.
Keep it in an environment file your process manager loads (systemd's
`EnvironmentFile=`, for instance), readable only by you and the server's
user. The `api_tokens` entry adds a `user` token for your application, read
from a file the application's user can read as well:

```sh
printf 'SUBMILLI_SERVER_TOKEN=%s\n' "$(openssl rand -hex 32)" > /etc/submilli/server.env
openssl rand -hex 32 > /etc/submilli/app.token
chmod 600 /etc/submilli/server.env /etc/submilli/app.token
```

```sh
set -a; . /etc/submilli/server.env; set +a
SUBMILLI_HOME=/var/lib/submilli submilli-server --config /etc/submilli/server.yaml
```

```text
INFO submilli_server::auth: inbound authentication enabled tokens="SUBMILLI_SERVER_TOKEN (admin), app (user)"
INFO submilli_server::serve: submilli-server listening addr=127.0.0.1:8128
```

The first line names the tokens the server loaded and their roles, never the
tokens themselves. If it is missing and the server exited instead, the error
names the token it couldn't load. Your application reads
`/etc/submilli/app.token` and sends it as `Authorization: Bearer <token>`.
Your own commands read the server token from the same environment file:

```sh
set -a; . /etc/submilli/server.env; set +a
submilli server status
```

Leave `bind` at `127.0.0.1`. Your application connects to
`http://127.0.0.1:8128`, and nothing on the network can. The token is what
separates your application from every other process on the machine; the
loopback bind is what keeps the token off the network. If other machines have
to reach the server, put a reverse proxy that terminates TLS in front of it
rather than changing `bind`.

If a package needs to call a service on your private network, allow just
that address in the same file (`network: { allow_ip: ["10.0.12.7"] }`);
[Outbound network](/docs/server#outbound-network) explains why the server
blocks private addresses until you do.

Back up `/var/lib/submilli` with the rest of the machine. Keep the store key
out of that backup, or store it separately: keeping the two apart is what
makes the encryption worth having. `submilli upgrade` replaces
both binaries with the latest release; restart the server afterwards.

## With Docker Compose

If your application runs in containers on one host, run the server as one
more container. Submilli publishes a `compose.yaml` that does the hard parts.
Download it next to your own, generate the token it requires into `.env`,
and start it:

```sh
curl -fsSLO https://raw.githubusercontent.com/submilli/submilli-runtime/main/compose.yaml
printf 'SUBMILLI_SERVER_TOKEN=%s\n' "$(openssl rand -hex 32)" > .env
chmod 600 .env
docker compose up -d
docker compose ps
```

```text
NAME                   IMAGE                                      COMMAND                  SERVICE    CREATED          STATUS                    PORTS
myproject-submilli-1   ghcr.io/submilli/submilli-runtime:latest   "/usr/local/bin/subm…"   submilli   12 seconds ago   Up 12 seconds (healthy)   127.0.0.1:8128->8128/tcp
```

Compose reads `.env` from the directory it runs in. Keep that file out of
source control. Without the variable Compose stops before starting anything:

```text
error while interpolating services.submilli.environment.SUBMILLI_SERVER_TOKEN: required variable SUBMILLI_SERVER_TOKEN is missing a value: set SUBMILLI_SERVER_TOKEN, for example in .env - generate one with `openssl rand -hex 32`
```

The server's log opens by naming the token it loaded:

```text
submilli-1  | … INFO submilli_server::auth: inbound authentication enabled tokens="SUBMILLI_SERVER_TOKEN (admin)"
```

### How it keeps other callers out

Every request needs the token. The server container gets
`SUBMILLI_SERVER_TOKEN` from `.env` and needs no config file for it.

On top of that, the server is kept off the network, because it speaks plain
HTTP and a token sent in the clear is only as private as the network it
crosses. The server listens on every address inside its container, as it has
to, or nothing outside the container could reach it. Who can actually reach
it is decided by how the port is published and which network it's on.

The file publishes the server's port as `127.0.0.1:8128`, not `8128`. That
difference matters more with Docker than it looks: Docker routes published
ports around the host's firewall, so a plain `8128:8128` is reachable from
your whole network even on a host where `ufw` says the port is closed.

Loopback publishing only covers the host, though. Other containers reach the
server over Docker's networks, not the published port. So the server sits on
its own network, `submilli-net`, and only containers you put on that network
can reach it. Add your application to it, give it the token, and call the
server by its service name:

```yaml title="compose.yaml (your service)"
services:
  app:
    image: your-application
    environment:
      SUBMILLI_URL: http://submilli:8128
      SUBMILLI_SERVER_TOKEN: ${SUBMILLI_SERVER_TOKEN:?}
    networks:
      - submilli-net
```

Your application sends that value as `Authorization: Bearer <token>`. Don't
put anything else on `submilli-net`. A container on Docker's default network
can't connect at all; its requests time out.

The `submilli server` commands read the same variable from your shell, over
the loopback port:

```sh
set -a; . ./.env; set +a
submilli server status
```

### Giving the application a user token

The token in `.env` is an admin token, and the fragment above hands it to
your application. That is fine while every container on the host is yours.
Before the agent's programs or its prompts come from anywhere you don't fully
trust, give the application a `user` token instead, so that nothing on the
application's side can rewrite the blueprint that constrains the agent.

Extra tokens come from a config file, each read from a file of its own.
Create both in a directory next to `compose.yaml`:

```sh
mkdir -p server-config
openssl rand -hex 32 > server-config/app-token
printf 'api_tokens:\n  - name: app\n    role: user\n    token_file: /etc/submilli/config/app-token\n' \
  > server-config/server.yaml
chmod 0755 server-config
chmod 0444 server-config/server.yaml server-config/app-token
printf 'SUBMILLI_APP_TOKEN=%s\n' "$(cat server-config/app-token)" >> .env
```

```yaml title="compose.override.yaml"
services:
  submilli:
    volumes:
      - ./server-config:/etc/submilli/config:ro
    environment:
      SUBMILLI_CONFIG: /etc/submilli/config/server.yaml
  app:
    environment:
      SUBMILLI_SERVER_TOKEN: ${SUBMILLI_APP_TOKEN:?}
```

```sh
docker compose up -d
docker compose logs submilli | grep 'inbound authentication'
```

```text
submilli-1  | … INFO submilli_server::auth: inbound authentication enabled tokens="SUBMILLI_SERVER_TOKEN (admin), app (user)"
```

The application still reads `SUBMILLI_SERVER_TOKEN`; the value it finds
there is now the `user` token, and the admin token stays on the host. The
`0444` is there because Docker doesn't change the owner of a bind-mounted
file and the server runs as user 65532: a `0600` file owned by you is
unreadable in the container, and the server refuses to start. Keep the
directory out of source control.

### What else the file sets up

- **State on a named volume.** `submilli-state` holds `$SUBMILLI_HOME`, so
  blueprints, sessions, packages, and secrets survive `docker compose down`
  and image upgrades. `docker compose down -v` deletes it.
- **Scratch space in memory.** Each run's scratch directory lives in a 256 MB
  `tmpfs` at `/tmp`, so it never grows the container's disk.
- **A locked-down container.** The server runs as a non-root user on a
  read-only filesystem with every Linux capability dropped. The image has no
  shell. These are extra layers beneath the sandbox itself.
- **A health check** using `submilli-server --health-check`, which is why
  `docker compose ps` can say `healthy`. It calls `/healthz`, which needs no
  token.
- **Ten seconds to stop.** Docker waits that long before forcing the
  container to stop, which covers the server's own five-second drain. If you raise
  `--shutdown-grace`, raise `stop_grace_period` with it.

### Blueprints and packages

Keep blueprints in a directory next to `compose.yaml` and mount it as the
seed directory, so every start loads them:

```yaml title="compose.override.yaml"
services:
  submilli:
    volumes:
      - ./blueprints:/etc/submilli/blueprints:ro
    environment:
      SUBMILLI_BLUEPRINT_SEED_DIR: /etc/submilli/blueprints
```

```sh
docker compose up -d
docker compose logs submilli | grep 'seed reconcile'
```

```text
submilli-1  | … INFO submilli_server::blueprint_seed: blueprint seed reconcile complete dir=/etc/submilli/blueprints seeded=1 skipped=0 failed=0 unresolved_secrets=0
```

Compose reads `compose.override.yaml` automatically, so the shipped file
stays untouched.

Packages install over the published port, from the host, with the same
command as on a laptop and the admin token in your shell. The server fetches
and builds them itself:

```sh
submilli server packages install acme/billing-package
```

### The secret store's key

The secret store stays off until the server has a key. Give it one as a file,
not an environment variable: environment variables show up in
`docker inspect` and `docker compose config`. The server token above is an
environment variable all the same, because a token is easy to replace; the
store key decrypts everything already in the store, so it gets the extra
care.

```sh
head -c 32 /dev/urandom | base64 > submilli-store-key
chmod 0444 submilli-store-key
```

```yaml title="compose.override.yaml (added to the same file)"
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

The `0444` matters on a Linux host. Outside Docker Swarm, Compose mounts the
file with its original owner and mode, and the server runs as user 65532, so
a `0600` file owned by you is unreadable and the server refuses to start:

```text
Error: opening secret store: secret store key: reading key file `/run/secrets/submilli-store-key`: Permission denied (os error 13)
```

Docker Desktop on macOS and Windows hides file ownership, so a `0600` key
works there and then fails on the Linux server you deploy to. Set `0444`
everywhere. Keep the key out of source control and out of your volume
backups.

### Calling your internal services

The server blocks programs from calling private addresses, which includes
other containers on your Docker networks. So a package that calls one of your
own services, say an inventory API in another container, fails with a
generic network error until you allow that service's address:

```yaml title="compose.override.yaml"
services:
  submilli:
    environment:
      SUBMILLI_ALLOW_IP: 172.21.0.3
```

`SUBMILLI_ALLOW_IP` takes an address or a range, comma-separated for more
than one. Allow the narrowest thing that works: a container's address can
change when it's recreated, so for a service that moves, give its network a
fixed subnet and allow that. Expect this line in the log once you do; it's
there so that a setting like this never goes unnoticed:

```text
WARN submilli_server: the outbound egress guard was widened by environment variables; the config file cannot revoke these vars="SUBMILLI_ALLOW_IP"
```

### Upgrading and backing up

To upgrade, point the image at a new tag and recreate the container:

```sh
SUBMILLI_IMAGE=ghcr.io/submilli/submilli-runtime:<version> docker compose up -d
```

Pin a version tag rather than following `latest`, so an upgrade happens
when you choose it. The `compose.yaml` you downloaded relies on the server
reading `SUBMILLI_SERVER_TOKEN`, so it needs a server release that does. The
volume carries the state across.

To back up, copy the volume while the server is stopped. Compose prefixes
the volume name with the project name, usually the directory name;
`docker volume ls` shows it:

```sh
docker compose stop submilli
docker run --rm -v myproject_submilli-state:/state:ro -v "$PWD":/backup busybox \
  tar czf /backup/submilli-state.tgz -C /state .
docker compose start submilli
```

## On Kubernetes

If your application runs on Kubernetes, install the server with the Submilli
Helm chart:

```sh
helm install submilli oci://ghcr.io/submilli/charts/submilli -f values.yaml
```

`values.yaml` holds your settings; the sections below build it up, and an
empty file is a valid start. Add `--version` to pin a chart version, so
upgrades happen when you choose them.

This gives you one server pod, a Service called `submilli`, a persistent
volume for its state, a Secret holding two generated API tokens, and a
network policy that lets nothing reach it yet.
The chart's [README](https://github.com/submilli/submilli-runtime/tree/main/charts/submilli)
is the reference for every value; this section covers the decisions you'll
actually make.

### Tokens

The chart generates an admin token and a user token into a Secret named
`submilli-auth` (the release name, then `-auth`), mounts them into the server
pod as files, and points the server's config at them with `token_file`. Read
one out when you need it:

```sh
kubectl get secret submilli-auth -o jsonpath='{.data.admin-token}' | base64 -d
```

The chart doesn't use `SUBMILLI_SERVER_TOKEN` on the server: both tokens are
`api_tokens` entries, so your application starts out with a `user` token.
It gets that token from the same Secret, as an environment variable:

```yaml title="your application's Deployment (fragment)"
env:
  - name: SUBMILLI_URL
    value: http://submilli.submilli.svc:8128
  - name: SUBMILLI_SERVER_TOKEN
    valueFrom:
      secretKeyRef:
        name: submilli-auth
        key: user-token
```

A pod can only reference a Secret in its own namespace. If your application
runs in a different namespace from the server, create the tokens yourself,
put the user token in a Secret in the application's namespace, and give the
server both through `auth.existingSecret`:

```sh
kubectl -n submilli create secret generic submilli-tokens \
  --from-literal=admin-token="$(openssl rand -hex 32)" \
  --from-literal=user-token="$(openssl rand -hex 32)"
```

```yaml title="values.yaml"
auth:
  existingSecret: submilli-tokens
```

Use `auth.existingSecret` with Argo CD, too, and with any pipeline that runs
`helm template` and applies the output. The chart keeps its generated tokens
stable across upgrades by reading the live Secret back; rendered without
access to the cluster, it has nothing to read and would produce new tokens on
every sync, locking out every caller.

The server reads tokens when it starts. To rotate one, change the Secret,
then restart the pod and the application that uses it:

```sh
kubectl rollout restart statefulset/submilli
```

### Letting your application in

A token says what a caller may do; it doesn't limit who can try. In a
cluster, any pod in any namespace can reach any Service unless something says
otherwise, so a user token that leaks into a log or another team's manifest
would work from anywhere. So the chart also installs a default-deny
NetworkPolicy, and you list the pods that may call the server:

```yaml title="values.yaml"
networkPolicy:
  allowFrom:
    - namespaceSelector:
        matchLabels:
          kubernetes.io/metadata.name: my-app
      podSelector:
        matchLabels:
          app.kubernetes.io/name: my-app
```

Both selectors in one list item means "pods with this label, in this
namespace". Written as two separate items, it would mean "any pod in this
namespace, or any pod with this label anywhere", which is much wider.
Your application then calls `http://submilli.<namespace>.svc:8128`.

The policy has three limits worth knowing:

- **Your cluster has to enforce it.** A NetworkPolicy is only a request; the
  cluster's network plugin enforces it, and some don't. Where it isn't
  enforced, nothing warns you. `helm test submilli` checks this directly: it
  starts a pod that shouldn't get through and fails if it does.
- **It admits pods, not requests.** What an admitted pod may call is decided
  by the token it holds, which is why your application gets the user token
  and not the admin one.
- **`kubectl port-forward` is a separate way in.** It goes around the policy,
  and it is how you run `submilli server` commands against the cluster: forward
  the port, put the admin token in `SUBMILLI_SERVER_TOKEN`, and the commands
  work as they do locally. Anyone who can read the Secret can do the same, so
  it's worth deciding who on your team has that permission.

### Blueprints and secrets

Blueprints go in your values file. The chart puts them in a ConfigMap,
mounts it as the seed directory, and the server loads them on every pod
start:

```yaml title="values.yaml"
blueprints:
  billing: |
    name: billing
    default: deny
    secrets:
      STRIPE_KEY: { file: /etc/submilli/secrets/stripe/api-key }
    permissions:
      main:
        - capability: http.post
          action: allow
          filter: host == "api.stripe.com"

secrets:
  stripe:
    secretName: stripe-credentials
    key: api-key
```

Secret values stay in ordinary Kubernetes Secrets. The `secrets:` map mounts
each one at `/etc/submilli/secrets/<name>/<key>`, and the blueprint reads it
from that path with a `file:` source. Only blueprints from the values file
may do that; a blueprint registered over the API can't read files. Never put
secret values in `values.yaml` itself: Helm stores them in plain text and
prints them in `helm get values`.

The two maps have to agree on the path, and the server won't stop you if
they don't. It registers the blueprint anyway, logs a warning, and counts it
in `unresolved_secrets=1` on the reconcile line; the first program that needs
the secret fails. `helm test` catches it before that:

```text
FAIL: a blueprint declares a secret at /etc/submilli/secrets/stripe/apikey, but the chart does not mount anything there. Paths the chart mounts: /etc/submilli/secrets/stripe/api-key
```

The chart writes the server's config file for you, from its own values. For
a server setting the chart has no value for, add it under `config:` with the
same key the [config file](/docs/server#configure-it) uses:

```yaml title="values.yaml"
config:
  max_execution_time: 30
```

The chart refuses keys it sets itself, such as `bind` and `port`, and names
the value to use instead. Environment variables passed through `extraEnv`
still override the file.

Run `helm test submilli` after every install and upgrade. It registers a
small blueprint of its own, `submilli-helmtest-exec`, to run a program
through the server, so expect to see it in `submilli server blueprint list`.

The server's own encrypted secret store is off in the chart. Its encryption
protects the volume only if the key lives somewhere the volume doesn't, and
a Kubernetes Secret in the same namespace usually doesn't qualify. Turn it on
(`secretStore.enabled`) when the key comes from a KMS or a CSI secrets
driver.

### Calling your internal services

Pod and Service addresses in a cluster are private addresses, so the server
blocks programs from calling them until you allow them. For a package that
calls one of your own services, find the Service's cluster IP and pass it
through the chart's `extraEnv`:

```sh
kubectl -n internal get svc inventory -o jsonpath='{.spec.clusterIP}'
```

```yaml title="values.yaml"
extraEnv:
  - name: SUBMILLI_ALLOW_IP
    value: 10.96.45.205
```

Programs keep calling the service by name (`http://inventory.internal.svc/`);
the server checks the address the name resolves to. A Service keeps its
cluster IP until it's deleted, so allow that one address rather than the
cluster's whole Service range. As with Compose, the server logs a warning
naming `SUBMILLI_ALLOW_IP` at startup, to keep the setting visible.

The chart's NetworkPolicy only controls who can call the server, not what
the server calls. The address check above is what limits outbound calls.

### Memory

One value, `execution.maxMemoryMB` (50 by default), sets both the server's
per-execution memory limit and the pod's memory, so the two can't drift
apart:

```text
pod memory = 128 Mi + 16 × maxMemoryMB    (928 Mi at the defaults)
```

The multiplier is large because the limit counts memory a program holds,
not memory the process uses on the way. One execution near its limit has
been measured at more than 12 times that in process memory, all released
when it finishes. The formula covers one such execution at a time; several
at once can use more, so if your programs handle large data, measure your own
peak.

### Storage, replicas, and upgrades

State lives on a PersistentVolumeClaim per pod, which survives
`helm uninstall` so that uninstalling doesn't delete your data. Delete the
claim by hand when you mean to.

The chart runs the server as a StatefulSet, and its volume uses the
`ReadWriteOncePod` access mode (Kubernetes 1.29 and later). Both exist for
the same reason: the server's stores assume one process at a time, and the
chart makes sure only one pod ever writes to a volume. On an older
cluster, set `persistence.accessMode: ReadWriteOnce`.

Choose the storage settings before installing. Access mode, storage class,
and size are fixed when the claim is created, and `helm upgrade` can't change
them.

The pod's log opens with `inbound authentication enabled tokens="admin
(admin), user (user)"`, as in the other setups. The server listens on every
address in the pod, since a Service can only reach a pod that does; the tokens
and the NetworkPolicy are what keep it private. The pod's probes call
`/healthz`, which needs no token.

`replicaCount` above 1 gives you several independent servers, not one
bigger server. Blueprints from the values file reach all of them, but
anything done over the API (registered blueprints, sessions, packages,
secrets) lands only on the pod that handled it. A client that opens a session
has to keep talking to the same pod, through the headless Service:

```text
submilli-0.submilli-headless.<namespace>.svc:8128
```

Leave it at 1 unless your application does that.

To upgrade, run `helm upgrade submilli oci://ghcr.io/submilli/charts/submilli
-f values.yaml --version <new version>`. Each chart version deploys a
matching server version unless you set `image.tag`, and the pod restarts onto
the same volume. The same command applies changes to your values file. To back up, snapshot the claims
(`state-submilli-0`, …) with your cluster's VolumeSnapshot support.

## Before you go live

- The startup log says `inbound authentication enabled` and lists the tokens
  you expect. A request with no token gets `401`.
- Your application, and any agent that connects to the server, holds a
  `user` token, not the one in the server's `SUBMILLI_SERVER_TOKEN`. The
  admin token is only where blueprints are deployed from.
- Only your application can reach the server. On one machine, `bind` is
  `127.0.0.1`. With Compose, the port is published on `127.0.0.1` and only
  your application shares `submilli-net`. On Kubernetes, `helm test` passes.
- If the server has to be reachable at a public address, it has the tokens
  and TLS in front of it, from a reverse proxy or an ingress. The token alone
  does not make a plain-HTTP endpoint safe: sent unencrypted, it can be read
  in transit and reused.
- `$SUBMILLI_HOME` is on storage that survives a redeploy, and it's backed
  up.
- Blueprints come from source control through the seed directory, and the
  startup log shows `failed=0`.
- The secret store's key, if you use one, is stored and backed up separately
  from the volume.
- The server's limits suit your workload, with `max_execution_time` set.
  [Resource limits](/docs/resource-limits) explains each one.
- Outbound access to internal services is opened only as far as a package
  needs. See [Outbound network](/docs/server#outbound-network).
