#!/usr/bin/env bash
#
# End-to-end checks that a guest path cannot leave its VFS root, and that a
# blueprint cannot choose which host directory that root is.
#
#   scripts/vfs-containment-smoke.sh
#
# The Rust suites cover both properties, but they drive an in-process router and
# call the resolver directly. That cannot reach what an operator and an attacker
# actually touch: a config file refused at boot, a symlink planted on a real
# filesystem between two real processes, the MCP transport's own path into the
# file tools, or the CLI. Those need processes, so they live here.
#
# Everything lands in a temp dir removed on exit. Nothing touches ~/.submilli.
#
# No `set -e`: every check is counted rather than fatal, so one failure still
# leaves you with the full picture. The exit status is the tally.
set -uo pipefail
PORT=${PORT:-8199}

V=$(mktemp -d); pass=0; fail=0
cleanup() { [ -f "$V/pid" ] && kill "$(cat "$V/pid")" 2>/dev/null
            sleep 0.3; rm -rf "$V"; }
trap cleanup EXIT

ok()   { pass=$((pass+1)); printf '  \033[32mPASS\033[0m  %s\n' "$1"; }
bad()  { fail=$((fail+1)); printf '  \033[31mFAIL\033[0m  %s\n         got: %s\n' "$1" "${2:0:200}"; }
# expect_in <needle> <haystack> <label>
expect_in() { case "$2" in *"$1"*) ok "$3";; *) bad "$3" "$2";; esac; }
expect_not() { case "$2" in *"$1"*) bad "$3" "$2";; *) ok "$3";; esac; }

# The server refuses to start without an API token. It reads the admin token
# from SUBMILLI_SERVER_TOKEN, and every config below adds a `user` token from a
# file, so the boot refusals further down are about the volume map and not
# about a missing token. Registration and /v1/volumes are admin calls; running
# code and MCP use the user token.
SUBMILLI_SERVER_TOKEN=$(python3 -c 'import secrets; print(secrets.token_hex(32))')
export SUBMILLI_SERVER_TOKEN
USER_TOKEN=$(python3 -c 'import secrets; print(secrets.token_hex(32))')
export USER_TOKEN
printf '%s\n' "$USER_TOKEN" > "$V/user-token"

# ---------------------------------------------------------------- helpers
config() { # config <volumes-block-file> <blueprint_dir>
  cat <<EOF
port: $PORT
api_tokens:
  - { name: smoke-user, role: user, token_file: $V/user-token }
blueprint_dir: ${2:-$V/home/blueprints}
session_store_dir: $V/home/sessions
vfs_session_dir: $V/home/vfs/sessions
package_store_dir: $V/home/packages
secret_store:
  dir: $V/home/secrets
volumes:
$1
EOF
}
# boot <config-file> -> prints the refusal, or "STARTED" if the server came up
boot() {
  local out; out=$("$SRV" --config "$1" 2>&1 &
                   p=$!; sleep 2.5; kill $p 2>/dev/null; wait $p 2>/dev/null)
  case "$out" in *"listening"*) echo "STARTED (no refusal)";; *) echo "$out" | grep -i "^Error" | head -1;; esac
}
put() { # put <port> <name> <yaml>  -> "<code> <message-or-body>"
  python3 - "$1" "$2" "$3" <<'PY'
import json, os, sys, urllib.request, urllib.error
port, name, yaml = sys.argv[1], sys.argv[2], sys.argv[3]
req = urllib.request.Request(f"http://localhost:{port}/v1/blueprints/{name}", method="PUT",
    data=json.dumps({"yaml": yaml}).encode(), headers={"content-type": "application/json",
    "authorization": "Bearer " + os.environ["SUBMILLI_SERVER_TOKEN"]})
try:
    with urllib.request.urlopen(req) as r: print(r.status, r.read().decode())
except urllib.error.HTTPError as e: print(e.code, json.loads(e.read()).get("message", ""))
PY
}
run() { # run <port> <blueprint> <code> -> the result, or "ERROR: <message>"
  python3 - "$1" "$2" "$3" <<'PY'
import json, os, sys, urllib.request
port, bp, code = sys.argv[1], sys.argv[2], sys.argv[3]
req = urllib.request.Request(f"http://localhost:{port}/v1/execute", method="POST",
    data=json.dumps({"blueprint": bp, "code": code}).encode(), headers={"content-type": "application/json",
    "authorization": "Bearer " + os.environ["USER_TOKEN"]})
body = json.load(urllib.request.urlopen(req))
err = body.get("error")
print(("ERROR: " + str(err.get("message"))) if err else (body.get("result") or ""))
PY
}
mcp() { # mcp <port> <blueprint> <tool> <args-json>
  # TRAP: the endpoint needs an `initialize` handshake, the session id echoed back
  # in a header, an SSE-capable Accept header, and the FIRST `data:` frame is an
  # empty keepalive — hence `grep '^data: {'`.
  local port=$1 bp=$2 sid
  sid=$(curl -s -D "$V/h.txt" -o /dev/null -X POST "localhost:$port/mcp/$bp" \
    -H "authorization: Bearer $USER_TOKEN" \
    -H 'content-type: application/json' -H 'accept: application/json, text/event-stream' \
    -d '{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-06-18","capabilities":{},"clientInfo":{"name":"verify","version":"0"}}}'
    grep -i '^mcp-session-id:' "$V/h.txt" | tr -d '\r' | awk '{print $2}')
  curl -s -X POST "localhost:$port/mcp/$bp" -H 'content-type: application/json' \
    -H "authorization: Bearer $USER_TOKEN" \
    -H 'accept: application/json, text/event-stream' -H "mcp-session-id: $sid" \
    -d "{\"jsonrpc\":\"2.0\",\"id\":9,\"method\":\"tools/call\",\"params\":{\"name\":\"$3\",\"arguments\":$4}}" \
  | grep '^data: {' | head -1 | sed 's/^data: //' \
  | python3 -c "
import json,sys
d=json.load(sys.stdin); r=d.get('result')
if r is None: print('ERROR: ' + str(d.get('error',{}).get('message')))
else: print(json.dumps(r.get('structuredContent', r)))"
}
ALLOW='default: allow
permissions:
  main: []
'

# ---------------------------------------------------------------- fixtures
mkdir -p "$V/home" "$V/volumes/work" "$V/outside" "$V/elsewhere"
W="$V/volumes/work"
echo -n OUTSIDE-SECRET > "$V/outside/secret.txt"
mkdir -p "$W/sub" "$W/pkgs/a" "$W/pkgs/b"
echo -n INSIDE  > "$W/sub/ok.txt"
echo -n SIBLING > "$W/pkgs/b/lib.txt"
ln -sfn "$V/outside" "$W/escape"        # absolute, leaves the volume
ln -sfn "../../outside" "$W/esc_rel"    # relative, leaves the volume
ln -sfn "./sub"  "$W/inner"             # stays inside
ln -sfn "../b"   "$W/pkgs/a/sibling"    # up one, still inside (the npm/pnpm shape)
ln -sfn "$W/pkgs/b" "$W/pkgs/a/abs"     # absolute but resolves inside -- still refused
# TRAP: a link at the volume ROOT pointing at `../x` genuinely LEAVES the volume.
# Refusing it is correct, not over-refusal. Links that stay inside must either be
# same-directory (`./sub`) or start deep enough that `..` cannot reach the root.

echo "==> building the server under test"
cargo build -q -p submilli-server -p submilli || exit 1
SRV=./target/debug/submilli-server

# ---------------------------------------------------------------- boot refusals
echo; echo "=== Boot refusals (the server must not come up) ==="
config "  work: $V/home" > "$V/c1.yaml"
expect_in "contains the secret store" "$(boot "$V/c1.yaml")" "a volume containing a server directory"

config "  work: relative/dir" > "$V/c2.yaml"
expect_in "give an absolute path" "$(boot "$V/c2.yaml")" "a relative volume target"

ln -sfn "$V/elsewhere" "$W/alias"
config "  work: $W
  inner: $W/alias" > "$V/c3.yaml"
expect_in "is inside volume 'work'" "$(boot "$V/c3.yaml")" "a volume at a symlink inside another volume  (NEW)"
rm -f "$W/alias"

config "  work: $V/STATE" "$V/state/blueprints" > "$V/c4.yaml"
if [ "$(uname)" = "Darwin" ]; then
  expect_in "blueprint store" "$(boot "$V/c4.yaml")" "a case alias of a guarded directory  (NEW, macOS/Windows)"
else
  expect_in "STARTED" "$(boot "$V/c4.yaml")" "a case alias is NOT refused on a case-sensitive filesystem"
fi

# ---------------------------------------------------------------- live server
echo; echo "=== Registration ==="
config "  work: $W" > "$V/ok.yaml"
"$SRV" --config "$V/ok.yaml" > "$V/server.log" 2>&1 & echo $! > "$V/pid"
for _ in $(seq 40); do grep -q listening "$V/server.log" && break; sleep 0.25; done
grep -q listening "$V/server.log" || { echo "server did not start:"; cat "$V/server.log"; exit 1; }

expect_in "the \`path\` key is retired" "$(put $PORT attack "name: attack
vfs:
  mode: persistent
  path: /
$ALLOW")" "the original attack -- \`path: /\` is refused at registration"

expect_in "is not declared on this server" "$(put $PORT ghost "name: ghost
vfs:
  mode: persistent
  volume: nope
$ALLOW")" "an undeclared volume name is refused, and the declared ones are listed"

expect_in "200" "$(put $PORT good "name: good
vfs:
  mode: persistent
  volume: work
$ALLOW")" "a declared volume is accepted"
expect_in "200" "$(put $PORT none "name: none
vfs: none
$ALLOW")" "a \`vfs: none\` blueprint is accepted"
expect_in '{"volumes":["work"]}' "$(curl -s -H "authorization: Bearer $SUBMILLI_SERVER_TOKEN" localhost:$PORT/v1/volumes)" "GET /v1/volumes returns names only"
expect_in '"error":"unauthorized"' "$(curl -s localhost:$PORT/v1/volumes)" "GET /v1/volumes without a token is refused"
expect_in '"error":"forbidden"' "$(curl -s -H "authorization: Bearer $USER_TOKEN" localhost:$PORT/v1/volumes)" "GET /v1/volumes with the user token is refused"

echo; echo "=== Containment at runtime ==="
rd() { run $PORT good "import { readText } from \"submilli:fs\";
function main(): string { const t = readText(\"$1\"); return t === null ? \"null\" : t; }"; }
expect_in "escapes the VFS root" "$(rd /escape/secret.txt)"   "read through an absolute escaping link"
expect_in "escapes the VFS root" "$(rd /esc_rel/secret.txt)"  "read through a relative escaping link"
expect_in "escapes the VFS root" "$(rd /../outside/secret.txt)" "lexical parent escape"
expect_in "escapes the VFS root" "$(rd /pkgs/a/abs/lib.txt)"  "absolute link that resolves inside is still refused"
expect_in "INSIDE"  "$(rd /inner/ok.txt)"           "an internal link into a subdirectory still traverses"
expect_in "SIBLING" "$(rd /pkgs/a/sibling/lib.txt)" "the monorepo sibling shape (../b) still traverses"

expect_in "PERSISTED" "$(run $PORT good 'import { writeText, readText } from "submilli:fs";
function main(): string | null { writeText("/hello.txt", "PERSISTED"); return readText("/hello.txt"); }')" "write/read round trip"
expect_in "PERSISTED" "$(run $PORT good 'import { readText } from "submilli:fs";
function main(): string | null { return readText("/hello.txt"); }')" "the file survives a separate execute (durable volume)"

listing=$(run $PORT good 'import { list } from "submilli:fs";
function main(): string { let o = ""; for (const e of list("/", true)) { o = o + e.path + " [" + e.kind + "]\n"; } return o; }')
expect_in "/escape [symlink]" "$listing" "a recursive listing reports the link as a link"
expect_not "secret.txt"       "$listing" "a recursive listing does not descend through it"
expect_not "$V" "$(rd /escape/secret.txt)$(rd /nope.txt)" "no host path in any response body"

echo; echo "=== MCP file tools ==="
expect_in "escapes the VFS root" "$(mcp $PORT good submilli__files__read '{"path":"/escape/secret.txt"}')" "files.read through the escaping link"
expect_in "escapes the VFS root" "$(mcp $PORT good submilli__files__list '{"path":"/escape"}')"            "files.list of the escaping link"
expect_in "filesystem is disabled" "$(mcp $PORT none submilli__files__read '{"path":"/Cargo.toml"}')"      "\`vfs: none\` really has no filesystem"
expect_in "PERSISTED" "$(mcp $PORT good submilli__files__read '{"path":"/hello.txt"}')"                    "files.read works against a persistent volume"

echo; echo "=== CLI ==="
printf 'import { readText } from "submilli:fs";\nfunction main(): string { const t = readText("/escape/secret.txt"); return t === null ? "null" : t; }\n' > "$V/esc.ts"
expect_in "escapes the VFS root" "$(./target/debug/submilli run "$V/esc.ts" --vfs "$W" 2>&1)" "submilli run --vfs inherits containment"

echo; printf '=== %d passed, %d failed ===\n' "$pass" "$fail"
[ "$fail" = 0 ]
