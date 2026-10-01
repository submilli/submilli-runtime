#!/usr/bin/env bash
#
# Walks the quickstart chapter's reader journey against the real CLI and server,
# and asserts both outcomes: the total, and the denial.
#
# The chapter is written from this script's transcript. The containment runs one
# way — every command the chapter prints appears between the `reader journey`
# fences below, verbatim and in the chapter's order. Inside a fence, `save <path>`
# stands for a code block the chapter tells the reader to save; everything else is
# a command the chapter prints. The one exception is the chapter's opening
# `mkdir`/`cd`, whose analogue is the throwaway workdir this script creates.
# Readiness waits, assertions, controls, and teardown are harness-only and live
# outside the fences.
#
# With SUBMILLI_QUICKSTART_CHAPTER pointing to the separately maintained book,
# `--blocks-only` runs just the drift check between the chapter's code blocks
# and the files here, without touching the CLI or the server.
#
# The binaries resolve through $SUBMILLI and $SUBMILLI_SERVER, defaulting to
# `cargo run` so this works on a checkout where no installer has placed anything.
# That indirection is the only permitted divergence between script and prose.
#
#   ./verify.sh                                    # from a source checkout
#   SUBMILLI=submilli SUBMILLI_SERVER=submilli-server ./verify.sh   # installed

set -euo pipefail

SRC="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"

# The journey runs in a throwaway directory, so the cargo fallback has to name
# the workspace explicitly — there is no Cargo.toml above it to find.
REPO_ROOT="$(cd "$SRC/../.." && pwd)"
: "${SUBMILLI:=cargo run -q --manifest-path $REPO_ROOT/Cargo.toml -p submilli --}"
: "${SUBMILLI_SERVER:=cargo run -q --manifest-path $REPO_ROOT/Cargo.toml -p submilli-server --}"

# A throwaway home, so "empty package store" never means the developer's real
# ~/.submilli/packages — CI's package tests depend on it.
SUBMILLI_HOME="$(mktemp -d)"
export SUBMILLI_HOME
# The journey names its own server and token. Settings inherited from the
# developer's shell would point the CLI at another server or another token, or
# hand the server a config the chapter never mentions.
unset SUBMILLI_SERVER_URL SUBMILLI_SERVER_TOKEN SUBMILLI_SERVER_TOKEN_FILE
unset SUBMILLI_CONFIG SUBMILLI_ALLOW_UNAUTHENTICATED
WORK="$(mktemp -d)"
SERVER_PID=""

cleanup() {
  if [[ -n "$SERVER_PID" ]] && kill -0 "$SERVER_PID" 2>/dev/null; then
    # Ask first. Under the cargo fallback $SERVER_PID is the wrapper, not the
    # server, so killing it can leave an orphan holding the port.
    $SUBMILLI server stop >/dev/null 2>&1 || true
    kill "$SERVER_PID" 2>/dev/null || true
    wait "$SERVER_PID" 2>/dev/null || true
  fi
  rm -rf "$WORK" "$SUBMILLI_HOME"
}
trap cleanup EXIT

fail() { echo "FAIL: $*" >&2; exit 1; }

# Every file the chapter has the reader author must be printed in the chapter in
# full, byte for byte. Prose that drifts from the example is the failure mode
# this whole script exists to catch, and this is the half of it a transcript
# cannot check.
check_chapter_blocks() {
  local chapter="${SUBMILLI_QUICKSTART_CHAPTER:?set SUBMILLI_QUICKSTART_CHAPTER to the book chapter for --blocks-only}"
  [[ -f "$chapter" ]] || fail "chapter not found at $chapter"
  python3 - "$chapter" "$SRC" <<'PYEOF' || fail "the chapter no longer prints these files verbatim"
import pathlib, re, sys
chapter, src = pathlib.Path(sys.argv[1]), pathlib.Path(sys.argv[2])
blocks = [b for _, b in re.findall(r"```(\w*)\n(.*?)```", chapter.read_text(), re.S)]
missing = [p for p in ("blueprint.yaml", "total.ts",
                       "total-injected.ts", "app.mjs", "package/src/lib.ts",
                       "package/capabilities.yaml")
           if (src / p).read_text() not in blocks]
for p in missing:
    print(f"  not printed verbatim in the chapter: {p}", file=sys.stderr)

# agent.py is excerpted rather than printed whole, so check the lines a reader
# copies: the MCP endpoint, the token header, and the session-variable header.
text = chapter.read_text()
agent = (src / "agent.py").read_text()
for needle in [l.strip() for l in agent.splitlines()
               if '"url"' in l or '"Authorization"' in l or "submilli-variables" in l]:
    if needle not in text:
        missing.append("agent.py")
        print(f"  agent.py line missing from the chapter: {needle}", file=sys.stderr)

sys.exit(1 if missing else 0)
PYEOF
}

step() { printf '\n\033[1m# %s\033[0m\n' "$*"; }

# `save <path>` stands for a code block the chapter hands the reader.
save() { mkdir -p "$(dirname "$WORK/$1")"; cp "$SRC/$1" "$WORK/$1"; }

expect_contains() {
  local haystack=$1 needle=$2 what=$3
  [[ "$haystack" == *"$needle"* ]] || fail "$what: expected to contain '$needle', got: $haystack"
}

expect_missing() {
  local haystack=$1 needle=$2 what=$3
  [[ "$haystack" != *"$needle"* ]] || fail "$what: expected NOT to contain '$needle', got: $haystack"
}

# `/healthz` answers without a token, so readiness does not depend on the
# token being right. `server status` does, and proves the CLI's token works.
wait_for_server() {
  for _ in $(seq 1 120); do
    if curl -fsS -o /dev/null http://127.0.0.1:8128/healthz 2>/dev/null; then
      $SUBMILLI server status >/dev/null || fail "the server is up but refused the token in SUBMILLI_SERVER_TOKEN"
      return 0
    fi
    kill -0 "$SERVER_PID" 2>/dev/null || fail "the server exited before it became ready"
    sleep 0.5
  done
  fail "server did not become ready"
}

# A server already on the port would answer every request from the developer's
# real package store, and the run would pass for the wrong reason. Any HTTP
# answer counts, which is why curl runs without -f: another listener's 404
# means the port is taken just as much as a submilli-server's 200.
require_port_free() {
  if curl -sS -o /dev/null http://127.0.0.1:8128/healthz 2>/dev/null; then
    fail "something is already listening on 127.0.0.1:8128 — stop it first (submilli server stop)"
  fi
}

case "${1:-}" in
  --blocks-only)
    check_chapter_blocks
    echo "chapter blocks match the example."
    exit 0
    ;;
  "") ;;
  *) fail "unknown argument: $1 (only --blocks-only is accepted)" ;;
esac

# ---------------------------------------------------------------------------
# The package, the filter, and the attack
# ---------------------------------------------------------------------------

cd "$WORK"

step "the package"

# --- reader journey ---
$SUBMILLI build init @acme/billing package
save package/src/lib.ts
rm package/tests/lib.test.ts
$SUBMILLI build check
$SUBMILLI build publish-local
cat package/capabilities.yaml
save blueprint.yaml
$SUBMILLI blueprint lint blueprint.yaml
# --- end reader journey ---

diff -u "$SRC/package/capabilities.yaml" package/capabilities.yaml \
  || fail "the derived capability schema no longer matches the one the chapter prints"

# The chapter prints `checked @acme/billing v0.1.0` and nothing else.
check_out="$($SUBMILLI build check 2>&1)"
expect_missing "$check_out" "warning:" "the package compiles without warnings"

# The committed test is not part of the reader journey any more, but it still
# has to pass — it is the package's only real coverage.
save package/tests/lib.test.ts
$SUBMILLI build test >/dev/null

# Publishing twice must be safe: a reader who edits and republishes should not
# have to reset the store.
$SUBMILLI build publish-local >/dev/null

step "the server"
require_port_free

# --- reader journey, continued ---
save total.ts
save total-injected.ts
save app.mjs
export SUBMILLI_SERVER_TOKEN=$(openssl rand -hex 32)
$SUBMILLI_SERVER &
# --- end reader journey ---
SERVER_PID=$!
wait_for_server

# --- reader journey, continued ---
$SUBMILLI server blueprint apply blueprint.yaml
node app.mjs total.ts
node app.mjs total-injected.ts
# --- end reader journey ---

legit="$(node app.mjs total.ts)"
expect_contains "$legit" "[result]  2 charges, 6150 cents" "the legitimate run"
expect_missing "$legit" "[denied]" "the legitimate run is not denied"

injected="$(node app.mjs total-injected.ts)"
expect_contains "$injected" "[program] 2 charges, 6150 cents" "the injected run's legitimate work completes"
expect_contains "$injected" "[denied]" "the injected run is denied"
expect_contains "$injected" "acme.com/charges.list" "the denial names the capability"
expect_contains "$injected" "policy denied" "the denial names a reason"
expect_missing "$injected" "[result]" "the injected run returns nothing to the caller"

if [[ -n "${SUBMILLI_QUICKSTART_CHAPTER:-}" ]]; then
  frame="$(grep -o 'at listCharges (@acme/billing/lib:[0-9]*:[0-9]*)' "$SUBMILLI_QUICKSTART_CHAPTER" | head -1)"
  [[ -n "$frame" ]] || fail "could not find the chapter's denial frame to check"
  expect_contains "$injected" "$frame" "the chapter's printed denial frame still matches the runtime"
fi

# ---------------------------------------------------------------------------
# Controls — prove the assertions above are load-bearing, not decorative
# ---------------------------------------------------------------------------

step "controls"

# Without the filter the injected call is allowed. If this still denies, the
# denial above was coming from somewhere other than the filter.
grep -v '^    filter: customerId' blueprint.yaml > unfiltered.blueprint.yaml
sed -i.bak 's/^name: quickstart$/name: quickstart-unfiltered/' unfiltered.blueprint.yaml
$SUBMILLI server blueprint apply unfiltered.blueprint.yaml >/dev/null
unfiltered="$(sed 's/"quickstart"/"quickstart-unfiltered"/' app.mjs > app-unfiltered.mjs && node app-unfiltered.mjs total-injected.ts)"
expect_missing "$unfiltered" "[denied]" "without the filter the injected call is allowed"
expect_contains "$unfiltered" "reconciliation: 1 charges" "without the filter the other customer's charges come back"

# Flip the binding and both outcomes invert. This is the assertion that ties the
# denial to the value the application supplied: same blueprint, same programs,
# only the client's binding changed.
sed 's/"cus_northwind"/"cus_initech"/' app.mjs > app-flipped.mjs
flipped_legit="$(node app-flipped.mjs total.ts)"
expect_contains "$flipped_legit" "[denied]" "with cus_initech bound, the northwind call is denied"
flipped_injected="$(node app-flipped.mjs total-injected.ts)"
expect_contains "$flipped_injected" "[denied]" "the flipped run still denies the first call it makes"

# An unregistered blueprint fails legibly rather than crashing the client.
missing="$(sed 's/"quickstart"/"no-such-blueprint"/' app.mjs > app-missing.mjs && node app-missing.mjs total.ts 2>&1)"
expect_contains "$missing" "no-such-blueprint" "an unregistered blueprint names itself in the error"
expect_missing "$missing" "[result]" "an unregistered blueprint does not run the program"

# The token is what lets the application in at all. Without one it stops before
# it calls; with a wrong one, or with no header, the server refuses and nothing
# runs.
if notoken="$(env -u SUBMILLI_SERVER_TOKEN node app.mjs total.ts 2>&1)"; then
  fail "without SUBMILLI_SERVER_TOKEN the application should exit non-zero, got: $notoken"
fi
expect_contains "$notoken" "SUBMILLI_SERVER_TOKEN is not set" "a missing token is named"
if wrongtoken="$(SUBMILLI_SERVER_TOKEN="$(openssl rand -hex 32)" node app.mjs total.ts 2>&1)"; then
  fail "a token the server does not know should be refused, got: $wrongtoken"
fi
expect_contains "$wrongtoken" "[refused]" "an unknown token is refused"
expect_missing "$wrongtoken" "[result]" "an unknown token does not run the program"
unauthenticated="$(curl -sS -o /dev/null -w '%{http_code}' -X POST \
  -H 'content-type: application/json' -d '{}' http://127.0.0.1:8128/v1/execute)"
[[ "$unauthenticated" == "401" ]] || fail "a request with no token: expected 401, got $unauthenticated"

# The variable is required, not defaulted.
novar="$(sed 's/variables: { customerId }/variables: {}/' app.mjs > app-novar.mjs && node app-novar.mjs total.ts 2>&1)"
expect_contains "$novar" "customerId" "omitting the variable is rejected by name"
expect_missing "$novar" "[result]" "omitting the variable does not run the program"

step "chapter drift"
if [[ -n "${SUBMILLI_QUICKSTART_CHAPTER:-}" ]]; then
  check_chapter_blocks
fi

step "teardown"
# --- reader journey, continued ---
$SUBMILLI server stop
# --- end reader journey ---
wait "$SERVER_PID" 2>/dev/null || true
SERVER_PID=""

# ---------------------------------------------------------------------------
# The role split — harness-only, not part of the chapter's journey
# ---------------------------------------------------------------------------
#
# The chapter runs everything on one admin token and tells the reader to give
# an agent a `user` token before it runs anywhere less trusted. This is that
# setup: a second server whose config adds a `user` token, and the same
# application, unchanged, holding it. It still runs programs, and it cannot
# rewrite the blueprint that constrains it.

step "the role split"
require_port_free
openssl rand -hex 32 > "$WORK/app.token"
cat > "$WORK/roles.yaml" <<EOF
api_tokens:
- name: app
  role: user
  token_file: $WORK/app.token
EOF
$SUBMILLI_SERVER --config "$WORK/roles.yaml" &
SERVER_PID=$!
wait_for_server
app_token="$(cat "$WORK/app.token")"

as_user="$(SUBMILLI_SERVER_TOKEN="$app_token" node app.mjs total.ts)"
expect_contains "$as_user" "[result]  2 charges, 6150 cents" "a user token runs the program"
if as_user_apply="$(SUBMILLI_SERVER_TOKEN="$app_token" $SUBMILLI server blueprint apply blueprint.yaml 2>&1)"; then
  fail "a user token should not be able to apply a blueprint, got: $as_user_apply"
fi
expect_contains "$as_user_apply" "admin" "the refusal names the role the endpoint needs"

$SUBMILLI server stop >/dev/null
wait "$SERVER_PID" 2>/dev/null || true
SERVER_PID=""

printf '\n\033[32mPASS\033[0m  the total came back, and the injected call was denied.\n'
