{{/*
Chart name, overridable. Truncated to 63 characters because these values land in
DNS-1123 resource names and Kubernetes labels, both of which cap there.
*/}}
{{- define "submilli.name" -}}
{{- default .Chart.Name .Values.nameOverride | trunc 63 | trimSuffix "-" -}}
{{- end -}}

{{/*
Fully qualified resource name. `fullnameOverride` wins; otherwise the release
name is used alone when it already contains the chart name, so a conventional
`helm install submilli submilli/submilli-runtime` does not produce `submilli-submilli-runtime`.
*/}}
{{- define "submilli.fullname" -}}
{{- if .Values.fullnameOverride -}}
{{- .Values.fullnameOverride | trunc 63 | trimSuffix "-" -}}
{{- else -}}
{{- $name := default .Chart.Name .Values.nameOverride -}}
{{- if contains $name .Release.Name -}}
{{- .Release.Name | trunc 63 | trimSuffix "-" -}}
{{- else -}}
{{- printf "%s-%s" .Release.Name $name | trunc 63 | trimSuffix "-" -}}
{{- end -}}
{{- end -}}
{{- end -}}

{{- define "submilli.chart" -}}
{{- printf "%s-%s" .Chart.Name .Chart.Version | replace "+" "_" | trunc 63 | trimSuffix "-" -}}
{{- end -}}

{{/*
Selector labels — the identity labels only.

A StatefulSet's `spec.selector.matchLabels` is immutable after creation, so this
set must never gain a value that changes between releases. Putting
`app.kubernetes.io/version` or `helm.sh/chart` in here makes every appVersion
bump fail `helm upgrade` with "field is immutable", recoverable only by deleting
the StatefulSet. That is the reason this is split from `submilli.labels` rather
than being one helper.
*/}}
{{- define "submilli.selectorLabels" -}}
app.kubernetes.io/name: {{ include "submilli.name" . }}
app.kubernetes.io/instance: {{ .Release.Name }}
{{- end -}}

{{/*
Full metadata labels. Safe to change between releases; applied to
`metadata.labels`, never to a selector.
*/}}
{{- define "submilli.labels" -}}
helm.sh/chart: {{ include "submilli.chart" . }}
{{ include "submilli.selectorLabels" . }}
{{- if .Chart.AppVersion }}
app.kubernetes.io/version: {{ .Chart.AppVersion | quote }}
{{- end }}
app.kubernetes.io/managed-by: {{ .Release.Service }}
{{- end -}}

{{- define "submilli.serviceAccountName" -}}
{{- if .Values.serviceAccount.create -}}
{{- default (include "submilli.fullname" .) .Values.serviceAccount.name -}}
{{- else -}}
{{- default "default" .Values.serviceAccount.name -}}
{{- end -}}
{{- end -}}

{{/*
Memory request/limit, in MiB, derived from the same knob that caps a single
execution. One input rather than two independent numbers, so raising the
execution cap without raising the container limit — the classic way to turn a
tunable into an OOMKill — is not expressible.

Headroom, not a bound. `maxMemoryMB` caps live GC heap; peak RSS during an
allocation-heavy execution was measured at more than 12x that, and the server
has no max-concurrency setting, so concurrent executions near the cap can still
exceed the limit. SUB-704 tracks making the cap mean something an operator can
size from.
*/}}
{{- define "submilli.memoryMi" -}}
{{- $r := .Values.resources -}}
{{- printf "%dMi" (add $r.baselineMi (mul $r.headroomMultiplier .Values.execution.maxMemoryMB)) -}}
{{- end -}}

{{/*
Pod termination grace. The server's own drain budget plus the ~1.3s its runtime
and telemetry teardown add on top; undershooting means the drain is SIGKILLed
partway through, which is the outcome `shutdownGrace` exists to avoid.
*/}}
{{- define "submilli.terminationGrace" -}}
{{- add .Values.server.shutdownGrace 3 -}}
{{- end -}}

{{/*
Mount path for a referenced Kubernetes Secret. A blueprint's `secrets:` block
names this exact path in a `file:` source, so it is a contract between two
separate values maps rather than an implementation detail — changing it breaks
every blueprint that reads a secret.
*/}}
{{- define "submilli.secretMountPath" -}}
/etc/submilli/secrets
{{- end -}}

{{/*
Read-only directory the server reconciles blueprints from at boot. Distinct from
the writable store on the volume; pointing both at one path would make the store
read-only forever.
*/}}
{{- define "submilli.seedPath" -}}
/etc/submilli/blueprints
{{- end -}}

{{- define "submilli.homePath" -}}
/var/lib/submilli
{{- end -}}

{{/*
Name of the headless Service that governs the StatefulSet.

Truncated to 63 before the suffix, not after: `submilli.fullname` already
truncates to 63, so appending "-headless" to a long release name produces a name
the API server rejects as an invalid DNS label at install time. Four places have
to agree on this string — the Service, the StatefulSet's serviceName, and both
test hooks' base URL — so it lives here rather than being rebuilt at each.
*/}}
{{- define "submilli.headlessName" -}}
{{- printf "%s-headless" (include "submilli.fullname" . | trunc 54 | trimSuffix "-") -}}
{{- end }}

{{/*
Name of the ConfigMap holding the server's config file. Truncated before the
suffix for the same reason as `submilli.headlessName`.
*/}}
{{- define "submilli.configName" -}}
{{- printf "%s-config" (include "submilli.fullname" . | trunc 56 | trimSuffix "-") -}}
{{- end -}}

{{/*
Directory the config file is mounted in, and the file the server is pointed at.
*/}}
{{- define "submilli.configMountPath" -}}
/etc/submilli/config
{{- end -}}

{{/*
Name of the Secret holding the API tokens: the operator's when
`auth.existingSecret` is set, otherwise the one templates/secret-auth.yaml
generates. The StatefulSet, the Secret itself, NOTES.txt, and the API test hook
all have to agree on it.
*/}}
{{- define "submilli.authSecretName" -}}
{{- if .Values.auth.existingSecret -}}
{{- .Values.auth.existingSecret -}}
{{- else -}}
{{- printf "%s-auth" (include "submilli.fullname" . | trunc 58 | trimSuffix "-") -}}
{{- end -}}
{{- end -}}

{{/*
Directory the token Secret is mounted in. The config file names token files
under it, so the ConfigMap and the pod spec must agree on this path.
*/}}
{{- define "submilli.authMountPath" -}}
/etc/submilli/auth
{{- end -}}

{{/*
The server's config file (server.yaml), as YAML text.

Chart-owned keys come from dedicated values; everything else passes through from
`config:`. The file holds paths to tokens and keys, never the material itself,
which is what lets it live in a ConfigMap.

`port` is deliberately absent: SUBMILLI_PORT stays in the pod environment (see
the StatefulSet) and an explicit environment variable outranks this file, so a
`port` here could only ever be a value that is silently ignored.

The StatefulSet hashes this output for its rollout trigger, so it must be
deterministic: `toYaml` sorts map keys, and nothing random may be added here.
*/}}
{{- define "submilli.serverConfig" -}}
{{- $extra := deepCopy (.Values.config | default dict) -}}
{{- /*
Refused rather than merged. Each of these either also sizes something in the
pod spec (the memory limit, the termination grace, a mount path), so a second
source for it would let the two drift apart, or is outranked by the environment
and would do nothing.
*/ -}}
{{- $owned := dict
      "bind" "set server.bind instead"
      "port" "set server.port instead; it also drives the container port, the Services, and the NetworkPolicy"
      "vfs_ephemeral_dir" "the chart fixes it at /tmp, the only writable path outside the state volume"
      "max_execution_memory" "set execution.maxMemoryMB instead; the pod's memory limit is derived from it"
      "shutdown_grace" "set server.shutdownGrace instead; terminationGracePeriodSeconds is derived from it"
      "blueprint_seed_dir" "the chart sets it when blueprints is non-empty"
      "allow_unauthenticated" "set auth.enabled=false instead"
      "github_token_file" "set githubToken.existingSecret and githubToken.key instead, which also mount the token"
    -}}
{{- range $key, $instead := $owned -}}
{{-   if hasKey $extra $key -}}
{{-     fail (printf "config.%s is owned by the chart: %s" $key $instead) -}}
{{-   end -}}
{{- end -}}
{{- if hasKey $extra "secret_store" -}}
{{-   if not (kindIs "map" $extra.secret_store) -}}
{{-     fail "config.secret_store must be a map" -}}
{{-   end -}}
{{-   if hasKey $extra.secret_store "key_file" -}}
{{-     fail "config.secret_store.key_file is owned by the chart: set secretStore.existingSecret and secretStore.key instead, which also mount the key" -}}
{{-   end -}}
{{- end -}}
{{- if and .Values.auth.enabled (eq .Values.auth.adminTokenKey .Values.auth.userTokenKey) -}}
{{-   fail "auth.adminTokenKey and auth.userTokenKey must differ: one key would give both roles the same token, which the server refuses" -}}
{{- end -}}
{{- $config := dict "bind" .Values.server.bind "max_execution_memory" .Values.execution.maxMemoryMB "shutdown_grace" .Values.server.shutdownGrace -}}
{{- /*
SUBMILLI_HOME relocates five of the server's six state directories (kept under
its server/ subdirectory; an older volume is moved into that shape on the first
boot). The ephemeral VFS root is the sixth and still defaults to the OS temp
dir, so under readOnlyRootFilesystem it must be pointed at a writable volume.
Omitting this breaks /v1/execute — the product — while /healthz stays green,
which is why the chart's own test exercises execute rather than health alone.
*/ -}}
{{- $_ := set $config "vfs_ephemeral_dir" "/tmp" -}}
{{- if .Values.blueprints -}}
{{- /*
The read-only source the server reconciles from at boot. A different directory
from the writable store on the state volume — pointing both at one path would
make the store read-only forever.
*/ -}}
{{-   $_ := set $config "blueprint_seed_dir" (include "submilli.seedPath" .) -}}
{{- end -}}
{{- if and .Values.secretStore.enabled .Values.secretStore.existingSecret -}}
{{- /*
A file, never an env var holding the key itself: environment is readable
through /proc/self/environ.
*/ -}}
{{-   $_ := set $config "secret_store" (dict "key_file" (printf "/etc/submilli/secret-store/%s" .Values.secretStore.key)) -}}
{{- end -}}
{{- if .Values.githubToken.existingSecret -}}
{{-   $_ := set $config "github_token_file" (printf "/etc/submilli/github/%s" .Values.githubToken.key) -}}
{{- end -}}
{{- if .Values.auth.enabled -}}
{{- /*
`token_file`, for the same reason as the secret-store key. The chart's two
entries come first and their names are reserved, so an operator's extra entry
can add a caller but cannot shadow or replace the tokens NOTES.txt and
`helm test` rely on.
*/ -}}
{{-   $authDir := include "submilli.authMountPath" . -}}
{{-   $tokens := list
        (dict "name" "admin" "role" "admin" "token_file" (printf "%s/%s" $authDir .Values.auth.adminTokenKey))
        (dict "name" "user" "role" "user" "token_file" (printf "%s/%s" $authDir .Values.auth.userTokenKey))
      -}}
{{-   $extraTokens := get $extra "api_tokens" | default list -}}
{{-   if not (kindIs "slice" $extraTokens) -}}
{{-     fail "config.api_tokens must be a list of entries with name, role, and token_file" -}}
{{-   end -}}
{{-   range $entry := $extraTokens -}}
{{-     if not (kindIs "map" $entry) -}}
{{-       fail "config.api_tokens entries must be maps with name, role, and token_file" -}}
{{-     end -}}
{{-     $name := get $entry "name" -}}
{{-     if not (and (kindIs "string" $name) (trim $name)) -}}
{{-       fail "config.api_tokens: every entry needs a name, written as a non-empty string" -}}
{{-     end -}}
{{-     if has $name (list "admin" "user") -}}
{{-       fail (printf "config.api_tokens: the name %q is reserved for the chart's own token; pick another name" $name) -}}
{{-     end -}}
{{-     $tokens = append $tokens $entry -}}
{{-   end -}}
{{-   $_ := set $config "api_tokens" $tokens -}}
{{- else -}}
{{-   if hasKey $extra "api_tokens" -}}
{{-     fail "config.api_tokens is set while auth.enabled is false: the server refuses tokens together with allow_unauthenticated. Set auth.enabled=true, or remove config.api_tokens" -}}
{{-   end -}}
{{-   $_ := set $config "allow_unauthenticated" true -}}
{{- end -}}
{{- range $key, $value := $extra -}}
{{-   if eq $key "secret_store" -}}
{{- /*   Merged one level down, so `dir` or `key_env` can sit beside the chart's `key_file`. */ -}}
{{-     $store := get $config "secret_store" | default dict -}}
{{-     range $storeKey, $storeValue := $value -}}
{{-       $_ := set $store $storeKey $storeValue -}}
{{-     end -}}
{{-     $_ := set $config "secret_store" $store -}}
{{-   else if ne $key "api_tokens" -}}
{{-     $_ := set $config $key $value -}}
{{-   end -}}
{{- end -}}
{{- $box := dict "v" $config -}}
{{- include "submilli.normalizeNumbers" $box -}}
{{- toYaml $box.v -}}
{{- end -}}

{{/*
Rewrites whole-number floats as integers, recursively, in place.

Numbers in a values file reach templates as float64, and a large one can print
in exponent form (`1e+10`). Every numeric key in the server's config is an
unsigned integer, and it refuses that spelling at boot — for exactly the
operators raising a limit well above its default.

Templates cannot return values, so the argument is a one-key dict,
`dict "v" <value>`, and the result is read back from its `v`. Lists are rebuilt
rather than edited because a template cannot assign to a list element.
*/}}
{{- define "submilli.normalizeNumbers" -}}
{{- $value := .v -}}
{{- if kindIs "map" $value -}}
{{-   range $key, $item := $value -}}
{{-     $box := dict "v" $item -}}
{{-     include "submilli.normalizeNumbers" $box -}}
{{-     $_ := set $value $key $box.v -}}
{{-   end -}}
{{- else if kindIs "slice" $value -}}
{{-   $items := list -}}
{{-   range $item := $value -}}
{{-     $box := dict "v" $item -}}
{{-     include "submilli.normalizeNumbers" $box -}}
{{-     $items = append $items $box.v -}}
{{-   end -}}
{{-   $_ := set . "v" $items -}}
{{- else if kindIs "float64" $value -}}
{{- /*   The bounds keep the conversion inside int64; past them the value is left for the server to reject. */ -}}
{{-   if and (eq (floor $value) $value) (lt $value 9e18) (gt $value -9e18) -}}
{{-     $_ := set . "v" (int64 $value) -}}
{{-   end -}}
{{- end -}}
{{- end -}}
