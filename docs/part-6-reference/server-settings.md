---
title: "Server settings"
description: "Every submilli-server setting with its config-file key, flag, and SUBMILLI_* variable, the precedence between them, and the tokens, directories, secret store, volumes, outbound network block, limits, telemetry, health, logs, audit trail, and shutdown they control."
slug: reference/server-settings
sidebar:
  order: 9
authorship:
  label: ai-assisted
  confirmed: true
  contentHash: "b7db325fe76d2dcbb41e870584a06bdcecedaa78bb98c9fd5ad5aa0e9266e155"
  confirmedAt: "2026-10-05T13:01:53.009Z"
---

This page describes how `submilli-server` is configured. It covers every
setting with its config-file key, flag, and environment variable, how the three combine,
and what each group of settings controls.

## Config file

The config file is YAML, named by `--config <path>` or `SUBMILLI_CONFIG`, and
read once at startup. Every key is optional. A key the server doesn't know
stops it from starting, ``unknown field `prot`, expected one of `bind`,
`port`, …``.

## Settings

A top-level key has the flag `--` plus the key with `-` for `_`, and the
variable `SUBMILLI_` plus the key in capitals. Keys inside a block have the
flat names listed. A dash means the form doesn't exist.

| File key | Flag | Variable | Type | Default | Description |
| --- | --- | --- | --- | --- | --- |
| — | `--config` | `SUBMILLI_CONFIG` | path | none | The config file. |
| `bind` | `--bind` | `SUBMILLI_BIND` | IP address | `127.0.0.1` | Address to listen on. See [`bind` and `port`](#bind-and-port). |
| `port` | `--port` | `SUBMILLI_PORT` | port number | `8128` | TCP port to listen on. |
| `tls.cert_file` | `--tls-cert-file` | `SUBMILLI_TLS_CERT_FILE` | path | none | PEM certificate chain. See [TLS](#tls). |
| `tls.key_file` | `--tls-key-file` | `SUBMILLI_TLS_KEY_FILE` | path | none | Matching PEM private key. |
| — | — | `SUBMILLI_SERVER_TOKEN` | string | none | An API token with the `admin` role. See [`api_tokens`](#api_tokens). |
| `api_tokens` | — | — | list | `[]` | API tokens, each read from a file. |
| `allow_unauthenticated` | `--allow-unauthenticated` | `SUBMILLI_ALLOW_UNAUTHENTICATED` | boolean | `false` | Serve without API tokens. |
| `mcp_allowed_hosts` | `--mcp-allowed-host`, repeatable | `SUBMILLI_MCP_ALLOWED_HOSTS`, comma-separated | list of `host[:port]` | `[]` | `Host` headers the MCP endpoint accepts beside the loopback names. See [`mcp_allowed_hosts`](#mcp_allowed_hosts). |
| `mcp_oauth` | — | — | block | no providers | OAuth client registrations for MCP servers. See [`mcp_oauth`](#mcp_oauth). |
| `github_token_file` | — | — | path | none | GitHub token for Package installs. See [`github_token_file`](#github_token_file). |
| `database_path` | `--database-path` | `SUBMILLI_DATABASE_PATH` | path | `$SUBMILLI_HOME/server/db/submilli.db` | SQLite database file. |
| `blueprint_dir` | `--blueprint-dir` | `SUBMILLI_BLUEPRINT_DIR` | path | `$SUBMILLI_HOME/server/blueprints` | Obsolete. |
| `session_store_dir` | `--session-store-dir` | `SUBMILLI_SESSION_STORE_DIR` | path | `$SUBMILLI_HOME/server/sessions` | Session lifecycle records. |
| `vfs_session_dir` | `--vfs-session-dir` | `SUBMILLI_VFS_SESSION_DIR` | path | `$SUBMILLI_HOME/server/vfs/sessions` | Files of `per_session` filesystems. |
| `vfs_ephemeral_dir` | `--vfs-ephemeral-dir` | `SUBMILLI_VFS_EPHEMERAL_DIR` | path | the OS temp directory | Scratch directories of `ephemeral` filesystems. |
| `package_store_dir` | `--package-store-dir` | `SUBMILLI_PACKAGE_STORE_DIR` | path | `$SUBMILLI_HOME/server/packages` | Installed Packages. |
| `volume_dir` | `--volume-dir` | `SUBMILLI_VOLUME_DIR` | path | `$SUBMILLI_HOME/server/volumes` | `managed-local` volumes. |
| `secret_store.dir` | `--secret-store-dir` | `SUBMILLI_SECRET_STORE_DIR` | path | `$SUBMILLI_HOME/server/secrets` | The encrypted secret store. See [`secret_store`](#secret_store). |
| `secret_store.key_file` | `--secret-store-key-file` | `SUBMILLI_SECRET_STORE_KEY_FILE` | path | none | File holding the store's key. |
| `secret_store.key_env` | `--secret-store-key-env` | `SUBMILLI_SECRET_STORE_KEY_ENV` | variable name | `SUBMILLI_SECRET_KEY` | Variable holding the store's key. |
| `volumes` | — | — | map | `{}` | Named volumes Blueprints may use. See [`volumes`](#volumes). |
| `network.allow_ip` | `--allow-ip`, repeatable | `SUBMILLI_ALLOW_IP`, comma-separated | list of IP addresses or CIDR ranges | `[]` | Addresses the outbound block lets through. See [`network`](#network). |
| `network.allow_localhost` | `--allow-localhost` | `SUBMILLI_ALLOW_LOCALHOST` | boolean | `false` | Let outbound calls reach loopback. |
| `network.allow_private` | `--allow-private` | `SUBMILLI_ALLOW_PRIVATE` | boolean | `false` | Let outbound calls reach the private ranges. |
| `max_execution_memory` | `--max-execution-memory` | `SUBMILLI_MAX_EXECUTION_MEMORY` | megabytes, at least 1 | `50` | Memory one run may hold. See [Limits](#limits). |
| `max_execution_time` | `--max-execution-time` | `SUBMILLI_MAX_EXECUTION_TIME` | seconds | `0`, no limit | Time one run may take. |
| `max_execution_fuel` | `--max-execution-fuel` | `SUBMILLI_MAX_EXECUTION_FUEL` | count, at least 1 | `1T` | Fuel one run may burn. |
| `max_execution_stack` | `--max-execution-stack` | `SUBMILLI_MAX_EXECUTION_STACK` | kibibytes, 1 to 16384 | `512` | Stack one run may use. |
| `max_session_state_memory` | `--max-session-state-memory` | `SUBMILLI_MAX_SESSION_STATE_MEMORY` | megabytes, at least 1 | `1024` | `submilli:session` state across all sessions. |
| `max_llm_tokens` | `--max-llm-tokens` | `SUBMILLI_MAX_LLM_TOKENS` | count, at least 1 | `20M` | Model tokens across all runs. |
| `max_execution_llm_tokens` | `--max-execution-llm-tokens` | `SUBMILLI_MAX_EXECUTION_LLM_TOKENS` | count, at least 1 | `1M` | Model tokens one run may spend. |
| `max_llm_concurrency` | `--max-llm-concurrency` | `SUBMILLI_MAX_LLM_CONCURRENCY` | prompts, at least 1 | `4` | Prompts of one `llm.batch` in flight at once. |
| — | — | `SUBMILLI_DENY_WARNINGS` | `1` or unset | unset | Refuse every Package install that has a code warning, whatever the caller asks. |
| `telemetry` | — | `SUBMILLI_TELEMETRY` | boolean | `false` | Report to the Submilli maintainers. See [`telemetry`](#telemetry). |
| `telemetry_include_source` | — | `SUBMILLI_TELEMETRY_INCLUDE_SOURCE` | boolean | `false` | Attach failed programs' source to reports. |
| `logging.file` | `--log-file` | `SUBMILLI_LOG_FILE` | path | standard output | Append server logs to a file. See [Logs](#logs). |
| `logging.audit.enabled` | — | — | boolean | `true` | Write audit records. See [Audit trail](#audit-trail). |
| `logging.audit.file` | — | — | path | the log's output | Append audit records to a file of their own. |
| `logging.audit.allows` | — | — | `summary`, `all`, or `none` | `summary` | How allowed operations are recorded. |
| `shutdown_grace` | `--shutdown-grace` | `SUBMILLI_SHUTDOWN_GRACE` | seconds | `5` | How long running requests may finish after a stop. See [Shutdown](#shutdown). |
| — | `--health-check` | — | — | — | Probe a running server and exit. See [Health and status](#health-and-status). |

## Precedence

When sources disagree, the most specific wins:

| Rank | Source |
| --- | --- |
| 1 | A flag |
| 2 | A `SUBMILLI_*` variable |
| 3 | The config file |
| 4 | The `HOST` and `PORT` variables, for `bind` and `port` only |
| 5 | The default |

Some settings add up instead. The `network` grants and `mcp_allowed_hosts`
combine across every source, and `allow_unauthenticated` is on when any
source turns it on. For `telemetry`, `false` in the file wins over the
variable. `SUBMILLI_HOME` moves the base of every default path
(`~/.submilli` when unset).

## Value formats

| Type | Accepted values |
| --- | --- |
| Boolean variable | `1`, `true`, `yes`, or `on`, in any case, mean true. Any other value means false. |
| Boolean in the file | `true` or `false`. |
| Count | A whole number with an optional decimal suffix: `K` (thousand), `M` (million), `B` (billion), or `T` (trillion), case-insensitive, with no space before it. Underscores may separate digits, as in `10_000_000_000`. Fractions and scientific notation are refused. |
| Megabytes, kibibytes | Whole numbers. A megabyte is 1,048,576 bytes and a kibibyte 1,024 bytes. |
| Seconds | Whole numbers. |
| List variable | Comma-separated. Whitespace around each item is ignored. |

An empty variable counts as unset. A variable or flag that is set but
doesn't parse stops the server from starting:

```text
Error: $SUBMILLI_PORT: expected a port number, got `abc`
```

## `bind` and `port`

The defaults, `127.0.0.1` and `8128`, accept connections from the same
machine only. The plain `HOST` and `PORT` variables that hosting platforms
set rank below the config file. `PORT` alone also binds `0.0.0.0`, unless a
flag, a `SUBMILLI_*` variable, the file, or `HOST` names the address.

## TLS

HTTPS is disabled when neither TLS file is set. Setting both `tls.cert_file`
and `tls.key_file` enables HTTPS on the configured port for all endpoints.
Setting only one, invalid PEM, unreadable files, or a key that does not match
the certificate stops startup. Each PEM file is limited to 1 MiB.

```yaml title="server.yaml (fragment)"
tls:
  cert_file: /etc/submilli/server.crt
  key_file: /etc/submilli/server.key
```

Certificates are loaded at startup. Restart to rotate them. The listener limits
pending TLS handshakes to 128 and gives each ten seconds. Authentication and
MCP hostname checks still apply. `--health-check` uses the configured public
key to verify the local HTTPS server, without the CLI trust store or a hostname
check against the loopback probe address. Certificate validity still applies.

## `api_tokens`

Every request except `GET /healthz` carries an API token as
`Authorization: Bearer <token>`. Tokens come from `SUBMILLI_SERVER_TOKEN`,
one `admin` token, and from `api_tokens` in the file. Both may be used. The
server doesn't start without at least one token, unless
`allow_unauthenticated` is on.

| Key | Value |
| --- | --- |
| `name` | A unique name, shown in logs. The token itself never is |
| `role` | `admin` or `user` |
| `token_file` | A file holding the token. Surrounding whitespace is removed |

```yaml title="server.yaml (fragment)"
api_tokens:
  - name: app
    role: user
    token_file: /etc/submilli/app.token
```

A token is at least 32 characters of letters, digits, and `- . _ ~ + /`,
with optional trailing `=`. `openssl rand -hex 32` makes one. The server
keeps only a digest of each token. To rotate one, add an entry with the same
role, restart, move the callers, remove the old entry, and restart again.

| Role | May call |
| --- | --- |
| `user` | What a harness needs: running programs, sessions, the MCP endpoint, and a Blueprint's prompt, Packages, and built-ins ([HTTP API](/docs/reference/http-api)) |
| `admin` | Everything, including Blueprints, secrets, Packages, status, and shutdown |

`allow_unauthenticated` serves every caller that reaches the port, with no
token, and can't be combined with tokens. The server logs a warning when it
runs this way.

## `mcp_allowed_hosts`

The MCP endpoint accepts a request only when its `Host` header is
`localhost`, `127.0.0.1`, or `::1`. Any other answers `403 Forbidden: Host
header is not allowed`. `mcp_allowed_hosts` adds names, each as the client
sends it, with the port when the client includes one:

```yaml title="server.yaml (fragment)"
mcp_allowed_hosts:
  - submilli:8128
```

## Directories

| Setting | Holds | Default |
| --- | --- | --- |
| `blueprint_dir` | Obsolete. | `$SUBMILLI_HOME/server/blueprints` |
| `session_store_dir` | Open sessions' lifecycle records, so sessions and idle timeouts survive a restart. | `$SUBMILLI_HOME/server/sessions` |
| `vfs_session_dir` | The files of each session with a `per_session` filesystem. | `$SUBMILLI_HOME/server/vfs/sessions` |
| `vfs_ephemeral_dir` | One directory per run of an `ephemeral` filesystem, deleted when the run ends. | The OS temp directory |
| `package_store_dir` | Packages installed with `submilli server packages install`. | `$SUBMILLI_HOME/server/packages` |
| `volume_dir` | One directory per `managed-local` volume. | `$SUBMILLI_HOME/server/volumes` |
| `secret_store.dir` | The encrypted secret store. | `$SUBMILLI_HOME/server/secrets` |

The server creates each directory it needs. It also reads the CLI's Package
store, `$SUBMILLI_HOME/packages`, as a read-only fallback.

## `secret_store`

The secret store holds the values of a Blueprint's `store:` secrets and the
OAuth tokens of MCP logins, encrypted with a key. The key is base64 of 32 bytes, such as
`head -c 32 /dev/urandom | base64`.

| Key | Value |
| --- | --- |
| `dir` | The store's directory |
| `key_file` | A file holding the key |
| `key_env` | The variable holding the key, `SUBMILLI_SECRET_KEY` by default |

`key_file` wins when both are set. Without a key the store is off and secret
commands answer `no secret store is configured on this server`. A key that
can't be read stops the server from starting. Values go in and never come
back out through the API.

## `volumes`

Named volumes a Blueprint may use as its filesystem root or mount under
`vfs.mounts`. Each entry maps a name to:

| Key | Value |
| --- | --- |
| `kind` | Required. `managed-local`: the server keeps the files at `<volume_dir>/<name>`, created on first use. `local-path`: the files are in the directory `path` names, which the server never creates or deletes. |
| `path` | Required for `local-path`, refused for `managed-local`. An absolute path. |
| `access` | `read_write` (default) or `read_only`. A Blueprint can narrow it, never widen it. |
| `size_limit` | Required. A size such as `500MB` or `10GB`, or `unlimited`. Units are binary: `KB` is 1,024 bytes, `MB` 1,024 KB, `GB` 1,024 MB, `TB` 1,024 GB. `B` or no unit means bytes. |

```yaml title="server.yaml (fragment)"
volumes:
  project-memory:
    kind: managed-local
    size_limit: 1GB
```

One `size_limit` covers the volume across every session and Blueprint that
uses it, and the server never deletes a volume's files. A declaration that
is incomplete, overlaps a server directory or another volume, or names one
directory twice stops the server from starting. [Mount a shared
volume](/docs/server/mount-a-shared-volume) shows a volume in use.

## `network`

The server blocks every outbound connection a program causes, through
`submilli:http`, Git, model providers, and MCP servers, from reaching an
internal address, whatever the Blueprint allows. It checks the addresses a
name resolves to, so a public name pointing inside is blocked too.

| Addresses | Blocked unless |
| --- | --- |
| Loopback: `127.0.0.0/8`, `::1` | `allow_localhost` is on, or `allow_ip` covers the address |
| Private: `10.0.0.0/8`, `172.16.0.0/12`, `192.168.0.0/16`, `100.64.0.0/10`, `fc00::/7` | `allow_private` is on, or `allow_ip` covers the address |
| Link-local, including the cloud metadata endpoint `169.254.169.254`: `169.254.0.0/16`, `fe80::/10`; broadcast `255.255.255.255`; unspecified `0.0.0.0`, `::` | `allow_ip` covers the address |

| Key | Value |
| --- | --- |
| `allow_ip` | A list of addresses (`10.0.12.7`) or CIDR ranges (`10.0.0.0/24`). An address it covers is always let through. |
| `allow_localhost` | Lets loopback through. |
| `allow_private` | Lets every private range through. |

Grants add up across the flags, variables, and file, and none revokes
another's. A blocked call throws an `Error` the program can catch. Inside a
container, `localhost` is the container itself. Reach a service on the host
or in another container through its address, with `allow_ip`.

## Limits

The `max_*` settings bound each run and the server as a whole, and a
Blueprint can't raise them. [Errors and
limits](/docs/reference/errors-and-limits) lists what each one bounds and
what a program sees. [Set limits](/docs/server/set-limits) shows how to
choose them.

## `mcp_oauth`

`mcp_oauth.providers` registers the OAuth client the server logs in as for
MCP servers that need one:

| Key | Value |
| --- | --- |
| `match` | Required. The authorization server's host, such as `github.com`. |
| `client_id` | Required. The client ID. |
| `client_secret` | A literal, `${env.VAR}`, or `${secrets.KEY}` from the secret store, resolved when used. |
| `scopes` | A list of scopes to request. |

```yaml title="server.yaml (fragment)"
mcp_oauth:
  providers:
    - match: github.com
      client_id: Iv1.example
      client_secret: ${secrets.GITHUB_CLIENT_SECRET}
```

## `github_token_file`

A file holding the GitHub token the server sends when it installs Packages,
so installs can reach private repositories. Without it, installs reach only public ones.
The file is read again on every install, so replacing it rotates the token.
[Install private Packages on a server](/docs/server/install-private-packages)
shows it in use.

## `telemetry`

Off by default. When on, the server reports to the Submilli maintainers:

| Reported | When |
| --- | --- |
| Crashes; usage counters (sessions opened, programs run by outcome, Package and built-in lookups, Blueprint changes); for each failed program, its error `kind` and the first line of its message | `telemetry` is on |
| The failed program's source and its full error, including the backtrace or the diagnostics that quote its lines | `telemetry` and `telemetry_include_source` are both on |
| Client IP addresses, request headers, the hosts of programs' outbound calls | Never |

## Health and status

`GET /healthz` answers `200` with no token once the process is serving.
`submilli-server --health-check` probes it and exits `0` when the server
answers, reading the address from its own config file and environment, not
the serving process's flags. `GET /v1/status`, with an `admin` token, returns
the bind address, process ID, open sessions, and registered Blueprints.

## Logs

The server logs to standard output at `info` and above. `RUST_LOG` sets the
filter, such as `RUST_LOG=submilli_server=debug`. To append to a file instead:

```yaml title="server.yaml (fragment)"
logging:
  file: /var/log/submilli/server.log
```

`--log-file` overrides `SUBMILLI_LOG_FILE`, which overrides `logging.file`.
The parent directory must exist. An invalid or inaccessible file fails startup.
Existing contents are retained across restarts.

On Unix, `SIGHUP` reopens the configured path after external rotation. If it
fails, the server keeps the old file and reports the error to standard error.
The server does no rotation or retention itself.

Records use logfmt, one event per line, with no ANSI colours. The leading keys
are `ts` (UTC, milliseconds), `level`, `stream=log`, `target`, and `msg`, followed
by event fields. Values are quoted and escaped when needed.

Every program the server runs adds an execution record:

```text
ts=2026-10-03T15:29:46.963Z level=info stream=log target=submilli_server::execute msg="execution finished" blueprint=probe session=e4940e90-2fb3-48df-baea-af505766958f fuel=100000 wasm_fuel=99943 host_fuel=57 memory_peak=65536 wall_ms=19 outcome=fuel_exhausted
```

| Field | Value |
| --- | --- |
| `blueprint` | The Blueprint the program ran under. |
| `session` | The session ID. |
| `fuel` | Fuel consumed, `wasm_fuel` plus `host_fuel`. |
| `wasm_fuel` | Fuel the program's own instructions consumed. |
| `host_fuel` | Fuel the standard library charged for work done on the program's behalf. |
| `memory_peak` | The most memory the run held, in bytes. |
| `wall_ms` | Elapsed milliseconds. |
| `outcome` | `ok`, `fuel_exhausted`, `timeout`, `memory_exhausted`, or `error`. |

## Audit trail

The server writes audit records alongside its log, as logfmt lines with
`stream=audit`. `RUST_LOG` doesn't filter them. They go to the log's
output, or to `logging.audit.file`, which, like the log file, is
appended to and reopened on `SIGHUP`.

```yaml title="server.yaml (fragment)"
logging:
  audit:
    file: /var/log/submilli/audit.log
    allows: summary
```

`allows` decides how operations a program was allowed are recorded.
`summary` writes one record per run for each caller, capability, and
rule, with a count. `all` writes one per operation, and `none` writes none.
Refusals are always recorded one by one. The complete JSON context passed
to a Package or built-in permission check is recorded. Secure the audit
destination accordingly.

Writing a record never stops a run. If the output can't be written, the
server reports it to standard error and keeps serving. [Audit
trail](/docs/reference/audit-trail) lists every record and its fields.

## Shutdown

SIGTERM, SIGINT, `submilli server stop`, and `POST /v1/shutdown` each stop the
server. It stops accepting connections and new request work. Running handlers
continue even if their clients disconnect. Connected clients can receive their
responses while the server drains.

HTTP responses, owned request tasks, and database cleanup share the
`shutdown_grace` deadline. When the deadline expires, unfinished handlers are
cancelled. A second signal skips the remaining wait. Database cleanup can
continue until the process exits.

A disconnect during request-body upload can still cause a read error. Streaming
a response remains tied to its connection.

## Command-line help

<!-- generated:server-cli -->

```text
Submilli HTTP execution server

Usage: submilli-server [OPTIONS]

Options:
      --config <CONFIG>
          YAML config file supplying values for the options below. Any flag passed on the command line overrides the corresponding file value. Env: `$SUBMILLI_CONFIG`
      --log-file <PATH>
          Append server logs to this file instead of standard output. The parent directory must exist. On Unix, SIGHUP reopens it for external rotation. Env: `$SUBMILLI_LOG_FILE`, which outranks the config file
      --bind <BIND>
          Address to bind. Falls back to the `$HOST` env var, or `0.0.0.0` when `$PORT` is set (so it's reachable on Render and similar hosts). [default: 127.0.0.1] Env: `$SUBMILLI_BIND`, which outranks the config file and `$HOST`
      --port <PORT>
          TCP port to listen on. Falls back to the `$PORT` env var (set by Render and similar hosts), then 8128. [default: 8128] Env: `$SUBMILLI_PORT`, which outranks the config file and `$PORT`
      --tls-cert-file <PATH>
          Certificate chain PEM file. HTTPS is enabled only when both TLS files are set. Env: `$SUBMILLI_TLS_CERT_FILE`
      --tls-key-file <PATH>
          Private key PEM file matching the certificate. Read at startup; restart to rotate. Env: `$SUBMILLI_TLS_KEY_FILE`
      --blueprint-dir <BLUEPRINT_DIR>
          Source directory for the one-time blueprint import into SQLite. The directory moves to archive/blueprints/ beside its original location. [default: ~/.submilli/server/blueprints (override the base with $SUBMILLI_HOME)] Env: `$SUBMILLI_BLUEPRINT_DIR`
      --session-store-dir <SESSION_STORE_DIR>
          Directory the session lifecycle store persists to and loads from on startup — the bookkeeping that makes resume and idle reaping survive a restart. Mount on durable storage. [default: ~/.submilli/server/sessions] Env: `$SUBMILLI_SESSION_STORE_DIR`
      --database-path <PATH>
          Server SQLite database file. The parent directory is created at boot. [default: ~/.submilli/server/db/submilli.db] Env: `$SUBMILLI_DATABASE_PATH`
      --vfs-session-dir <VFS_SESSION_DIR>
          Durable root for `per_session` VFS directories. Mount on a PersistentVolume so a session's files survive a server restart. [default: ~/.submilli/server/vfs/sessions] Env: `$SUBMILLI_VFS_SESSION_DIR`
      --vfs-ephemeral-dir <VFS_EPHEMERAL_DIR>
          Root for `ephemeral` scratch directories. Defaults to the OS temp dir; point it at volatile storage (tmpfs / `emptyDir`) to keep them off the durable volume. Env: `$SUBMILLI_VFS_EPHEMERAL_DIR`
      --volume-dir <VOLUME_DIR>
          Root for `managed-local` named volumes, one directory per volume name. Mount on durable storage: named volumes outlive sessions and restarts. The server creates a volume's directory on first use and never deletes it. [default: ~/.submilli/server/volumes] Env: `$SUBMILLI_VOLUME_DIR`
      --secret-store-dir <SECRET_STORE_DIR>
          Directory backing the encrypted secret store (one sealed file per secret). Not the CLI's store at ~/.submilli/secrets, which `submilli secret put` and `mcp authenticate` fill and `submilli run` reads. [default: ~/.submilli/server/secrets] Env: `$SUBMILLI_SECRET_STORE_DIR`
      --package-store-dir <PACKAGE_STORE_DIR>
          Root of the package store that `submilli server packages install` writes to and the runtime loads packages from first. Created if absent. Packages published locally with `submilli build publish-local` or `submilli install` (~/.submilli/packages) are readable as a fallback and never written. [default: ~/.submilli/server/packages] Env: `$SUBMILLI_PACKAGE_STORE_DIR`
      --secret-store-key-env <SECRET_STORE_KEY_ENV>
          Name of the env var holding the base64-encoded 32-byte store key. The store enables itself when this var is set; it stays off when unset. The key itself is never passed on the command line. [default: SUBMILLI_SECRET_KEY] Env: `$SUBMILLI_SECRET_STORE_KEY_ENV`
      --secret-store-key-file <SECRET_STORE_KEY_FILE>
          Path to a file holding the base64-encoded 32-byte store key. Takes priority over `--secret-store-key-env` when both are given. Env: `$SUBMILLI_SECRET_STORE_KEY_FILE`
      --allow-localhost
          Permit outbound HTTP to IPv4 + IPv6 loopback. Off by default to block SSRF against services on the server host. Additive with the config file: enabling on either side grants it. Env: `$SUBMILLI_ALLOW_LOCALHOST` (`1`/`true`/`yes`/`on`), also additive
      --allow-private
          Permit outbound HTTP to all RFC1918 / CGNAT / IPv6-ULA private ranges. Off by default to block SSRF against the internal network. Additive with the config file. Env: `$SUBMILLI_ALLOW_PRIVATE` (`1`/`true`/`yes`/`on`), also additive
      --allow-ip <IP|CIDR>
          Permit outbound HTTP to a specific address or CIDR range, overriding the default block (repeatable). Accepts `1.2.3.4` or `10.0.0.0/24`. Env: `$SUBMILLI_ALLOW_IP` (comma-separated), additive with both
      --mcp-allowed-host <HOST>
          Extra `Host` header the MCP endpoint accepts, on top of the loopback defaults (rmcp's DNS-rebinding guard); repeatable. Behind a reverse proxy or PaaS, set the host the client targets — e.g. `submilli-ai:10000` on Render or `your-app.onrender.com`. Also settable via the config file or `$SUBMILLI_MCP_ALLOWED_HOSTS` (comma-separated)
      --shutdown-grace <SECONDS>
          How long in-flight requests may keep running after SIGTERM or SIGINT before their connections are dropped; a second signal skips the rest of the wait. Teardown adds up to ~1.3s on top, so keep the total under the container runtime's own grace period (Docker allows 10s) — overshoot and the drain is SIGKILLed halfway through instead. [default: 5] Env: `$SUBMILLI_SHUTDOWN_GRACE`, which outranks the config file
      --max-execution-memory <MEGABYTES>
          Memory one execution may hold live, in megabytes. An allocation that would pass it ends the run with `memory exhausted`, instead of growing until the host or the container's own limit stops it. This is what makes a container's `--memory` sizeable: budget roughly this times peak concurrency. Note that strings are UTF-16, so text costs two bytes per character — a 25 MB document needs ~50 MB here. [default: 50] Env: `$SUBMILLI_MAX_EXECUTION_MEMORY`, which outranks the config file
      --max-execution-time <SECONDS>
          Execution timeout in whole seconds; 0 disables it. [default: disabled] Starts at imported packages' top-level statements; epoch checks may interrupt up to one tick later. Pending host calls are not cancelled by this timeout. Env: `$SUBMILLI_MAX_EXECUTION_TIME`, which outranks the config file
      --max-execution-fuel <FUEL>
          Fuel one execution may burn, roughly one unit per Wasm instruction; a program that runs out ends with `fuel exhausted`. The backstop for a runaway loop when no execution time is set. [default: 1000000000000] Env: `$SUBMILLI_MAX_EXECUTION_FUEL`, which outranks the config file. Accepts decimal K/M/B/T suffixes and digit separators, e.g. 1T or 10_000
      --max-execution-stack <KIBIBYTES>
          Wasm stack one execution may use, in kibibytes; deeper recursion ends with `call stack exhausted`. [default: 512] Env: `$SUBMILLI_MAX_EXECUTION_STACK`, which outranks the config file
      --max-session-state-memory <MEGABYTES>
          Memory every live session's `submilli:session` state may hold *in total*, in megabytes. Unlike `--max-execution-memory`, which bounds one execution, this bounds the process against session count: reservations are taken atomically, so a `set` that would push the server past this is refused rather than evicting another session's state. [default: 1024] Env: `$SUBMILLI_MAX_SESSION_STATE_MEMORY`, which outranks the config file
      --max-llm-tokens <TOKENS>
          Tokens every live execution's `submilli:llm` calls may spend *in total*. This is the ceiling on what the server can spend against the operator's provider credential: reservations cover the prompt plus the reserved output before dispatch, so a call that would push the server past this is refused rather than billed. [default: 20000000] Env: `$SUBMILLI_MAX_LLM_TOKENS`, which outranks the config file. Accepts decimal K/M/B/T suffixes and digit separators, e.g. 20M or 10_000
      --max-execution-llm-tokens <TOKENS>
          Tokens a *single* execution's `submilli:llm` calls may spend. Bounds one run where `--max-llm-tokens` bounds the process, so one program cannot consume the whole server's budget. [default: 1000000] Env: `$SUBMILLI_MAX_EXECUTION_LLM_TOKENS`, which outranks the config file. Accepts decimal K/M/B/T suffixes and digit separators, e.g. 1M or 10_000
      --max-llm-concurrency <PROMPTS>
          Prompts one `llm.batch` dispatches at once. Bounded deliberately: unbounded fan-out manufactures the rate-limit errors it then cannot honor a `retry-after` against. [default: 4] Env: `$SUBMILLI_MAX_LLM_CONCURRENCY`, which outranks the config file
      --allow-unauthenticated
          Serve the API without authentication: every caller that can reach the port has full access. The server otherwise refuses to start until it has a token — `$SUBMILLI_SERVER_TOKEN`, which is an admin token, or entries under `api_tokens` in the config file. For a server whose network already admits only its own application, and for local experiments. Cannot be combined with either source of tokens. Env: `$SUBMILLI_ALLOW_UNAUTHENTICATED` (`1`/`true`/`yes`/`on`)
      --health-check
          Probe a running server and exit 0 when it answers, non-zero otherwise — the container `HEALTHCHECK`, which has no shell or `curl` to call. The address is resolved from this process's own config file and environment, so the probe follows a non-default bind or port instead of drifting from it. It cannot see flags given only to the serving process, so a container should set the address via `$SUBMILLI_BIND`/`$SUBMILLI_PORT` or `--config` rather than `CMD` arguments
  -h, --help
          Print help
  -V, --version
          Print version
```

<!-- /generated:server-cli -->
