#!/usr/bin/env bash
#
# Replay the public Deploy with Compose guide against the published release
# files and image.  The YAML inputs are extracted from the guide itself so a
# documentation change cannot silently leave this check exercising a different
# stack.
#
#   scripts/docker-compose-e2e.sh
#
# This is intentionally a Linux CI check.  It uses no registry credentials and
# never prints the generated environment, Compose config, or token-bearing
# container inspection output.

set -euo pipefail

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
DOC="${REPO_ROOT}/docs/part-4-server/06-deploy-with-compose.md"
PROJECT="submilli-compose-e2e-${GITHUB_RUN_ID:-$$}"
WORKDIR="$(mktemp -d)"

cleanup() {
    docker compose --env-file "$WORKDIR/.env" -p "$PROJECT" -f "$WORKDIR/compose.yaml" \
        -f "$WORKDIR/compose.override.yaml" -f "$WORKDIR/compose.store.yaml" \
        down -v >/dev/null 2>&1 || true
    rm -rf "$WORKDIR"
}
trap cleanup EXIT

die() { printf 'FAIL: %s\n' "$1" >&2; exit 1; }
ok() { printf 'ok: %s\n' "$1"; }

command -v docker >/dev/null || die "Docker is required"
docker compose version >/dev/null || die "Docker Compose is required"

extract_fence() {
    local title="$1" output="$2" occurrence="${3:-1}"
    python3 - "$DOC" "$title" "$output" "$occurrence" <<'PY'
import pathlib, re, sys

doc = pathlib.Path(sys.argv[1]).read_text()
title = re.escape(sys.argv[2])
matches = list(re.finditer(
    rf"```(?:yaml|sh)[^\n]*title=[\"']{title}[\"'][^\n]*\n(.*?)```",
    doc,
    re.DOTALL,
))
occurrence = int(sys.argv[4])
if len(matches) < occurrence:
    raise SystemExit(f"document fence not found: {sys.argv[2]} occurrence {occurrence}")
pathlib.Path(sys.argv[3]).write_text(matches[occurrence - 1].group(1))
PY
}

mkdir -p "$WORKDIR"
read -r RELEASE IMAGE < <(python3 - "$DOC" <<'PY'
import pathlib, re, sys

doc = pathlib.Path(sys.argv[1]).read_text()
start = doc.split("## Start it\n", 1)[1].split("\n## ", 1)[0]
commands = re.search(r"```sh\n(.*?)```", start, re.DOTALL).group(1)
release = re.search(r"curl -fsSLO https://raw\.githubusercontent\.com/submilli/submilli-runtime/(v[0-9.]+)/compose\.yaml", commands)
image = re.search(r"printf 'SUBMILLI_IMAGE=(ghcr\.io/submilli/submilli-runtime:([0-9.]+))\\n", commands)
assert release and image, "Start commands must specify a release and image pin"
assert release.group(1) == "v" + image.group(2), "Compose and image releases differ"
print(release.group(1), image.group(1))
PY
)
curl -fsSL "https://raw.githubusercontent.com/submilli/submilli-runtime/${RELEASE}/compose.yaml" \
    -o "$WORKDIR/compose.yaml"

# These are the exact complete/optional YAML examples in the public guide.
extract_fence "server.yaml" "$WORKDIR/server.yaml"
extract_fence "compose.override.yaml" "$WORKDIR/compose.override.yaml"
extract_fence "compose.override.yaml (additions)" "$WORKDIR/compose.store.yaml"
extract_fence "compose.override.yaml (fragment)" "$WORKDIR/tls-submilli.yaml"

# The guide deliberately leaves the application image for the operator.  Use a
# deterministic utility image only for the CI fixture; all server configuration
# remains the documented configuration.
sed -i.bak 's#image: your-application#image: curlimages/curl:8.10.1#' \
    "$WORKDIR/compose.override.yaml"
rm -f "$WORKDIR/compose.override.yaml.bak"

ADMIN_TOKEN="$(openssl rand -hex 32)"
APP_TOKEN="$(openssl rand -hex 32)"
printf 'SUBMILLI_IMAGE=%s\nSUBMILLI_SERVER_TOKEN=%s\nSUBMILLI_APP_TOKEN=%s\n' \
    "$IMAGE" "$ADMIN_TOKEN" "$APP_TOKEN" >"$WORKDIR/.env"
printf '%s\n' "$APP_TOKEN" >"$WORKDIR/app-token"
chmod 0600 "$WORKDIR/.env"
head -c 32 /dev/urandom | base64 >"$WORKDIR/submilli-store-key"
chmod 0444 "$WORKDIR/server.yaml" "$WORKDIR/app-token" "$WORKDIR/submilli-store-key"

COMPOSE=(docker compose --env-file "$WORKDIR/.env" -p "$PROJECT" -f "$WORKDIR/compose.yaml" \
    -f "$WORKDIR/compose.override.yaml" -f "$WORKDIR/compose.store.yaml")

# Validate the merged files without emitting the resolved configuration, which
# contains both bearer tokens by design.
"${COMPOSE[@]}" config --quiet
"${COMPOSE[@]}" up -d --wait submilli >/dev/null
ok "published Compose file and documented override start healthy"

SERVER_ID=$("${COMPOSE[@]}" ps -q submilli)
[[ -n "$SERVER_ID" ]] || die "Compose did not create the server container"

readonly_root=$(docker inspect "$SERVER_ID" --format '{{.HostConfig.ReadonlyRootfs}}')
[[ "$readonly_root" == true ]] || die "server root filesystem is writable"
user=$(docker inspect "$SERVER_ID" --format '{{.Config.User}}')
[[ "$user" == "65532" || "$user" == "65532:65532" ]] \
    || die "server is running as ${user:-root}, expected uid 65532"
docker inspect "$SERVER_ID" --format '{{json .HostConfig.PortBindings}}' | python3 -c '
import json, sys
assert json.load(sys.stdin) == {"8128/tcp": [{"HostIp": "127.0.0.1", "HostPort": "8128"}]}, "Unexpected published ports"
'
ok "server is non-root, read-only, and loopback-published"

api() {
    curl -fsS -H "Authorization: Bearer $ADMIN_TOKEN" "$@"
}

status=$(curl -sS -o /dev/null -w '%{http_code}' \
    http://127.0.0.1:8128/v1/status)
[[ "$status" == 401 ]] || die "missing token returned HTTP ${status}, expected 401"
status=$(curl -sS -o /dev/null -w '%{http_code}' \
    -H 'Authorization: Bearer unknown-token' http://127.0.0.1:8128/v1/status)
[[ "$status" == 401 ]] || die "unknown token returned HTTP ${status}, expected 401"
ok "missing and unknown tokens are rejected"

api -X POST http://127.0.0.1:8128/v1/secrets \
    -H 'content-type: application/json' \
    -d '{"key":"compose_e2e","value":"loaded"}' >/dev/null
ok "documented file-mounted store key is usable"

blueprint='{"yaml":"name: persisted\nvfs: per_session\ndefault: allow\n"}'
api -X PUT http://127.0.0.1:8128/v1/blueprints/persisted \
    -H 'content-type: application/json' -d "$blueprint" >/dev/null

app_curl() {
    "${COMPOSE[@]}" run --rm --no-deps --entrypoint sh app -c "set -eu; $1"
}

# The app reaches the service over submilli-net and can execute, while its
# user-scoped token cannot perform an admin mutation.
app_curl 'curl -fsS "$SUBMILLI_URL/healthz" >/dev/null'
execute_response=$(app_curl "curl -fsS -X POST \"\$SUBMILLI_URL/v1/execute\" -H \"Authorization: Bearer \$SUBMILLI_SERVER_TOKEN\" -H \"content-type: application/json\" -d '{\"blueprint\":\"persisted\",\"code\":\"export function main(): string { return \\\"compose ok\\\"; }\"}'")
python3 -c 'import json,sys
b=json.load(sys.stdin)
assert b["status"] == "finished" and b["result"] == "compose ok" and b["error"] is None
' <<<"$execute_response"
admin_code=$(app_curl "curl -sS -o /dev/null -w '%{http_code}' -X PUT \"\$SUBMILLI_URL/v1/blueprints/denied\" -H \"Authorization: Bearer \$SUBMILLI_SERVER_TOKEN\" -H \"content-type: application/json\" -d '{\"yaml\":\"name: denied\"}'")
[[ "$admin_code" == 403 ]] || die "user token admin mutation returned HTTP ${admin_code}, expected 403"
ok "application service reaches the server and is denied admin mutation"

# Replay the two TLS fragments from the guide.  The curl fixture uses the
# documented certificate mount as CURL_CA_BUNDLE, its equivalent of the guide's
# NODE_EXTRA_CA_CERTS example.
extract_fence "compose.override.yaml (fragment)" "$WORKDIR/tls-app.yaml" 2
openssl req -x509 -newkey rsa:2048 -nodes -days 1 \
    -subj '/CN=submilli' -addext 'subjectAltName=DNS:submilli' \
    -keyout "$WORKDIR/server.key" -out "$WORKDIR/server.crt" >/dev/null 2>&1
chmod 0444 "$WORKDIR/server.crt" "$WORKDIR/server.key"
sed -i.bak 's/NODE_EXTRA_CA_CERTS:/CURL_CA_BUNDLE:/' "$WORKDIR/tls-app.yaml"
rm -f "$WORKDIR/tls-app.yaml.bak"
COMPOSE_TLS=("${COMPOSE[@]}" -f "$WORKDIR/tls-submilli.yaml" -f "$WORKDIR/tls-app.yaml")
"${COMPOSE[@]}" down >/dev/null
"${COMPOSE_TLS[@]}" up -d --wait submilli >/dev/null
curl --fail --silent --show-error --noproxy submilli \
    --resolve submilli:8128:127.0.0.1 --cacert "$WORKDIR/server.crt" \
    https://submilli:8128/healthz >/dev/null
app_curl_tls() {
    "${COMPOSE_TLS[@]}" run --rm --no-deps --entrypoint sh app -c "set -eu; $1"
}
app_curl_tls 'curl -fsS "$SUBMILLI_URL/healthz" >/dev/null'
ok "documented TLS mounts enable HTTPS health checks and app trust"

"${COMPOSE_TLS[@]}" down >/dev/null
"${COMPOSE[@]}" up -d --wait submilli >/dev/null

api -sS http://127.0.0.1:8128/v1/status | grep -q persisted \
    || die "blueprint was not present before recreate"
"${COMPOSE[@]}" up -d --force-recreate --wait submilli >/dev/null
api -sS http://127.0.0.1:8128/v1/status | grep -q persisted \
    || die "blueprint did not survive container recreate"
"${COMPOSE[@]}" down >/dev/null
"${COMPOSE[@]}" up -d --wait submilli >/dev/null
api -sS http://127.0.0.1:8128/v1/status | grep -q persisted \
    || die "blueprint did not survive down/up without -v"
ok "state survives recreate and down/up"

printf 'Compose end-to-end checks passed for %s\n' "$IMAGE"
