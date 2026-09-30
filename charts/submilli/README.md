# submilli

Runs [`submilli-server`](https://github.com/submilli/submilli-runtime) on Kubernetes — an
agent-native execution environment that runs a strict TypeScript subset in a WasmGC
sandbox with capability-based security.

This chart wraps the multi-arch container image with the standard resource set and
carries the same hardening the repo's `compose.yaml` applies on a single host,
translated into a pod spec.

## Status

**Not published to a registry yet.** The chart needs the first server release
with inbound authentication, which is newer than `0.1.6`: it configures the
server through a config file carrying `api_tokens` and probes `/healthz`, and an
earlier server rejects the one and does not serve the other. The chart is not
pushed to an OCI registry, and the GHCR image package is private until launch.

This chart has never been published, so there is no earlier revision of it in the
wild and no upgrade path from one is provided. Its shape changed during
development — an earlier revision used a Deployment with a single shared
PersistentVolumeClaim — and `helm upgrade` across that change does not carry the
data: the new pod comes up on a fresh, empty volume while the old claim survives,
mounted by nothing. If you installed from a checkout while that was the shape,
uninstall and delete the old claim rather than upgrading.

Until both change, install from a checkout and supply a pull secret:

```bash
kubectl create secret docker-registry ghcr-creds \
  --docker-server=ghcr.io --docker-username=YOUR_USER --docker-password="$GITHUB_TOKEN"

helm install submilli ./charts/submilli --set 'imagePullSecrets[0].name=ghcr-creds'
```

Then, from inside the cluster or through a port-forward:

```bash
kubectl port-forward svc/submilli 8128:8128
curl -i http://127.0.0.1:8128/healthz

TOKEN=$(kubectl get secret submilli-auth -o jsonpath='{.data.admin-token}' | base64 -d)
curl -H "Authorization: Bearer $TOKEN" http://127.0.0.1:8128/v1/status
```

`/healthz` answers anyone; everything else needs a token. See
[Authentication](#authentication).

### Upgrading from 0.1.x

0.2.0 turns authentication on. Upgrading an existing release generates the two
tokens and the server starts refusing requests without one, so **every existing
caller gets 401 until it sends a token**. Read the tokens out of the Secret
(below) and hand them out before or immediately after the upgrade, or set
`auth.enabled: false` for the upgrade and turn it on once callers are ready.

The chart's own settings also moved from environment variables into a config
file. Nothing to do unless you set one of `SUBMILLI_BIND`,
`SUBMILLI_MAX_EXECUTION_MEMORY`, `SUBMILLI_SHUTDOWN_GRACE`,
`SUBMILLI_VFS_EPHEMERAL_DIR`, or `SUBMILLI_BLUEPRINT_SEED_DIR` through
`extraEnv`: those still win over the file, exactly as they used to win over the
chart's own variable of the same name.

## Read this before exposing it

**The server executes code for whoever holds a token.** A caller with the `user`
token can run arbitrary programs in the sandbox; one with the `admin` token can
also register and rewrite blueprints, manage secrets and packages, and stop the
process with `POST /v1/shutdown`. The tokens are bearer tokens: whoever has the
string has the access, with no further identity behind it.

So the chart does not rest on the tokens alone. On a single host `compose.yaml`
publishes the port loopback-only; Kubernetes has no equivalent, and a ClusterIP
Service is reachable from every pod in every namespace by default. A token that
leaks — into a log, a crash dump, an over-shared Secret — would then be usable
from anywhere in the cluster. The defaults put a second, independent layer
behind it:

| Default | Why |
|---|---|
| `auth.enabled: true` | Every route but `/healthz` needs a bearer token. |
| `networkPolicy.enabled: true` | Unusual for a chart. Default-deny ingress, scoped to this release's pod, so a token is only usable from workloads you allowlisted. |
| `ingress.enabled: false` | An Ingress publishes a code-execution API to whatever can reach it, behind nothing but a bearer token. |
| `automountServiceAccountToken: false` | Nothing to steal from the pod if the sandbox boundary is crossed. The ServiceAccount is granted no RBAC either. |
| `readOnlyRootFilesystem: true` | Shrinks what a filesystem read can reach to only what is deliberately mounted. |

Three things the NetworkPolicy does **not** do, worth knowing before you rely on it:

- **It is only enforced if your CNI implements NetworkPolicy.** Where it does not,
  the object is accepted by the API server and silently does nothing — no error, no
  warning. `helm test` includes a check that fails loudly in that case rather than
  letting it pass unnoticed.
- **It authorises a pod, not a request.** Which routes an allowlisted pod may call
  is decided by the token it holds, not by the policy.
- **`kubectl port-forward` bypasses it entirely.** It tunnels through the API
  server, so anyone with `pods/portforward` permission reaches the server whatever
  the policy says — and is then stopped by the tokens, if they are on.

If you enable the Ingress, configure `ingress.tls`. A bearer token sent over plain
HTTP is readable by everything on the path.

## Authentication

Callers send `Authorization: Bearer <token>`. A missing or unknown token is a
`401`; a `user` token on an admin route is a `403`.

| Token | May call |
|---|---|
| `admin` | Everything. |
| `user` | `POST /v1/execute`, everything under `/v1/sessions`, `/mcp/{blueprint}`, and the read-only `GET /v1/blueprints/{name}/prompt`, `…/packages/search`, `…/packages/docs`, `…/builtins`, and `…/builtins/docs`. |

Everything not in the `user` row — including `GET /v1/status`, blueprint
create/update/delete, `/v1/secrets`, `/v1/packages`, and `/v1/shutdown` — needs
`admin`. **Give agents the `user` token.** The `admin` token is for whatever
deploys blueprints.

`GET /healthz` needs no token and returns an empty `200`. The pod's probes use
it, which is what lets them work without the admin token being written into the
pod spec.

### Reading the tokens

By default the chart generates both into a Secret named `<release>-submilli-auth`
(`submilli-auth` for a release named `submilli`):

```bash
kubectl get secret submilli-auth -n NAMESPACE -o jsonpath='{.data.admin-token}' | base64 -d
kubectl get secret submilli-auth -n NAMESPACE -o jsonpath='{.data.user-token}' | base64 -d
```

They are generated once. Every later `helm upgrade` reads the live Secret back
and reuses what it finds, so an upgrade never rotates a token out from under its
callers. The Secret also survives `helm uninstall`, like the state volume, and a
reinstall under the same release name picks it up again; delete it by hand when
you want new tokens.

### Bringing your own Secret

```yaml
auth:
  existingSecret: submilli-tokens
  adminTokenKey: admin-token   # the defaults
  userTokenKey: user-token
```

```bash
kubectl create secret generic submilli-tokens \
  --from-literal=admin-token="$(openssl rand -hex 32)" \
  --from-literal=user-token="$(openssl rand -hex 32)"
```

Each token must be at least 32 characters of letters, digits, and `-._~+/`
(optionally `=`-padded), and the two must differ. The server refuses to start
otherwise, and says which rule was broken.

**Argo CD, and any `helm template | kubectl apply` flow, must use
`auth.existingSecret`.** Keeping the generated tokens stable depends on Helm's
`lookup`, which reads the live Secret at render time. Rendering without cluster
access returns nothing, so the chart would mint two new tokens on every sync and
lock out every caller each time.

### Rotating a token

Edit the Secret, then restart the pod:

```bash
kubectl rollout restart statefulset/submilli
```

The server reads tokens once, at boot, so a changed Secret does nothing until
then. The restart is a separate, deliberate step: the Secret is left out of the
pod's rollout checksum on purpose, since the chart cannot see inside a Secret it
did not generate. To rotate a *generated* token, delete its key from the Secret
and run `helm upgrade` — the missing key is regenerated and the other is left
alone — then restart.

`helm test` checks the running server against the Secret, and fails if the admin
token in it is not one the server accepts.

### More callers

Extra tokens go in `config.api_tokens` and are added after the chart's two. The
names `admin` and `user` are reserved. The token itself comes from the
environment, from a Secret of yours:

```yaml
config:
  api_tokens:
    - name: ci
      role: admin
      token_env: CI_API_TOKEN
extraEnv:
  - name: CI_API_TOKEN
    valueFrom:
      secretKeyRef: { name: ci-token, key: token }
```

### Turning it off

```yaml
auth:
  enabled: false
```

The server then serves every route to anyone who can reach the port, and network
reachability is the only access control. That is a reasonable setting for a
throwaway cluster and for nothing else. Leave the NetworkPolicy on and never
combine this with an Ingress.

## Server configuration

The chart renders the server's config file, `server.yaml`, into a ConfigMap and
points the server at it. A change to it rolls the pod on the next `helm upgrade`.
The file holds paths to tokens and keys, never the material itself.

Settings with a dedicated value (`server.bind`, `execution.maxMemoryMB`,
`server.shutdownGrace`, `blueprints`, `secretStore`, `auth`) are written by the
chart. Anything else the server accepts goes under `config:`, using the server's
own key names and units:

```yaml
config:
  max_execution_time: 30          # seconds
  max_execution_fuel: 10000000000
  network:
    allow_private: true
```

The server rejects a key it does not know and refuses to boot, so a typo shows
up as a crash-looping pod with the key named in its log rather than as a setting
that silently does nothing.

**Keys the chart owns are refused in `config:`**, at render time, with an error
naming the value to use instead: `bind`, `port`, `vfs_ephemeral_dir`,
`max_execution_memory`, `shutdown_grace`, `blueprint_seed_dir`,
`allow_unauthenticated`, and `secret_store.key_file`. Most of them also size
something in the pod spec — the memory limit, the termination grace period, a
mount path — and two sources for one number is how those drift apart. The rest of
`secret_store` (`dir`, `key_env`) passes through.

**`extraEnv` is an override.** The server ranks an explicit `SUBMILLI_*`
environment variable above its config file, so one set through `extraEnv` beats
both the chart's own settings and `config:`. Prefer `config:`; keep `extraEnv`
for what only the environment can carry, such as a provider credential. The port
is the one setting the chart itself still passes as a variable
(`SUBMILLI_PORT`), to defend against the kubelet injecting a variable of that
name for a Service called `submilli`.

## Blueprints

Supply them declaratively through `values.yaml`; they are rendered into a ConfigMap,
mounted read-only, and reconciled into the server's store on every pod start.

```yaml
blueprints:
  demo: |
    name: demo
    default: deny
    vfs:
      mode: ephemeral
    permissions:
      main:
        - capability: http.get
          action: allow
          filter: host == "api.example.com"
```

Two behaviours that surprise people:

- **The files win.** Editing or deleting one of these blueprints through the API is
  undone on the next pod restart. To change one, change `values.yaml` and run
  `helm upgrade`. The overwritten version is kept on disk as a revision, so the
  revert is auditable rather than silent.
- **The chart never deletes.** Blueprints you registered through the API and did not
  list here are left alone — nothing can tell "removed from source control" apart
  from "created deliberately at runtime".

Reconciling on *every* start, rather than once at install, is what makes this
survive a pod restart on ephemeral storage. It is why the chart does not use a Helm
install hook: hooks fire on release events, and the failure that has to be survived
is a pod-start event.

## Secrets

Reference existing Kubernetes Secrets. Never put secret values in `values.yaml` —
they land in plaintext in the Helm release object and in every `helm get values`
output.

```yaml
secrets:
  stripe:
    secretName: stripe-credentials
    key: api-key
```

Each entry mounts at `/etc/submilli/secrets/<name>/<key>`, which is the path a
blueprint reads with a `file:` source:

```yaml
blueprints:
  billing: |
    name: billing
    secrets:
      STRIPE_KEY: { file: /etc/submilli/secrets/stripe/api-key }
```

That path is a contract between the two maps, not an implementation detail — the
chart chooses where the Secret lands and the blueprint has to name the same place.
`helm test` cross-checks the two and fails if they drift, because nothing else
does: the server seeds a blueprint whose secret cannot resolve rather than
refusing it, so the mistake surfaces on the first real request instead of at
deploy time.

**`file:` sources work only for blueprints supplied here.** A blueprint you
register at runtime through `POST`/`PUT /v1/blueprints` is rejected with
`forbidden_secret_source` if it declares an `env:` or `file:` secret, whichever
token sent it. That is deliberate: an admin token is permission to manage
blueprints, not to make the server read its own environment and files — which
would include the secret-store key and the API tokens themselves. Blueprints in
`blueprints:` are supplied locally by the operator, so they are not subject to
it. For runtime-registered blueprints, use a `store:` secret.

The server's own encrypted-at-rest store (`secretStore.enabled`) is **off by
default**. It protects the volume against offline disclosure and only earns that if
its key comes from a different trust domain than the data it protects; a key kept in
a Kubernetes Secret beside the volume buys very little. Turn it on when you have a
KMS or CSI-driver key source.

## Memory

One knob, `execution.maxMemoryMB`, feeds both the server's per-execution cap and
the pod's memory request and limit:

```
limits.memory = requests.memory = baselineMi + (headroomMultiplier × maxMemoryMB)
```

At defaults that is `128 + (16 × 50)` = **928Mi**. Two independent numbers would
drift — raise the container limit and the server never uses it, lower it and the
pod OOMKills immediately.

**`maxMemoryMB` bounds live GC heap, not process RSS, and the gap is large.**
Measured against the real server: it idles at ~8 MiB, but a *single* execution
holding ~33 MB of live string under a 50 MB cap peaked at **628 MiB of RSS** —
more than 12× the cap — because transient garbage is not collected until the run
ends. The memory is fully released afterwards, so that is a peak, not a
steady-state need.

`headroomMultiplier` covers the worst case observed for one execution. It is
**not a concurrency count and not a guarantee**: the server has no
max-concurrency setting, so several executions near the cap at once can still
exceed the limit and OOMKill the pod. [SUB-704] tracks making the cap into
something an operator can size a container from.

If your workload is allocation-heavy, measure it rather than trusting the
default — and measure steady-state RSS under sustained load, not RSS at startup.

[SUB-704]: https://linear.app/submilli/issue/SUB-704

## Storage

`persistence.enabled` defaults to **true**. Blueprints registered through the API,
sessions, installed packages, and secrets all live on the volume, under its
`server/` subdirectory, and the failure mode of `false` is silent data loss on
reschedule while the failure mode of `true` is a loud unbound-PVC error at
install. Prefer the loud one.

A volume written by a server that kept those directories at the top level of the
volume is moved under `server/` on the first boot of a server that does not,
before it opens any of them. `packages/` is the exception: it stays at the top
level and the server reads it as a fallback after its own `server/packages`, so
packages installed there earlier keep resolving but count as unmanaged (they
cannot be uninstalled through the API). The move is one-way, so rolling that
server back means moving the directories up again by hand.

The PVC survives `helm uninstall` by design; delete it by hand when you want the
data gone. Node-local volumes cannot follow a pod to another node, so surviving a
node drain needs a StorageClass whose volumes are not node-local.

### Running more than one server

`replicaCount` sets how many servers to run. Each gets **its own volume** and
shares nothing with the others — no clustering, no leader, no replication. They
are independent servers that happen to be deployed together, not a scaled copy of
one.

That makes addressing part of the deployment, and it is the thing to understand
before raising the number:

| | Consistent across servers? |
|---|---|
| Blueprints from `blueprints:` | **Yes** — one ConfigMap, seeded into every pod at boot |
| Blueprints registered through the API | **No** — only on the pod that served the request |
| Sessions, secrets, installed packages | **No** — same |

So a client must talk to a *specific* server, through the headless Service:

```
<release>-submilli-0.<release>-submilli-headless.<namespace>.svc:8128
```

The ordinary `<release>-submilli` Service load-balances across all of them. Above
one replica that means the same request can land anywhere: register a blueprint
and the next call may reach a server that has never heard of it. Verified — three
servers, one blueprint registered on ordinal 0, and it was visible on ordinal 0
only.

Sessions cannot migrate between servers either, since a session lives on the
volume of the pod that created it. A client that picks a server must stay on it.

Claims are named `state-<release>-submilli-<ordinal>` and are retained on both
`helm uninstall` and scale-down, so scaling from 3 to 1 and back returns each
server to its own state rather than a blank volume. To restore from a snapshot,
create a claim with that exact name *before* installing — the controller adopts a
pre-existing claim matching the convention and ignores one that does not.

**`persistence.size` is fixed at creation.** `volumeClaimTemplates` cannot be
changed by an upgrade; growing a volume means resizing the claims by hand.

### The access mode defaults to `ReadWriteOncePod`, and that needs Kubernetes 1.29+

Two independent mechanisms keep exactly one writer on the volume, and they cover
different attackers.

**The workload kind covers the controller.** The server runs as a StatefulSet
precisely so that no replacement pod is created while the current one is still
terminating — see [Why a StatefulSet](#why-a-statefulset) for the failure that
motivates it.

**The access mode covers everything else.** A hand-written pod, a `kubectl debug`
shell, or a second StatefulSet pointed at another release's claims never passes
through this controller at all. `ReadWriteOncePod` makes the scheduler itself
refuse them:

```
0/1 nodes are available: 1 node(s) unavailable due to PersistentVolumeClaim
with ReadWriteOncePod access mode already in-use by another pod.
```

**The requirement is the cluster version, not the storage driver.** Enforcement
lives in kube-scheduler and kubelet and is volume-type agnostic, so it works on
any provisioner — including non-CSI ones such as the `local-path` class in `kind`.
A CSI driver advertising `SINGLE_NODE_MULTI_WRITER` adds storage-layer fencing on
top; AWS EBS and GCP PD do not advertise it, and `ReadWriteOncePod` still works
correctly there with scheduler-level enforcement.

Use **Kubernetes 1.29 or newer**, where the mode went stable. It was beta and
on by default in 1.27-1.28, but the gate could be disabled there, so 1.29 is the
first version an operator can rely on without checking.

**Two ways it can fail, both loud:**

- **Cluster too old, or the gate disabled.** The API server rejects the claim
  synchronously and `helm install` fails immediately with
  `Unsupported value: "ReadWriteOncePod"` listing the modes it does accept.
  Nothing is created.
- **A provisioner that explicitly rejects the mode** (OpenEBS LocalPV Hostpath,
  or `local-path-provisioner` older than v0.0.25 — which means `kind` older than
  v0.26.0). The PVC stays `Pending` with a `ProvisioningFailed` event carrying
  the provisioner's own message. Check with `kubectl describe pvc`.

In either case, set:

```yaml
persistence:
  accessMode: ReadWriteOnce
```

`ReadWriteOnce` restricts the volume to one *node*, not one pod, so any number of
pods scheduled to that node can mount it at once. The StatefulSet still prevents
the chart from producing a second writer, so this fallback is safe for ordinary
operation — what it gives up is protection against pods created outside the
release.

### Why a StatefulSet

For the at-most-one guarantee, not for stable identity or per-replica storage —
neither of which means anything at a single replica.

A Deployment cannot provide it. `strategy: Recreate` governs the rollout path
only. When a pod is replaced for any other reason — an eviction, a
`kubectl drain`, a taint, a preemption — the ReplicaSet controller treats it as
gone the moment it carries a deletion timestamp and starts the replacement while
the old container is still running and still holding the volume. Reproduced on a
cluster at `replicas: 1` with `Recreate` set: the terminating pod and its
replacement both mounted the volume and each could read the other's writes.

That is not a survivable race here. The blueprint, session, and secret stores
take no lock: two processes hand out the same blueprint revision number and
overwrite each other's files, and a booting process deletes session directories
it has no record of. The failure is silent data loss, not a mount error.

The StatefulSet controller will not proceed past a terminating ordinal, which
closes it on every supported Kubernetes version, independent of access mode.

**Pick the access mode at install time.** A bound claim's spec is immutable, so
changing `persistence.accessMode` on an existing release does not migrate
anything — the upgrade fails and the release is left in `failed` state, still
serving from the previous revision. The same applies to
`persistence.storageClass` and `persistence.size`, which come from
`volumeClaimTemplates` and cannot be changed by an upgrade at all.

To actually switch, uninstall, delete the retained `state-<release>-submilli-<n>`
claims, and pre-create claims with the same names and the mode you want —
restoring each from a VolumeSnapshot of the old one if you need the data — before
installing again. The StatefulSet adopts a pre-existing claim whose name matches
the template and ignores one that does not.

`ReadWriteMany` is not an accepted value. It exists to let many pods on many
nodes write at once, which is precisely what corrupts these stores, and offering
it would imply the server supports a topology it does not.

## Values

`values.yaml` documents every key inline, and `values.schema.json` validates them on
`install`, `upgrade`, `lint`, and `template` — so a typo like `persistance.enabled`
is an error rather than a setting that silently does nothing. Run `helm lint` against
your override file to get that check without a cluster.

## Compatibility

`Chart.yaml` is `apiVersion: v2`, which both Helm 3 and Helm 4 install. Verified
against Helm 4.2.3 and Helm 3.21.3. `kubeVersion` requires Kubernetes 1.25 or newer,
where the security-context fields the pod spec sets stabilised.

The floor stays at 1.25 even though the default access mode needs 1.29, because
`persistence.accessMode: ReadWriteOnce` makes the chart work on 1.25-1.28 and
raising `kubeVersion` would block those clusters from installing at all rather
than letting them opt down. See [Storage](#the-access-mode-defaults-to-readwriteoncepod-and-that-needs-kubernetes-129)
for what that trade costs.
