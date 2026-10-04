use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr};
use std::path::PathBuf;
use std::time::Duration;

use anyhow::{Context, Result};
use clap::Parser;
use ipnet::IpNet;
use submilli_server::serve;

mod count;
mod file_config;
mod migrate;

/// Long enough for a loaded server to answer `/healthz`, short enough to land
/// inside the image's `HEALTHCHECK --timeout=5s` — so a hung probe reports its
/// own failure with a reason instead of being killed by the daemon.
const HEALTH_CHECK_TIMEOUT: Duration = Duration::from_secs(4);

/// Shutting down has three stages, and only the first is `--shutdown-grace`.
/// The other two are bounded here because they are otherwise unbounded and
/// stack on top of it: the container runtime is timing the whole process, not
/// the drain, and overshooting its window means SIGKILL — the exact outcome the
/// grace period exists to avoid.
///
/// Stage 2: axum spawns a task per connection, so the dropped server future
/// leaves work running. A plain runtime drop waits for it (and cannot cancel a
/// `spawn_blocking` task at all); `shutdown_timeout` returns regardless.
const RUNTIME_TEARDOWN_BUDGET: Duration = Duration::from_millis(250);

/// Stage 3: `ClientInitGuard::drop` blocks while it flushes queued events, and
/// sentry's own default is 2s — too much to inherit silently when it lands
/// after everything else.
const TELEMETRY_FLUSH_BUDGET: Duration = Duration::from_secs(1);

#[derive(Parser)]
#[command(
    name = "submilli-server",
    version,
    about = "Submilli HTTP execution server"
)]
pub struct Cli {
    /// YAML config file supplying values for the options below. Any flag passed
    /// on the command line overrides the corresponding file value.
    /// Env: `$SUBMILLI_CONFIG`.
    #[arg(long)]
    config: Option<PathBuf>,

    /// Append server logs to this file instead of standard output. The parent
    /// directory must exist. On Unix, SIGHUP reopens it for external rotation.
    /// Env: `$SUBMILLI_LOG_FILE`, which outranks the config file.
    #[arg(long, value_name = "PATH")]
    log_file: Option<PathBuf>,

    /// Address to bind. Falls back to the `$HOST` env var, or `0.0.0.0` when
    /// `$PORT` is set (so it's reachable on Render and similar hosts).
    /// [default: 127.0.0.1]
    /// Env: `$SUBMILLI_BIND`, which outranks the config file and `$HOST`.
    #[arg(long)]
    bind: Option<IpAddr>,

    /// TCP port to listen on. Falls back to the `$PORT` env var (set by Render and
    /// similar hosts), then 8128. [default: 8128]
    /// Env: `$SUBMILLI_PORT`, which outranks the config file and `$PORT`.
    #[arg(long)]
    port: Option<u16>,

    /// Certificate chain PEM file. HTTPS is enabled only when both TLS files are set.
    /// Env: `$SUBMILLI_TLS_CERT_FILE`.
    #[arg(long, value_name = "PATH")]
    tls_cert_file: Option<PathBuf>,

    /// Private key PEM file matching the certificate. Read at startup; restart to rotate.
    /// Env: `$SUBMILLI_TLS_KEY_FILE`.
    #[arg(long, value_name = "PATH")]
    tls_key_file: Option<PathBuf>,

    /// Directory the registered blueprints are persisted to and loaded from on
    /// startup. Created if absent. [default: ~/.submilli/server/blueprints
    /// (override the base with $SUBMILLI_HOME)]
    /// Env: `$SUBMILLI_BLUEPRINT_DIR`.
    #[arg(long)]
    blueprint_dir: Option<PathBuf>,

    /// Directory the session lifecycle store persists to and loads from on
    /// startup — the bookkeeping that makes resume and idle reaping survive a
    /// restart. Mount on durable storage. [default: ~/.submilli/server/sessions]
    /// Env: `$SUBMILLI_SESSION_STORE_DIR`.
    #[arg(long)]
    session_store_dir: Option<PathBuf>,

    /// Durable root for `per_session` VFS directories. Mount on a
    /// PersistentVolume so a session's files survive a server restart.
    /// [default: ~/.submilli/server/vfs/sessions]
    /// Env: `$SUBMILLI_VFS_SESSION_DIR`.
    #[arg(long)]
    vfs_session_dir: Option<PathBuf>,

    /// Root for `ephemeral` scratch directories. Defaults to the OS temp dir;
    /// point it at volatile storage (tmpfs / `emptyDir`) to keep them off the
    /// durable volume.
    /// Env: `$SUBMILLI_VFS_EPHEMERAL_DIR`.
    #[arg(long)]
    vfs_ephemeral_dir: Option<PathBuf>,

    /// Root for `managed-local` named volumes, one directory per volume name.
    /// Mount on durable storage: named volumes outlive sessions and restarts.
    /// The server creates a volume's directory on first use and never deletes
    /// it. [default: ~/.submilli/server/volumes]
    /// Env: `$SUBMILLI_VOLUME_DIR`.
    #[arg(long)]
    volume_dir: Option<PathBuf>,

    /// Directory backing the encrypted secret store (one sealed file per
    /// secret). Not the CLI's store at ~/.submilli/secrets, which
    /// `submilli secret put` and `mcp authenticate` fill and `submilli run`
    /// reads.
    /// [default: ~/.submilli/server/secrets]
    /// Env: `$SUBMILLI_SECRET_STORE_DIR`.
    #[arg(long)]
    secret_store_dir: Option<PathBuf>,

    /// Root of the package store that `submilli server packages install` writes to
    /// and the runtime loads packages from first. Created if absent. Packages
    /// published locally with `submilli build publish-local` or `submilli install`
    /// (~/.submilli/packages) are readable as a fallback and never written.
    /// [default: ~/.submilli/server/packages]
    /// Env: `$SUBMILLI_PACKAGE_STORE_DIR`.
    #[arg(long)]
    package_store_dir: Option<PathBuf>,

    /// Name of the env var holding the base64-encoded 32-byte store key. The
    /// store enables itself when this var is set; it stays off when unset. The
    /// key itself is never passed on the command line. [default: SUBMILLI_SECRET_KEY]
    /// Env: `$SUBMILLI_SECRET_STORE_KEY_ENV`.
    #[arg(long)]
    secret_store_key_env: Option<String>,

    /// Path to a file holding the base64-encoded 32-byte store key. Takes
    /// priority over `--secret-store-key-env` when both are given.
    /// Env: `$SUBMILLI_SECRET_STORE_KEY_FILE`.
    #[arg(long)]
    secret_store_key_file: Option<PathBuf>,

    /// Permit outbound HTTP to IPv4 + IPv6 loopback. Off by default to block
    /// SSRF against services on the server host. Additive with the config file:
    /// enabling on either side grants it.
    /// Env: `$SUBMILLI_ALLOW_LOCALHOST` (`1`/`true`/`yes`/`on`), also additive.
    #[arg(long)]
    allow_localhost: bool,

    /// Permit outbound HTTP to all RFC1918 / CGNAT / IPv6-ULA private ranges.
    /// Off by default to block SSRF against the internal network. Additive with
    /// the config file.
    /// Env: `$SUBMILLI_ALLOW_PRIVATE` (`1`/`true`/`yes`/`on`), also additive.
    #[arg(long)]
    allow_private: bool,

    /// Permit outbound HTTP to a specific address or CIDR range, overriding the
    /// default block (repeatable). Accepts `1.2.3.4` or `10.0.0.0/24`.
    /// Env: `$SUBMILLI_ALLOW_IP` (comma-separated), additive with both.
    #[arg(long = "allow-ip", value_name = "IP|CIDR", value_parser = file_config::parse_ip_or_cidr)]
    allow_ip: Vec<IpNet>,

    /// Extra `Host` header the MCP endpoint accepts, on top of the loopback
    /// defaults (rmcp's DNS-rebinding guard); repeatable. Behind a reverse proxy
    /// or PaaS, set the host the client targets — e.g. `submilli-ai:10000` on
    /// Render or `your-app.onrender.com`. Also settable via the config file or
    /// `$SUBMILLI_MCP_ALLOWED_HOSTS` (comma-separated).
    #[arg(long = "mcp-allowed-host", value_name = "HOST")]
    mcp_allowed_host: Vec<String>,

    /// How long in-flight requests may keep running after SIGTERM or SIGINT
    /// before their connections are dropped; a second signal skips the rest of
    /// the wait. Teardown adds up to ~1.3s on top, so keep the total under the
    /// container runtime's own grace period (Docker allows 10s) — overshoot and
    /// the drain is SIGKILLed halfway through instead. [default: 5]
    /// Env: `$SUBMILLI_SHUTDOWN_GRACE`, which outranks the config file.
    #[arg(long, value_name = "SECONDS")]
    shutdown_grace: Option<u64>,

    /// Memory one execution may hold live, in megabytes. An allocation that
    /// would pass it ends the run with `memory exhausted`, instead of growing
    /// until the host or the container's own limit stops it. This is what
    /// makes a container's `--memory` sizeable: budget roughly this times peak
    /// concurrency.
    /// Note that strings are UTF-16, so text costs two bytes per character —
    /// a 25 MB document needs ~50 MB here. [default: 50]
    /// Env: `$SUBMILLI_MAX_EXECUTION_MEMORY`, which outranks the config file.
    #[arg(long, value_name = "MEGABYTES")]
    max_execution_memory: Option<u64>,

    /// Execution timeout in whole seconds; 0 disables it. [default: disabled]
    /// Starts at main; epoch checks may interrupt up to one tick later.
    /// Pending host calls are not cancelled by this timeout.
    /// Env: `$SUBMILLI_MAX_EXECUTION_TIME`, which outranks the config file.
    #[arg(long, value_name = "SECONDS")]
    max_execution_time: Option<u64>,

    /// Fuel one execution may burn, roughly one unit per Wasm instruction; a
    /// program that runs out ends with `fuel exhausted`. The backstop for a
    /// runaway loop when no execution time is set. [default: 1000000000000]
    /// Env: `$SUBMILLI_MAX_EXECUTION_FUEL`, which outranks the config file.
    /// Accepts decimal K/M/B/T suffixes and digit separators, e.g. 1T or 10_000.
    #[arg(long, value_name = "FUEL", value_parser = count::parse_count)]
    max_execution_fuel: Option<u64>,

    /// Wasm stack one execution may use, in kibibytes; deeper recursion ends
    /// with `call stack exhausted`. [default: 512]
    /// Env: `$SUBMILLI_MAX_EXECUTION_STACK`, which outranks the config file.
    #[arg(long, value_name = "KIBIBYTES")]
    max_execution_stack: Option<u64>,

    /// Memory every live session's `submilli:session` state may hold *in total*,
    /// in megabytes. Unlike `--max-execution-memory`, which bounds one execution,
    /// this bounds the process against session count: reservations are taken
    /// atomically, so a `set` that would push the server past this is refused
    /// rather than evicting another session's state. [default: 1024]
    /// Env: `$SUBMILLI_MAX_SESSION_STATE_MEMORY`, which outranks the config file.
    #[arg(long, value_name = "MEGABYTES")]
    max_session_state_memory: Option<u64>,

    /// Tokens every live execution's `submilli:llm` calls may spend *in total*.
    /// This is the ceiling on what the server can spend against the operator's
    /// provider credential: reservations cover the prompt plus the reserved
    /// output before dispatch, so a call that would push the server past this is
    /// refused rather than billed. [default: 20000000]
    /// Env: `$SUBMILLI_MAX_LLM_TOKENS`, which outranks the config file.
    /// Accepts decimal K/M/B/T suffixes and digit separators, e.g. 20M or 10_000.
    #[arg(long, value_name = "TOKENS", value_parser = count::parse_count)]
    max_llm_tokens: Option<u64>,

    /// Tokens a *single* execution's `submilli:llm` calls may spend. Bounds one
    /// run where `--max-llm-tokens` bounds the process, so one program cannot
    /// consume the whole server's budget. [default: 1000000]
    /// Env: `$SUBMILLI_MAX_EXECUTION_LLM_TOKENS`, which outranks the config file.
    /// Accepts decimal K/M/B/T suffixes and digit separators, e.g. 1M or 10_000.
    #[arg(long, value_name = "TOKENS", value_parser = count::parse_count)]
    max_execution_llm_tokens: Option<u64>,

    /// Prompts one `llm.batch` dispatches at once. Bounded deliberately:
    /// unbounded fan-out manufactures the rate-limit errors it then cannot honor
    /// a `retry-after` against. [default: 4]
    /// Env: `$SUBMILLI_MAX_LLM_CONCURRENCY`, which outranks the config file.
    #[arg(long, value_name = "PROMPTS")]
    max_llm_concurrency: Option<usize>,

    /// Serve the API without authentication: every caller that can reach the
    /// port has full access. The server otherwise refuses to start until it
    /// has a token — `$SUBMILLI_SERVER_TOKEN`, which is an admin token, or
    /// entries under `api_tokens` in the config file. For a server whose
    /// network already admits only its own application, and for local
    /// experiments. Cannot be combined with either source of tokens.
    /// Env: `$SUBMILLI_ALLOW_UNAUTHENTICATED` (`1`/`true`/`yes`/`on`).
    #[arg(long)]
    allow_unauthenticated: bool,

    /// Probe a running server and exit 0 when it answers, non-zero otherwise —
    /// the container `HEALTHCHECK`, which has no shell or `curl` to call. The
    /// address is resolved from this process's own config file and environment,
    /// so the probe follows a non-default bind or port instead of drifting from
    /// it. It cannot see flags given only to the serving process, so a container
    /// should set the address via `$SUBMILLI_BIND`/`$SUBMILLI_PORT` or
    /// `--config` rather than `CMD` arguments.
    #[arg(long)]
    health_check: bool,
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    // Ahead of the runtime and Sentry: the probe runs every 30s for the life of
    // the container, and should pay for neither a runtime nor a liveness metric
    // swamping the real invocation counts.
    if cli.health_check {
        return health_check(&cli);
    }

    let mut resolved = file_config::resolve(cli)?;

    // Init before the async runtime starts so the guard binds the Sentry hub for
    // every worker thread the runtime spawns. Skipped when telemetry is disabled
    // (the default unless explicitly enabled through the environment or config).
    submilli_server::runner::set_telemetry_include_source(resolved.telemetry_include_source);
    let _guard = resolved.telemetry.then(|| {
        sentry::init((
            "https://3de786dd0e1733e40a3e3425ab3e4ddc@o4511530557702144.ingest.us.sentry.io/4511530561110016",
            sentry::ClientOptions {
                release: sentry::release_name!(),
                // Never send client IPs or request headers; nothing here needs
                // them and there is no reason to store them.
                // https://docs.sentry.io/platforms/rust/data-management/data-collected
                send_default_pii: false,
                shutdown_timeout: TELEMETRY_FLUSH_BUDGET,
                ..Default::default()
            },
        ))
    });

    let log_output = submilli_server::logging::LogOutput::open(resolved.log_file)?;
    tracing_subscriber::fmt()
        .with_ansi(false)
        .log_internal_errors(false)
        .event_format(submilli_server::logging::Logfmt::default())
        .fmt_fields(submilli_server::logging::LogfmtFields)
        .with_writer(log_output.clone())
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "submilli_server=info,info".into()),
        )
        .try_init()
        .map_err(|error| anyhow::anyhow!("cannot initialize server logging: {error}"))?;

    // Only now is there a subscriber to warn to.
    if let Some(migration) = &resolved.migration {
        log_migration(migration);
    }
    let egress_grants = file_config::env_egress_grants();
    if !egress_grants.is_empty() {
        tracing::warn!(
            vars = egress_grants.join(", "),
            "the outbound egress guard was widened by environment variables; the config file cannot revoke these"
        );
    }

    let runtime = submilli_server::runtime(&resolved.config)?;
    let audit = submilli_server::audit::AuditLog::new(
        resolved.config.audit.clone(),
        Some(log_output.clone()),
    );
    resolved.config.audit_log = Some(audit.clone());
    let audit_reopen = submilli_server::logging::ReopenTask::start(audit.output())
        .map_err(|_| {
            use std::io::Write;
            let _ = writeln!(
                std::io::stderr().lock(),
                "cannot start audit log rotation monitor"
            );
        })
        .ok();
    let reopen = submilli_server::logging::ReopenTask::start(log_output)?;
    let result = runtime.block_on(serve(
        resolved.addr,
        resolved.config,
        resolved.shutdown_grace,
    ));
    let reopen_result = reopen.stop();
    if let Some(audit_reopen) = audit_reopen
        && audit_reopen.stop().is_err()
    {
        use std::io::Write;
        let _ = writeln!(
            std::io::stderr().lock(),
            "cannot stop audit log rotation monitor"
        );
    }
    // Consumes the runtime, so this replaces the implicit drop rather than
    // preceding it — the drop is what would otherwise wait indefinitely.
    runtime.shutdown_timeout(RUNTIME_TEARDOWN_BUDGET);
    result.and_then(|()| reopen_result.map_err(Into::into))
}

/// Report what the boot migration did. It ran before the subscriber existed,
/// so this is the first chance to say so.
fn log_migration(migration: &migrate::MigrationReport) {
    if !migration.moved.is_empty() {
        tracing::info!(
            root = %migration.root.display(),
            moved = migration.moved.join(", "),
            "moved the server's state directories under <root>/server; the CLI's packages/ \
             stays where it was"
        );
    }
    if !migration.published.is_empty() {
        tracing::info!(
            from = %migration.staging_dir().display(),
            to = %migration.server_dir().display(),
            published = migration.published.join(", "),
            "published state directories an interrupted migration had staged"
        );
    }
    if migration.resumed
        && migration.moved.is_empty()
        && migration.published.is_empty()
        && migration.secrets.is_none()
        && migration.staged_secrets.is_none()
        && migration.stale_staging.is_none()
        && migration.legacy_split_failed.is_none()
        && migration.beside_server.is_empty()
        && migration.linked_defaults.is_empty()
    {
        tracing::info!(
            root = %migration.root.display(),
            "picked up an interrupted migration; nothing further to move"
        );
    }
    if let Some(secrets) = &migration.secrets {
        tracing::info!(
            moved = secrets.moved.len(),
            left = secrets.left.len(),
            skipped = secrets.skipped.len(),
            legacy = %migration.legacy_secrets_dir().display(),
            "split the secret store by key: entries that open under the configured key moved \
             to server/secrets; entries that do not (the CLI's plaintext values, or blobs \
             sealed under another key) stay in the legacy directory"
        );
        if !secrets.collided.is_empty() {
            tracing::warn!(
                keys = secrets.collided.join(", "),
                legacy = %migration.legacy_secrets_dir().display(),
                "sealed entries left in the CLI's secrets/ because server/secrets already holds \
                 them; the server reads the copies under server/. If a legacy copy is the value \
                 wanted (written by an older release after the split), re-enter it with \
                 `submilli server secret put <key>`; then delete the legacy file \
                 (`submilli secret delete <key>` removes it by name) to silence this"
            );
        }
    }
    if let Some(staged) = &migration.staged_secrets {
        tracing::info!(
            moved = staged.moved.len(),
            left = staged.left.len(),
            staged = %migration.staged_secrets_dir().display(),
            "merged sealed entries an interrupted migration had staged into server/secrets; \
             entries that do not open under the configured key stay staged"
        );
        if !staged.collided.is_empty() {
            tracing::warn!(
                keys = staged.collided.join(", "),
                staged = %migration.staged_secrets_dir().display(),
                "staged sealed entries left in place because server/secrets already holds \
                 them; the copies under server/ are the live ones. Remove the staged files by \
                 hand to silence this"
            );
        }
    }
    if let Some(reason) = &migration.legacy_split_failed {
        tracing::warn!(
            legacy = %migration.legacy_secrets_dir().display(),
            reason,
            "could not finish moving sealed entries from the CLI's secrets/ into \
             server/secrets; entries not moved stay there and are retried on the next keyed boot"
        );
    }
    for path in &migration.linked_defaults {
        tracing::warn!(
            path = %path.display(),
            server = %migration.server_dir().display(),
            "default state directory is a symlink and was not moved; the server now reads the \
             same-named directory under server/. Point the setting at the link's target, or \
             move the target's contents under server/ and remove the link"
        );
    }
    for path in &migration.beside_server {
        tracing::warn!(
            path = %path.display(),
            "legacy state directory found beside an already migrated server/; it is not read. \
             Move any contents under server/ by hand if they are wanted, then remove it"
        );
    }
    for path in &migration.left_behind {
        tracing::warn!(
            path = %path.display(),
            "legacy directory left in place: it still holds something the server does not own, \
             or it could not be removed"
        );
    }
    if let Some(path) = &migration.stale_staging {
        tracing::warn!(
            path = %path.display(),
            "an interrupted migration left state staged here that could not be published: \
             server/ already holds it, it is a sealed secret store this boot's key cannot open \
             (no key, a different key, or an explicit secret-store directory), or it is \
             nothing this migration stages. Move what is wanted under server/ by hand before \
             removing it"
        );
    }
}

/// `GET /healthz` against the address this process's configuration resolves
/// to. Any failure to reach a healthy server surfaces as an `Err`, which exits
/// non-zero. The endpoint needs no token, so the probe never reads one.
fn health_check(cli: &Cli) -> Result<()> {
    let addr = file_config::resolve_bind_addr(cli)?;
    let tls_files = file_config::resolve_tls_files(cli)?;
    let protocol = if tls_files.is_some() { "https" } else { "http" };
    let url = format!("{protocol}://{}/healthz", probe_target(addr));
    let config = ureq::Agent::config_builder()
        .max_redirects(0)
        .timeout_global(Some(HEALTH_CHECK_TIMEOUT))
        .build();
    let agent = if let Some((cert_file, _)) = tls_files {
        let certs = submilli_server::tls::certificates(&cert_file)?;
        let leaf = certs.first().context("TLS certificate chain is empty")?;
        let pin = submilli_shared::tls::fingerprint(leaf)?;
        let verifier = submilli_shared::tls::Verifier::local_probe(pin)?;
        submilli_shared::tls::agent(config, submilli_shared::tls::client_config(verifier)?, &url)?
    } else {
        config.into()
    };
    agent
        .get(&url)
        .call()
        .with_context(|| format!("probing {url}"))?;
    Ok(())
}

/// A wildcard bind is not a connectable address, so probe loopback instead —
/// where a server listening on all interfaces is reachable anyway.
fn probe_target(addr: SocketAddr) -> SocketAddr {
    let ip = match addr.ip() {
        IpAddr::V4(v4) if v4.is_unspecified() => IpAddr::V4(Ipv4Addr::LOCALHOST),
        IpAddr::V6(v6) if v6.is_unspecified() => IpAddr::V6(Ipv6Addr::LOCALHOST),
        ip => ip,
    };
    SocketAddr::new(ip, addr.port())
}
