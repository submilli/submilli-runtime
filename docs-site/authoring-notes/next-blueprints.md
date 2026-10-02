# Blueprints: chapter briefs

Drafts under `docs/next/blueprints/`, Part 2 of the target structure in
`~/.claude/plans/diataxis-docs-plan.md`. Every page is a Diátaxis how-to
guide: it opens with "This guide shows you how to…" and the steps it
covers, names its example and tells the reader to substitute their own,
then gives the commands in the order they are run, with what each writes.
Explanation stays to one or two sentences next to the output it explains;
options and grammar go to the reference by a "Refer to…" link. The pages
reuse one example blueprint, the support agent's, but each stands alone.

## Start a blueprint

- **Type:** how-to.
- **Purpose:** The lifecycle of a blueprint with the CLI: install and
  read a package (locally and on a server), init, lint, add the package,
  grant one operation with a fixed filter, declare the secret (store and
  harness) and lint clean, declare a variable and replace the grant with
  one that uses it (`capability remove` then `add`), test both ways, see
  the whole file, see the prompt, register it on a server and run a
  program there.
  Every output in that sequence is from one replay with CLI 0.1.6.
- **Starting point:** Part 1; a package in the local store.
- **Boundaries:** No filter grammar beyond one paragraph (permissions
  reference). Fields and blocks are the blueprint file reference. How the
  server commands reach a server is Connect the CLI (Part 4). Applying,
  replacing, and removing registered blueprints are Part 4; how a harness
  supplies harness secrets is the Connect a harness tutorials (Part 5).
- **Evidence:** Assembled from `blueprints` ("Start from nothing allowed",
  "Add a package", "Grant the operation", "Declare the secret", "Check
  it", "The whole file"), `curated-packages` and `cli` ("Install a
  package", "Author a blueprint", "Keep secrets locally"), `harness`
  ("Credentials that belong to the session", one sentence), `server`
  ("Secrets", "Install packages", "Register blueprints"). New sentences:
  the opening, the `--all-capabilities` sentence, the secret-store
  definition, the "test both directions" closing.

## HTTP and credentials

- **Type:** how-to.
- **Purpose:** Why the authorization proxy exists, the HTTP capability
  inventory, allow a direct call, run a program, inject the credential and
  run it again, put the secret on the server and register, plain HTTP.
- **Example:** GitHub's REST API, `/rate_limit`, replaces the fictional
  `status.acme.com`: it is real, any reader with a token can run it, and
  its answer proves the injection (60 requests an hour anonymous, 5,000
  with a token) without the program changing. `User-Agent` is sent as a
  `Map` because `Headers` is a `Map<string, string>`; GitHub answers 403
  without one.
- **Evidence:** `blueprints` ("Call an endpoint with a credential",
  "Explicitly allowing plain HTTP"), `permissions` ("The standard
  library", the HTTP rows and redirect paragraph), `standard-library`
  (the `http.get` program shape), `server` ("Secrets", "Register
  blueprints"). Every local output is from a scratch run with CLI 0.1.6
  and a real token from `gh auth token`; the "51 of 60" remaining count
  is whatever that run saw. The server section ran for real too (see
  the server note below). The plain-HTTP example
  (`legacy.internal.acme.com`) was run on a fresh scratch blueprint; the
  top-level flag was added by hand and lint accepted it.

## Keep files and state

- **Type:** how-to.
- **Purpose:** One thread from an empty `notes` blueprint: what
  `submilli:fs` can do (docs excerpt), its eight capabilities, four grants,
  a notes program run fresh and then with `--vfs` shared between runs, the
  vfs modes and idle timeout, the size cap with a real `RangeError`, then
  `submilli:session` the same way, ending with the whole file.
- **Evidence:** `blueprints` ("Let programs remember"), `permissions`
  (path normalization, glob crossing `/`), `resource-limits`
  ("Filesystem"). Every output is from one scratch replay with CLI 0.1.6:
  the `docs` excerpts are trimmed signature lists; the path-escape denial
  was run (`/notes/../secrets.md` refused under `fs.write`); the
  original `RangeError` came from a copy with `size_limit: 1KB` (the chapter
  now names it `QuotaExceededError`, per SUB-1128); the
  whole file is as the CLI rewrote it after a hand edit (`1h` to
  `'3600s'`, `100MB` to `104857600`).
- **Session section:** run for real against `submilli-server` 0.1.6
  started locally with `--allow-unauthenticated` on port 8129 under the
  scratch `SUBMILLI_HOME`: `blueprint apply`, `POST /v1/sessions`, two
  `execute` calls, `DELETE`. The page shows port 8128 and the bearer
  header a real server needs; the session id is the real one. `note.ts`
  twice in that session returned both lines, and `run-code` answered
  "nothing saved yet", as the page says.
- **Finding:** under CLI 0.1.6, `submilli run` has no session store at
  all (`TypeError: session.get: this runtime has no session store
  configured`); the live chapter's claim that a run outside a session
  "gets a session of its own" is not what 0.1.6 does. Filed in the launch
  project; the issue also asks for the docs to be updated when fixed.

## Allow Git

- **Type:** how-to.
- **Purpose:** Why an agent needs the repository; one thread from an
  empty `coder` blueprint: identity, what `submilli:git` and
  `submilli:code` can do, the four Git capabilities, grants for one
  public repository, a program that clones, searches, edits, and commits,
  then the whole file. The username and `GIT_TOKEN` are part of the
  identity step (HTTPS only, no SSH), not a separate private-repository
  section.
- **Example:** `octocat/Hello-World` on GitHub, because it is public, one
  file, and the clone, edit, and commit run for real under `submilli run`.
- **Evidence:** `blueprints` ("Let the program commit"), `permissions`
  ("Git capabilities"), `cli` ("Configure Git"), `standard-library`
  ("Git repositories", "Coding-agent workspace tools"). Every output is
  from one scratch replay with CLI 0.1.6; the program was re-run after
  the username and token were added (commit `3324dc2`), and the whole
  file is that blueprint. The
  `submilli docs submilli:code` block is a trimmed excerpt.
- **Findings:** (1) `code.search` and `code.tree` stat a hidden path at
  the root before traversing (bisected: a filter allowing `/.*` passes,
  `/` alone does not), so a `fs.stat` filter narrowed to `/repo` denies
  every search; the page grants `fs.stat` unfiltered and says why. Filed
  as SUB-1223; narrow the grant and drop that sentence when it lands.
  (2) `submilli docs submilli:git` answered "unknown package" because the
  CLI hid opt-in Git like unscoped server discovery; fixed on 2026-10-01 so
  `submilli docs` and `submilli search` show the whole library, and the
  page now quotes the real excerpt. (3) `blueprint git show` printed
  `username: Some("agent")`, a Rust debug rendering; fixed the same day.
  The page describes the command rather than quoting its output.

## Allow model calls

- **Type:** how-to.
- **Purpose:** Why a program calls a model (bulk work outside the
  context window, a typed verdict); one thread from an empty `triage`
  blueprint: key, the `llm` catalog and lint, what the module can do and
  its one capability, the grant and a real denial under a narrower
  filter, a triage program, the whole file, then the key, apply, and
  `run-code` on a server. `output_reserve` moved to the blueprint file
  reference (Doron: "I lean toward reference").
- **Evidence:** `blueprints` ("Let the program call a model"),
  `standard-library` ("Model calls": the program is a shortened version
  of its `triage.ts`, and the `ok: false` sentence), `resource-limits`
  ("Model spending"). Every CLI output, the lint, the docs and capability
  excerpts, the denial, and the whole file are from one scratch replay
  with CLI 0.1.6. The CLI rewrites the hand-written description without
  quotes, which the whole file shows. The server section ran for real on
  the local `submilli-server` 0.1.6 with the key in its encrypted store;
  the `run-code` verdict is that run's, worded differently from the local
  one because the model answers differently each time. The
  provider types come from lint's own error for an unknown type ("use
  one of: anthropic, google, openai, openai-compatible"), and the
  `base_url` requirement from its error for `openai-compatible` without
  one; both probed on copies of the file.
- **Run:** `triage.ts` ran for real with the Anthropic key from the
  repository's `.env` (loaded into the scratch store, never printed). The
  page shows the returned `Verdict` pretty-printed; the CLI prints it as
  one JSON line.

## Add an MCP server

- **Type:** how-to.
- **Purpose:** Why an MCP server becomes a package; declare, allow
  tools, call, credentials, OAuth, the same blueprint on a server
  (apply, auth-status, network rules, server login, run-code, providers
  in the config file, discovery), failure.
- **Boundaries:** Tool-to-function mapping, output schemas, the `mcp`
  block's fields, and the local-versus-server command table go to the
  MCP servers reference page. The Operate MCP servers page in Part 4 was
  dropped (Doron, 2026-10-01); its content is this page's server section.
- **Evidence:** `mcp-servers` (all sections except "How tools become
  functions", "Output schemas", "What a program gets back", and "With a
  coding agent", which are condensed to one sentence and a link), and
  `server-mcp` (register and check, network rules, log in, providers,
  discovery, left-out servers). Outputs are the two live chapters'
  verified runs where they involve Playwright (no Playwright server
  here). Run for real with CLI 0.1.6: "Allow its tools" (`capability remove` then `add`, with
  `add-mcp --no-probe`); the whole Linear API-key path, with the
  `LINEAR_API_KEY` from the repository's `.env` loaded into the scratch
  store, never printed: `add-mcp --authorization-bearer`, `docs
  @mcp/linear` (59 tools; the `get_issue` entry is quoted verbatim), the
  grant, `teams.ts` returning "Submilli", the `list_users` denial; and
  the OAuth probe (`add-mcp` with no flag on a fresh blueprint) with the
  local `auth-status` PENDING line; the OAuth login itself, with Doron
  approving it in his browser (the page trims the authorize URL after
  `client_id=`, since the real one carries the PKCE challenge and state),
  the ACTIVE `auth-status` after it, and `teams.ts` under the login (68
  tools discovered, against 59 with the key); and the whole server
  section, on the local `submilli-server` 0.1.6 with a secret store:
  `apply` ("Added"), the pending status with both servers, the pre-login
  `run-code` compile error, the `link.ts` network-policy warning on a
  server started without `--allow-localhost` (its text now ends
  `--allow-private`), the server-side login approved in Doron's browser,
  the active status, and `run-code teams.ts` ("Submilli"). The pending
  status and pre-login run were captured after a `deauthenticate`, since
  Playwright was declared after the first login; the page shows them in
  logical order. In the Linear run outputs, the
  `warning: @mcp/playwright: server unavailable` line that this machine
  prints because no Playwright server is running was left out. The
  credential table was dropped; the four cases are now in prose.
  `remove` drops every rule for the capability, so it must precede the
  grant; `add-mcp` writing a deny under `default: deny` is redundant
  (candidate issue, not filed).
  The old chapter's local login outputs named a `tracker` blueprint; the
  page says `browse`, the name the page's own `init` gives, in those two
  lines.

## Verification

- CLI 0.1.6 in a scratch project under an isolated `SUBMILLI_HOME`, with
  the billing package from the Part 1 brief published to its store. Every
  `✓` line, `capability list`, `git show`, lint result, and YAML fragment
  on the first five pages is copied from that run, as is the whole file at
  the end of page 1. `add-package` now reports `http.post`, matching the
  package's real call.
- Test runs: `cus_initech` denial and the missing-variable refusal are
  from that run. The `cus_northwind` success line is from the earlier
  stub version of the package; with the real call it fails on DNS for the
  fictional host.
- `blueprint prompt` in 0.1.6 prints the whole execute-tool description
  (117 lines for the example blueprint), so page 1 quotes the one
  policy-dependent paragraph as an excerpt. Verified by diffing the
  output before and after granting `fs.read` and `http.get` and setting
  `vfs: per_session`: the module list, an auth-proxy paragraph, and the
  `Sandbox:` and `Network:` lines are what change.
- `secret put` prompts were fed through a pipe; the `Value for … [hidden]`
  line is from the old chapter's interactive run. The harness secret
  command and its output are from this run.
- Server commands: `submilli-server` 0.1.6 was run locally under the
  scratch `SUBMILLI_HOME` (`--allow-unauthenticated`, a generated
  `--secret-store-key-file`, port 8129). Verified for real: `server secret
  put` ("Stored secret …"), `blueprint apply` ("Added …" then "Updated …"
  on a second apply), page 2's `run-code rate.ts` ("5000 of 5000 …"), and
  page 3's whole session flow. Not real: page 1's `packages install`
  output (the repository is fictional; text from the server chapter) and
  page 1's `run-code` line (the server reaches the real package, which
  fails on DNS for the fictional billing host; "credited 1500 cents" is
  from the stub). The `Value for … [hidden]` prompt lines follow the
  server chapter; values were piped in.
- MCP: `add-mcp --no-probe` and `capability add mcp.playwright` outputs
  are from this run (no Playwright server running). The Playwright run,
  the Linear `add-mcp` and `authenticate` outputs, and the
  unavailable-server error are from the old chapter's verified runs, not
  re-run here.
- Every sentence of behavior was traced back to the live chapter it came
  from after the how-to pass; the ones that could not be were cut or
  reworded to the source.
