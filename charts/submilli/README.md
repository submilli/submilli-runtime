# submilli

Runs [`submilli-server`](https://github.com/submilli/submilli-runtime) on Kubernetes — an
agent-native execution environment that runs a strict TypeScript subset in a WasmGC
sandbox with capability-based security.

This chart wraps the multi-arch container image with the standard resource set and
carries the same hardening the repo's `compose.yaml` applies on a single host,
translated into a pod spec.

## Install

Install a published chart version from GHCR. The chart and its default runtime
image can be pulled without a registry login or an image pull secret:

```bash
helm install submilli oci://ghcr.io/submilli/charts/submilli --version 0.3.4 --wait
helm test submilli --logs
```

Chart 0.3.4 deploys runtime 0.2.0. Pin the chart version to control upgrades.
See [Publishing](PUBLISHING.md) for the workflow and first-publication steps.

If you installed an early development checkout that used a Deployment and one
shared PersistentVolumeClaim, back up its state before switching to this chart.
The StatefulSet creates a separate claim for each pod, so that old volume is not
moved automatically.

Then, from inside the cluster or through a port-forward:

```bash
kubectl port-forward svc/submilli 8128:8128
TOKEN=$(kubectl get secret submilli-auth -o jsonpath='{.data.admin-token}' | base64 -d)
curl -H "Authorization: Bearer $TOKEN" http://127.0.0.1:8128/v1/status
```

## Read this before exposing it

**Every request needs a bearer token.** The chart generates two into the Secret
`<fullname>-auth` and keeps them across upgrades and uninstalls: `admin-token`
for the whole API, and `user-token`, which can run code, use sessions and MCP,
and read what a blueprint offers, but cannot change a blueprint. Give
applications the user token. The server reads tokens at boot, so after changing
the Secret run `kubectl rollout restart statefulset/<fullname>`.

- `auth.existingSecret` names a Secret you manage instead (keys
  `auth.adminTokenKey` and `auth.userTokenKey`). Use it with Argo CD or any flow
  that applies `helm template` output: rendered without cluster access, the
  chart cannot read its Secret back and would generate new tokens on every sync.
- `auth.enabled: false` serves without tokens. Network reachability is then the
  only access control.
- Upgrading a release from chart 0.1.x turns authentication on; callers must
  start sending a token.

A token is not the only layer. The server speaks plain HTTP, and a ClusterIP
Service is reachable from every pod in every namespace by default. So the
chart's defaults are deliberately restrictive:

| Default | Why |
|---|---|
| `networkPolicy.enabled: true` | Unusual for a chart, and the reason is the paragraph above. Default-deny ingress, scoped to this release's pod. |
| `ingress.enabled: false` | An Ingress publishes a code-execution API outside the cluster; it needs TLS, or the token crosses the network in the clear. |
| `automountServiceAccountToken: false` | Nothing to steal from the pod if the sandbox boundary is crossed. The ServiceAccount is granted no RBAC either. |
| `readOnlyRootFilesystem: true` | Shrinks what a filesystem read can reach to only what is deliberately mounted. |

Three things the NetworkPolicy does **not** do, worth knowing before you rely on it:

- **It is only enforced if your CNI implements NetworkPolicy.** Where it does not,
  the object is accepted by the API server and silently does nothing — no error, no
  warning. `helm test` includes a check that fails loudly in that case rather than
  letting it pass unnoticed.
- **It authorises a pod, not a request.** What an allowlisted pod may call is
  decided by the token it holds.
- **`kubectl port-forward` bypasses it entirely.** It tunnels through the API
  server, so anyone with `pods/portforward` permission reaches the server whatever
  the policy says.

## Blueprints and secrets

The encrypted secret store is enabled by default. The chart generates its
32-byte encryption key in a Kubernetes Secret, reuses it across upgrades, and
retains it on uninstall alongside the persistent data. Back up both the key
Secret and the volume: restoring one without the other cannot recover secrets.

Populate application secrets with the CLI, then apply blueprints from your
checkout or deployment job. Connect the CLI to the server with an admin token:

```sh
printf '%s' "$STRIPE_KEY" | submilli server secret put stripe-key
submilli server blueprint apply ./blueprints/billing.yaml
```

```yaml title="blueprints/billing.yaml"
name: billing
default: deny
secrets:
  STRIPE_KEY: { store: stripe-key }
```

Application-supplied, session-scoped credentials use `harness:` declarations.
Secret values do not belong in blueprint files or chart values.

To supply your own encryption key, set `secretStore.existingSecret` and
`secretStore.key` (default `key`). The referenced Kubernetes Secret entry must
contain base64 text encoding exactly 32 random bytes. For GitOps and offline
rendering, supply this Secret explicitly: those renderers cannot look up the
existing generated key and would otherwise generate a different one each time.
Set `secretStore.enabled: false` only when the server does not need stored secrets.

### Private packages

Package installs fetch from GitHub with the server's own token, never the
caller's. Without one they reach public repositories only. Give the server a
fine-grained personal access token with **Repository permissions → Contents:
Read-only** on the package repositories (GitHub adds Metadata: Read-only), in
a Secret:

```sh
kubectl create secret generic submilli-github --from-file=token=./github-token
```

```yaml
githubToken:
  existingSecret: submilli-github
  key: token
```

The chart mounts it at `/etc/submilli/github/<key>` and sets
`github_token_file`. The server reads the file on every install, so updating
the Secret rotates the token without a restart, once the kubelet refreshes the
mount. A fine-grained token covers one owner's repositories; a classic token
with the `repo` scope also works but can write to every repository you can.

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
sessions, installed packages, secrets, and `managed-local` named volumes all live
on the volume, under its `server/` subdirectory, and the failure mode of `false`
is silent data loss on reschedule while the failure mode of `true` is a loud
unbound-PVC error at install. Prefer the loud one.

Named volumes are declared under `config.volumes` (see `values.yaml`). A
`managed-local` volume is stored under `server/volumes/` on this claim, so it
needs nothing else; a custom `config.volume_dir` must also sit on durable storage.
A `local-path` volume names a directory inside the container, which you mount
yourself, for example from a Secret or ConfigMap through `secrets:`.

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

## Server configuration

The chart renders the server's config file, `server.yaml`, into a ConfigMap
from its values, and restarts the pod when it changes. `config:` passes any
other server setting through:

```yaml
config:
  max_execution_time: 30
  network:
    allow_ip: ["10.0.12.7"]
```

Keys the chart sets from its own values (`bind`, `port`, `max_execution_memory`,
`shutdown_grace`, `vfs_ephemeral_dir`,
`secret_store.key_file`, `allow_unauthenticated`, `github_token_file`) are
refused there, with a message naming the value to use. Entries under
`config.api_tokens` are added after the chart's two, each with a `token_file`
that a `secrets:` mount provides. `extraEnv` still overrides the file,
because the server ranks a `SUBMILLI_*` variable above it.

The chart automatically allows MCP requests addressed to its Service's short
name, namespace-qualified name, `.svc` name, and `.svc.cluster.local` name,
using `service.port`. It also allows each pod's headless DNS names using
`server.port`, and hosts from `ingress.hosts` when the Ingress is enabled.
Loopback access remains available for port-forwarding.

Add other names under `config.mcp_allowed_hosts`; these extend the generated
list. For example, use `["submilli.agents.svc.corp.example:8128"]` for a custom
cluster DNS domain. Wildcard Ingress rules need concrete hostnames in this
list: the MCP host check does not expand wildcards. Ingress host entries omit
the port, so the server accepts those names on any port. When `service.port`
or `server.port` is 80, the corresponding DNS names also get bare-host entries
because HTTP clients omit the default port; those entries likewise accept any
port for those specific names.

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

## Native HTTPS

The server uses plain HTTP by default. Supply an existing TLS Secret to enable
HTTPS on the same port:

```yaml
tls:
  enabled: true
  existingSecret: submilli-tls
  certKey: tls.crt
  privateKeyKey: tls.key
```

Create it with `kubectl create secret tls submilli-tls --cert=server.crt --key=server.key`.
The certificate must include the Service names clients use and the pod-0
headless name used by `helm test`, `<fullname>-0.<headless-service>`.
The test pods mount only the public certificate and verify it with curl's
`--cacert`; for a private issuer, include its verification chain in the PEM.
HTTPS health probes do not verify certificates; the API test does.

Certificate files are loaded at startup. Restart the StatefulSet after rotating
the Secret. `config.tls` is reserved; use the TLS values so mounts, configuration,
probes, and tests remain consistent. TLS Secret key names must differ.

`ingress.tls` controls the Ingress frontend separately. With native TLS enabled,
configure your controller's HTTPS backend protocol and certificate trust; for
ingress-nginx, the backend protocol annotation is
`nginx.ingress.kubernetes.io/backend-protocol: HTTPS`.
See the [Kubernetes guide](https://submilli.ai/docs/server/deploy-on-kubernetes)
and [CLI trust guide](https://submilli.ai/docs/server/connect-the-cli#trust-a-self-signed-server)
for certificate creation and self-signed trust approval.
