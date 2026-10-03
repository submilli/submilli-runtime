---
title: "Deploy on Kubernetes"
description: "How to install the server in your cluster with the Helm chart: generated tokens, a network policy that admits only your application, the encrypted secret store and its key, blueprints registered through the API, memory sizing, storage, and upgrades."
slug: server/deploy-on-kubernetes
sidebar:
  order: 7
---

If your application runs on Kubernetes, the server runs in the same
cluster, installed with the Helm chart, as a Service your application
reaches by name.

This guide shows you how to deploy it with the chart: install it, give
your application its token, let your application in through the network
policy, store secrets and register blueprints, allow an internal service,
size memory, and choose storage before the first install. The chart's
[README](https://github.com/submilli/submilli-runtime/tree/main/charts/submilli)
is the reference for every value.

## Install it

```sh
helm install submilli oci://ghcr.io/submilli/charts/submilli -f values.yaml
```

`values.yaml` holds your settings; the sections below build it up, and an
empty file is a valid start. Add `--version` to pin a chart version, so
upgrades happen when you choose them. This gives you one server pod, a
Service called `submilli`, a persistent volume for its state, an
encrypted secret store with its key in a Secret, and a network policy
that lets nothing reach it yet.

## Give your application its token

The chart generates an admin token and a user token into a Secret named
`submilli-auth` and keeps them across upgrades. Give your application the
user token:

```yaml title="your application's Deployment (fragment)"
env:
  - name: SUBMILLI_SERVER_TOKEN
    valueFrom:
      secretKeyRef:
        name: submilli-auth
        key: user-token
```

The admin token is under `admin-token` in the same Secret, for the
`submilli server` commands; [Connect the
CLI](/docs/server/connect-the-cli) reads it into a file. With Argo
CD, or any pipeline that applies `helm template` output, create the
Secret yourself and name it in `auth.existingSecret`: rendered without
access to the cluster, the chart would generate new tokens on every sync.

## Let your application in

The chart installs a default-deny NetworkPolicy, and you list the pods
that may call the server:

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
Your application then calls `http://submilli.<namespace>.svc:8128`. An
application that connects over MCP needs the server to accept that name
as a `Host` header, and the chart arranges it: the Service's short and
namespace-qualified names, each pod's headless name, and any Ingress
host are written into the server's `mcp_allowed_hosts`. A name the chart
can't know, such as one under a cluster DNS suffix other than
`cluster.local`, goes under `config:`, which passes settings to the
server's config file in the server's own names and here extends the
generated list:

```yaml title="values.yaml"
config:
  mcp_allowed_hosts:
    - submilli.agents.svc.internal:8128
```

The policy has three limits worth knowing:

- **Your cluster has to enforce it.** A NetworkPolicy is only a request;
  the cluster's network plugin enforces it, and some don't. Where it
  isn't enforced, nothing warns you. `helm test submilli` checks this
  directly: it starts a pod that shouldn't get through and fails if it
  does.
- **It admits pods, not requests.** What an admitted pod may call is up
  to the token it holds.
- **`kubectl port-forward` is a separate way in.** It's handy for
  debugging, and it's worth deciding who on your team should have that
  permission.

## Enable HTTPS

Native HTTPS is off by default. To enable it, create a Kubernetes TLS Secret
in the server's namespace from your PEM certificate chain and private key:

```sh
kubectl create secret tls submilli-tls --cert=server.crt --key=server.key
```

Then add these values and install or upgrade the release:

```yaml title="values.yaml"
tls:
  enabled: true
  existingSecret: submilli-tls
```

The chart mounts the files read-only and configures HTTPS on the existing
server port, 8128. Health probes and `helm test` use HTTPS too. The API test
pods mount only the public certificate; they verify the server rather than
skipping certificate checks. Kubelet HTTPS probes check availability without
verifying the certificate, so `helm test` supplies the certificate verification
check.

Choose certificate SANs for the addresses your clients use. The Helm tests
address pod 0 as `submilli-0.submilli-headless`, so include that name too. For
the release `submilli` in namespace `default`, a self-signed example is:

```sh
openssl req -x509 -newkey rsa:2048 -nodes -days 365 \
  -keyout server.key -out server.crt -subj '/CN=submilli.default.svc' \
  -addext 'subjectAltName=DNS:submilli,DNS:submilli.default.svc,DNS:submilli.default.svc.cluster.local,DNS:submilli-0.submilli-headless,DNS:submilli-0.submilli-headless.default.svc,DNS:submilli-0.submilli-headless.default.svc.cluster.local' \
  -addext 'basicConstraints=critical,CA:FALSE'
chmod 0600 server.key
```

Generate the files before creating the Secret. Adjust the release, namespace,
cluster DNS suffix, and pod names for your deployment. With a private issuer,
include the issuer chain needed by the chart's test clients in `server.crt`.
Applications now call `https://submilli.<namespace>.svc:8128` and need their own
trust configuration for a private or self-signed certificate.

[Connect the CLI](/docs/server/connect-the-cli#trust-a-self-signed-server)
explains its approval prompt and fingerprint registration for deploy jobs. That
page also covers preserving the certificate hostname with `kubectl port-forward`.
TLS does not replace API tokens, NetworkPolicy, or the MCP hostname allowlist.

The server reads certificates at startup. After updating the Secret, restart it:

```sh
kubectl rollout restart statefulset/submilli
```

`tls.certKey` and `tls.privateKeyKey` select keys other than the standard
`tls.crt` and `tls.key`. Use these chart values instead of `config.tls`, which
is reserved so the config, mounts, probes, and tests agree.

Native TLS and `ingress.tls` are separate: the latter configures the certificate
clients see at the Ingress. If you enable both, configure your controller to
connect to the backend over HTTPS. For ingress-nginx, set
`ingress.annotations.nginx.ingress.kubernetes.io/backend-protocol: HTTPS`;
other controllers use their own settings. Backend certificate verification is
also a controller setting, not something this chart configures automatically.

## Store secrets and register blueprints

The server's secret store is on from the first install. The chart
generates its 32-byte key into a Secret named `submilli-secret-store`,
reuses it on every upgrade, and keeps it when the release is
uninstalled, like the volume. Back up the two together: a volume
restored without its key can't give its secrets back, and a key without
the volume has nothing to open.

Everything else reaches the server through its API, with the admin
token. Reach it from your machine as [Connect the
CLI](/docs/server/connect-the-cli) shows, with the token read from
the Secret, then put each blueprint's secrets in the store and register
the blueprint:

```sh
submilli server secret put billing_api_key
submilli server blueprint apply blueprint.yaml
```

```text
Value for 'billing_api_key': [hidden]
Stored secret 'billing_api_key'
Added blueprint 'support'
```

A deploy job does the same with the admin token in its secrets, as
[Manage blueprints in Git](/docs/tutorials/manage-blueprints-in-git)
builds; what registration checks, and how to replace or remove a
blueprint, is [Register a
blueprint](/docs/server/register-a-blueprint). For a package in a
private repository the server needs a GitHub token from a Secret; refer
to [Install private packages](/docs/server/install-private-packages).

With Argo CD, or any pipeline that renders the chart without the cluster,
the key needs the same treatment as the tokens: create the Secret
yourself, holding the output of `head -c 32 /dev/urandom | base64` under
the key `key`, and name it in `secretStore.existingSecret`, or the chart
would generate a different key on every sync and lock the store.

Run `helm test submilli` after every install and upgrade. It registers a
small blueprint of its own through the API, runs a program under it, and
removes it when it is done.

## Allow an internal service

Pod and Service addresses in a cluster are private addresses, so the
server blocks programs from calling them until you allow them. For a
package that calls one of your own services, find the Service's cluster
IP and allow that one address, through `config:` again:

```sh
kubectl -n internal get svc inventory -o jsonpath='{.spec.clusterIP}'
```

```yaml title="values.yaml"
config:
  network:
    allow_ip:
      - 10.96.45.205
```

Programs keep calling the service by name
(`http://inventory.internal.svc/`); the server checks the address the
name resolves to. A Service keeps its cluster IP until it's deleted, so
allow that one address rather than the cluster's whole Service range.

The chart's NetworkPolicy only controls who can call the server, not what
the server calls. The address check above is what limits outbound calls.

## Size memory

One value, `execution.maxMemoryMB` (50 by default), sets both the
server's per-execution memory limit and the pod's memory, so the two
can't drift apart:

```text
pod memory = 128 Mi + 16 × maxMemoryMB    (928 Mi at the defaults)
```

The multiplier is large because the limit counts memory a program holds,
not memory the process uses on the way. One execution near its limit has
been measured at more than 12 times that in process memory, all released
when it finishes. The formula covers one such execution at a time;
several at once can use more, so if your programs handle large data,
measure your own peak. Other limits go under `config:`, in the server's
own names; [Set limits](/docs/server/set-limits) chooses them.

## Choose storage before the first install

State lives on a PersistentVolumeClaim per pod, which survives `helm
uninstall` so that uninstalling doesn't delete your data. Delete the
claim by hand when you mean to. A named volume of kind `managed-local`,
declared under `config.volumes` as [Mount a shared
volume](/docs/server/mount-a-shared-volume) shows, lives on the same
claim.

The chart runs the server as a StatefulSet, and its volume uses the
`ReadWriteOncePod` access mode (Kubernetes 1.29 and later). Both exist
for the same reason: the server's stores assume one process at a time,
and the chart makes sure only one pod ever writes to a volume. On an
older cluster, set `persistence.accessMode: ReadWriteOnce`.

Access mode, storage class, and size are fixed when the claim is created,
and `helm upgrade` can't change them, so choose them before installing.

`replicaCount` above 1 gives you several independent servers, not one
bigger server: anything done over the API (registered blueprints,
sessions, packages, secrets) lands only on the pod that handled it. A
client that opens a session has to keep talking to the same pod, through
the headless Service:

```text
submilli-0.submilli-headless.<namespace>.svc:8128
```

Leave it at 1 unless your application does that.

## Upgrade and back up

```sh
helm upgrade submilli oci://ghcr.io/submilli/charts/submilli -f values.yaml --version <new version>
```

Each chart version deploys a matching server version unless you set
`image.tag`, and the pod restarts onto the same volume, with the same
tokens and the same store key. The same command applies changes to your
values file. To back up, snapshot the claims (`state-submilli-0`, …) with
your cluster's VolumeSnapshot support, and save the `submilli-secret-store`
Secret with them.
