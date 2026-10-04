# Server: chapter briefs

Drafts under `docs/next/server/`, Part 4 of the target structure in
`~/.claude/plans/diataxis-docs-plan.md`. Every page is a Diátaxis how-to
guide: one paragraph on why the reader needs it, then "This guide shows
you how to…" with the steps, the example named as substitutable, the
commands in the order they are run with what each prints. Explanation
stays beside the output it explains; every setting goes to the server
settings reference, every limit and error to Errors and limits, every
command option to the CLI reference, by a "Refer to…" link. The content
is moved from the live `server`, `resource-limits`, `deploying`, and
`cli` chapters, and from the SUB-1229 branch for private packages; new
sentences are the openings and the connecting lines.

## What was run

Outputs on pages 1 to 5 are from throwaway servers built from `main`
(af0592b) under an isolated `SUBMILLI_HOME`
(`scratchpad/srvdemo/home`), on ports 8130 to 8136, with the billing
package in the CLI's store and the support blueprint from Part 2's
Publish page. Ports are shown as 8128 and paths as `/etc/submilli/…`;
pids and everything else are as printed. Page 8's outputs are from main
990fa92, after PR #47. What was not run:
`server packages install` from GitHub (the repository is private; the
line on page 3 is the live chapter's), the Compose and Kubernetes
sections (Docker could not reach a daemon from the session; their
outputs are the live `deploying` chapter's), and nothing else: page 8's
private installs ran for real against the runtime repository.

## Run the server

- **Purpose:** Start it with a token, a `user` token for the
  application, keep it private, the config file as the place to decide
  where state lives (each directory's setting, default, and whether it
  needs a persistent disk), the secret store's key. On Doron's review
  (2026-10-02) the full settings template, the precedence rules, and
  the flag-name mapping were cut: limits are Set limits, and the full
  file with the arguments is the server settings reference.
- **Evidence:** `server` ("Start it", "Who can reach it", "Configure
  it", "Secrets", "Where it keeps state"). Real: the two startup lines,
  `status`, the file-only start (`tokens="admin (admin)"`, Doron's
  request of 2026-10-02) and the two-token start (`admin (admin), app
  (user)`), `status --token-file`, the `no secret store is configured`
  message.
  Doron asked (2026-10-02) that the page show the config file rather
  than flags, so `allow_unauthenticated`, `mcp_allowed_hosts`, and
  `secret_store.key_file` are shown as file settings. The MCP `Host`
  guard was probed for real: an `initialize` request with `Host:
  submilli:8128` or `submilli.agents.svc:8128` gets `403 Forbidden: Host
  header is not allowed` until the name is in `mcp_allowed_hosts`;
  neither the chart nor `compose.yaml` sets it, so the Compose and
  Kubernetes pages now say to add it (`SUBMILLI_MCP_ALLOWED_HOSTS` and
  `config.mcp_allowed_hosts`). The user-token `run-code`
  and refused `apply` proof was cut on Doron's review (2026-10-02); the
  role refusal still appears on Connect the CLI. A
  cold-read review by a fresh agent (2026-10-01) found twelve reader
  obstacles across the part, all applied: among them the admin token
  clobbered in the shell here, the secret-name confusion on Register,
  the Compose file overwriting the reader's own and the token never
  reaching the shell, the one-shot image pin, and the 12× versus 16×
  memory multiplier between Set limits and Kubernetes. The store key moved here from
  the plan's Register page (2026-10-01, Doron: page 22 kept only for
  what no other page says). Telemetry, health, logs, and the outbound
  network block go to the server settings reference; the volumes map's
  boot checks go there too.

## Connect the CLI

- **Purpose:** `--server` and `SUBMILLI_SERVER_URL`, the token from the
  environment or a file, `status` as the first check, the role table.
- **Evidence:** `server` ("Start it", "Who can reach it"), the `server`
  subcommands' `--help`. Real: `status` read through the machine's LAN
  address from a server bound to `0.0.0.0`, the no-token error, the
  role refusal. The role table is from trying each command with the
  `user` token: `run-code`, `session`, and `docs` work; `status`,
  `packages list`, `secret list`, `blueprint apply`, and `stop` are
  refused. The Compose and Kubernetes sentences (`docker compose exec`,
  `kubectl port-forward`, reading `admin-token` from the Secret) are
  from `compose.yaml`'s comments and the chart README, not run.
  Doron asked (2026-10-02) for two more sections: the two variables in
  the shell's startup file, and a `submilli-use <name>` function that
  switches them from `~/.submilli/servers/<name>/{url,token}`. The
  function was run for real in zsh and bash against two throwaway
  servers with different tokens (ports 8142 and 8143), including the
  refusal of an unknown name; the `status` output shown after
  `submilli-use staging` is the earlier real one from the server bound
  to `0.0.0.0`. Side finding, not filed: `submilli server status | head
  -2` makes the CLI panic with `failed printing to stdout: Broken pipe`.

## Register a blueprint

- **Purpose:** Dependencies first, what registration checks and refuses,
  `run-code` as the proof, update and remove. Cut to what Publish a
  package and Start a blueprint don't say (Doron, 2026-10-01).
- **Evidence:** `server` ("Install packages", "Secrets", "Register
  blueprints"). Real: `secret put`, `Added`, `Updated`, the missing
  secret error, the `env` source refusal, `add` on a taken name,
  `run-code` against Stripe test mode, `status` before and after
  `remove` with a session open, `unknown session`. The `env`/`file` refusal
  paragraph and its error were cut on 2026-10-02: the sources are being
  removed (SUB-1265). The page is written for SUB-1267 (registration
  should refuse a blueprint whose package isn't installed) as fixed: the
  package check in the list and its error are expected, not captured;
  the current behaviour (registered, fails on the first program) was
  reproduced for the issue. Not run: `packages
  install` (live output). The seed directory is on Deploy on one
  machine.

- **Update (2026-10-02, main 0924ccc):** PR #52 landed SUB-1267.
  Re-captured on a fresh server from the new build: the `package check
  failed … is not installed` line is now real and identical to the
  predicted one, and a second real error was added for a package rule
  missing from the package's list (`requires `secrets.get` with filter
  …, but `permissions.@acme/billing` has no matching rule`). The secret
  check runs before the package check. The frontmatter note and the
  index flag are gone.

## Set limits

- **Purpose:** The table, fuel as the primary limit (what it counts,
  what a budget buys, `--report` to measure), the time limit as the
  backstop and why it is off by default, memory and the container, the
  stack, model spending, session state. Reworked on 2026-10-02 at
  Doron's request: fuel first, time explained as weak. The page is
  written for SUB-1269 (host functions charge fuel) and SUB-1271
  (`submilli run --report`, a log line on the server) and SUB-1272
  (`1B`-style count literals) as fixed; the
  host-side rows of "What a budget buys" are estimates against the
  charging scheme in SUB-1269, and SUB-1270 replaces them with
  measurements. Doron's "around 800 executions waiting on I/O on a 64 GB
  machine" was left out as a number and kept as "hundreds", pending his
  confirmation.
- **Evidence:** `resource-limits` ("At a glance", "Time", "Memory", "Fuel
  and stack", "Model spending", "Session state"), `server` ("Limits").
  Real: `timeout exceeded` under `max_execution_time: 2`, `memory
  exhausted` under the default 50 MB (13 seconds to reach), `call stack
  exhausted` (the first frames; the real error prints 3,337 frames, each
  with its source lines, 470 KB in all, a candidate issue). The
  standard library's fixed limits, the Git limits, and the filesystem
  `size_limit` details go to Errors and limits and Keep files and state.
- **Fuel calibration (2026-10-02, release build from main 990fa92,
  Apple Silicon, `submilli run --fuel N` bisected to within 1%; scratch
  `scratchpad/fuel`):** fuel counts the program's own Wasm instructions
  only; work done by host functions costs no fuel however long it takes.
  Loop of 1,000,000 `% 1000003` iterations: 38.1 M fuel, 101 ms wall
  including compilation. `Map` 10,000 set + 10,000 get with string keys:
  911 K fuel, 1,090 ms. `JSON.parse` + `JSON.stringify` of a 120 KB array
  of 20,000 numbers: 502 K fuel, 516 ms. `toUpperCase` + `replaceAll` on
  1 MB: 1,009 fuel, 62 ms. `split(" ")` of 100 KB into 20,701 pieces:
  1,009 fuel, 74 ms. 4,000 string `+=` appends: 140 K fuel, 515 ms (the
  copying is host-side). The calibration also found the performance
  pathologies collected in SUB-1268 (`push`, `sort`, `Record` insertion,
  `indexOf`/`slice`/`charCodeAt`/`startsWith` per-call cost, `matchAll`
  copying the input per match); the programs that hit them were kept
  out of the table. The `fuel exhausted` error on the page is from the
  scratch server under `max_execution_fuel: 1000000000` (the debug
  server build took 77 s to burn it; the release CLI burns a billion in
  about 2.1 s under `--report`, which is the figure the page implies).
  The `--report` output and the server's `execution finished` log line
  are real (2026-10-02, release build of the SUB-1271 implementation
  while it was still uncommitted in this checkout): the loop reports
  38,000,026 fuel, 0.1 MB peak, 115 ms wall; the bisection had found
  38,132,808, the smallest budget that completes, so the page now uses
  the reported figure. The implementing agent had pasted a debug-build
  report (2.8 s) and a second copy of `million.ts` into the section;
  both were replaced. The log line is from blueprint `limits`, shown
  with the page's blueprint name. SUB-1274's `matchAll` fix was checked
  on the same build: 1 MB with 18,000 matches in 850 ms, 7.3 MB peak.

## Deploy on Linux

- **Purpose:** Place the server on a Linux machine: the installer for
  the system, a system user and the two directories, the token files
  and key under `/etc/submilli`, the config file from Run the server,
  the systemd unit, upgrade and backup. Renamed from "Deploy on one
  machine" and made Linux-specific on Doron's review (2026-10-02); the
  machine setup itself stays on Run the server. The seed-directory
  section and the reconcile-line outputs were dropped (SUB-1265).
- **Evidence:** `deploying` ("On one machine") for the shape; the
  installer's `--install-dir` flag from `install.sh`'s usage line (the
  published URL is 404 until launch, so the installer could not be run);
  the unit file, `useradd`, `install -d`, `systemctl` and `journalctl`
  commands are written for Linux and not run (no systemd on the
  authoring machine), which is why the page shows no journal output.
  `TimeoutStopSec=10` mirrors `compose.yaml`'s `stop_grace_period`.
  Doron's framing (2026-10-02): the application is on another machine,
  so the server binds `0.0.0.0`, the port is opened in the cloud's
  firewall or security group (no `ufw` command: on cloud machines the
  host firewall isn't where it's done, Doron), and `mcp_allowed_hosts`
  carries `$(hostname -f)` through an unquoted `tee` heredoc. The
  config file and the unit are written with `sudo tee <<EOF`.

## Deploy with Compose

- **Purpose:** The published file, the network, the store key as a
  file, `SUBMILLI_ALLOW_IP`, upgrade and backup. The seed-directory
  section was cut on 2026-10-02 (SUB-1265); a short section points to
  Register a blueprint instead.
- **Evidence:** `deploying` ("With Docker Compose") and `compose.yaml`.
  Outputs are the live chapter's, not re-run. On Doron's review (2026-10-02)
  the download was pinned to the release tag (`v0.1.6`, git tags carry
  the `v`; image tags from `release.yml`'s semver pattern do not) and
  the image pinned in `.env` from the start, so the `docker compose ps`
  line's IMAGE column was changed from `:latest` to `:0.1.6` to match;
  the upgrade section re-downloads the file at the new tag. The private-packages
  blocks are on Install private packages.

## Deploy on Kubernetes

- **Purpose:** Install, tokens, the network policy, the secret store
  and its key Secret, blueprints through the API, `config:` for an
  internal service, memory, storage, upgrades. Completed on 2026-10-02
  after PR #51 (SUB-1265, 5ea3f49) landed: the chart turns the encrypted
  store on by default, generates `submilli-secret-store` once and keeps
  it across upgrades and uninstall, and takes `secretStore.existingSecret`
  for rendered-without-cluster pipelines; secrets reach the store only
  through `submilli server secret put`. The seed directory, the
  `blueprints:`/`secrets:` values, and `file:` secrets are gone, so the
  page's "Store secrets and register blueprints" section replaces them.
  `extraEnv` with `SUBMILLI_ALLOW_IP` became `config.network.allow_ip`
  as a block list, to match Run the server's "config file, not flags".
- **Evidence:** `deploying` ("On Kubernetes") at 5ea3f49, the chart's
  README, `values.yaml`, `NOTES.txt`, and `chart-ci.yml`'s install job
  (`kubectl exec … submilli server secret put … --token-file
  /etc/submilli/auth/admin-token`), read from the fetched commit; the
  checkout itself was not fast-forwarded because PR #51 overlaps the
  other agent's uncommitted `run.rs`. Not run on a cluster: Docker and
  kind are unreachable from the session, and the chart's `install on
  kind` CI job is gated off (`SUBMILLI_CHART_INSTALL_ENABLED`), so the
  `secret put` and `apply` outputs are the CLI's, from the Compose-free
  scratch server used for Register a blueprint.

## Install private packages on a server

- **Purpose:** The fine-grained token, the server's `github_token_file`,
  the Compose and chart mounts, the server's error table. On Doron's
  review (2026-10-02) the page was made server-only, since half of it
  was the reader's own machine: the CLI's side (`authenticate` with its
  real output, `auth-status`, the sources, CI) moved to Start a
  blueprint's "Install the package", and the local error table and
  `deauthenticate` are parked for the CLI reference. Rewritten on 2026-10-02 after PR #47 replaced the SSH design
  (SUB-1229) with GitHub tokens; the SSH draft is gone.
- **Evidence:** `cli` ("Install a package"), `server` ("Configure it",
  "Install packages"), `deploying` (the Compose and Kubernetes token
  sections), `compose.yaml`, and the chart README, all as PR #47 wrote
  them; the implementation agent's draft of this page was the starting
  point. Real, with the CLI and server built from main 990fa92 under a
  fresh home (`scratchpad/ghdemo`): the no-token error from `install`
  and from the server, `authenticate` from piped stdin, `auth-status`,
  `install` and `server packages install` of a private repository, `up
  to date`, the missing-file start-up error, the wrong-ref and
  no-access errors. The runs used the runtime repository and the
  `@submilli/jina` package with Doron's GitHub CLI token; the page shows
  them as `acme/billing-package`, `@acme/billing`, and the login
  `octocat`, with the commit as printed. Not run: Compose and the chart.

## Mount a shared volume

- **Purpose:** Declare a named volume on the server (two kinds, the
  required `size_limit`, `access`), mount it under `vfs.mounts` or as the
  root, the registration refusals and the `persistent` migration
  diagnostic, a program that writes under one mount and reads another
  across two runs, the read-only `PermissionDeniedError`, `fs.info()`,
  the local-run refusal, where the files live and how to remove them.
  Added on 2026-10-02 at Doron's request, for SUB-1222 (named volumes
  and mounts; `persistent` removed), which is on its branch in
  `~/git/submilli-wt1` (10bce40), not on main; the agent there was told
  not to touch `docs/next`.
- **Evidence:** SUB-1222's edits to the live `server` ("Volumes"),
  `blueprints` ("The filesystem"), `resource-limits`, and `llm-prompt.md`
  for the semantics; every output is real, from the branch's build under
  `scratchpad/volumes` (port 8147): `Added`, the two refusals, the
  migration error, the two runs, the `PermissionDeniedError`, the
  `fs.info()` lines, the volume directory, and the `submilli run`
  refusal. Side finding, not filed: `JSON.stringify(fs.info())` returns
  `{}`, so the page prints the fields by hand. Keep files and state's
  mode table, Register a blueprint's checks, Run the server's state
  table and example file, and Set limits' last paragraph were updated
  to match.

## Main 10da5c4 pass (2026-10-03)

PRs #53 to #63 landed; the pages were checked against a release build of
10da5c4. Set limits: `QuotaExceededError` replaces `RangeError` for the
model-token, session-state, and filesystem budgets (SUB-1128;
`RangeError` stays for per-request caps); the `1B` literal is real
(SUB-1272: `max_execution_fuel: 1B`, `loop.ts` ends `fuel_exhausted` at
1,000,000,000 fuel in 1.9 s); `--report` and the log line are on main
(SUB-1271); `million.ts` still 38,000,026 fuel. SUB-1269 (host functions
charge fuel) is still open: only the submilli-wasm upgrade landed, so the
frontmatter note stays. Compose: the shipped `compose.yaml` now sets
`SUBMILLI_MCP_ALLOWED_HOSTS: submilli:8128` (SUB-1260), so the override
block for it was removed and the override file is introduced at the
store-key section. Kubernetes: the chart generates `mcp_allowed_hosts`
from the Service, pod, and Ingress names (SUB-1260), so the page's
`config.mcp_allowed_hosts` is now the extension for names the chart
can't know. Run the server: one clause says Compose and the chart list
their own names. No server-side change to the Host guard.

