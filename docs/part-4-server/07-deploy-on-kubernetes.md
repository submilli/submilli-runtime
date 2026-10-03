---
title: "Deploy on Kubernetes"
description: "Install the Helm chart, configure application access and HTTPS, and manage state, secrets, and upgrades."
slug: server/deploy-on-kubernetes
sidebar:
  order: 7
---

Deploy the server alongside your application with the Helm chart. Keep your
settings in `values.yaml`; an empty file uses the defaults. See the
[chart reference](https://github.com/submilli/submilli-runtime/tree/main/charts/submilli)
for all values.

## Install it

```sh
helm install submilli oci://ghcr.io/submilli/charts/submilli -f values.yaml
```

The defaults create one server pod, the `submilli` Service, persistent state,
an encrypted secret store, and a default-deny NetworkPolicy. Use `--version`
to pin a chart version.

## Give your application its token

The chart generates and retains admin and user tokens in `submilli-auth`.
Supply the user token to your application:

```yaml title="your application's Deployment (fragment)"
env:
  - name: SUBMILLI_SERVER_TOKEN
    valueFrom:
      secretKeyRef:
        name: submilli-auth
        key: user-token
```

The CLI uses `admin-token` from the same Secret; see
[Connect the CLI](/docs/server/connect-the-cli). For Argo CD or `helm template`,
create the Secret yourself and set `auth.existingSecret` to avoid regenerated
tokens on each sync.

## Let your application in

Allow application pods through the NetworkPolicy:

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

Selectors in one item must both match. Separate items allow either match.
The cluster's network plugin must enforce NetworkPolicy; `helm test submilli`
checks it. Admitted pods still need an API token. `kubectl port-forward`
provides separate access controlled by Kubernetes permissions.

Applications call `http://submilli.<namespace>.svc:8128`. The chart adds Service,
pod, and Ingress names to `mcp_allowed_hosts`. Add custom names through `config`:

```yaml title="values.yaml"
config:
  mcp_allowed_hosts:
    - submilli.agents.svc.internal:8128
```

## Enable HTTPS

HTTPS is off by default. Supply a certificate covering the client DNS names
and `submilli-0.submilli-headless`, which Helm tests use. For release `submilli`
in namespace `default`, a self-signed certificate can be generated with:

```sh
openssl req -x509 -newkey rsa:2048 -nodes -days 365 \
  -keyout server.key -out server.crt -subj '/CN=submilli.default.svc' \
  -addext 'subjectAltName=DNS:submilli,DNS:submilli.default.svc,DNS:submilli.default.svc.cluster.local,DNS:submilli-0.submilli-headless,DNS:submilli-0.submilli-headless.default.svc,DNS:submilli-0.submilli-headless.default.svc.cluster.local' \
  -addext 'basicConstraints=critical,CA:FALSE'
chmod 0600 server.key
kubectl create secret tls submilli-tls --cert=server.crt --key=server.key
```

Adjust names for your release and namespace. Create the Secret in the server's
namespace; for a private issuer, include its chain in `server.crt`.

```yaml title="values.yaml"
tls:
  enabled: true
  existingSecret: submilli-tls
```

Apply the values with `helm upgrade`. Clients now use
`https://submilli.<namespace>.svc:8128`. Probes and Helm tests use HTTPS;
Helm tests verify the certificate. Self-signed certificates need
[verified CLI trust](/docs/server/connect-the-cli#trust-a-self-signed-server)
and separate trust in application clients.

After replacing the Secret, restart the server:

```sh
kubectl rollout restart statefulset/submilli
```

Use `tls` chart values, not `config.tls`. Native HTTPS is separate from
`ingress.tls`; enabling both requires your Ingress controller to use HTTPS
for the backend. Custom Secret keys and Ingress options are in the chart reference.

## Store secrets and register blueprints

The chart retains the secret store's key in `submilli-secret-store`, including
across uninstall. Back up that Secret with the state volume; both are needed
to restore secrets.

Connect with the admin token, then register secrets and blueprints:

```sh
submilli server secret put billing_api_key
submilli server blueprint apply blueprint.yaml
```

See [Register a blueprint](/docs/server/register-a-blueprint),
[Manage blueprints in Git](/docs/tutorials/manage-blueprints-in-git), and
[Install private packages](/docs/server/install-private-packages) for those workflows.

For Argo CD or `helm template`, create the store key Secret yourself. Put the
output of `head -c 32 /dev/urandom | base64` under its `key` entry and set
`secretStore.existingSecret`.

Run `helm test submilli` after each install or upgrade.

## Allow an internal service

Programs cannot call private addresses until explicitly allowed. Find the
service's cluster IP and add it to `config.network.allow_ip`:

```sh
kubectl -n internal get svc inventory -o jsonpath='{.spec.clusterIP}'
```

```yaml title="values.yaml"
config:
  network:
    allow_ip:
      - 10.96.45.205
```

Programs can still use `http://inventory.internal.svc/`; the server checks its
resolved address. NetworkPolicy controls incoming access, while `allow_ip`
controls outgoing calls.

## Size memory

`execution.maxMemoryMB` defaults to 50. It sets the per-execution limit and pod
memory:

```text
pod memory = 128 Mi + 16 × maxMemoryMB    (928 Mi at the defaults)
```

The formula covers one execution near its limit; concurrent executions can
require more. Measure your workload's peak. Other limits go under `config`;
see [Set limits](/docs/server/set-limits).

## Choose storage before the first install

Each StatefulSet pod has a persistent claim that survives `helm uninstall`.
Managed local volumes use the same claim. Storage class, size, and access mode
cannot be changed with `helm upgrade`.

The default `ReadWriteOncePod` mode requires Kubernetes 1.29 or later. On older
clusters, use `persistence.accessMode: ReadWriteOnce`.

Keep `replicaCount: 1` unless clients pin sessions to individual pods. Replicas
have independent sessions, blueprints, packages, and secrets. Address a specific
pod through the headless Service:

```text
submilli-0.submilli-headless.<namespace>.svc:8128
```

## Upgrade and back up

```sh
helm upgrade submilli oci://ghcr.io/submilli/charts/submilli -f values.yaml --version <new version>
```

Chart versions select matching server versions unless `image.tag` overrides it.
Upgrades retain state, tokens, and the store key. Back up the claims
(`state-submilli-0`, …) and `submilli-secret-store` Secret together.
