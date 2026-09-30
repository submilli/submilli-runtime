#!/usr/bin/env bash
#
# Smoke tests for the submilli-server image, ordered cheapest-first so a
# fundamentally broken image fails in seconds rather than after a compose boot.
#
#   scripts/docker-smoke.sh <image-ref> [oci-tarball]
#
# Runs identically against a locally-built image and against the release
# workflow's candidate, which is the point: the loop for fixing the image should
# not be "push to CI and wait". Build one locally with:
#
#   mkdir -p dist/$(docker version --format '{{.Server.Arch}}')
#   cp target/<musl-target>/release/submilli{,-server} dist/<arch>/
#   docker buildx build --load -t submilli-local:dev .
#   scripts/docker-smoke.sh submilli-local:dev
#
# The optional second argument is an OCI tarball produced by a two-platform
# `--output type=oci,dest=...` build. It is checked structurally, because arm64
# cannot be executed on a standard GitHub runner and adding QEMU to emulate an
# interpreter running a Wasm engine is not a trade worth making. This is a
# deliberate coverage limit — arm64 is proven to contain a correct binary, not
# proven to run one. A manual arm64 run on Apple silicon covers the rest.

set -euo pipefail

IMAGE="${1:?usage: docker-smoke.sh <image-ref> [oci-tarball]}"
OCI_TAR="${2:-}"

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
PROJECT="submilli-smoke-$$"
WORKDIR=$(mktemp -d)
CONTAINERS=()
VOLUMES=()

cleanup() {
    local id
    for id in "${CONTAINERS[@]:-}"; do
        [[ -n "$id" ]] && docker rm -f "$id" >/dev/null 2>&1 || true
    done
    docker compose -p "$PROJECT" down -v >/dev/null 2>&1 || true
    for id in "${VOLUMES[@]:-}"; do
        [[ -n "$id" ]] && docker volume rm -f "$id" >/dev/null 2>&1 || true
    done
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

# The server refuses to start without API tokens, so every container below gets
# a config declaring two and the tokens themselves through the environment.
# Exported because compose.yaml interpolates the same two names. The config is
# mode 0444 for the reason the store key is elsewhere: Docker does not chown a
# bind mount, and the server runs as uid 65532.
gen_token() { python3 -c 'import secrets; print(secrets.token_hex(32))'; }
SUBMILLI_ADMIN_TOKEN=$(gen_token)
SUBMILLI_USER_TOKEN=$(gen_token)
export SUBMILLI_ADMIN_TOKEN SUBMILLI_USER_TOKEN
cat >"${WORKDIR}/server.yaml" <<'YAML'
api_tokens:
  - name: smoke-admin
    role: admin
    token_env: SUBMILLI_ADMIN_TOKEN
  - name: smoke-user
    role: user
    token_env: SUBMILLI_USER_TOKEN
YAML
chmod 0444 "${WORKDIR}/server.yaml"
AUTH_ARGS=(
    -v "${WORKDIR}/server.yaml:/etc/submilli/server.yaml:ro"
    -e SUBMILLI_CONFIG=/etc/submilli/server.yaml
    -e SUBMILLI_ADMIN_TOKEN -e SUBMILLI_USER_TOKEN
)

# Each call carries the least-privileged token that works for it: admin for
# blueprints and status, user for running code.
admin_curl() { curl -H "Authorization: Bearer ${SUBMILLI_ADMIN_TOKEN}" "$@"; }
user_curl() { curl -H "Authorization: Bearer ${SUBMILLI_USER_TOKEN}" "$@"; }

# `http_code curl URL`, `http_code user_curl URL`: the status alone.
# A connection failure yields curl's own 000 rather than ending the script
# under `set -e`, so the caller's check reports what it was looking for.
http_code() { "$@" -s -o /dev/null -w '%{http_code}' || true; }

# Readiness, not liveness: a container that never answers is a failure, so this
# has a deadline rather than looping forever. /healthz is the one endpoint that
# answers without a token.
wait_for_health() {
    local port=$1 deadline=$((SECONDS + 60))
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

now() { python3 -c 'import time; print(time.time())'; }

# The blueprint store is managed over the API, so the checked-in fixture is a
# source to register rather than a directory to mount.
put_blueprint() {
    local port=$1 payload
    payload=$(python3 -c 'import json,sys; print(json.dumps({"yaml": sys.stdin.read()}))' \
        <"${REPO_ROOT}/examples/docker/blueprints/demo.yaml")
    admin_curl -sf -X PUT "http://127.0.0.1:${port}/v1/blueprints/demo" \
        -H 'content-type: application/json' -d "$payload"
}

step "1. The binary runs at all"
version=$(docker run --rm "$IMAGE" --version)
[[ -n "$version" ]] || die "--version produced no output"
ok "--version -> ${version}"

step "2. Refuses to serve without tokens"
# The image must not ship a way around authentication: with no config it exits
# non-zero and says what to configure, rather than serving an open API.
if refusal=$(docker run --rm "$IMAGE" 2>&1); then
    die "a bare \`docker run\` started a server with no API tokens"
fi
grep -q 'api_tokens' <<<"$refusal" || die "the refusal does not name api_tokens: ${refusal}"
ok "a bare run exits non-zero and names api_tokens"

step "3. Boots with no state mounts, and checks every caller"
# No volumes beyond the config file: proves the image is not secretly
# mount-dependent and that the state directories under SUBMILLI_HOME are created
# lazily by a process running as uid 65532.
bare_port=$(free_port)
bare=$(docker run -d -p "127.0.0.1:${bare_port}:8128" "${AUTH_ARGS[@]}" "$IMAGE")
CONTAINERS+=("$bare")
wait_for_health "$bare_port" || die "container never answered /healthz"
status=$(admin_curl -sf "http://127.0.0.1:${bare_port}/v1/status")
[[ "$(json_field status <<<"$status")" == "running" ]] || die "/v1/status: ${status}"
ok "/v1/status reports running with no state mounts"

anonymous=$(http_code curl "http://127.0.0.1:${bare_port}/v1/status")
[[ "$anonymous" == "401" ]] || die "/v1/status without a token got HTTP ${anonymous}, expected 401"
as_user=$(http_code user_curl "http://127.0.0.1:${bare_port}/v1/status")
[[ "$as_user" == "403" ]] || die "/v1/status with the user token got HTTP ${as_user}, expected 403"
ok "no token 401, user token on an admin route 403"

# The probe is what Docker and Compose gate on, and it runs as its own process
# with no access to the server's flags. Checking it here rather than trusting the
# HEALTHCHECK directive to be well-formed.
docker exec "$bare" /usr/local/bin/submilli-server --health-check >/dev/null \
    || die "--health-check failed against a healthy server"
ok "--health-check exits 0 inside the container"
docker rm -f "$bare" >/dev/null

step "4. SUBMILLI_* reaches the server and the probe through the image"
# The env layer is what a container is configured with, so it has to work from
# inside the image and not merely in `cargo run`. The port is the load-bearing
# one: the healthcheck resolves its address from these same variables, so a
# regression here silently breaks Compose's `condition: service_healthy` rather
# than failing loudly.
alt_port=$(free_port)
envc=$(docker run -d -p "127.0.0.1:${alt_port}:9443" \
    -e SUBMILLI_BIND=0.0.0.0 -e SUBMILLI_PORT=9443 "${AUTH_ARGS[@]}" "$IMAGE")
CONTAINERS+=("$envc")
wait_for_health "$alt_port" || die "container did not honour SUBMILLI_PORT"
bound=$(admin_curl -sf "http://127.0.0.1:${alt_port}/v1/status" | json_field bind_addr)
[[ "$bound" == "0.0.0.0:9443" ]] || die "expected bind_addr 0.0.0.0:9443, got ${bound}"
ok "SUBMILLI_BIND/SUBMILLI_PORT -> ${bound}"

deadline=$((SECONDS + 60))
health=""
while ((SECONDS < deadline)); do
    health=$(docker inspect "$envc" --format '{{.State.Health.Status}}')
    [[ "$health" == "starting" ]] || break
    sleep 1
done
[[ "$health" == "healthy" ]] || die "healthcheck reported '${health}' on a non-default port"
ok "Docker healthcheck goes healthy on a non-default port"
docker rm -f "$envc" >/dev/null

step "5. A path variable relocates state through the image"
# Distinct from the port check: this exercises the path-valued arm of the env
# layer, and proves the blueprint store honours it. The store is a revision log
# (index.json plus <name>.<rev>.yaml), not a directory of loose files, so it is
# inspected for that shape rather than for a demo.yaml.
bp_vol="${PROJECT}-bp"
VOLUMES+=("$bp_vol")
docker volume create "$bp_vol" >/dev/null
bp_port=$(free_port)
bpc=$(docker run -d -p "127.0.0.1:${bp_port}:8128" -v "${bp_vol}:/var/lib/submilli" \
    -e SUBMILLI_BLUEPRINT_DIR=/var/lib/submilli/relocated "${AUTH_ARGS[@]}" "$IMAGE")
CONTAINERS+=("$bpc")
wait_for_health "$bp_port" || die "container with SUBMILLI_BLUEPRINT_DIR never answered"
put_blueprint "$bp_port" >/dev/null
# Tolerates a missing directory so the greps below report *what* went wrong
# rather than the script dying on busybox's exit status.
listing=$(docker run --rm -v "${bp_vol}:/state" busybox ls /state/relocated 2>/dev/null || true)
grep -qE '^index\.json$' <<<"$listing" || die "blueprint store did not land in the relocated dir: ${listing}"
grep -qE '^demo\.[0-9]+\.yaml$' <<<"$listing" || die "no revision file in the relocated dir: ${listing}"
ok "SUBMILLI_BLUEPRINT_DIR relocated the store"
docker rm -f "$bpc" >/dev/null

step "6. The documented compose story works end to end"
# compose.yaml takes its two tokens from the environment exported above, and
# writes its own config file, so this also proves that path rather than the
# bind-mounted config the earlier steps used.
# SUBMILLI_IMAGE must point at the candidate. Without it compose resolves the
# published reference, which on a first release does not exist and on later ones
# silently smoke-tests the *previous* image while the candidate ships unverified
# — and the job passes either way, which is what makes that the dangerous one.
export SUBMILLI_IMAGE="$IMAGE"
docker compose -p "$PROJECT" -f "${REPO_ROOT}/compose.yaml" up -d >/dev/null 2>&1
wait_for_health 8128 || die "compose stack never answered /healthz"
ok "compose stack is serving"

created=$(put_blueprint 8128 | json_field name)
[[ "$created" == "demo" ]] || die "blueprint PUT returned: ${created}"
registered=$(admin_curl -sf http://127.0.0.1:8128/v1/status \
    | python3 -c 'import json,sys; print(",".join(json.load(sys.stdin)["blueprints"]))')
[[ "$registered" == *demo* ]] || die "demo missing from /v1/status blueprints: ${registered}"
ok "blueprint registered and listed"

# The user token is what an application holds: enough to run code, and not
# enough to touch the blueprint it runs under.
result=$(user_curl -sf -X POST http://127.0.0.1:8128/v1/execute \
    -H 'content-type: application/json' \
    -d '{"blueprint":"demo","code":"export function main(): string { return \"smoke ok\"; }"}' \
    | json_field result)
[[ "$result" == "smoke ok" ]] || die "execute returned '${result}', expected 'smoke ok'"
ok "execute round-trip returned the expected body"
as_user=$(http_code user_curl -X DELETE http://127.0.0.1:8128/v1/blueprints/demo)
[[ "$as_user" == "403" ]] || die "the user token deleting a blueprint got HTTP ${as_user}, expected 403"
ok "the user token cannot remove the blueprint it runs under"

step "7. Shutdown is graceful, not a SIGKILL"
# The only end-to-end proof that the server's signal handling reached the shipped
# image. Without it `docker stop` waits out the full grace period and the
# container exits 137; with it the process exits 0 in well under a second.
container=$(docker compose -p "$PROJECT" ps -q submilli)
start=$(now)
docker compose -p "$PROJECT" stop >/dev/null 2>&1
elapsed=$(python3 -c 'import sys,time; print(f"{time.time()-float(sys.argv[1]):.2f}")' "$start")
code=$(docker inspect "$container" --format '{{.State.ExitCode}}')
[[ "$code" == "0" ]] || die "exit code ${code} (137 means Docker had to SIGKILL it)"
python3 -c 'import sys; sys.exit(0 if float(sys.argv[1]) < 8 else 1)' "$elapsed" \
    || die "took ${elapsed}s to stop; Docker SIGKILLs at 10s"
ok "stopped in ${elapsed}s with exit code 0"
docker compose -p "$PROJECT" down -v >/dev/null 2>&1

if [[ -n "$OCI_TAR" ]]; then
    step "8. Both platforms carry a binary of their own architecture"
    python3 - "$OCI_TAR" <<'PY'
import gzip, io, json, sys, tarfile

# Read the machine type straight out of the ELF header of the binaries each
# platform's image actually contains. Comparing layer or manifest digests across
# platforms looks like it would catch a wrong-arch copy but does not: buildx
# emits distinct digests for byte-identical content, so that check passes on a
# broken image. e_machine is the fact that matters and it cannot be faked by
# build metadata.
ELF_MACHINE = {0x3E: "amd64", 0xB7: "arm64"}
WANTED = ("usr/local/bin/submilli", "usr/local/bin/submilli-server")

EXPECTED = {"linux/amd64", "linux/arm64"}

with tarfile.open(sys.argv[1]) as tar:
    def blob(digest):
        return tar.extractfile("blobs/sha256/" + digest.split(":")[1]).read()

    index = json.loads(blob(json.load(tar.extractfile("index.json"))["manifests"][0]["digest"]))

    entries = {}
    for entry in index["manifests"]:
        platform = entry.get("platform", {})
        entries[f"{platform.get('os')}/{platform.get('architecture')}"] = entry

    # Validated before any layer is opened: an attestation's layers are JSON
    # rather than tars, so extracting first would fail on the wrong thing.
    # `unknown/unknown` here means a provenance attestation slipped back in.
    extra = set(entries) - EXPECTED
    if extra:
        sys.exit(f"   FAIL  unexpected platforms in the manifest list: {sorted(extra)}")
    missing = EXPECTED - set(entries)
    if missing:
        sys.exit(f"   FAIL  missing platforms: {sorted(missing)}")

    found = {}
    for name in EXPECTED:
        binaries = {}
        for layer in json.loads(blob(entries[name]["digest"]))["layers"]:
            raw = blob(layer["digest"])
            if layer["mediaType"].endswith("gzip"):
                raw = gzip.decompress(raw)
            with tarfile.open(fileobj=io.BytesIO(raw)) as layer_tar:
                for member in layer_tar.getmembers():
                    path = member.name.lstrip("./")
                    if path in WANTED and member.isfile():
                        binaries[path] = layer_tar.extractfile(member).read(20)
        found[name] = binaries

for name, binaries in sorted(found.items()):
    want = name.split("/")[1]
    for path in WANTED:
        header = binaries.get(path)
        if header is None:
            sys.exit(f"   FAIL  {name}: {path} is missing from the image")
        if header[:4] != b"\x7fELF":
            sys.exit(f"   FAIL  {name}: {path} is not an ELF binary")
        machine = int.from_bytes(header[18:20], "little")
        got = ELF_MACHINE.get(machine, f"unknown (e_machine={machine:#x})")
        if got != want:
            sys.exit(f"   FAIL  {name}: {path} is a {got} binary")
    print(f"   ok  {name} carries {want} binaries")
PY
fi

printf '\nAll smoke checks passed against %s\n' "$IMAGE"
