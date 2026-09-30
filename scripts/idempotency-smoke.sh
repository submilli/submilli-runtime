#!/usr/bin/env bash
#
# End-to-end checks for `Idempotency-Key` on the session execute endpoint,
# against real binaries on a real filesystem.
#
#   scripts/idempotency-smoke.sh [path-to-submilli-server] [--slow]
#
# The integration suite drives an in-process axum router with an in-memory
# ledger. That cannot reach the things this feature actually promises: fsync
# ordering on a real filesystem, a reservation surviving `kill -9`, a client
# hanging up mid-execution, or two writers racing into a session directory that
# does not exist yet. Those need processes, so they live here.
#
# Every program calls a mock HTTP server, so the mock's hit count *is* the
# number of executions — no inferring "it did not run again" from a response
# body that a replay would produce either way.
#
# `--slow` adds the in-progress check, which has to outlast the 300s waiter
# bound and so takes ~5.5 minutes on its own. It is the only way to observe
# `503 idempotency_in_progress` end to end: the bound is a constant with no
# config knob, so lowering it means testing a binary nobody ships.

set -euo pipefail

SERVER_BIN="${1:-}"
if [[ "$SERVER_BIN" == "--slow" ]]; then SERVER_BIN=""; SLOW=1; fi
SLOW="${SLOW:-0}"
[[ "${2:-}" == "--slow" ]] && SLOW=1

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
SERVER_BIN="${SERVER_BIN:-${REPO_ROOT}/target/debug/submilli-server}"

STATE_DIR="$(mktemp -d "${TMPDIR:-/tmp}/submilli-idem-XXXXXX")"
SERVER_PID=""
MOCK_PID=""
SERVER_PORT=""
MOCK_PORT=""
FAILURES=0

cleanup() {
    [[ -n "$SERVER_PID" ]] && kill -9 "$SERVER_PID" 2>/dev/null || true
    [[ -n "$MOCK_PID" ]] && kill -9 "$MOCK_PID" 2>/dev/null || true
    # Restored in case a check left it read-only and died before its own repair.
    chmod -R u+rwX "$STATE_DIR" 2>/dev/null || true
    rm -rf "$STATE_DIR"
}
trap cleanup EXIT

step() { printf '\n== %s\n' "$1"; }
ok() { printf '   ok    %s\n' "$1"; }
skip() { printf '   skip  %s\n' "$1"; }
fail() { printf '   FAIL  %s\n' "$1" >&2; FAILURES=$((FAILURES + 1)); }

free_port() {
    python3 -c 'import socket
s = socket.socket(); s.bind(("127.0.0.1", 0)); print(s.getsockname()[1]); s.close()'
}

json_field() { python3 -c 'import json,sys
try:
    print(json.load(sys.stdin).get(sys.argv[1], ""))
except Exception:
    print("")' "$1"; }

hex_of() { python3 -c 'import sys; print(sys.argv[1].encode().hex())' "$1"; }

# ---------------------------------------------------------------- mock server

start_mock() {
    MOCK_PORT=$(free_port)
    cat >"${STATE_DIR}/mock.py" <<'PY'
import http.server, sys, threading, time, urllib.parse

hits = 0
lock = threading.Lock()


class Handler(http.server.BaseHTTPRequestHandler):
    def do_GET(self):
        global hits
        parsed = urllib.parse.urlparse(self.path)
        query = urllib.parse.parse_qs(parsed.query)
        # /count and /reset are the harness talking to itself, never a program
        # execution, so they must not move the counter this whole script reads.
        if parsed.path == "/count":
            return self.reply(str(hits))
        if parsed.path == "/reset":
            with lock:
                hits = 0
            return self.reply("reset")
        # /wait stalls without counting. A program that has to outlive the 300s
        # waiter bound cannot do it in one call — the verb helpers hardcode a 30s
        # per-request timeout — so it loops, and those loop iterations must not
        # be mistaken for repeat executions.
        if parsed.path == "/wait":
            time.sleep(float(query.get("secs", ["5"])[0]))
            return self.reply("waited")
        with lock:
            hits += 1
        if parsed.path == "/hold":
            time.sleep(float(query.get("secs", ["5"])[0]))
        self.reply("mock ok")

    def reply(self, body):
        data = body.encode()
        try:
            self.send_response(200)
            self.send_header("content-type", "text/plain")
            self.send_header("content-length", str(len(data)))
            self.end_headers()
            self.wfile.write(data)
        except (BrokenPipeError, ConnectionResetError):
            # Expected, and the point: the disconnect check abandons a request
            # mid-flight, so the guest's connection is gone by the time the
            # response is written. The hit was already counted, which is all
            # this mock is here to record. Left unhandled it prints a traceback
            # that reads like a failure in an otherwise passing run.
            pass

    def log_message(self, *args):
        pass


class Server(http.server.ThreadingHTTPServer):
    # Belt and braces for the same abandoned-connection case: anything that
    # escapes `reply` would otherwise reach the default handler, which prints a
    # traceback to stderr.
    def handle_error(self, request, client_address):
        pass


# Threading matters: a held request must not block the concurrent checks.
Server(("127.0.0.1", int(sys.argv[1])), Handler).serve_forever()
PY
    python3 "${STATE_DIR}/mock.py" "$MOCK_PORT" &
    MOCK_PID=$!
    local deadline=$((SECONDS + 15))
    while ((SECONDS < deadline)); do
        curl -sf "http://127.0.0.1:${MOCK_PORT}/count" >/dev/null 2>&1 && return 0
        sleep 0.2
    done
    printf 'mock server never came up\n' >&2
    exit 1
}

mock_hits() { curl -sf "http://127.0.0.1:${MOCK_PORT}/count"; }
mock_reset() { curl -sf "http://127.0.0.1:${MOCK_PORT}/reset" >/dev/null; }

# ---------------------------------------------------------------- the server

# The server refuses to start without an API token; one admin token in its
# environment is all these checks need, and every call sends it.
SERVER_TOKEN=$(python3 -c 'import secrets; print(secrets.token_hex(32))')
api_curl() { curl -H "Authorization: Bearer ${SERVER_TOKEN}" "$@"; }

start_server() {
    SUBMILLI_SERVER_TOKEN="$SERVER_TOKEN" \
    SUBMILLI_BIND=127.0.0.1 \
    SUBMILLI_PORT="$SERVER_PORT" \
    SUBMILLI_SESSION_STORE_DIR="${STATE_DIR}/sessions" \
    SUBMILLI_BLUEPRINT_DIR="${STATE_DIR}/blueprints" \
    SUBMILLI_VFS_SESSION_DIR="${STATE_DIR}/vfs" \
    SUBMILLI_SECRET_STORE_DIR="${STATE_DIR}/secrets" \
    SUBMILLI_PACKAGE_STORE_DIR="${STATE_DIR}/packages" \
    SUBMILLI_ALLOW_LOCALHOST=1 \
        "$SERVER_BIN" >>"${STATE_DIR}/server.log" 2>&1 &
    SERVER_PID=$!
    local deadline=$((SECONDS + 60))
    while ((SECONDS < deadline)); do
        curl -sf "http://127.0.0.1:${SERVER_PORT}/healthz" >/dev/null 2>&1 && return 0
        sleep 0.3
    done
    printf 'server never answered /healthz; see %s/server.log\n' "$STATE_DIR" >&2
    exit 1
}

stop_server() {  # graceful
    [[ -n "$SERVER_PID" ]] || return 0
    kill -TERM "$SERVER_PID" 2>/dev/null || true
    wait "$SERVER_PID" 2>/dev/null || true
    SERVER_PID=""
}

crash_server() {  # SIGKILL: no unwinding, no Drop, no flush
    [[ -n "$SERVER_PID" ]] || return 0
    kill -9 "$SERVER_PID" 2>/dev/null || true
    wait "$SERVER_PID" 2>/dev/null || true
    SERVER_PID=""
}

BASE=""

put_blueprint() {  # name yaml
    python3 -c 'import json,sys; print(json.dumps({"yaml": sys.argv[1]}))' "$2" \
        | api_curl -sf -X PUT "${BASE}/v1/blueprints/$1" \
            -H 'content-type: application/json' -d @- >/dev/null
}

open_session() {  # blueprint -> session id
    api_curl -sf -X POST "${BASE}/v1/sessions" -H 'content-type: application/json' \
        -d "{\"blueprint\":\"$1\"}" | json_field session_id
}

# Prints the HTTP status on the first line and the body on the rest, so a caller
# can assert on either without a second request.
execute() {  # session key code   (empty key = unkeyed)
    local sid=$1 key=$2 code=$3 body args=()
    body=$(python3 -c 'import json,sys; print(json.dumps({"code": sys.argv[1]}))' "$code")
    [[ -n "$key" ]] && args=(-H "Idempotency-Key: ${key}")
    api_curl -s -w $'\n%{http_code}' -X POST "${BASE}/v1/sessions/${sid}/execute" \
        -H 'content-type: application/json' "${args[@]}" -d "$body" \
        | python3 -c 'import sys
raw = sys.stdin.read()
body, _, status = raw.rpartition("\n")
print(status.strip()); print(body, end="")'
}

status_of() { printf '%s' "$1" | head -1; }
body_of() { printf '%s' "$1" | tail -n +2; }
error_of() { body_of "$1" | json_field error; }

hitter() { printf 'import { get, Response } from "submilli:http";
function main(): string {
  const r: Response = get("http://127.0.0.1:%s/hit");
  return r.body;
}' "$MOCK_PORT"; }

holder() { printf 'import { get, Response } from "submilli:http";
function main(): string {
  const r: Response = get("http://127.0.0.1:%s/hold?secs=%s");
  return r.body;
}' "$MOCK_PORT" "$1"; }

# Registers exactly one execution, then stalls for roughly `$1 * 25` seconds in
# 25s slices. One long call cannot do this: `get` hardcodes a 30s per-request
# timeout that a guest cannot raise, so the program has to loop.
staller() { printf 'import { get, Response } from "submilli:http";
function main(): string {
  const r: Response = get("http://127.0.0.1:%s/hit");
  let i: number = 0;
  while (i < %s) {
    get("http://127.0.0.1:%s/wait?secs=25");
    i = i + 1;
  }
  return r.body;
}' "$MOCK_PORT" "$1" "$MOCK_PORT"; }

ledger_dir() { printf '%s/sessions/idempotency/%s' "$STATE_DIR" "$(hex_of "$1")"; }

# ---------------------------------------------------------------- setup

[[ -x "$SERVER_BIN" ]] || {
    printf 'no server binary at %s\n\nBuild one with:\n  cargo build -p submilli-server\n' \
        "$SERVER_BIN" >&2
    exit 1
}

SERVER_PORT=$(free_port)
BASE="http://127.0.0.1:${SERVER_PORT}"
start_mock
start_server
# The mock speaks plain HTTP on loopback, which a blueprint refuses unless it
# says otherwise.
put_blueprint smoke $'name: smoke\nvfs: per_session\ndefault: allow\nallow_insecure_http: true\n'
put_blueprint brief $'name: brief\nvfs: per_session\ndefault: allow\nallow_insecure_http: true\nidle_timeout: 5s\n'
# Declares a package that is not installed, so importing it fails at resolution
# — before the runner — which is the only externally reachable pre-dispatch
# failure. Code that does not import it runs normally under this blueprint.
put_blueprint unresolvable $'name: unresolvable\nvfs: per_session\ndefault: allow\nallow_insecure_http: true\npackages:\n  - "@nonexistent-org/nonexistent-pkg"\n'
printf 'server %s | mock %s | state %s\n' "$SERVER_PORT" "$MOCK_PORT" "$STATE_DIR"

# ================================================================ tier 1
# New or reworked code with no automated coverage of this path.

step "1. A reaped session does not reach the model as program output"
# `idle_timeout: 5s` plus the reaper's 30s tick. Slow, but this is the only way
# to see the 404 the fail-closed guard exists for: the harness must raise rather
# than hand `{"error":"unknown session"}` to the model as its own result.
mock_reset
sid=$(open_session brief)
resp=$(execute "$sid" "reaped:1" "$(hitter)")
[[ "$(status_of "$resp")" == "200" ]] || fail "warm-up execute: $(status_of "$resp")"
printf '   ...waiting out idle_timeout + reaper tick (~40s)\n'
sleep 40
resp=$(execute "$sid" "reaped:2" "$(hitter)")
if [[ "$(status_of "$resp")" == "404" ]]; then
    ok "reaped session answers 404 (client must raise, not return this body)"
else
    fail "expected 404 after the reaper ran, got $(status_of "$resp"): $(body_of "$resp")"
fi

step "2. Two first writes racing into one new session directory"
# The root-fsync rework: both writers must end up durable, not just whichever
# one created the directory. Reverting it passes every automated test, so this
# is the only check that exercises the concurrent path at all.
mock_reset
sid=$(open_session smoke)
execute "$sid" "race:a" "$(hitter)" >"${STATE_DIR}/race-a.out" &
pid_a=$!
execute "$sid" "race:b" "$(hitter)" >"${STATE_DIR}/race-b.out" &
pid_b=$!
wait "$pid_a" "$pid_b"
a_status=$(head -1 "${STATE_DIR}/race-a.out")
b_status=$(head -1 "${STATE_DIR}/race-b.out")
entries=$(ls "$(ledger_dir "$sid")" 2>/dev/null | grep -c '\.json$' || true)
if [[ "$a_status" == "200" && "$b_status" == "200" && "$entries" == "2" ]]; then
    ok "both concurrent first writes landed (${entries} ledger entries)"
else
    fail "concurrent first writes: a=${a_status} b=${b_status} entries=${entries}"
fi

step "3. A client that hangs up mid-execution"
# `ClaimGuard` was rewritten after this was last checked by hand. The program
# must run exactly once, and the abandoned reservation must read as
# indeterminate rather than being cleared for a retry.
mock_reset
sid=$(open_session smoke)
body=$(python3 -c 'import json,sys; print(json.dumps({"code": sys.argv[1]}))' "$(holder 8)")
api_curl -s --max-time 2 -X POST "${BASE}/v1/sessions/${sid}/execute" \
    -H 'content-type: application/json' -H 'Idempotency-Key: disconnect:1' \
    -d "$body" >/dev/null 2>&1 || true
sleep 1
resp=$(execute "$sid" "disconnect:1" "$(holder 8)")
hits=$(mock_hits)
if [[ "$(error_of "$resp")" == "idempotency_incomplete" && "$hits" == "1" ]]; then
    ok "one execution, retry refused as incomplete"
else
    fail "disconnect: error=$(error_of "$resp") hits=${hits} (want idempotency_incomplete / 1)"
fi
sleep 8  # let the abandoned execution finish before the next check reads the mock

if [[ "$SLOW" == "1" ]]; then
    step "4. A slow execution reports in-progress, not unknown"
    mock_reset
    sid=$(open_session smoke)
    execute "$sid" "slow:1" "$(staller 14)" >"${STATE_DIR}/slow.out" &
    slow_pid=$!
    sleep 2
    printf '   ...waiting out the 300s waiter bound\n'
    resp=$(execute "$sid" "slow:1" "$(staller 14)")
    hits=$(mock_hits)
    if [[ "$(error_of "$resp")" == "idempotency_in_progress" && "$hits" == "1" ]]; then
        ok "duplicate outlived its bound and got in_progress, original still running"
    else
        fail "slow: error=$(error_of "$resp") hits=${hits} (want idempotency_in_progress / 1)"
    fi
    wait "$slow_pid" 2>/dev/null || true
else
    skip "4. in-progress check (pass --slow; takes ~5.5 min)"
fi

# ================================================================ tier 2
# Verified by hand before, but the code underneath has changed since.

step "5. A crash mid-execution leaves the key indeterminate"
mock_reset
sid=$(open_session smoke)
execute "$sid" "crash:1" "$(holder 20)" >/dev/null 2>&1 &
# Wait for the program to actually reach the mock — killing before it does would
# prove nothing. Bounded, so a failed execute reports rather than hanging.
deadline=$((SECONDS + 30))
until [[ "$(mock_hits)" == "1" ]]; do
    ((SECONDS < deadline)) || { fail "program never reached the mock before the crash"; break; }
    sleep 0.2
done
before=$(cat "$(ledger_dir "$sid")"/*.json 2>/dev/null | json_field state || true)
crash_server
start_server
resp=$(execute "$sid" "crash:1" "$(holder 20)")
hits=$(mock_hits)
if [[ "$(error_of "$resp")" == "idempotency_incomplete" && "$hits" == "1" ]]; then
    ok "reservation survived SIGKILL as '${before:-reserved}'; retry refused, nothing re-ran"
else
    fail "crash: error=$(error_of "$resp") hits=${hits} (want idempotency_incomplete / 1)"
fi

step "6. A retry after the original finished replays byte for byte"
mock_reset
sid=$(open_session smoke)
first=$(execute "$sid" "replay:1" "$(hitter)")
second=$(execute "$sid" "replay:1" "$(hitter)")
hits=$(mock_hits)
if [[ "$(body_of "$first")" == "$(body_of "$second")" && "$hits" == "1" ]]; then
    ok "identical body, one execution"
else
    fail "replay: hits=${hits}, bodies $( [[ "$(body_of "$first")" == "$(body_of "$second")" ]] && echo match || echo differ)"
fi

step "7. A concurrent duplicate waits rather than running a second time"
mock_reset
sid=$(open_session smoke)
execute "$sid" "concurrent:1" "$(holder 4)" >"${STATE_DIR}/c1.out" &
p1=$!
sleep 1  # ensure the second request arrives while the first is genuinely running
execute "$sid" "concurrent:1" "$(holder 4)" >"${STATE_DIR}/c2.out" &
p2=$!
wait "$p1" "$p2"
hits=$(mock_hits)
if [[ "$(tail -n +2 "${STATE_DIR}/c1.out")" == "$(tail -n +2 "${STATE_DIR}/c2.out")" && "$hits" == "1" ]]; then
    ok "one execution, two identical responses"
else
    fail "concurrent duplicate: hits=${hits} (want 1), bodies may differ"
fi

step "8. A completed entry still replays after a clean restart"
mock_reset
sid=$(open_session smoke)
first=$(execute "$sid" "restart:1" "$(hitter)")
stop_server
start_server
second=$(execute "$sid" "restart:1" "$(hitter)")
hits=$(mock_hits)
if [[ "$(body_of "$first")" == "$(body_of "$second")" && "$hits" == "1" ]]; then
    ok "replay survived a restart"
else
    fail "restart replay: hits=${hits} (want 1)"
fi

# ================================================================ tier 3
# Well covered automatically; cheap to confirm against real binaries.

step "9. The same key with different code is a conflict"
mock_reset
sid=$(open_session smoke)
execute "$sid" "conflict:1" "$(hitter)" >/dev/null
resp=$(execute "$sid" "conflict:1" 'function main(): string { return "other"; }')
hits=$(mock_hits)
if [[ "$(error_of "$resp")" == "idempotency_conflict" && "$hits" == "1" ]]; then
    ok "conflict refused, nothing ran"
else
    fail "conflict: error=$(error_of "$resp") hits=${hits}"
fi

step "10. A failure before dispatch leaves no entry and frees the key"
# The trigger has to be a *package resolution* failure, which happens before the
# runner is reached. An unknown import is not one: the compiler reports it as a
# compile error, which is a real outcome and is recorded and replayed on purpose.
# A package the blueprint declares but that is not installed fails earlier, with
# nothing dispatched and no effects possible.
mock_reset
sid=$(open_session unresolvable)
resp=$(execute "$sid" "predispatch:1" 'import { thing } from "@nonexistent-org/nonexistent-pkg";
function main(): string { return thing; }')
kind=$(body_of "$resp" | python3 -c 'import json,sys
try:
    print((json.load(sys.stdin).get("error") or {}).get("kind", ""))
except Exception:
    print("")')
entries=$(ls -A "$(ledger_dir "$sid")" 2>/dev/null | grep -c '\.json$' || true)
# Reusing the key with *different* code is the discriminator: had the failure
# been recorded, this would be a 409 conflict. Running proves the key is free.
resp=$(execute "$sid" "predispatch:1" "$(hitter)")
hits=$(mock_hits)
if [[ "$kind" == "package_resolution" && "$entries" == "0" \
      && "$(status_of "$resp")" == "200" && "$hits" == "1" ]]; then
    ok "nothing recorded; the key was free and the retry executed"
else
    fail "pre-dispatch: kind=${kind} entries=${entries} status=$(status_of "$resp") hits=${hits}"
fi

step "11. Deleting a session purges its ledger"
sid=$(open_session smoke)
execute "$sid" "purge:1" "$(hitter)" >/dev/null
[[ -d "$(ledger_dir "$sid")" ]] || fail "no ledger directory to purge"
api_curl -sf -X DELETE "${BASE}/v1/sessions/${sid}" >/dev/null
if [[ ! -d "$(ledger_dir "$sid")" ]]; then
    ok "ledger directory removed with the session"
else
    fail "ledger directory survived the delete"
fi

step "12. A ledger the server cannot write refuses rather than running"
if [[ "$(id -u)" == "0" ]]; then
    skip "running as root; permission bits would not be enforced"
else
    mock_reset
    sid=$(open_session smoke)
    chmod 0500 "${STATE_DIR}/sessions/idempotency"
    resp=$(execute "$sid" "readonly:1" "$(hitter)")
    chmod 0700 "${STATE_DIR}/sessions/idempotency"
    hits=$(mock_hits)
    if [[ "$(error_of "$resp")" == "idempotency_unavailable" && "$hits" == "0" ]]; then
        ok "refused with idempotency_unavailable, nothing executed"
    else
        fail "read-only ledger: error=$(error_of "$resp") hits=${hits} (want idempotency_unavailable / 0)"
    fi
fi

# ================================================================ result

printf '\n'
if ((FAILURES == 0)); then
    printf 'All idempotency checks passed.\n'
    [[ "$SLOW" == "1" ]] || printf 'Re-run with --slow to include the in-progress check.\n'
else
    printf '%d check(s) failed. Server log: %s/server.log\n' "$FAILURES" "$STATE_DIR" >&2
    exit 1
fi
