#!/usr/bin/env bash
#
# Conformance run for the container image: does the *environment-dependent* half
# of the runtime work inside distroless?
#
#   scripts/docker-conformance.sh <image-ref>
#
# This deliberately does not run the fixture suite in the container. Those 1,000+
# fixtures prove language semantics, and semantics cannot vary by environment —
# same WasmGC module, same Rust host functions, same answer on a laptop and in
# distroless. `cargo test` stays the authority there. What can vary is every
# point where the runtime reaches out to the OS, so that is what this covers:
# TLS trust, DNS, the timezone database, the sandbox filesystem, the encrypted
# secret store, the inbound MCP host guard, state across a restart, and shutdown
# while work is actually in flight.
#
# Two things are knowingly *not* covered, because a gate that quietly omits
# things reads as broader than it is:
#
#   * arm64 executes nothing here — a GitHub runner cannot run it, and emulating
#     an interpreter running a Wasm engine under QEMU is not a trade worth
#     making. scripts/docker-smoke.sh proves arm64 carries the right binary;
#     running one is a manual check on Apple silicon at each release.
#
#   * Behaviour under a container memory limit. A guest can allocate past the
#     runtime's documented 50 MB cap without being refused, and the host memory
#     it takes to get there is large enough to OOM-kill a memory-limited
#     container — so there is no correct assertion to make here yet:
#
#         # 25 doublings builds a 67 MB string, above the 50 MB cap.
#         '{"code":"export function main(): string { let s = \"x\";
#            for (let i = 0; i < 25; i++) { s = s + s; } return s.length + \"\"; }"}'
#         # --memory=2g   -> returns len 33554432, no trap
#         # --memory=512m -> container exits 137, OOMKilled
#
#     Tracked separately; add the assertion once the cap refuses it.

set -euo pipefail

IMAGE="${1:?usage: docker-conformance.sh <image-ref>}"

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
PREFIX="submilli-conf-$$"
VOLUME="${PREFIX}-state"
WORKDIR=$(mktemp -d)
CONTAINERS=()

cleanup() {
    local id
    for id in "${CONTAINERS[@]:-}"; do
        [[ -n "$id" ]] && docker rm -f "$id" >/dev/null 2>&1 || true
    done
    docker volume rm -f "$VOLUME" >/dev/null 2>&1 || true
    rm -rf "$WORKDIR"
}
trap cleanup EXIT

step() { printf '\n== %s\n' "$1"; }
ok() { printf '   ok  %s\n' "$1"; }
die() { printf '   FAIL  %s\n' "$1" >&2; exit 1; }

free_port() {
    python3 -c 'import socket
s = socket.socket(); s.bind(("127.0.0.1", 0)); print(s.getsockname()[1]); s.close()'
}

# Each call carries the least-privileged token that works for it: admin for
# blueprints, secrets and status, user for executions, sessions and MCP.
admin_curl() { curl -H "Authorization: Bearer ${SUBMILLI_ADMIN_TOKEN}" "$@"; }
user_curl() { curl -H "Authorization: Bearer ${SUBMILLI_USER_TOKEN}" "$@"; }

# /healthz is the one endpoint that answers without a token.
wait_for_health() {
    local port=$1 deadline=$((SECONDS + 90))
    while ((SECONDS < deadline)); do
        if curl -sf "http://127.0.0.1:${port}/healthz" >/dev/null 2>&1; then
            return 0
        fi
        sleep 0.5
    done
    return 1
}

json_field() {
    python3 -c 'import json,sys; print(json.load(sys.stdin).get(sys.argv[1], ""))' "$1"
}

put_blueprint() {
    python3 -c 'import json,sys; print(json.dumps({"yaml": sys.stdin.read()}))' \
        <"${REPO_ROOT}/examples/docker/blueprints/${1}.yaml" \
        | admin_curl -sf -X PUT "http://127.0.0.1:${2}/v1/blueprints/${1}" \
            -H 'content-type: application/json' -d @- >/dev/null
}

execute() {
    local port=$1 blueprint=$2 file=$3
    python3 -c 'import json,sys; print(json.dumps({"blueprint": sys.argv[1], "code": sys.stdin.read()}))' \
        "$blueprint" <"$file" \
        | user_curl -sf -X POST "http://127.0.0.1:${port}/v1/execute" \
            -H 'content-type: application/json' -d @-
}

# An MCP `initialize`, which is all that is needed to learn whether the request
# got past rmcp's DNS-rebinding guard. It carries a valid token so that the
# status it reports is the guard's verdict and not the token check's.
mcp_initialize_status() {
    local port=$1 host=$2
    user_curl -s -o /dev/null -w '%{http_code}' -X POST "http://127.0.0.1:${port}/mcp/conformance" \
        -H "Host: ${host}" \
        -H 'content-type: application/json' \
        -H 'accept: application/json, text/event-stream' \
        -d '{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-06-18","capabilities":{},"clientInfo":{"name":"conformance","version":"0"}}}'
}

# Mode 0444 rather than the conventional 0600: compose's uid/gid/mode secret
# attributes are ignored outside swarm and Docker does not chown bind mounts, so
# a root-owned 0600 key is unreadable by uid 65532 — and the store fails boot
# rather than degrading.
printf '%s' "$(head -c 32 /dev/urandom | base64)" >"${WORKDIR}/store-key"
chmod 0444 "${WORKDIR}/store-key"
docker volume create "$VOLUME" >/dev/null

# The server refuses to start without API tokens. The config names the
# environment variables they arrive in; it is 0444 for the same reason as the
# store key above.
gen_token() { python3 -c 'import secrets; print(secrets.token_hex(32))'; }
SUBMILLI_ADMIN_TOKEN=$(gen_token)
SUBMILLI_USER_TOKEN=$(gen_token)
export SUBMILLI_ADMIN_TOKEN SUBMILLI_USER_TOKEN
cat >"${WORKDIR}/server.yaml" <<'YAML'
api_tokens:
  - name: conformance-admin
    role: admin
    token_env: SUBMILLI_ADMIN_TOKEN
  - name: conformance-user
    role: user
    token_env: SUBMILLI_USER_TOKEN
YAML
chmod 0444 "${WORKDIR}/server.yaml"
AUTH_ARGS=(
    -v "${WORKDIR}/server.yaml:/etc/submilli/server.yaml:ro"
    -e SUBMILLI_CONFIG=/etc/submilli/server.yaml
    -e SUBMILLI_ADMIN_TOKEN -e SUBMILLI_USER_TOKEN
)

PORT=$(free_port)
MAIN=$(docker run -d --read-only \
    -p "127.0.0.1:${PORT}:8128" \
    -v "${VOLUME}:/var/lib/submilli" \
    --tmpfs /tmp:mode=1777 \
    -v "${WORKDIR}/store-key:/run/secrets/store-key:ro" \
    -e SUBMILLI_SECRET_STORE_KEY_FILE=/run/secrets/store-key \
    -e SUBMILLI_BIND=0.0.0.0 -e SUBMILLI_PORT=8128 \
    -e SUBMILLI_MCP_ALLOWED_HOSTS=submilli.internal \
    -e SUBMILLI_SHUTDOWN_GRACE=3 \
    "${AUTH_ARGS[@]}" \
    "$IMAGE")
CONTAINERS+=("$MAIN")
wait_for_health "$PORT" || die "the conformance container never answered /healthz"

step "1. The encrypted secret store round-trips against a mounted key file"
# Storing proves uid 65532 read a 0444 root-owned file and the cipher was built
# from it — but it only ever *encrypts*, so on its own it cannot tell a correct
# key from any other 32 bytes.
admin_curl -sf -X POST "http://127.0.0.1:${PORT}/v1/secrets" -H 'content-type: application/json' \
    -d '{"key":"conformance_key","value":"conformance-value"}' >/dev/null \
    || die "storing a secret failed; the mounted key file was not usable"
ok "secret stored through a file-mounted key"

# The decrypt half. Registering `conformance` runs verify_secrets over its
# `store:`-sourced declaration, which reads and opens the sealed file — so a 2xx
# here is the mounted key actually decrypting, not merely being present.
#
# This is where the store is proven because it is the only place it *can* be:
# `secrets.get` is refused to main-authored code whatever the policy says, so no
# program executed through /v1/execute can read a value back. That is the
# carve-out working as designed, not a hole to route around.
put_blueprint conformance "$PORT" \
    || die "registering a blueprint with a store-backed secret failed; the key did not decrypt"
ok "sealed value decrypted through the mounted key"
put_blueprint conformance-ephemeral "$PORT"

step "2. Every OS-facing surface, in one round-trip"
result=$(execute "$PORT" conformance "${REPO_ROOT}/examples/docker/conformance.ts" | json_field result)
expected="conformance ok: tls, tzdata, vfs"
[[ "$result" == "$expected" ]] || die "conformance program returned '${result}'"
ok "$result"

step "3. The ephemeral VFS root is the tmpfs, not the volume"
# Under a read-only root filesystem a write that landed anywhere but the tmpfs
# would fail outright, so success plus absence from the volume pins it down.
# shellcheck disable=SC2016  # `${...}` here is a TypeScript template literal
scratch=$(user_curl -sf -X POST "http://127.0.0.1:${PORT}/v1/execute" -H 'content-type: application/json' \
    -d '{"blueprint":"conformance-ephemeral","code":"import fs from \"submilli:fs\";\nexport function main(): string {\n  assert(fs.info().mode === \"ephemeral\", `mode ${fs.info().mode}`);\n  fs.writeText(\"/scratch-marker.txt\", \"ephemeral-ok\");\n  return fs.readText(\"/scratch-marker.txt\")!;\n}\n"}' \
    | json_field result)
[[ "$scratch" == "ephemeral-ok" ]] || die "ephemeral write returned '${scratch}'"
strays=$(docker run --rm -v "${VOLUME}:/state" busybox find /state -name 'scratch-marker.txt' | wc -l)
((strays == 0)) || die "the ephemeral write landed on the state volume"
ok "ephemeral write succeeded and left nothing on the volume"

step "4. The inbound MCP host guard admits the configured host and no other"
# The likeliest failure in the whole image, and the one nobody expects: rmcp's
# DNS-rebinding guard accepts only loopback by default, so an agent reaching the
# container by service name, published port, or proxy is rejected out of the box.
# The guard is separate from the token check and runs after it, so both requests
# carry a valid token and differ only in `Host`.
allowed=$(mcp_initialize_status "$PORT" submilli.internal)
[[ "$allowed" == "200" ]] || die "the configured MCP host got HTTP ${allowed}"
refused=$(mcp_initialize_status "$PORT" evil.example.com)
[[ "$refused" == "403" ]] || die "an unlisted MCP host got HTTP ${refused}, expected 403"
ok "configured host 200, unlisted host 403"

step "5. Blueprints and session files survive a restart"
session=$(user_curl -sf -X POST "http://127.0.0.1:${PORT}/v1/sessions" -H 'content-type: application/json' \
    -d '{"blueprint":"conformance"}' | json_field session_id)
[[ -n "$session" ]] || die "could not open a session"
user_curl -sf -X POST "http://127.0.0.1:${PORT}/v1/sessions/${session}/execute" \
    -H 'content-type: application/json' \
    -d '{"code":"import fs from \"submilli:fs\";\nexport function main(): string {\n  fs.writeText(\"/survivor.txt\", \"before restart\");\n  return \"written\";\n}\n"}' >/dev/null \
    || die "could not write into the session VFS"

docker restart "$MAIN" >/dev/null
wait_for_health "$PORT" || die "the container did not come back after a restart"

listed=$(admin_curl -sf "http://127.0.0.1:${PORT}/v1/status" \
    | python3 -c 'import json,sys; print(",".join(json.load(sys.stdin)["blueprints"]))')
[[ "$listed" == *conformance* ]] || die "blueprints did not survive the restart: ${listed}"
survivor=$(user_curl -sf -X POST "http://127.0.0.1:${PORT}/v1/sessions/${session}/execute" \
    -H 'content-type: application/json' \
    -d '{"code":"import fs from \"submilli:fs\";\nexport function main(): string {\n  return fs.readText(\"/survivor.txt\")!;\n}\n"}' \
    | json_field result)
[[ "$survivor" == "before restart" ]] || die "session VFS did not survive the restart: '${survivor}'"
ok "blueprints listed and the session's file read back intact"

step "6. Shutdown returns while an execution is still running"
# The scenario an idle-container test would falsely certify. `serve()` cannot
# cancel an interpreter loop that never yields, so this is the only check that
# exercises the stages after the drain rather than the drain alone.
user_curl -s -m 300 -X POST "http://127.0.0.1:${PORT}/v1/execute" -H 'content-type: application/json' \
    -d '{"blueprint":"conformance","code":"export function main(): number {\n  let acc = 0;\n  for (let i = 0; i < 200000000; i++) {\n    acc = acc + i;\n  }\n  return acc;\n}\n"}' \
    >/dev/null 2>&1 &
sleep 3
curl -sf "http://127.0.0.1:${PORT}/healthz" >/dev/null || die "the server died before the stop test began"

start=$(python3 -c 'import time; print(time.time())')
# `-t 10` pins Docker's own SIGKILL fallback rather than inheriting the daemon's
# default, which is not 10s everywhere — Docker Desktop ships containers with a
# 1s StopTimeout, under which the kill lands before any drain can finish and the
# result says more about the host than about the image.
docker stop -t 10 "$MAIN" >/dev/null
elapsed=$(python3 -c 'import sys,time; print(f"{time.time()-float(sys.argv[1]):.2f}")' "$start")
python3 -c 'import sys; sys.exit(0 if float(sys.argv[1]) < 8 else 1)' "$elapsed" \
    || die "took ${elapsed}s with an execution in flight; Docker SIGKILLs at 10s"
code=$(docker inspect "$MAIN" --format '{{.State.ExitCode}}')
# 137 means Docker had to SIGKILL: the process outlived its own budget and an
# orchestrator would read a routine stop as a crash.
[[ "$code" == "0" ]] || die "exited ${code} with an execution in flight, expected 0"
ok "stopped in ${elapsed}s with exit code 0, mid-execution"

step "7. Without the allowlist, the same host is refused"
# Proves step 4 passed because SUBMILLI_MCP_ALLOWED_HOSTS admitted the host, not
# because the guard was off. The service is built once per blueprint and cached,
# so this needs its own container rather than an env change.
guard_port=$(free_port)
guarded=$(docker run -d --read-only \
    -p "127.0.0.1:${guard_port}:8128" \
    -v "${VOLUME}:/var/lib/submilli" \
    --tmpfs /tmp:mode=1777 \
    -e SUBMILLI_BIND=0.0.0.0 -e SUBMILLI_PORT=8128 \
    "${AUTH_ARGS[@]}" \
    "$IMAGE")
CONTAINERS+=("$guarded")
wait_for_health "$guard_port" || die "the allowlist-free container never answered"
without=$(mcp_initialize_status "$guard_port" submilli.internal)
[[ "$without" == "403" ]] || die "an unconfigured host got HTTP ${without}, expected 403"
loopback=$(mcp_initialize_status "$guard_port" 127.0.0.1)
[[ "$loopback" == "200" ]] || die "loopback got HTTP ${loopback}; local access must never be dropped"
ok "unconfigured host 403, loopback still 200"

printf '\nConformance passed against %s\n' "$IMAGE"
