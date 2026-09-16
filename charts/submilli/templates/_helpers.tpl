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
