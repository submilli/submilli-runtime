---
title: "Deploying"
description: "Running submilli-server next to your application in production: on one machine, with Docker Compose, or on Kubernetes with the Helm chart."
slug: deploying
sidebar:
  order: 16
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
changes is how each setup handles the same three jobs.

## What every deployment has to get right

**Only your application can reach the server.** The server doesn't
authenticate callers yet. Anything that can connect to its port can run
programs, register or replace blueprints, and stop it. So the server should
sit where your application can reach it and nothing else can: loopback on a
single machine, a private network in Compose, a network policy in
Kubernetes. Each section below shows how its setup does this, and where it
stops protecting you.

**State lives on storage that outlasts the process.** Registered blueprints,
open sessions, installed packages, and secrets all live under
`$SUBMILLI_HOME`. [Where it keeps state](/docs/server#where-it-keeps-state)
explains what losing each one costs. On a machine that's a directory; in a
container it has to be a volume, or it's gone at the next deploy.

**Blueprints and secrets arrive without manual steps.** Keep blueprints in
source control and let the server load them at startup from a [seed
directory](/docs/server#blueprints-from-a-directory). Keep secret values out
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
```

```sh
SUBMILLI_HOME=/var/lib/submilli submilli-server --config /etc/submilli/server.yaml
```

Leave `bind` at `127.0.0.1`. Your application connects to
`http://127.0.0.1:8128`, and nothing on the network can. If you ever see this
at startup, the server is listening on a public address:

```text
WARN submilli_server::config: bound outside loopback: this server has no inbound authentication, so anything that can reach this port can run code, manage blueprints, and stop the server; make sure only your application can reach it (https://submilli.ai/docs/deploying/)
```

Back up `/var/lib/submilli` with the rest of the machine. Keep the store key
out of that backup, or store it separately: a backup that holds both the
sealed secrets and their key protects nothing. `submilli upgrade` replaces
both binaries with the latest release; restart the server afterwards.

## With Docker Compose

If your application runs in containers on one host, run the server as one
more container. The repository ships a `compose.yaml` at its root that does
the hard parts; copy it next to your own. Start it:

```sh
docker compose up -d
docker compose ps
```

```text
NAME                  IMAGE                                      STATUS                    PORTS
submilli-submilli-1   ghcr.io/submilli/submilli-runtime:latest   Up 12 seconds (healthy)   127.0.0.1:8128->8128/tcp
```

### How it keeps other callers out

The file publishes the server's port as `127.0.0.1:8128`, not `8128`. That
difference matters more with Docker than it looks: Docker routes published
ports around the host's firewall, so a plain `8128:8128` is reachable from
your whole network even on a host where `ufw` says the port is closed.

Loopback publishing only covers the host, though. Other containers reach the
server over Docker's networks, not the published port. So the server sits on
its own network, `submilli-net`, and only containers you put on that network
can reach it. Add your application to it and call the server by its service
name:

```yaml title="compose.yaml (your service)"
services:
  app:
    image: your-application
    environment:
      SUBMILLI_URL: http://submilli:8128
    networks:
      - submilli-net
```

Any container on `submilli-net` gets the full API, so don't put anything else
there.

### What else the file sets up

- **State on a named volume.** `submilli-state` holds `$SUBMILLI_HOME`, so
  blueprints, sessions, packages, and secrets survive `docker compose down`
  and image upgrades. `docker compose down -v` deletes it.
- **Scratch space in memory.** Each run's scratch directory lives in a 256 MB
  `tmpfs` at `/tmp`, so it never grows the container's disk.
- **A locked-down container.** The server runs as a non-root user on a
  read-only filesystem with every Linux capability dropped. The image has no
  shell. If a program ever escaped the sandbox, there would be little inside
  the container to use.
- **A health check** using `submilli-server --health-check`, which is why
  `docker compose ps` can say `healthy`.
- **Ten seconds to stop.** Docker waits that long before killing the
  container, which covers the server's own five-second drain. If you raise
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
command as on a laptop. The server fetches and builds them itself:

```sh
submilli server packages install acme/billing-package
```

### The secret store's key

The secret store stays off until the server has a key. Give it one as a file,
not an environment variable: environment variables show up in
`docker inspect` and `docker compose config`.

```sh
head -c 32 /dev/urandom | base64 > submilli-store-key
chmod 0444 submilli-store-key
```

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

The `0444` is required. Outside Docker Swarm, Compose mounts the file with
its original owner and mode, and the server runs as user 65532, so a
`0600` file owned by you is unreadable and the server refuses to start. Keep
the key out of source control and out of your volume backups.

### Upgrading and backing up

To upgrade, point the image at a new tag and recreate the container:

```sh
SUBMILLI_IMAGE=ghcr.io/submilli/submilli-runtime:0.1.6 docker compose up -d
```

Pin a version tag rather than following `latest`, so an upgrade happens
when you choose it. The volume carries the state across.

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

If your application runs on Kubernetes, install the server with the Helm
chart in `charts/submilli`. The chart isn't published to a registry yet, and
the image is private until launch, so for now install from a checkout of the
repository with a pull secret:

```sh
kubectl create secret docker-registry ghcr-creds \
  --docker-server=ghcr.io --docker-username=YOUR_USER --docker-password="$GITHUB_TOKEN"

helm install submilli ./charts/submilli --set 'imagePullSecrets[0].name=ghcr-creds'
```

This gives you one server pod, a Service called `submilli`, a persistent
volume for its state, and a network policy that lets nothing reach it yet.
The chart's [README](https://github.com/submilli/submilli-runtime/tree/main/charts/submilli)
is the reference for every value; this section covers the decisions you'll
actually make.

### Letting your application in

In a cluster, any pod in any namespace can reach any Service unless
something says otherwise. So the chart installs a default-deny
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
- **It admits pods, not requests.** Every pod you allow gets the whole API,
  including replacing blueprints.
- **`kubectl port-forward` goes around it.** Anyone allowed to port-forward
  into the namespace can reach the server. That's fine for debugging, but
  it's worth knowing who has that permission.

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

The two maps have to agree on the path, and the server won't catch a
mismatch at startup: it registers the blueprint and fails on the first
request that needs the secret. `helm test` cross-checks them.

The server's own encrypted secret store is off in the chart. Its encryption
protects the volume only if the key lives somewhere the volume doesn't, and
a Kubernetes Secret in the same namespace usually doesn't qualify. Turn it on
(`secretStore.enabled`) when the key comes from a KMS or a CSI secrets
driver.

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
at once can still exceed it and get the pod killed for running out of memory.
If your programs handle large data, measure your own peak usage.

### Storage, replicas, and upgrades

State lives on a PersistentVolumeClaim per pod, which survives
`helm uninstall` so that uninstalling doesn't delete your data. Delete the
claim by hand when you mean to.

The chart runs the server as a StatefulSet, and its volume uses the
`ReadWriteOncePod` access mode (Kubernetes 1.29 and later). Both exist for
the same reason: the server's stores assume one process at a time, and two
pods writing the same volume would silently corrupt them. On an older
cluster, set `persistence.accessMode: ReadWriteOnce`.

Choose the storage settings before installing. Access mode, storage class,
and size are fixed when the claim is created, and `helm upgrade` can't change
them.

`replicaCount` above 1 gives you several independent servers, not one
bigger server. Blueprints from the values file reach all of them, but
anything done over the API (registered blueprints, sessions, packages,
secrets) lands only on the pod that handled it. A client that opens a session
has to keep talking to the same pod, through the headless Service:

```text
submilli-0.submilli-headless.<namespace>.svc:8128
```

Leave it at 1 unless your application does that.

To upgrade, `helm upgrade submilli ./charts/submilli` with the new chart. The
image tag follows the chart's version unless you set `image.tag`, and the
pod restarts onto the same volume. To back up, snapshot the claims
(`state-submilli-0`, …) with your cluster's VolumeSnapshot support.

## Before you go live

- Only your application can reach the server. `helm test` passes on
  Kubernetes, and there's no bind warning in the log anywhere else.
- `$SUBMILLI_HOME` is on storage that survives a redeploy, and it's backed
  up.
- Blueprints come from source control through the seed directory, and the
  startup log shows `failed=0`.
- The secret store's key, if you use one, is stored and backed up separately
  from the volume.
- The server's limits suit your workload. [Limits](/docs/server#limits)
  explains each one.
- Outbound access to internal services is opened only as far as a package
  needs. See [Outbound network](/docs/server#outbound-network).

Inbound authentication is planned. When it ships, this chapter will show how
to supply the credential in each setup, and the network rules above become a
second layer rather than the only one.
