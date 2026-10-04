//! Configuration sources for the `submilli-server` binary: the YAML config
//! file, the environment, and the CLI flags they layer under.
//!
//! Environment variables occupy two tiers, because they arrive with two very
//! different degrees of intent. Most specific wins:
//!
//! 1. a CLI flag
//! 2. an explicit `SUBMILLI_*` variable — an operator set this for this service
//! 3. the `--config` file — deployment-wide configuration
//! 4. ambient `HOST` / `PORT` — injected by the platform, not a statement about
//!    this app, and notably `PORT`'s mere presence implies a `0.0.0.0` bind
//! 5. the built-in default
//!
//! Collapsing tiers 2 and 4 into one rule breaks something either way. Put all
//! env above the file and a config saying `bind: 127.0.0.1` is silently
//! overridden the moment the service runs anywhere that injects `$PORT` — a
//! server published by ambient platform noise.
//! Put all env below the file and an env-configured deployment (a Helm chart's
//! `values.yaml`, a PaaS dashboard) loses without warning to a mounted
//! ConfigMap.
//!
//! Three pre-existing variables deliberately sit outside this ladder:
//! `SUBMILLI_HOME` only feeds *defaults*, `SUBMILLI_TELEMETRY` has veto
//! semantics, and `SUBMILLI_MCP_ALLOWED_HOSTS` is additive.

use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use anyhow::{Context, Result};
use ipnet::IpNet;
use serde::Deserialize;
use submilli_server::config::{
    OAuthProvider, ServerDirectories, VolumeTable, default_blueprint_dir,
    default_cli_package_store_dir, default_managed_volume_root, default_package_store_dir,
    default_secret_store_dir, default_session_storage_root, default_session_store_dir,
    validate_volumes,
};
use submilli_server::{
    ApiToken, AuthConfig, DEFAULT_MAX_EXECUTION_TOKENS, DEFAULT_MAX_STORE_BYTES, FileSecretStore,
    KeySource, LlmLimits, NetworkPolicy, Role, RuntimeConfig, ServerConfig,
};
use submilli_shared::github::GithubToken;
use submilli_shared::secret_store::SecretStore;
use submilli_shared::secret_store::check_key;

use crate::Cli;

const DEFAULT_BIND: IpAddr = IpAddr::V4(Ipv4Addr::LOCALHOST);
const DEFAULT_PORT: u16 = 8128;
const DEFAULT_SECRET_KEY_ENV: &str = "SUBMILLI_SECRET_KEY";
const SERVER_TOKEN_ENV: &str = "SUBMILLI_SERVER_TOKEN";

/// Leaves room for the two teardown stages that follow the drain (see
/// `main`'s budget) inside Docker's 10s default before it escalates to SIGKILL.
pub(crate) const DEFAULT_SHUTDOWN_GRACE_SECS: u64 = 5;

/// Loopback `Host` values the MCP endpoint always accepts — mirrors rmcp's own
/// default. Operator-supplied hosts are appended to these, so configuring extra
/// hosts never silently drops local access.
const MCP_LOOPBACK_HOSTS: [&str; 3] = ["localhost", "127.0.0.1", "::1"];

#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FileConfig {
    pub bind: Option<IpAddr>,
    pub port: Option<u16>,
    #[serde(default)]
    pub logging: LoggingFileConfig,
    #[serde(default)]
    pub tls: TlsFileConfig,
    pub blueprint_dir: Option<PathBuf>,
    pub session_store_dir: Option<PathBuf>,
    pub vfs_session_dir: Option<PathBuf>,
    pub vfs_ephemeral_dir: Option<PathBuf>,
    /// Root for `managed-local` volumes; see `--volume-dir`.
    pub volume_dir: Option<PathBuf>,
    pub package_store_dir: Option<PathBuf>,
    /// Seconds. `deny_unknown_fields` means omitting this would turn a
    /// `shutdown_grace:` key into a boot failure, contradicting `--config`'s
    /// promise to supply "values for the options below".
    pub shutdown_grace: Option<u64>,
    /// Megabytes of memory one execution may hold live.
    pub max_execution_memory: Option<u64>,
    /// Whole seconds; zero or omission disables execution timeout.
    pub max_execution_time: Option<u64>,
    /// Fuel one execution may burn, roughly one unit per Wasm instruction.
    #[serde(default, deserialize_with = "crate::count::deserialize_optional_count")]
    pub max_execution_fuel: Option<u64>,
    /// Kibibytes of Wasm stack one execution may use.
    pub max_execution_stack: Option<u64>,
    /// Megabytes of `submilli:session` state every live session may hold in
    /// total. Bounds the process against session count, where
    /// `max_execution_memory` bounds a single execution.
    pub max_session_state_memory: Option<u64>,
    /// Tokens every live execution's `submilli:llm` calls may spend in total.
    /// Bounds the process against the operator's provider credential, where
    /// `max_execution_llm_tokens` bounds a single run.
    #[serde(default, deserialize_with = "crate::count::deserialize_optional_count")]
    pub max_llm_tokens: Option<u64>,
    /// Tokens a single execution's `submilli:llm` calls may spend.
    #[serde(default, deserialize_with = "crate::count::deserialize_optional_count")]
    pub max_execution_llm_tokens: Option<u64>,
    /// Prompts one `llm.batch` dispatches at once.
    pub max_llm_concurrency: Option<usize>,
    #[serde(default)]
    pub network: NetworkFileConfig,
    #[serde(default)]
    pub secret_store: SecretStoreFileConfig,
    /// Extra `Host` headers the MCP endpoint accepts, on top of the loopback
    /// defaults (DNS-rebinding guard).
    #[serde(default)]
    pub mcp_allowed_hosts: Vec<String>,
    /// OAuth client apps for MCP servers (`mcp_oauth.providers`).
    #[serde(default)]
    pub mcp_oauth: McpOAuthFileConfig,
    /// Named volumes a blueprint's `vfs: { mode: named, volume: <name> }` or a
    /// `vfs.mounts` entry may name, as `name: {kind, path?, access?,
    /// size_limit}`. File-only, like `mcp_oauth`: the mapping from a name a
    /// blueprint can write to storage on the host is the whole security
    /// boundary, so it stays in one reviewable place rather than spreading
    /// across flags and environment variables.
    #[serde(
        default,
        deserialize_with = "submilli_server::config::deserialize_volume_table"
    )]
    pub volumes: VolumeTable,
    /// The tokens callers authenticate with, beside the admin token
    /// `$SUBMILLI_SERVER_TOKEN` supplies. File-only, like `volumes`: who else
    /// may call the API, and as what, stays in one reviewable place. The
    /// server refuses to start with no token at all unless
    /// `allow_unauthenticated` is set.
    #[serde(default)]
    pub api_tokens: Vec<ApiTokenFileConfig>,
    /// A file holding the GitHub token package installs send, which lets them
    /// reach private repositories. Read again on every install, so replacing
    /// the file rotates the token. File-only, like `api_tokens`: the server's
    /// own credential stays in one reviewable place.
    pub github_token_file: Option<PathBuf>,
    /// Serve without authentication. Additive with `--allow-unauthenticated`
    /// and `$SUBMILLI_ALLOW_UNAUTHENTICATED`, and refused alongside
    /// `api_tokens`, so no source can switch off tokens another configured.
    pub allow_unauthenticated: Option<bool>,
    /// Sentry crash-reporting + metrics. Defaults to off; set `true` to opt in.
    /// `SUBMILLI_TELEMETRY` can also opt in. A config-file `false` or a supplied
    /// environment value other than `1`/`true`/`yes`/`on` disables telemetry.
    pub telemetry: Option<bool>,
    /// Attach the failed program's source code to telemetry error reports.
    /// Off by default even when `telemetry` is on: the source is the most
    /// useful thing for diagnosing a runtime fault and also the most sensitive
    /// thing the report could carry, so it is a separate opt-in. Same rules as
    /// `telemetry`; `SUBMILLI_TELEMETRY_INCLUDE_SOURCE` can also opt in.
    pub telemetry_include_source: Option<bool>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TlsFileConfig {
    pub cert_file: Option<PathBuf>,
    pub key_file: Option<PathBuf>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct McpOAuthFileConfig {
    #[serde(default)]
    pub providers: Vec<OAuthProviderFileConfig>,
}

/// Server log and audit output settings.
#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LoggingFileConfig {
    pub file: Option<PathBuf>,
    #[serde(default)]
    pub audit: submilli_server::audit::AuditConfig,
}

/// One `mcp_oauth.providers` entry. `match` is the authorization-server host.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OAuthProviderFileConfig {
    #[serde(rename = "match")]
    pub match_host: String,
    pub client_id: String,
    /// Literal, `${env.VAR}`, or `${secrets.KEY}` — resolved server-side at use.
    #[serde(default)]
    pub client_secret: Option<String>,
    #[serde(default)]
    pub scopes: Vec<String>,
}

/// One `api_tokens` entry. The token is read from `token_file`; it never
/// appears in the config file itself, and there is deliberately no per-entry
/// environment form — the one token the environment supplies is
/// `$SUBMILLI_SERVER_TOKEN`.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ApiTokenFileConfig {
    /// Identifies the token in logs and error messages.
    pub name: String,
    pub role: Role,
    pub token_file: PathBuf,
}

#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SecretStoreFileConfig {
    pub dir: Option<PathBuf>,
    /// Name of the env var holding the base64 32-byte key. Mutually exclusive
    /// with `key_file`. The key itself never appears in the config file.
    pub key_env: Option<String>,
    /// Path to a file holding the base64 32-byte key. Mutually exclusive with
    /// `key_env`.
    pub key_file: Option<PathBuf>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NetworkFileConfig {
    pub allow_localhost: Option<bool>,
    pub allow_private: Option<bool>,
    /// Each entry is a bare IP or a CIDR, parsed with [`parse_ip_or_cidr`].
    #[serde(default)]
    pub allow_ip: Vec<String>,
}

/// Every environment-sourced setting, read once so the merge below is a pure
/// function of its inputs.
///
/// Tier-2 values stay as raw strings and are parsed during the merge, which
/// returns `Result`. Parsing here would have to discard a bad value, and for
/// `bind` in particular a discarded value fails *open*: the tier below is
/// ambient `$PORT`, whose mere presence implies `0.0.0.0`. So a typo in an
/// operator's `SUBMILLI_BIND=127.0.0.1` would silently publish the server.
/// Ambient `HOST`/`PORT` keep the tolerance
/// they have always had — a platform injects those, and erroring on one would
/// break deployments that never asked for it.
#[derive(Debug, Default)]
pub(crate) struct EnvConfig {
    config: Option<PathBuf>,
    log_file: Option<PathBuf>,
    bind: Option<String>,
    port: Option<String>,
    shutdown_grace: Option<String>,
    max_execution_memory: Option<String>,
    max_execution_time: Option<String>,
    max_execution_fuel: Option<String>,
    max_execution_stack: Option<String>,
    max_session_state_memory: Option<String>,
    max_llm_tokens: Option<String>,
    max_execution_llm_tokens: Option<String>,
    max_llm_concurrency: Option<String>,
    blueprint_dir: Option<PathBuf>,
    session_store_dir: Option<PathBuf>,
    vfs_session_dir: Option<PathBuf>,
    vfs_ephemeral_dir: Option<PathBuf>,
    volume_dir: Option<PathBuf>,
    secret_store_dir: Option<PathBuf>,
    package_store_dir: Option<PathBuf>,
    secret_store_key_env: Option<String>,
    secret_store_key_file: Option<PathBuf>,
    tls_cert_file: Option<PathBuf>,
    tls_key_file: Option<PathBuf>,
    allow_localhost: bool,
    allow_private: bool,
    allow_ip: Vec<String>,
    mcp_allowed_hosts: Vec<String>,
    allow_unauthenticated: bool,
    /// An admin token supplied through the environment; see [`server_token`].
    server_token: Option<String>,
    /// The variable is set but not valid Unicode. Kept apart from "unset" so
    /// a mangled token is reported instead of silently leaving the server
    /// without the token its operator meant it to have.
    server_token_not_unicode: bool,
    /// Tier 4. Injected by Render and similar hosts, which route external
    /// traffic in and expect the service on all interfaces — so `PORT` being
    /// set at all implies a `0.0.0.0` bind.
    ambient_bind: Option<IpAddr>,
    ambient_port: Option<u16>,
}

impl EnvConfig {
    fn from_env() -> Self {
        Self {
            server_token_not_unicode: matches!(
                std::env::var(SERVER_TOKEN_ENV),
                Err(std::env::VarError::NotUnicode(_))
            ),
            ..Self::from_lookup(|name| std::env::var(name).ok())
        }
    }

    /// Reading goes through a lookup so the rules — empty means unset, ambient
    /// values are tolerated — are testable without mutating the process
    /// environment out from under other tests.
    fn from_lookup(lookup: impl Fn(&str) -> Option<String>) -> Self {
        let var = |name: &str| {
            lookup(name)
                .map(|v| v.trim().to_owned())
                .filter(|v| !v.is_empty())
        };
        let path = |name: &str| var(name).map(PathBuf::from);
        // Same vocabulary as the telemetry opt-out, rather than a second
        // convention operators have to learn.
        let flag = |name: &str| {
            matches!(
                var(name).map(|v| v.to_ascii_lowercase()).as_deref(),
                Some("1" | "true" | "yes" | "on")
            )
        };
        let list = |name: &str| {
            var(name)
                .into_iter()
                .flat_map(|raw| {
                    raw.split(',')
                        .map(str::trim)
                        .filter(|s| !s.is_empty())
                        .map(str::to_owned)
                        .collect::<Vec<_>>()
                })
                .collect()
        };

        Self {
            config: path("SUBMILLI_CONFIG"),
            log_file: path("SUBMILLI_LOG_FILE"),
            tls_cert_file: path("SUBMILLI_TLS_CERT_FILE"),
            tls_key_file: path("SUBMILLI_TLS_KEY_FILE"),
            bind: var("SUBMILLI_BIND"),
            port: var("SUBMILLI_PORT"),
            shutdown_grace: var("SUBMILLI_SHUTDOWN_GRACE"),
            max_execution_memory: var("SUBMILLI_MAX_EXECUTION_MEMORY"),
            max_execution_time: var("SUBMILLI_MAX_EXECUTION_TIME"),
            max_execution_fuel: var("SUBMILLI_MAX_EXECUTION_FUEL"),
            max_execution_stack: var("SUBMILLI_MAX_EXECUTION_STACK"),
            max_session_state_memory: var("SUBMILLI_MAX_SESSION_STATE_MEMORY"),
            max_llm_tokens: var("SUBMILLI_MAX_LLM_TOKENS"),
            max_execution_llm_tokens: var("SUBMILLI_MAX_EXECUTION_LLM_TOKENS"),
            max_llm_concurrency: var("SUBMILLI_MAX_LLM_CONCURRENCY"),
            blueprint_dir: path("SUBMILLI_BLUEPRINT_DIR"),
            session_store_dir: path("SUBMILLI_SESSION_STORE_DIR"),
            vfs_session_dir: path("SUBMILLI_VFS_SESSION_DIR"),
            vfs_ephemeral_dir: path("SUBMILLI_VFS_EPHEMERAL_DIR"),
            volume_dir: path("SUBMILLI_VOLUME_DIR"),
            secret_store_dir: path("SUBMILLI_SECRET_STORE_DIR"),
            package_store_dir: path("SUBMILLI_PACKAGE_STORE_DIR"),
            secret_store_key_env: var("SUBMILLI_SECRET_STORE_KEY_ENV"),
            secret_store_key_file: path("SUBMILLI_SECRET_STORE_KEY_FILE"),
            allow_localhost: flag("SUBMILLI_ALLOW_LOCALHOST"),
            allow_private: flag("SUBMILLI_ALLOW_PRIVATE"),
            allow_ip: list("SUBMILLI_ALLOW_IP"),
            mcp_allowed_hosts: list("SUBMILLI_MCP_ALLOWED_HOSTS"),
            allow_unauthenticated: flag("SUBMILLI_ALLOW_UNAUTHENTICATED"),
            server_token: var(SERVER_TOKEN_ENV),
            server_token_not_unicode: false,
            ambient_bind: var("HOST")
                .and_then(|v| v.parse().ok())
                .or_else(|| var("PORT").map(|_| IpAddr::V4(Ipv4Addr::UNSPECIFIED))),
            ambient_port: var("PORT").and_then(|v| v.parse().ok()),
        }
    }

    /// The `SUBMILLI_ALLOW_*` variables that widened the egress guard, if any.
    fn egress_grants(&self) -> Vec<&'static str> {
        let mut granted = Vec::new();
        if self.allow_localhost {
            granted.push("SUBMILLI_ALLOW_LOCALHOST");
        }
        if self.allow_private {
            granted.push("SUBMILLI_ALLOW_PRIVATE");
        }
        if !self.allow_ip.is_empty() {
            granted.push("SUBMILLI_ALLOW_IP");
        }
        granted
    }
}

/// The upper tiers of the ladder, most specific first: CLI flag, explicit
/// `SUBMILLI_*`, config file. Ambient variables sit *below* the file, so
/// callers that have one apply it after this.
fn explicit<T>(cli: Option<T>, env: Option<T>, file: Option<T>) -> Option<T> {
    cli.or(env).or(file)
}

/// Parse a tier-2 variable, failing boot when it is set but unparseable rather
/// than falling through to a weaker source. `expected` completes the sentence
/// "expected ...", so the message names the fix the way `parse_ip_or_cidr`'s
/// does.
fn parse_env<T: std::str::FromStr>(
    name: &str,
    expected: &str,
    raw: Option<&String>,
) -> Result<Option<T>> {
    raw.map(|value| {
        value
            .parse()
            .map_err(|_| anyhow::anyhow!("${name}: expected {expected}, got `{value}`"))
    })
    .transpose()
}

fn parse_env_count(name: &str, raw: Option<&String>) -> Result<Option<u64>> {
    raw.map(|value| {
        crate::count::parse_count(value)
            .map_err(|error| anyhow::anyhow!("${name}: {error}, got `{value}`"))
    })
    .transpose()
}

/// Accept either a bare IP (`1.2.3.4` → host route) or a CIDR (`10.0.0.0/24`).
pub(crate) fn parse_ip_or_cidr(s: &str) -> Result<IpNet, String> {
    if let Ok(net) = s.parse::<IpNet>() {
        return Ok(net);
    }
    s.parse::<IpAddr>()
        .map(IpNet::from)
        .map_err(|_| format!("expected an IP address or CIDR range, got `{s}`"))
}

fn load(path: &Path) -> Result<FileConfig> {
    let text = std::fs::read_to_string(path)
        .with_context(|| format!("reading config file `{}`", path.display()))?;
    serde_yml::from_str(&text).with_context(|| format!("parsing config file `{}`", path.display()))
}

/// Resolve the address + [`ServerConfig`] the server runs with, plus whether
/// telemetry is enabled, reading the `--config` file (when given) and layering
/// the CLI flags on top. Also runs the one-way boot migration of a legacy
/// state layout (see [`crate::migrate`]), once every check that needs no disk
/// has passed.
pub(crate) fn resolve(cli: Cli) -> Result<Resolved> {
    let env = EnvConfig::from_env();
    let file = load_config_file(&cli, &env)?;
    let log_file = logging_file(&cli, &file, &env);
    // The migration is one-way, so every setting that can be refused without
    // touching the disk is checked first: a boot that is going to fail on a
    // bad port, limit, key, or volume must not reshape the volume on its way
    // out. `resolve` and `merge` repeat these cheaply; only opening the secret
    // store, which creates its directory, has to wait.
    preflight(&cli, &file, &env)?;
    let migration = crate::migrate::run(&legacy_layout(&cli, &file, &env))?;
    let telemetry = combine_telemetry(
        std::env::var("SUBMILLI_TELEMETRY").ok().as_deref(),
        file.telemetry,
    );
    let telemetry_include_source = telemetry
        && combine_telemetry(
            std::env::var("SUBMILLI_TELEMETRY_INCLUDE_SOURCE")
                .ok()
                .as_deref(),
            file.telemetry_include_source,
        );
    // A failure from here on exits before any subscriber exists to log the
    // migration, so what it moved is said on stderr before the error goes out.
    let resolved = shutdown_grace(&cli, &file, &env).and_then(|shutdown_grace| {
        merge(cli, file, env).map(|(addr, config)| (addr, config, shutdown_grace))
    });
    if resolved.is_err()
        && let Some(migration) = &migration
    {
        note_migration_before_exit(migration);
    }
    let (addr, config, shutdown_grace) = resolved?;
    Ok(Resolved {
        addr,
        config,
        telemetry,
        telemetry_include_source,
        shutdown_grace,
        migration,
        log_file,
    })
}

/// The settings this boot can refuse without touching any state directory.
fn preflight(cli: &Cli, file: &FileConfig, env: &EnvConfig) -> Result<()> {
    bind_addr(cli, file, env)?;
    if let Some((cert, key)) = tls_files(cli, file, env)? {
        submilli_server::tls::load(&cert, &key)?;
    }
    shutdown_grace(cli, file, env)?;
    resolve_network_policy(cli, file, env)?;
    max_execution_memory(cli, file, env)?;
    max_execution_time(cli, file, env)?;
    max_execution_fuel(cli, file, env)?;
    max_execution_stack(cli, file, env)?;
    max_session_state_memory(cli, file, env)?;
    max_llm_tokens(cli, file, env)?;
    max_execution_llm_tokens(cli, file, env)?;
    max_llm_concurrency(cli, file, env)?;
    resolve_auth(cli, file, env)?;
    if let Some(path) = &file.github_token_file {
        GithubToken::read_file(path).map_err(|e| anyhow::anyhow!("`github_token_file`: {e}"))?;
    }
    if let Some(key) = secret_key_source(cli, file, env) {
        check_key(&key).map_err(|e| anyhow::anyhow!("checking the secret-store key: {e}"))?;
    }
    // The guarded paths are the same before and after the migration: every
    // default it moves sits under the server root either way.
    let directories = guarded_directories(cli, file, env);
    validate_volumes(&file.volumes, &directories)?;
    crate::migrate::validate_dependencies(
        &legacy_layout(cli, file, env),
        &directories,
        &file.volumes,
    )?;
    Ok(())
}

/// Every directory the volume check guards, with defaults applied, resolved
/// without opening anything.
fn guarded_directories(cli: &Cli, file: &FileConfig, env: &EnvConfig) -> ServerDirectories {
    ServerDirectories {
        blueprint_dir: Some(
            explicit(
                cli.blueprint_dir.clone(),
                env.blueprint_dir.clone(),
                file.blueprint_dir.clone(),
            )
            .unwrap_or_else(default_blueprint_dir),
        ),
        package_store_root: Some(
            explicit(
                cli.package_store_dir.clone(),
                env.package_store_dir.clone(),
                file.package_store_dir.clone(),
            )
            .unwrap_or_else(default_package_store_dir),
        ),
        package_fallback_root: Some(default_cli_package_store_dir()),
        secret_store_dir: Some(secret_store_dir(cli, file, env)),
        secret_store_key_file: secret_key_file(cli, file, env),
        api_token_files: file
            .api_tokens
            .iter()
            .map(|token| token.token_file.clone())
            .collect(),
        github_token_file: file.github_token_file.clone(),
        tls_cert_file: explicit(
            cli.tls_cert_file.clone(),
            env.tls_cert_file.clone(),
            file.tls.cert_file.clone(),
        ),
        tls_key_file: explicit(
            cli.tls_key_file.clone(),
            env.tls_key_file.clone(),
            file.tls.key_file.clone(),
        ),
        session_storage_root: Some(
            explicit(
                cli.vfs_session_dir.clone(),
                env.vfs_session_dir.clone(),
                file.vfs_session_dir.clone(),
            )
            .unwrap_or_else(default_session_storage_root),
        ),
        session_store_dir: Some(
            explicit(
                cli.session_store_dir.clone(),
                env.session_store_dir.clone(),
                file.session_store_dir.clone(),
            )
            .unwrap_or_else(default_session_store_dir),
        ),
        ephemeral_storage_root: explicit(
            cli.vfs_ephemeral_dir.clone(),
            env.vfs_ephemeral_dir.clone(),
            file.vfs_ephemeral_dir.clone(),
        ),
        managed_volume_root: Some(
            explicit(
                cli.volume_dir.clone(),
                env.volume_dir.clone(),
                file.volume_dir.clone(),
            )
            .unwrap_or_else(default_managed_volume_root),
        ),
        // The path the config was read from, so a volume cannot be declared
        // over the file that declares volumes. Resolution consumes the file and
        // `ServerConfig` never carries the path, so this is where it is known.
        config_file: cli.config.clone().or_else(|| env.config.clone()),
    }
}

/// What the migration relocated, for an operator who sees this boot fail and
/// then finds the top-level directories gone. Nothing was lost; it says where.
fn note_migration_before_exit(migration: &crate::migrate::MigrationReport) {
    let server = migration.server_dir();
    if !migration.moved.is_empty() {
        eprintln!(
            "note: the state directories {} were already moved under `{}` by this boot; they \
             are intact there",
            migration.moved.join(", "),
            server.display()
        );
    }
    if !migration.published.is_empty() {
        eprintln!(
            "note: the state directories {} an earlier boot had staged were already published \
             under `{}` by this boot; they are intact there",
            migration.published.join(", "),
            server.display()
        );
    }
    let server_secrets = server.join(crate::migrate::SECRETS);
    if let Some(secrets) = &migration.secrets
        && !secrets.moved.is_empty()
    {
        eprintln!(
            "note: {} sealed secret(s) were already moved from `{}` to `{}` by this boot; they \
             are intact there",
            secrets.moved.len(),
            migration.legacy_secrets_dir().display(),
            server_secrets.display()
        );
    }
    if let Some(staged) = &migration.staged_secrets
        && !staged.moved.is_empty()
    {
        eprintln!(
            "note: {} staged sealed secret(s) were already moved from `{}` to `{}` by this \
             boot; they are intact there",
            staged.moved.len(),
            migration.staged_secrets_dir().display(),
            server_secrets.display()
        );
    }
}

/// Which of the server's state directories resolved from their defaults, and
/// so may be relocated by the boot migration. Provenance is only visible here,
/// before the defaults are applied in `merge`.
fn legacy_layout(cli: &Cli, file: &FileConfig, env: &EnvConfig) -> crate::migrate::LegacyLayout {
    let is_default = |cli: &Option<PathBuf>, env: &Option<PathBuf>, file: &Option<PathBuf>| {
        explicit(cli.as_ref(), env.as_ref(), file.as_ref()).is_none()
    };
    let secrets_default = is_default(
        &cli.secret_store_dir,
        &env.secret_store_dir,
        &file.secret_store.dir,
    );
    let key = secret_key_source(cli, file, env);
    crate::migrate::LegacyLayout {
        root: submilli_build::default_data_root(),
        key_configured: key.is_some(),
        blueprints: is_default(&cli.blueprint_dir, &env.blueprint_dir, &file.blueprint_dir),
        sessions: is_default(
            &cli.session_store_dir,
            &env.session_store_dir,
            &file.session_store_dir,
        ),
        vfs_sessions: is_default(
            &cli.vfs_session_dir,
            &env.vfs_session_dir,
            &file.vfs_session_dir,
        ),
        secrets: secrets_default.then_some(key).flatten(),
    }
}

/// Everything the binary needs from the three configuration sources.
pub(crate) struct Resolved {
    pub log_file: Option<PathBuf>,
    pub addr: SocketAddr,
    pub config: ServerConfig,
    pub telemetry: bool,
    /// Whether telemetry error reports carry the failed program's source.
    /// Always `false` when `telemetry` is.
    pub telemetry_include_source: bool,
    pub shutdown_grace: Duration,
    /// What the boot migration did, if it ran. Logged once a subscriber exists.
    pub migration: Option<crate::migrate::MigrationReport>,
}

fn logging_file(cli: &Cli, file: &FileConfig, env: &EnvConfig) -> Option<PathBuf> {
    cli.log_file
        .clone()
        .or_else(|| env.log_file.clone())
        .or_else(|| file.logging.file.clone())
}

/// The `SUBMILLI_ALLOW_*` variables that widened the outbound egress guard.
///
/// Worth saying out loud at startup: the `allow_*` settings are additive across
/// every source, so a config file that says `allow_private: false` cannot revoke
/// what the environment granted — and the environment is the weakest of the
/// three (Helm values, PaaS dashboards, a stray `docker run -e`).
pub(crate) fn env_egress_grants() -> Vec<&'static str> {
    EnvConfig::from_env().egress_grants()
}

/// The config file to read, from `--config` or `$SUBMILLI_CONFIG`.
fn load_config_file(cli: &Cli, env: &EnvConfig) -> Result<FileConfig> {
    match cli.config.as_ref().or(env.config.as_ref()) {
        Some(path) => load(path),
        None => Ok(FileConfig::default()),
    }
}

/// The address the server would bind, resolved from the same ladder [`merge`]
/// uses. Kept separate from [`resolve`] because the health probe needs only the
/// address: building the full [`ServerConfig`] opens the secret store on disk,
/// which a liveness check running every 30s has no business doing.
pub(crate) fn resolve_bind_addr(cli: &Cli) -> Result<SocketAddr> {
    let env = EnvConfig::from_env();
    let file = load_config_file(cli, &env)?;
    bind_addr(cli, &file, &env)
}

pub(crate) fn resolve_tls_files(cli: &Cli) -> Result<Option<(PathBuf, PathBuf)>> {
    let env = EnvConfig::from_env();
    let file = load_config_file(cli, &env)?;
    tls_files(cli, &file, &env)
}

fn tls_files(cli: &Cli, file: &FileConfig, env: &EnvConfig) -> Result<Option<(PathBuf, PathBuf)>> {
    let cert = explicit(
        cli.tls_cert_file.clone(),
        env.tls_cert_file.clone(),
        file.tls.cert_file.clone(),
    );
    let key = explicit(
        cli.tls_key_file.clone(),
        env.tls_key_file.clone(),
        file.tls.key_file.clone(),
    );
    match (cert, key) {
        (None, None) => Ok(None),
        (Some(cert), Some(key)) => Ok(Some((cert, key))),
        _ => anyhow::bail!(
            "HTTPS requires both tls.cert_file and tls.key_file (or --tls-cert-file and --tls-key-file)"
        ),
    }
}

/// The listen address, resolved down the full five-tier ladder.
fn bind_addr(cli: &Cli, file: &FileConfig, env: &EnvConfig) -> Result<SocketAddr> {
    let env_bind = parse_env("SUBMILLI_BIND", "an IP address", env.bind.as_ref())?;
    let env_port = parse_env("SUBMILLI_PORT", "a port number", env.port.as_ref())?;
    Ok(SocketAddr::new(
        explicit(cli.bind, env_bind, file.bind)
            .or(env.ambient_bind)
            .unwrap_or(DEFAULT_BIND),
        explicit(cli.port, env_port, file.port)
            .or(env.ambient_port)
            .unwrap_or(DEFAULT_PORT),
    ))
}

/// Optional elapsed execution limit; zero explicitly disables it.
fn max_execution_time(cli: &Cli, file: &FileConfig, env: &EnvConfig) -> Result<Option<Duration>> {
    let env_seconds = parse_env(
        "SUBMILLI_MAX_EXECUTION_TIME",
        "a whole number of seconds",
        env.max_execution_time.as_ref(),
    )?;
    Ok(
        explicit(cli.max_execution_time, env_seconds, file.max_execution_time)
            .filter(|seconds| *seconds != 0)
            .map(Duration::from_secs),
    )
}

/// How long in-flight requests may keep running after a shutdown signal.
fn shutdown_grace(cli: &Cli, file: &FileConfig, env: &EnvConfig) -> Result<Duration> {
    let env_grace = parse_env(
        "SUBMILLI_SHUTDOWN_GRACE",
        "a whole number of seconds",
        env.shutdown_grace.as_ref(),
    )?;
    let secs = explicit(cli.shutdown_grace, env_grace, file.shutdown_grace)
        .unwrap_or(DEFAULT_SHUTDOWN_GRACE_SECS);
    Ok(Duration::from_secs(secs))
}

/// How much memory one execution may hold live, in bytes.
///
/// Exposed because the runtime's cap is a hard ceiling with no in-band way
/// around it: a program that needs more traps, and only an operator can decide
/// that a workload legitimately needs a bigger budget. `0` is rejected rather
/// than treated as "unlimited" — unbounded is the state this setting exists to
/// end, and an operator who wants a huge budget can say so in megabytes.
fn max_execution_memory(cli: &Cli, file: &FileConfig, env: &EnvConfig) -> Result<u64> {
    let env_mb = parse_env(
        "SUBMILLI_MAX_EXECUTION_MEMORY",
        "a whole number of megabytes",
        env.max_execution_memory.as_ref(),
    )?;
    let Some(mb) = explicit(cli.max_execution_memory, env_mb, file.max_execution_memory) else {
        return Ok(DEFAULT_MAX_STORE_BYTES);
    };
    if mb == 0 {
        anyhow::bail!("max execution memory must be at least 1 MB, got 0");
    }
    Ok(mb.saturating_mul(1024 * 1024))
}

/// How much fuel one execution may burn before it stops with `fuel exhausted`.
/// `0` is rejected: it would stop every program before its first instruction.
fn max_execution_fuel(cli: &Cli, file: &FileConfig, env: &EnvConfig) -> Result<u64> {
    let env_fuel = parse_env_count(
        "SUBMILLI_MAX_EXECUTION_FUEL",
        env.max_execution_fuel.as_ref(),
    )?;
    let Some(fuel) = explicit(cli.max_execution_fuel, env_fuel, file.max_execution_fuel) else {
        return Ok(RuntimeConfig::default().fuel);
    };
    if fuel == 0 {
        anyhow::bail!("max execution fuel must be at least 1, got 0");
    }
    Ok(fuel)
}

/// The largest `max_execution_stack`, in KiB. Every runtime thread's native
/// stack is sized from it (`RuntimeConfig::native_stack_size`), so it is bounded
/// to keep that reservation reasonable.
const MAX_EXECUTION_STACK_KIB: u64 = 16 * 1024;

/// How deep one execution's calls may go, as a Wasm stack budget in bytes.
fn max_execution_stack(cli: &Cli, file: &FileConfig, env: &EnvConfig) -> Result<usize> {
    let env_kib = parse_env(
        "SUBMILLI_MAX_EXECUTION_STACK",
        "a whole number of kibibytes",
        env.max_execution_stack.as_ref(),
    )?;
    let Some(kib) = explicit(cli.max_execution_stack, env_kib, file.max_execution_stack) else {
        return Ok(RuntimeConfig::default().max_wasm_stack);
    };
    if kib == 0 {
        anyhow::bail!("max execution stack must be at least 1 KiB, got 0");
    }
    if kib > MAX_EXECUTION_STACK_KIB {
        anyhow::bail!(
            "max execution stack must be at most {MAX_EXECUTION_STACK_KIB} KiB, got {kib}"
        );
    }
    kib.checked_mul(1024)
        .and_then(|bytes| usize::try_from(bytes).ok())
        .ok_or_else(|| anyhow::anyhow!("max execution stack of {kib} KiB is too large"))
}

/// `None` leaves [`ServerConfig::max_session_state_memory`] unset, so the
/// manager's own default applies — the same shape every other optional path
/// takes, rather than baking the default in twice.
fn max_session_state_memory(cli: &Cli, file: &FileConfig, env: &EnvConfig) -> Result<Option<u64>> {
    let env_mb = parse_env(
        "SUBMILLI_MAX_SESSION_STATE_MEMORY",
        "a whole number of megabytes",
        env.max_session_state_memory.as_ref(),
    )?;
    let Some(mb) = explicit(
        cli.max_session_state_memory,
        env_mb,
        file.max_session_state_memory,
    ) else {
        return Ok(None);
    };
    if mb == 0 {
        anyhow::bail!("max session state memory must be at least 1 MB, got 0");
    }
    Ok(Some(mb.saturating_mul(1024 * 1024)))
}

/// `None` leaves [`ServerConfig::max_llm_tokens`] unset, so the session
/// manager's own default applies rather than being baked in twice.
///
/// Tokens, not megabytes: the unit the provider bills in and the refusal message
/// names, so there is no boundary conversion to get wrong.
fn max_llm_tokens(cli: &Cli, file: &FileConfig, env: &EnvConfig) -> Result<Option<u64>> {
    let env_tokens = parse_env_count("SUBMILLI_MAX_LLM_TOKENS", env.max_llm_tokens.as_ref())?;
    let Some(tokens) = explicit(cli.max_llm_tokens, env_tokens, file.max_llm_tokens) else {
        return Ok(None);
    };
    if tokens == 0 {
        anyhow::bail!("max llm tokens must be at least 1, got 0");
    }
    Ok(Some(tokens))
}

/// The per-execution token ceiling. Unlike the aggregate this has no `Option` in
/// [`ServerConfig`] — it lives inside `llm_limits`, whose other fields are
/// embedder-only — so an unset value resolves to the runtime's own default here
/// rather than at the construction site.
fn max_execution_llm_tokens(cli: &Cli, file: &FileConfig, env: &EnvConfig) -> Result<u64> {
    let env_tokens = parse_env_count(
        "SUBMILLI_MAX_EXECUTION_LLM_TOKENS",
        env.max_execution_llm_tokens.as_ref(),
    )?;
    let Some(tokens) = explicit(
        cli.max_execution_llm_tokens,
        env_tokens,
        file.max_execution_llm_tokens,
    ) else {
        return Ok(DEFAULT_MAX_EXECUTION_TOKENS);
    };
    if tokens == 0 {
        anyhow::bail!("max execution llm tokens must be at least 1, got 0");
    }
    Ok(tokens)
}

/// The batch fan-out bound (KTD4).
///
/// `0` is rejected rather than clamped: it would read as "no concurrency", but a
/// zero-permit semaphore is a deadlock, and silently treating it as `1` hides an
/// operator's mistake behind a serial dispatch they did not ask for.
fn max_llm_concurrency(cli: &Cli, file: &FileConfig, env: &EnvConfig) -> Result<Option<usize>> {
    let env_limit = parse_env(
        "SUBMILLI_MAX_LLM_CONCURRENCY",
        "a whole number of prompts",
        env.max_llm_concurrency.as_ref(),
    )?;
    let Some(limit) = explicit(cli.max_llm_concurrency, env_limit, file.max_llm_concurrency) else {
        return Ok(None);
    };
    if limit == 0 {
        anyhow::bail!("max llm concurrency must be at least 1, got 0");
    }
    Ok(Some(limit))
}

/// Telemetry requires an explicit opt-in. A config-file opt-out always wins;
/// a supplied environment value must also explicitly enable telemetry.
fn combine_telemetry(env_setting: Option<&str>, file_setting: Option<bool>) -> bool {
    if file_setting == Some(false) {
        return false;
    }
    match env_setting {
        Some(value) => matches!(
            value.trim().to_ascii_lowercase().as_str(),
            "1" | "true" | "yes" | "on"
        ),
        None => file_setting == Some(true),
    }
}

/// Merge of the CLI flags, environment, and parsed [`FileConfig`]. Split out
/// from [`resolve`] so the precedence rules are unit-testable without reading
/// the process environment — though not entirely without disk I/O, since
/// [`resolve_secret_store`] opens the store it builds.
fn merge(cli: Cli, file: FileConfig, env: EnvConfig) -> Result<(SocketAddr, ServerConfig)> {
    // Borrow for the address, policy, and store before the `.or()` chains below
    // move fields out.
    let addr = bind_addr(&cli, &file, &env)?;
    validate_volumes(&file.volumes, &guarded_directories(&cli, &file, &env))?;
    let network_policy = resolve_network_policy(&cli, &file, &env)?;
    let auth = resolve_auth(&cli, &file, &env)?;
    let tls = tls_files(&cli, &file, &env)?
        .map(|(cert, key)| submilli_server::tls::load(&cert, &key))
        .transpose()?;
    let secret_store = resolve_secret_store(&cli, &file, &env)?;
    let mcp_allowed_hosts = resolve_mcp_allowed_hosts(&cli, &file, &env);
    let runtime = RuntimeConfig {
        max_store_bytes: max_execution_memory(&cli, &file, &env)?,
        timeout: max_execution_time(&cli, &file, &env)?,
        fuel: max_execution_fuel(&cli, &file, &env)?,
        max_wasm_stack: max_execution_stack(&cli, &file, &env)?,
        ..RuntimeConfig::default()
    };
    let max_session_state_memory = max_session_state_memory(&cli, &file, &env)?;
    let max_llm_tokens = max_llm_tokens(&cli, &file, &env)?;
    let max_llm_concurrency = max_llm_concurrency(&cli, &file, &env)?;
    // The other `LlmLimits` fields are embedder-only, the way `session_kv_limits`
    // is: the per-execution ceiling is the one an operator tunes alongside the
    // aggregate, so it is the only one that gets a rung on the ladder.
    let llm_limits = LlmLimits {
        per_execution_tokens: max_execution_llm_tokens(&cli, &file, &env)?,
        ..LlmLimits::default()
    };

    let blueprint_dir = explicit(cli.blueprint_dir, env.blueprint_dir, file.blueprint_dir)
        .unwrap_or_else(default_blueprint_dir);
    let session_storage_root = explicit(
        cli.vfs_session_dir,
        env.vfs_session_dir,
        file.vfs_session_dir,
    )
    .unwrap_or_else(default_session_storage_root);
    let session_store_dir = explicit(
        cli.session_store_dir,
        env.session_store_dir,
        file.session_store_dir,
    )
    .unwrap_or_else(default_session_store_dir);
    let ephemeral_storage_root = explicit(
        cli.vfs_ephemeral_dir,
        env.vfs_ephemeral_dir,
        file.vfs_ephemeral_dir,
    );
    let package_store_root = explicit(
        cli.package_store_dir,
        env.package_store_dir,
        file.package_store_dir,
    );
    let managed_volume_root = explicit(cli.volume_dir, env.volume_dir, file.volume_dir)
        .unwrap_or_else(default_managed_volume_root);

    let mcp_oauth_providers = file
        .mcp_oauth
        .providers
        .into_iter()
        .map(|p| OAuthProvider {
            match_host: p.match_host,
            client_id: p.client_id,
            client_secret: p.client_secret,
            scopes: p.scopes,
        })
        .collect();

    let config = ServerConfig {
        audit: file.logging.audit,
        runtime,
        tls,
        // Named rather than left to `..ServerConfig::default()`, whose `auth`
        // is "no authentication": a field that went missing here would fail
        // open.
        auth,
        blueprint_dir: Some(blueprint_dir),
        session_store_dir: Some(session_store_dir),
        session_storage_root: Some(session_storage_root),
        ephemeral_storage_root,
        package_store_root,
        // The CLI's store is always readable as a fallback; there is no knob for
        // it because a locally published package resolving on the dev server is
        // the point, and production stores are simply empty there.
        package_fallback_root: Some(default_cli_package_store_dir()),
        network_policy,
        secret_store,
        mcp_allowed_hosts,
        mcp_oauth_providers,
        max_session_state_memory,
        llm_limits,
        max_llm_tokens,
        max_llm_concurrency,
        volumes: file.volumes,
        managed_volume_root: Some(managed_volume_root),
        github_token_file: file.github_token_file,
        ..ServerConfig::default()
    };
    Ok((addr, config))
}

/// The MCP `Host` allowlist, additive across all three sources (CLI flag, config
/// file, env). When the operator supplies any host, the result prepends the
/// loopback defaults so local access never breaks; with no host configured it's
/// `None`, leaving rmcp's built-in default in place.
fn resolve_mcp_allowed_hosts(cli: &Cli, file: &FileConfig, env: &EnvConfig) -> Option<Vec<String>> {
    let operator_hosts = cli
        .mcp_allowed_host
        .iter()
        .chain(&file.mcp_allowed_hosts)
        .chain(&env.mcp_allowed_hosts)
        .cloned();

    let mut hosts: Vec<String> = MCP_LOOPBACK_HOSTS.iter().map(|h| (*h).to_owned()).collect();
    hosts.extend(operator_hosts);
    (hosts.len() > MCP_LOOPBACK_HOSTS.len()).then_some(hosts)
}

/// Build the secret store. The directory and key env var both have defaults, so
/// the store auto-enables when a key is available: a configured `key_file` (it
/// wins over `key_env`), or the `key_env` variable actually being set in the
/// environment. An unset key env var leaves the store disabled (`Ok(None)`)
/// rather than failing boot; an explicit but unreadable `key_file` is an error.
/// CLI flags take precedence over the config file.
fn resolve_secret_store(
    cli: &Cli,
    file: &FileConfig,
    env: &EnvConfig,
) -> Result<Option<Arc<dyn SecretStore>>> {
    // No key file and the env var isn't set: leave the store off.
    let Some(key_source) = secret_key_source(cli, file, env) else {
        return Ok(None);
    };
    let store = FileSecretStore::open(secret_store_dir(cli, file, env), &key_source)
        .map_err(|e| anyhow::anyhow!("opening secret store: {e}"))?;
    Ok(Some(Arc::new(store)))
}

fn secret_store_dir(cli: &Cli, file: &FileConfig, env: &EnvConfig) -> PathBuf {
    explicit(
        cli.secret_store_dir.clone(),
        env.secret_store_dir.clone(),
        file.secret_store.dir.clone(),
    )
    .unwrap_or_else(default_secret_store_dir)
}

/// Where the store's key comes from, or `None` when no key is configured and
/// the store therefore stays off. A configured key file wins over the env var;
/// the env var counts only when it is actually set. The migration asks the
/// same question, so keyed-ness is decided in exactly one place.
fn secret_key_source(cli: &Cli, file: &FileConfig, env: &EnvConfig) -> Option<KeySource> {
    if let Some(path) = secret_key_file(cli, file, env) {
        return Some(KeySource::File(path));
    }
    let key_env = explicit(
        cli.secret_store_key_env.clone(),
        env.secret_store_key_env.clone(),
        file.secret_store.key_env.clone(),
    )
    .unwrap_or_else(|| DEFAULT_SECRET_KEY_ENV.into());
    std::env::var_os(&key_env)
        .is_some()
        .then_some(KeySource::Env(key_env))
}

fn secret_key_file(cli: &Cli, file: &FileConfig, env: &EnvConfig) -> Option<PathBuf> {
    explicit(
        cli.secret_store_key_file.clone(),
        env.secret_store_key_file.clone(),
        file.secret_store.key_file.clone(),
    )
}

/// Who may call the API. A server with no tokens is refused unless the
/// operator opted out, and an opt-out alongside tokens is refused too: the
/// opt-out is additive across the three sources, so without that rule a stray
/// environment variable could switch off tokens the config file declares.
fn resolve_auth(cli: &Cli, file: &FileConfig, env: &EnvConfig) -> Result<AuthConfig> {
    let allow_unauthenticated = cli.allow_unauthenticated
        || env.allow_unauthenticated
        || file.allow_unauthenticated.unwrap_or(false);
    let configured =
        !file.api_tokens.is_empty() || env.server_token.is_some() || env.server_token_not_unicode;
    match (configured, allow_unauthenticated) {
        (false, true) => Ok(AuthConfig::Disabled),
        (false, false) => anyhow::bail!(
            "no API tokens are configured, so nothing could call this server. Set \
             `${SERVER_TOKEN_ENV}` to an admin token (for example `openssl rand -hex 32`), \
             declare tokens under `api_tokens:` in the config file (`--config`), or serve \
             without authentication by setting `allow_unauthenticated: true` \
             (`--allow-unauthenticated`, `$SUBMILLI_ALLOW_UNAUTHENTICATED=1`)"
        ),
        (true, true) => anyhow::bail!(
            "API tokens are configured (`${SERVER_TOKEN_ENV}` or `api_tokens`) and \
             `allow_unauthenticated` is set (`--allow-unauthenticated`, \
             `$SUBMILLI_ALLOW_UNAUTHENTICATED`, or the config file); remove one — the server \
             either requires a token or it does not"
        ),
        (true, false) => api_tokens(file, env).map(AuthConfig::Tokens),
    }
}

/// The server token from the environment, then the config file's entries.
fn api_tokens(file: &FileConfig, env: &EnvConfig) -> Result<Vec<ApiToken>> {
    let mut tokens: Vec<ApiToken> = Vec::with_capacity(file.api_tokens.len() + 1);
    if let Some(token) = server_token(env)? {
        tokens.push(token);
    }
    for entry in &file.api_tokens {
        let token = api_token(entry)?;
        if let Some(other) = tokens.iter().find(|other| other.name() == token.name()) {
            anyhow::bail!(
                "two API tokens are named `{}`; give each a name of its own",
                other.name()
            );
        }
        if let Some(other) = tokens.iter().find(|other| other.same_token(&token)) {
            anyhow::bail!(
                "API tokens `{}` and `{}` hold the same token; give each a token of its own",
                other.name(),
                token.name()
            );
        }
        tokens.push(token);
    }
    Ok(tokens)
}

/// `$SUBMILLI_SERVER_TOKEN` as an admin token, named after the variable. It is
/// the variable the `submilli server` CLI sends, so a server and a CLI sharing
/// an environment agree on a token with no config file at all.
fn server_token(env: &EnvConfig) -> Result<Option<ApiToken>> {
    if env.server_token_not_unicode {
        anyhow::bail!(
            "`${SERVER_TOKEN_ENV}` does not hold valid Unicode, so it cannot be a bearer token"
        );
    }
    env.server_token
        .as_deref()
        .map(|token| {
            ApiToken::new(SERVER_TOKEN_ENV, Role::Admin, token)
                .map_err(|err| anyhow::anyhow!("the token in `${SERVER_TOKEN_ENV}` {err}"))
        })
        .transpose()
}

/// Read one entry's token. Messages name the entry and its file, and never the
/// token: this error reaches stderr and the logs.
fn api_token(entry: &ApiTokenFileConfig) -> Result<ApiToken> {
    let name = entry.name.as_str();
    if name.trim().is_empty() || name.chars().any(char::is_control) {
        anyhow::bail!(
            "an `api_tokens` entry has the name {name:?}; give every entry a plain single-line \
             name"
        );
    }
    let path = entry.token_file.display();
    let raw = std::fs::read_to_string(&entry.token_file)
        .with_context(|| format!("`api_tokens` entry `{name}`: reading token file `{path}`"))?;
    // A token written with `echo` or a Kubernetes Secret volume ends in a
    // newline the caller will never send.
    let token = raw.trim();
    if token.is_empty() {
        anyhow::bail!("`api_tokens` entry `{name}`: the token file `{path}` is empty");
    }
    ApiToken::new(name, entry.role, token).map_err(|err| {
        anyhow::anyhow!("`api_tokens` entry `{name}`: the token in token file `{path}` {err}")
    })
}

/// The `allow_*` settings are additive across all three sources: a permission
/// enabled on the command line, in the file, *or* in the environment is granted;
/// no source can revoke another's grant. A malformed entry is an error rather
/// than a silent drop — unlike a bad port, a network grant that vanishes is a
/// security surprise, so the message names the source it came from.
fn resolve_network_policy(cli: &Cli, file: &FileConfig, env: &EnvConfig) -> Result<NetworkPolicy> {
    let allow_localhost =
        cli.allow_localhost || env.allow_localhost || file.network.allow_localhost.unwrap_or(false);
    let allow_private =
        cli.allow_private || env.allow_private || file.network.allow_private.unwrap_or(false);

    let mut cidrs = cli.allow_ip.clone();
    for (source, entry) in file
        .network
        .allow_ip
        .iter()
        .map(|e| ("config `network.allow_ip`", e))
        .chain(env.allow_ip.iter().map(|e| ("$SUBMILLI_ALLOW_IP", e)))
    {
        let net = parse_ip_or_cidr(entry).map_err(|msg| anyhow::anyhow!("{source}: {msg}"))?;
        cidrs.push(net);
    }

    Ok(cidrs.iter().fold(
        NetworkPolicy::deny_private()
            .allow_localhost(allow_localhost)
            .allow_private(allow_private),
        |policy, net| policy.allow_cidr(*net),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap::Parser;
    use submilli_server::config::{Access, SizeLimit, VolumeSpec};

    #[test]
    fn count_budgets_accept_the_same_values_from_every_source() {
        for (raw, expected) in [
            ("1K", 1_000),
            ("2k", 2_000),
            ("20M", 20_000_000),
            ("3m", 3_000_000),
            ("10B", 10_000_000_000),
            ("1b", 1_000_000_000),
            ("1T", 1_000_000_000_000),
            ("2t", 2_000_000_000_000),
            ("10_000_000_000", 10_000_000_000),
            ("1_000K", 1_000_000),
            ("123", 123),
            ("18446744073709551615", u64::MAX),
        ] {
            let yaml = format!(
                "max_execution_fuel: {raw}\nmax_llm_tokens: {raw}\nmax_execution_llm_tokens: {raw}"
            );
            let file: FileConfig = serde_yml::from_str(&yaml).unwrap();
            let cli = Cli::try_parse_from([
                "submilli-server",
                "--max-execution-fuel",
                raw,
                "--max-llm-tokens",
                raw,
                "--max-execution-llm-tokens",
                raw,
            ])
            .unwrap();
            let env = env_from(&[
                ("SUBMILLI_MAX_EXECUTION_FUEL", raw),
                ("SUBMILLI_MAX_LLM_TOKENS", raw),
                ("SUBMILLI_MAX_EXECUTION_LLM_TOKENS", raw),
            ]);
            for (cli, file, env) in [
                (cli, FileConfig::default(), EnvConfig::default()),
                (empty_cli(), file, EnvConfig::default()),
                (empty_cli(), FileConfig::default(), env),
            ] {
                assert_eq!(max_execution_fuel(&cli, &file, &env).unwrap(), expected);
                assert_eq!(max_llm_tokens(&cli, &file, &env).unwrap(), Some(expected));
                assert_eq!(
                    max_execution_llm_tokens(&cli, &file, &env).unwrap(),
                    expected
                );
            }
        }
    }

    #[test]
    fn count_budgets_reject_malformed_and_overflowing_values() {
        for (field, flag, variable) in [
            (
                "max_execution_fuel",
                "--max-execution-fuel",
                "SUBMILLI_MAX_EXECUTION_FUEL",
            ),
            (
                "max_llm_tokens",
                "--max-llm-tokens",
                "SUBMILLI_MAX_LLM_TOKENS",
            ),
            (
                "max_execution_llm_tokens",
                "--max-execution-llm-tokens",
                "SUBMILLI_MAX_EXECUTION_LLM_TOKENS",
            ),
        ] {
            for raw in [
                "1.5M",
                "1 M",
                "1e10",
                "1MB",
                "-1",
                "K",
                "_1",
                "1_",
                "1__0",
                "1_K",
                "１K",
                "18446744073709551616",
                "18446744073709552K",
                "18446745T",
            ] {
                for value in [raw.to_owned(), format!("'{raw}'")] {
                    let yaml = format!("{field}: {value}");
                    assert!(serde_yml::from_str::<FileConfig>(&yaml).is_err(), "{yaml}");
                }
                assert!(
                    Cli::try_parse_from(["submilli-server", flag, raw]).is_err(),
                    "{flag} {raw}"
                );
                let env = env_from(&[(variable, raw)]);
                // Invalid environment values must fail even when a CLI value wins precedence.
                let cli = Cli::try_parse_from(["submilli-server", flag, "1K"]).unwrap();
                let error = preflight(&cli, &FileConfig::default(), &env).unwrap_err();
                assert!(error.to_string().contains(variable), "{error}");
            }
        }
    }

    #[test]
    fn count_budgets_preserve_precedence_defaults_and_zero_validation() {
        let file: FileConfig = serde_yml::from_str(
            "max_execution_fuel: '1K'\nmax_llm_tokens: 2K\nmax_execution_llm_tokens: 3K",
        )
        .unwrap();
        let env = env_from(&[
            ("SUBMILLI_MAX_EXECUTION_FUEL", "4K"),
            ("SUBMILLI_MAX_LLM_TOKENS", "5K"),
            ("SUBMILLI_MAX_EXECUTION_LLM_TOKENS", "6K"),
        ]);
        let cli = Cli::try_parse_from([
            "submilli-server",
            "--max-execution-fuel",
            "7K",
            "--max-llm-tokens",
            "8K",
            "--max-execution-llm-tokens",
            "9K",
        ])
        .unwrap();
        assert_eq!(max_execution_fuel(&cli, &file, &env).unwrap(), 7_000);
        assert_eq!(max_llm_tokens(&cli, &file, &env).unwrap(), Some(8_000));
        assert_eq!(max_execution_llm_tokens(&cli, &file, &env).unwrap(), 9_000);
        assert_eq!(
            max_execution_fuel(&empty_cli(), &file, &env).unwrap(),
            4_000
        );
        assert_eq!(
            max_llm_tokens(&empty_cli(), &file, &env).unwrap(),
            Some(5_000)
        );
        assert_eq!(
            max_execution_llm_tokens(&empty_cli(), &file, &env).unwrap(),
            6_000
        );

        let nulls: FileConfig = serde_yml::from_str(
            "max_execution_fuel: null\nmax_llm_tokens: null\nmax_execution_llm_tokens: null",
        )
        .unwrap();
        assert!(nulls.max_execution_fuel.is_none());
        assert!(nulls.max_llm_tokens.is_none());
        assert!(nulls.max_execution_llm_tokens.is_none());
        for field in [
            "max_execution_fuel",
            "max_llm_tokens",
            "max_execution_llm_tokens",
        ] {
            let file = serde_yml::from_str(&format!("{field}: 0K")).unwrap();
            assert!(preflight(&empty_cli(), &file, &EnvConfig::default()).is_err());
        }
        for field in [
            "max_execution_memory",
            "max_execution_stack",
            "max_session_state_memory",
        ] {
            assert!(serde_yml::from_str::<FileConfig>(&format!("{field}: 1K")).is_err());
        }
    }

    fn empty_cli() -> Cli {
        Cli {
            config: None,
            log_file: None,
            bind: None,
            port: None,
            blueprint_dir: None,
            session_store_dir: None,
            vfs_session_dir: None,
            vfs_ephemeral_dir: None,
            volume_dir: None,
            secret_store_dir: None,
            package_store_dir: None,
            secret_store_key_env: None,
            secret_store_key_file: None,
            allow_localhost: false,
            allow_private: false,
            allow_ip: vec![],
            mcp_allowed_host: vec![],
            shutdown_grace: None,
            max_execution_memory: None,
            max_execution_time: None,
            max_execution_fuel: None,
            max_execution_stack: None,
            max_session_state_memory: None,
            max_llm_tokens: None,
            max_execution_llm_tokens: None,
            max_llm_concurrency: None,
            // Opted out so the tests about every other setting reach `merge`
            // without declaring tokens; the auth tests turn it back off.
            allow_unauthenticated: true,
            health_check: false,
            tls_cert_file: None,
            tls_key_file: None,
        }
    }

    #[test]
    fn logging_file_walks_the_precedence_ladder() {
        let file: FileConfig = serde_yml::from_str("logging:\n  file: config.log\n").unwrap();
        let mut env = env_from(&[("SUBMILLI_LOG_FILE", "env.log")]);
        let mut cli = empty_cli();
        cli.log_file = Some("flag.log".into());
        assert_eq!(logging_file(&cli, &file, &env), Some("flag.log".into()));
        cli.log_file = None;
        assert_eq!(logging_file(&cli, &file, &env), Some("env.log".into()));
        env.log_file = None;
        assert_eq!(logging_file(&cli, &file, &env), Some("config.log".into()));
        assert_eq!(logging_file(&cli, &FileConfig::default(), &env), None);
        assert!(
            serde_yml::from_str::<FileConfig>("logging:\n  audit:\n    enabled: true\n").is_ok()
        );
        let parsed = Cli::try_parse_from(["submilli-server", "--log-file", "parsed.log"]).unwrap();
        assert_eq!(parsed.log_file, Some("parsed.log".into()));
    }

    #[test]
    fn tls_requires_both_files_and_walks_the_ladder() {
        let mut cli = empty_cli();
        let mut file = FileConfig::default();
        let mut env = EnvConfig::default();
        assert!(tls_files(&cli, &file, &env).unwrap().is_none());
        file.tls.cert_file = Some("file.crt".into());
        assert!(tls_files(&cli, &file, &env).is_err());
        file.tls.key_file = Some("file.key".into());
        assert_eq!(
            tls_files(&cli, &file, &env).unwrap(),
            Some(("file.crt".into(), "file.key".into()))
        );
        env.tls_cert_file = Some("env.crt".into());
        env.tls_key_file = Some("env.key".into());
        cli.tls_cert_file = Some("cli.crt".into());
        assert_eq!(
            tls_files(&cli, &file, &env).unwrap(),
            Some(("cli.crt".into(), "env.key".into()))
        );
        let parsed: FileConfig =
            serde_yml::from_str("tls:\n  cert_file: server.crt\n  key_file: server.key\n").unwrap();
        assert_eq!(parsed.tls.cert_file, Some("server.crt".into()));
    }

    #[test]
    fn parses_full_config() {
        let yaml = "
bind: 0.0.0.0
port: 9000
blueprint_dir: /data/blueprints
vfs_session_dir: /data/sessions
vfs_ephemeral_dir: /tmp/submilli
network:
  allow_localhost: true
  allow_private: false
  allow_ip:
    - 10.0.0.0/24
    - 1.2.3.4
";
        let cfg: FileConfig = serde_yml::from_str(yaml).unwrap();
        assert_eq!(cfg.bind, Some("0.0.0.0".parse().unwrap()));
        assert_eq!(cfg.port, Some(9000));
        assert_eq!(cfg.blueprint_dir, Some("/data/blueprints".into()));
        assert_eq!(cfg.vfs_session_dir, Some("/data/sessions".into()));
        assert_eq!(cfg.vfs_ephemeral_dir, Some("/tmp/submilli".into()));
        assert_eq!(cfg.network.allow_localhost, Some(true));
        assert_eq!(cfg.network.allow_private, Some(false));
        assert_eq!(cfg.network.allow_ip, vec!["10.0.0.0/24", "1.2.3.4"]);
    }

    #[test]
    fn parses_telemetry_flag() {
        let cfg: FileConfig = serde_yml::from_str("telemetry: false\n").unwrap();
        assert_eq!(cfg.telemetry, Some(false));
        assert!(FileConfig::default().telemetry.is_none());
        let cfg: FileConfig =
            serde_yml::from_str("telemetry: true\ntelemetry_include_source: true\n").unwrap();
        assert_eq!(cfg.telemetry_include_source, Some(true));
        assert!(FileConfig::default().telemetry_include_source.is_none());
    }

    #[test]
    fn combine_telemetry_rules() {
        assert!(!combine_telemetry(None, None));
        assert!(combine_telemetry(None, Some(true)));
        assert!(!combine_telemetry(None, Some(false)));
        for value in ["1", "true", "yes", "on", " TRUE ", "On"] {
            assert!(combine_telemetry(Some(value), None));
            assert!(combine_telemetry(Some(value), Some(true)));
            assert!(!combine_telemetry(Some(value), Some(false)));
        }
        for value in ["0", "false", "no", "off", "", "invalid"] {
            assert!(!combine_telemetry(Some(value), None));
            assert!(!combine_telemetry(Some(value), Some(true)));
        }
    }

    #[test]
    fn empty_config_is_all_none() {
        let cfg: FileConfig = serde_yml::from_str("{}").unwrap();
        assert!(cfg.bind.is_none());
        assert!(cfg.network.allow_ip.is_empty());
    }

    /// A config file that reaches `merge` without opening a secret store: the
    /// key env var name is one nothing sets, so the store stays disabled and the
    /// merge touches no filesystem beyond the volume check.
    fn merge_file(file: FileConfig) -> Result<ServerConfig> {
        let file = FileConfig {
            secret_store: SecretStoreFileConfig {
                key_env: Some("SUB_TEST_SECRET_KEY_UNSET".into()),
                ..file.secret_store
            },
            ..file
        };
        merge(empty_cli(), file, EnvConfig::default()).map(|(_, config)| config)
    }

    /// `ServerConfig` is not `Debug`, so `expect_err` is unavailable on a merge.
    fn merge_err(file: FileConfig) -> String {
        match merge_file(file) {
            Ok(_) => panic!("expected the merge to be refused"),
            Err(err) => err.to_string(),
        }
    }

    /// Every guarded directory placed under one root, so a test can point a
    /// volume at exactly one of them.
    fn dirs_under(root: &std::path::Path) -> ServerDirectories {
        ServerDirectories {
            blueprint_dir: Some(root.join("blueprints")),
            package_store_root: Some(root.join("packages")),
            package_fallback_root: Some(root.join("cli-packages")),
            secret_store_dir: Some(root.join("secrets")),
            secret_store_key_file: Some(root.join("keys/secret.b64")),
            api_token_files: vec![root.join("tokens/admin"), root.join("tokens/user")],
            github_token_file: Some(root.join("github/token")),
            tls_key_file: Some(root.join("tls/key.pem")),
            tls_cert_file: Some(root.join("tls/cert.pem")),
            session_storage_root: Some(root.join("vfs/sessions")),
            session_store_dir: Some(root.join("sessions")),
            ephemeral_storage_root: Some(root.join("scratch")),
            managed_volume_root: Some(root.join("volumes")),
            config_file: Some(root.join("etc/submilli.yaml")),
        }
    }

    /// A table of read-write, unlimited `local-path` volumes, the shape every
    /// overlap test needs.
    fn local_table<const N: usize>(entries: [(String, PathBuf); N]) -> VolumeTable {
        entries
            .into_iter()
            .map(|(name, path)| (name, VolumeSpec::local_path(path)))
            .collect()
    }

    /// The directories [`dirs_under`] filled in, each paired with the words its
    /// refusal uses. Destructured exhaustively, so a directory added to
    /// `ServerDirectories` fails to compile here until it is listed — and then
    /// every guard test below covers it without a second list to maintain.
    fn guarded_paths(dirs: &ServerDirectories) -> Vec<(PathBuf, &'static str)> {
        let ServerDirectories {
            blueprint_dir,
            package_store_root,
            package_fallback_root,
            secret_store_dir,
            secret_store_key_file,
            api_token_files,
            github_token_file,
            session_storage_root,
            session_store_dir,
            ephemeral_storage_root,
            managed_volume_root,
            config_file,
            tls_key_file,
            tls_cert_file,
        } = dirs.clone();
        [
            (blueprint_dir, "blueprint store"),
            (package_store_root, "package store"),
            (package_fallback_root, "fallback package store"),
            (secret_store_dir, "secret store"),
            (secret_store_key_file, "secret-store key file"),
            (github_token_file, "GitHub token file"),
            (tls_key_file, "TLS private key"),
            (tls_cert_file, "TLS certificate file"),
            (session_storage_root, "per-session VFS root"),
            (session_store_dir, "durable session store"),
            (ephemeral_storage_root, "ephemeral storage root"),
            (managed_volume_root, "managed volume root"),
            (config_file, "server config file"),
        ]
        .into_iter()
        .filter_map(|(path, owned)| Some((path?, owned)))
        .chain(
            api_token_files
                .into_iter()
                .map(|path| (path, "API token file")),
        )
        .collect()
    }

    fn refusal(volume: &str, target: PathBuf, dirs: &ServerDirectories) -> String {
        validate_volumes(&local_table([(volume.to_string(), target)]), dirs)
            .expect_err("volume should be refused")
            .to_string()
    }

    #[test]
    fn volumes_resolve_from_the_config_file() {
        let config = merge_file(FileConfig {
            volumes: local_table([
                ("work".to_string(), PathBuf::from("/srv/work")),
                ("data".to_string(), PathBuf::from("/srv/data")),
            ]),
            ..FileConfig::default()
        })
        .expect("merge");
        assert_eq!(config.volumes.len(), 2);
        assert_eq!(config.volumes["work"], VolumeSpec::local_path("/srv/work"));
        assert_eq!(config.volumes["data"], VolumeSpec::local_path("/srv/data"));
    }

    #[test]
    fn no_volumes_key_resolves_to_an_empty_table() {
        let config = merge_file(FileConfig::default()).expect("merge");
        assert!(config.volumes.is_empty());
        let parsed: FileConfig = serde_yml::from_str("port: 9000\n").unwrap();
        assert!(parsed.volumes.is_empty());
    }

    #[test]
    fn a_relative_volume_target_is_refused() {
        let err = merge_err(FileConfig {
            volumes: local_table([("work".to_string(), PathBuf::from("relative/dir"))]),
            ..FileConfig::default()
        });
        assert!(err.contains("absolute"), "got: {err}");
        assert!(err.contains("work"), "got: {err}");
    }

    #[test]
    fn an_empty_volume_name_is_refused() {
        let err = merge_err(FileConfig {
            volumes: local_table([(String::new(), PathBuf::from("/srv/work"))]),
            ..FileConfig::default()
        });
        assert!(err.contains("empty"), "got: {err}");
    }

    #[test]
    fn a_volume_name_containing_a_newline_is_refused() {
        let err = merge_err(FileConfig {
            volumes: local_table([("work\nfake".to_string(), PathBuf::from("/srv/work"))]),
            ..FileConfig::default()
        });
        assert!(err.contains("control character"), "got: {err}");
    }

    #[test]
    fn a_volume_swallowing_a_server_owned_directory_is_refused() {
        let root = tempfile::tempdir().unwrap();
        let dirs = dirs_under(root.path());
        // A volume at the directory itself already contains it, so pointing one
        // at each guarded path in turn exercises every row of the table.
        for (target, expected) in guarded_paths(&dirs) {
            let msg = refusal("work", target.clone(), &dirs);
            assert!(msg.contains(expected), "{}: got {msg}", target.display());
            assert!(msg.contains("contains"), "{}: got {msg}", target.display());
        }
        // A volume at the directory they all live under swallows them wholesale.
        let msg = refusal("work", root.path().to_path_buf(), &dirs);
        assert!(msg.contains("contains"), "got {msg}");
    }

    #[test]
    fn a_volume_inside_the_per_session_root_is_refused_for_data_loss() {
        let root = tempfile::tempdir().unwrap();
        let msg = refusal(
            "work",
            root.path().join("vfs/sessions/work"),
            &dirs_under(root.path()),
        );
        assert!(msg.contains("per-session VFS root"), "got {msg}");
        assert!(msg.contains("is inside"), "got {msg}");
        assert!(msg.contains("orphan reconciliation"), "got {msg}");
    }

    #[test]
    fn a_volume_inside_the_os_temp_dir_is_accepted() {
        // The ephemeral root defaults to the OS temp dir, and containment there
        // runs the other way — a volume under it is fine.
        let inside = tempfile::tempdir().unwrap();
        let dirs = ServerDirectories {
            ephemeral_storage_root: None,
            ..dirs_under(&std::env::temp_dir().join("submilli-volume-test-owned"))
        };
        validate_volumes(
            &local_table([("work".to_string(), inside.path().to_path_buf())]),
            &dirs,
        )
        .expect("a volume under the OS temp dir is fine");
    }

    /// The mirror of the symlink case below: there the *resolved* target overlaps
    /// and the written one does not, here the written one overlaps and the
    /// resolved one does not. A volume whose target is a link sitting inside
    /// another volume is a pathname a guest can repoint, so the mount that reads
    /// it later lands wherever the guest chose.
    #[test]
    fn a_volume_at_a_link_inside_another_volume_is_refused() {
        let root = tempfile::tempdir().unwrap();
        let outer = root.path().join("outer");
        let elsewhere = root.path().join("elsewhere");
        std::fs::create_dir_all(&outer).unwrap();
        std::fs::create_dir_all(&elsewhere).unwrap();
        let alias = outer.join("alias");
        #[cfg(unix)]
        std::os::unix::fs::symlink(&elsewhere, &alias).unwrap();
        #[cfg(windows)]
        std::os::windows::fs::symlink_dir(&elsewhere, &alias).unwrap();

        // Resolving the link lands outside `outer`, so only the written form of
        // the target shows the nesting.
        assert!(
            !std::fs::canonicalize(&alias)
                .unwrap()
                .starts_with(std::fs::canonicalize(&outer).unwrap()),
            "the link must resolve outside the outer volume for this to test anything"
        );
        let err = validate_volumes(
            &local_table([("outer".to_string(), outer), ("inner".to_string(), alias)]),
            &ServerDirectories::default(),
        )
        .expect_err("a volume at a link inside another volume must be refused");
        let msg = err.to_string();
        assert!(msg.contains("is inside volume 'outer'"), "got: {msg}");
    }

    /// A directory that does not exist yet keeps the operator's spelling — there
    /// is nothing on disk to fold its case against. Where the filesystem ignores
    /// case, two spellings then name one directory. The server's own directories
    /// are created lazily on first use, so the pair being compared at start-up is
    /// routinely two paths that are not there yet.
    #[test]
    fn a_case_alias_of_a_guarded_dir_is_refused_where_case_is_not_significant() {
        let root = tempfile::tempdir().unwrap();
        let dirs = ServerDirectories {
            blueprint_dir: Some(root.path().join("state/blueprints")),
            ..ServerDirectories::default()
        };
        let volumes = local_table([("work".to_string(), root.path().join("STATE"))]);
        let result = validate_volumes(&volumes, &dirs);

        if cfg!(any(target_os = "macos", windows)) {
            let msg = result
                .expect_err("`STATE` and `state` are one directory here")
                .to_string();
            assert!(msg.contains("blueprint store"), "got: {msg}");
        } else {
            result.expect("`STATE` and `state` are two directories here");
        }
    }

    #[test]
    fn an_overlap_visible_only_through_a_symlink_is_refused() {
        let root = tempfile::tempdir().unwrap();
        let real = root.path().join("real");
        std::fs::create_dir_all(real.join("blueprints")).unwrap();
        let link = root.path().join("link");
        #[cfg(unix)]
        std::os::unix::fs::symlink(&real, &link).unwrap();
        #[cfg(windows)]
        std::os::windows::fs::symlink_dir(&real, &link).unwrap();

        let dirs = ServerDirectories {
            blueprint_dir: Some(real.join("blueprints")),
            ..ServerDirectories::default()
        };
        // The declared target and the blueprint store share no textual prefix;
        // only resolving the link exposes the overlap.
        let msg = refusal("work", link, &dirs);
        assert!(msg.contains("blueprint store"), "got {msg}");
    }

    #[test]
    fn the_exposed_validator_refuses_a_programmatically_built_config() {
        let root = tempfile::tempdir().unwrap();
        let config = ServerConfig {
            blueprint_dir: Some(root.path().join("blueprints")),
            session_storage_root: Some(root.path().join("vfs/sessions")),
            package_store_root: Some(root.path().join("packages")),
            ephemeral_storage_root: Some(root.path().join("scratch")),
            volumes: local_table([("work".to_string(), root.path().to_path_buf())]),
            ..ServerConfig::default()
        };
        let err = validate_volumes(&config.volumes, &ServerDirectories::from_config(&config))
            .expect_err("the embedder path must reach the same refusal");
        assert!(err.to_string().contains("blueprint store"), "got {err}");
    }

    #[test]
    fn a_volume_at_the_durable_session_store_is_refused() {
        let root = tempfile::tempdir().unwrap();
        let err = merge_err(FileConfig {
            session_store_dir: Some(root.path().join("sessions")),
            volumes: local_table([("work".to_string(), root.path().join("sessions"))]),
            ..FileConfig::default()
        });
        assert!(err.contains("durable session store"), "got: {err}");
        assert!(err.contains("contains"), "got: {err}");
    }

    #[test]
    fn a_volume_inside_the_durable_session_store_is_refused() {
        let root = tempfile::tempdir().unwrap();
        let err = merge_err(FileConfig {
            session_store_dir: Some(root.path().join("sessions")),
            volumes: local_table([("work".to_string(), root.path().join("sessions/idempotency"))]),
            ..FileConfig::default()
        });
        assert!(err.contains("durable session store"), "got: {err}");
        assert!(err.contains("is inside"), "got: {err}");
    }

    #[test]
    fn a_volume_inside_the_package_store_is_refused() {
        let root = tempfile::tempdir().unwrap();
        let err = merge_err(FileConfig {
            package_store_dir: Some(root.path().join("packages")),
            volumes: local_table([("work".to_string(), root.path().join("packages/@acme"))]),
            ..FileConfig::default()
        });
        assert!(err.contains("package store"), "got: {err}");
        assert!(err.contains("is inside"), "got: {err}");
    }

    #[test]
    fn two_volumes_that_overlap_are_refused() {
        let err = merge_err(FileConfig {
            volumes: local_table([
                ("data".to_string(), PathBuf::from("/srv/data")),
                ("inner".to_string(), PathBuf::from("/srv/data/sub")),
            ]),
            ..FileConfig::default()
        });
        assert!(err.contains("data"), "got: {err}");
        assert!(err.contains("inner"), "got: {err}");
        assert!(err.contains("/srv/data/sub"), "got: {err}");
    }

    #[test]
    fn two_volumes_pointing_at_one_directory_are_refused() {
        let err = merge_err(FileConfig {
            volumes: local_table([
                ("data".to_string(), PathBuf::from("/srv/data")),
                ("alias".to_string(), PathBuf::from("/srv/data")),
            ]),
            ..FileConfig::default()
        });
        assert!(err.contains("alias"), "got: {err}");
        assert!(err.contains("data"), "got: {err}");
        assert!(err.contains("both point at"), "got: {err}");
    }

    fn parse_volumes(yaml: &str) -> std::result::Result<VolumeTable, String> {
        serde_yml::from_str::<FileConfig>(yaml)
            .map(|file| file.volumes)
            .map_err(|err| err.to_string())
    }

    #[test]
    fn volumes_parse_both_kinds_with_explicit_limits() {
        let volumes = parse_volumes(
            "volumes:\n  memory:\n    kind: managed-local\n    size_limit: 1GiB\n  handbook:\n    kind: local-path\n    path: /srv/handbook\n    access: read_only\n    size_limit: unlimited\n  raw:\n    kind: managed-local\n    size_limit: 2048\n",
        )
        .expect("parses");
        assert_eq!(
            volumes["memory"],
            VolumeSpec::managed(SizeLimit::Bytes(1 << 30))
        );
        assert_eq!(
            volumes["handbook"],
            VolumeSpec::local_path("/srv/handbook").with_access(Access::ReadOnly)
        );
        assert_eq!(volumes["raw"].size_limit, SizeLimit::Bytes(2048));
        assert_eq!(
            volumes["raw"].access,
            Access::ReadWrite,
            "read_write by default"
        );
    }

    #[test]
    fn volume_declarations_are_refused_with_the_edit_that_fixes_them() {
        for (yaml, expected) in [
            (
                "volumes:\n  work: /srv/work\n",
                "`work: {kind: local-path, path: /srv/work, size_limit: unlimited}`",
            ),
            ("volumes:\n  work: {size_limit: 1GB}\n", "needs a `kind`"),
            (
                "volumes:\n  work: {kind: managed-local}\n",
                "needs an explicit `size_limit`",
            ),
            (
                "volumes:\n  work: {kind: managed-local, path: /srv, size_limit: 1GB}\n",
                "`path` is only valid for `kind: local-path`",
            ),
            (
                "volumes:\n  work: {kind: local-path, size_limit: 1GB}\n",
                "needs a `path`",
            ),
            (
                "volumes:\n  work: {kind: s3, size_limit: 1GB}\n",
                "unknown kind `s3`",
            ),
            (
                "volumes:\n  work: {kind: managed-local, size_limit: lots}\n",
                "`size_limit`",
            ),
            (
                "volumes:\n  work: {kind: managed-local, size_limit: 1GB, quota: 1}\n",
                "unknown field `quota`",
            ),
        ] {
            let err = parse_volumes(yaml).expect_err(yaml);
            assert!(err.contains(expected), "{yaml}: got {err}");
        }
    }

    #[test]
    fn the_managed_volume_root_comes_from_flag_env_or_file() {
        let file = FileConfig {
            volume_dir: Some("/file/volumes".into()),
            ..FileConfig::default()
        };
        let env = EnvConfig {
            volume_dir: Some("/env/volumes".into()),
            ..EnvConfig::default()
        };
        let cli = Cli {
            volume_dir: Some("/cli/volumes".into()),
            ..empty_cli()
        };
        let root = |cli: &Cli, env: &EnvConfig, file: &FileConfig| {
            guarded_directories(cli, file, env).managed_volume_root
        };
        assert_eq!(root(&cli, &env, &file), Some(PathBuf::from("/cli/volumes")));
        assert_eq!(
            root(&empty_cli(), &env, &file),
            Some(PathBuf::from("/env/volumes"))
        );
        assert_eq!(
            root(&empty_cli(), &EnvConfig::default(), &file),
            Some(PathBuf::from("/file/volumes"))
        );
        assert_eq!(
            root(&empty_cli(), &EnvConfig::default(), &FileConfig::default()),
            Some(default_managed_volume_root())
        );
        let parsed: FileConfig = serde_yml::from_str("volume_dir: /data/volumes\n").unwrap();
        assert_eq!(parsed.volume_dir, Some("/data/volumes".into()));
    }

    #[test]
    fn managed_volume_names_must_be_directory_names() {
        let root = tempfile::tempdir().unwrap();
        for name in ["../escape", "a/b", ".hidden", "with space", &"x".repeat(65)] {
            let err = validate_volumes(
                &VolumeTable::from([(name.to_string(), VolumeSpec::managed(SizeLimit::Unlimited))]),
                &dirs_under(root.path()),
            )
            .expect_err(name);
            assert!(
                err.to_string().contains("managed-local volume name"),
                "{name}: {err}"
            );
        }
        validate_volumes(
            &VolumeTable::from([(
                "project-memory_2.v1".to_string(),
                VolumeSpec::managed(SizeLimit::Unlimited),
            )]),
            &dirs_under(root.path()),
        )
        .expect("a plain name is fine");
    }

    #[test]
    fn the_managed_volume_root_must_clear_every_server_owned_directory() {
        let root = tempfile::tempdir().unwrap();
        let managed = VolumeTable::from([(
            "memory".to_string(),
            VolumeSpec::managed(SizeLimit::Unlimited),
        )]);
        for (inside, expected) in [
            ("vfs/sessions/volumes", "per-session VFS root"),
            ("blueprints/volumes", "blueprint store"),
            ("secrets", "secret store"),
        ] {
            let dirs = ServerDirectories {
                managed_volume_root: Some(root.path().join(inside)),
                ..dirs_under(root.path())
            };
            let err = validate_volumes(&managed, &dirs).expect_err(inside);
            let msg = err.to_string();
            assert!(msg.contains("managed volume root"), "{inside}: {msg}");
            assert!(msg.contains(expected), "{inside}: {msg}");
        }
        // Unused, the root is not checked: no managed volume lives there.
        let dirs = ServerDirectories {
            managed_volume_root: Some(root.path().join("secrets")),
            ..dirs_under(root.path())
        };
        validate_volumes(&VolumeTable::new(), &dirs).expect("no managed volume declared");
    }

    #[test]
    fn a_local_path_volume_may_not_overlap_the_managed_root() {
        let root = tempfile::tempdir().unwrap();
        let dirs = dirs_under(root.path());
        let msg = refusal("work", root.path().join("volumes/memory"), &dirs);
        assert!(msg.contains("managed volume root"), "{msg}");
        assert!(msg.contains("is inside"), "{msg}");
    }

    #[test]
    fn a_managed_volume_and_a_local_path_at_its_directory_are_refused() {
        let root = tempfile::tempdir().unwrap();
        let dirs = ServerDirectories {
            managed_volume_root: Some(root.path().join("managed")),
            ..ServerDirectories::default()
        };
        let err = validate_volumes(
            &VolumeTable::from([
                (
                    "memory".to_string(),
                    VolumeSpec::managed(SizeLimit::Unlimited),
                ),
                (
                    "alias".to_string(),
                    VolumeSpec::local_path(root.path().join("elsewhere/memory")),
                ),
                (
                    "inner".to_string(),
                    VolumeSpec::local_path(root.path().join("managed/memory/sub")),
                ),
            ]),
            &dirs,
        )
        .expect_err("a local path inside a managed volume must be refused");
        assert!(err.to_string().contains("'inner'"), "{err}");
    }

    #[test]
    fn unknown_key_is_rejected() {
        let err = serde_yml::from_str::<FileConfig>("prot: 9000\n").unwrap_err();
        assert!(err.to_string().contains("prot"), "got: {err}");
    }

    #[test]
    fn defaults_apply_when_nothing_set() {
        let (addr, config) =
            merge(empty_cli(), FileConfig::default(), EnvConfig::default()).unwrap();
        assert_eq!(addr, "127.0.0.1:8128".parse().unwrap());
        assert_eq!(config.blueprint_dir.unwrap(), default_blueprint_dir());
        assert_eq!(
            config.session_storage_root.unwrap(),
            default_session_storage_root()
        );
        assert!(config.ephemeral_storage_root.is_none());
        assert!(config.secret_store.is_none());
    }

    #[test]
    fn secret_store_off_when_no_key_available() {
        // Defaults supply a dir and a key-env name, but with no key file and the
        // default env var unset, the store stays disabled rather than failing.
        let cli = Cli {
            secret_store_key_env: Some("SUB_TEST_SECRET_KEY_UNSET".into()),
            ..empty_cli()
        };
        assert!(
            resolve_secret_store(&cli, &FileConfig::default(), &EnvConfig::default())
                .unwrap()
                .is_none()
        );
    }

    fn write_key_file(dir: &std::path::Path, fill: u8) -> PathBuf {
        use base64::Engine as _;
        let key_path = dir.join("key.b64");
        std::fs::write(
            &key_path,
            base64::engine::general_purpose::STANDARD.encode([fill; 32]),
        )
        .unwrap();
        key_path
    }

    #[test]
    fn secret_store_constructs_from_key_file() {
        let tmp = tempfile::tempdir().unwrap();
        let cli = Cli {
            secret_store_dir: Some(tmp.path().join("secrets")),
            secret_store_key_file: Some(write_key_file(tmp.path(), 3)),
            ..empty_cli()
        };
        let store =
            resolve_secret_store(&cli, &FileConfig::default(), &EnvConfig::default()).unwrap();
        assert!(store.is_some());
    }

    #[test]
    fn secret_store_key_file_takes_priority_over_env() {
        let tmp = tempfile::tempdir().unwrap();
        let cli = Cli {
            secret_store_dir: Some(tmp.path().join("secrets")),
            // The env var is unset (would disable the store); the key file must
            // win so the store still builds.
            secret_store_key_env: Some("SUB_TEST_SECRET_KEY_UNSET".into()),
            secret_store_key_file: Some(write_key_file(tmp.path(), 4)),
            ..empty_cli()
        };
        let store =
            resolve_secret_store(&cli, &FileConfig::default(), &EnvConfig::default()).unwrap();
        assert!(store.is_some());
    }

    #[test]
    fn secret_store_constructs_from_env_key() {
        use base64::Engine as _;
        let tmp = tempfile::tempdir().unwrap();
        let var = "SUB_TEST_SECRET_KEY_ENVPATH";
        // SAFETY: unique var name; this test reads it on this thread only.
        unsafe {
            std::env::set_var(
                var,
                base64::engine::general_purpose::STANDARD.encode([6u8; 32]),
            );
        };
        let cli = Cli {
            secret_store_dir: Some(tmp.path().join("secrets")),
            secret_store_key_env: Some(var.into()),
            ..empty_cli()
        };
        let store =
            resolve_secret_store(&cli, &FileConfig::default(), &EnvConfig::default()).unwrap();
        assert!(store.is_some());
        unsafe { std::env::remove_var(var) };
    }

    #[test]
    fn file_overrides_defaults() {
        let file = FileConfig {
            bind: Some("0.0.0.0".parse().unwrap()),
            port: Some(9000),
            blueprint_dir: Some("/data/bp".into()),
            ..FileConfig::default()
        };
        let (addr, config) = merge(empty_cli(), file, EnvConfig::default()).unwrap();
        assert_eq!(addr, "0.0.0.0:9000".parse().unwrap());
        assert_eq!(config.blueprint_dir.unwrap(), PathBuf::from("/data/bp"));
    }

    #[test]
    fn cli_overrides_file() {
        let cli = Cli {
            port: Some(9999),
            ..empty_cli()
        };
        let file = FileConfig {
            bind: Some("0.0.0.0".parse().unwrap()),
            port: Some(9000),
            ..FileConfig::default()
        };
        let (addr, _) = merge(cli, file, EnvConfig::default()).unwrap();
        // CLI port wins; file bind still applies (CLI left it unset).
        assert_eq!(addr, "0.0.0.0:9999".parse().unwrap());
    }

    #[test]
    fn ambient_port_alone_implies_a_wildcard_bind() {
        // A PaaS (Render et al.) injects $PORT and routes external traffic in,
        // so a loopback bind would be unreachable there.
        let (addr, _) = merge(
            empty_cli(),
            FileConfig::default(),
            env_from(&[("PORT", "10000")]),
        )
        .unwrap();
        assert_eq!(addr, "0.0.0.0:10000".parse().unwrap());
    }

    #[test]
    fn config_file_port_beats_ambient_port() {
        let file = FileConfig {
            port: Some(9000),
            ..FileConfig::default()
        };
        let (addr, _) = merge(empty_cli(), file, env_from(&[("PORT", "10000")])).unwrap();
        assert_eq!(addr.port(), 9000);
    }

    #[test]
    fn config_file_loopback_survives_an_ambient_port() {
        // The regression the two-tier split exists to prevent. `$PORT` alone
        // implies a `0.0.0.0` bind, so a config file that deliberately says
        // loopback must not be flipped open by a platform that merely happens
        // to inject `$PORT`.
        let file = FileConfig {
            bind: Some("127.0.0.1".parse().unwrap()),
            ..FileConfig::default()
        };
        let (addr, _) = merge(empty_cli(), file, env_from(&[("PORT", "10000")])).unwrap();
        assert!(
            addr.ip().is_loopback(),
            "ambient $PORT overrode an explicit loopback bind: {addr}"
        );
        assert_eq!(addr.port(), 10000);
    }

    #[test]
    fn health_probe_address_follows_the_config_file() {
        // The reason the probe resolves config rather than hardcoding a URL: a
        // server moved off 8128 by its config file must still be probed there.
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("server.yaml");
        std::fs::write(&path, "bind: 0.0.0.0\nport: 9999\n").unwrap();
        let cli = Cli {
            config: Some(path),
            ..empty_cli()
        };
        let addr = resolve_bind_addr(&cli).unwrap();
        assert_eq!(addr, "0.0.0.0:9999".parse().unwrap());
    }

    #[test]
    fn health_probe_address_prefers_the_flag_over_the_file() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("server.yaml");
        std::fs::write(&path, "port: 9999\n").unwrap();
        let cli = Cli {
            config: Some(path),
            port: Some(7777),
            ..empty_cli()
        };
        assert_eq!(resolve_bind_addr(&cli).unwrap().port(), 7777);
    }

    #[test]
    fn mcp_allowed_hosts_none_when_unset() {
        let (_, config) = merge(empty_cli(), FileConfig::default(), EnvConfig::default()).unwrap();
        assert!(config.mcp_allowed_hosts.is_none());
    }

    #[test]
    fn mcp_allowed_hosts_prepends_loopback_defaults() {
        let cli = Cli {
            mcp_allowed_host: vec!["submilli-ai:10000".into()],
            ..empty_cli()
        };
        let (_, config) = merge(cli, FileConfig::default(), EnvConfig::default()).unwrap();
        let hosts = config.mcp_allowed_hosts.expect("hosts set");
        for default in MCP_LOOPBACK_HOSTS {
            assert!(hosts.iter().any(|h| h == default), "missing {default}");
        }
        assert!(hosts.iter().any(|h| h == "submilli-ai:10000"));
    }

    #[test]
    fn mcp_allowed_hosts_merges_cli_file_and_env() {
        let cli = Cli {
            mcp_allowed_host: vec!["from-cli".into()],
            ..empty_cli()
        };
        let file = FileConfig {
            mcp_allowed_hosts: vec!["from-file".into()],
            ..FileConfig::default()
        };
        let env = env_from(&[("SUBMILLI_MCP_ALLOWED_HOSTS", "from-env")]);
        let (_, config) = merge(cli, file, env).unwrap();
        let hosts = config.mcp_allowed_hosts.expect("hosts set");
        for expected in ["from-cli", "from-file", "from-env"] {
            assert!(hosts.iter().any(|h| h == expected), "missing {expected}");
        }
    }

    #[test]
    fn allow_ip_accepts_bare_ip_and_cidr() {
        assert!(parse_ip_or_cidr("1.2.3.4").is_ok());
        assert!(parse_ip_or_cidr("10.0.0.0/24").is_ok());
        assert!(parse_ip_or_cidr("not-an-ip").is_err());
    }

    #[test]
    fn bad_allow_ip_entry_errors() {
        let file = FileConfig {
            network: NetworkFileConfig {
                allow_ip: vec!["nonsense".into()],
                ..NetworkFileConfig::default()
            },
            ..FileConfig::default()
        };
        let Err(err) = merge(empty_cli(), file, EnvConfig::default()) else {
            panic!("expected a parse error for the bad allow_ip entry");
        };
        assert!(err.to_string().contains("network.allow_ip"), "got: {err}");
    }

    /// An [`EnvConfig`] over a fixed set of variables, so precedence is asserted
    /// without mutating the process environment other tests are reading.
    fn env_from(pairs: &[(&str, &str)]) -> EnvConfig {
        EnvConfig::from_lookup(|name| {
            pairs
                .iter()
                .find(|(key, _)| *key == name)
                .map(|(_, value)| (*value).to_owned())
        })
    }

    #[test]
    fn env_overrides_beat_the_config_file() {
        let env = env_from(&[
            ("SUBMILLI_BIND", "0.0.0.0"),
            ("SUBMILLI_PORT", "9100"),
            ("SUBMILLI_BLUEPRINT_DIR", "/env/bp"),
            ("SUBMILLI_SESSION_STORE_DIR", "/env/sessions"),
            ("SUBMILLI_VFS_SESSION_DIR", "/env/vfs"),
            ("SUBMILLI_VFS_EPHEMERAL_DIR", "/env/scratch"),
            ("SUBMILLI_PACKAGE_STORE_DIR", "/env/packages"),
        ]);
        let file = FileConfig {
            bind: Some("10.0.0.1".parse().unwrap()),
            port: Some(9000),
            blueprint_dir: Some("/file/bp".into()),
            session_store_dir: Some("/file/sessions".into()),
            vfs_session_dir: Some("/file/vfs".into()),
            vfs_ephemeral_dir: Some("/file/scratch".into()),
            package_store_dir: Some("/file/packages".into()),
            ..FileConfig::default()
        };
        let (addr, config) = merge(empty_cli(), file, env).unwrap();
        assert_eq!(addr, "0.0.0.0:9100".parse().unwrap());
        assert_eq!(config.blueprint_dir.unwrap(), PathBuf::from("/env/bp"));
        assert_eq!(
            config.session_store_dir.unwrap(),
            PathBuf::from("/env/sessions")
        );
        assert_eq!(
            config.session_storage_root.unwrap(),
            PathBuf::from("/env/vfs")
        );
        assert_eq!(
            config.ephemeral_storage_root.unwrap(),
            PathBuf::from("/env/scratch")
        );
        assert_eq!(
            config.package_store_root.unwrap(),
            PathBuf::from("/env/packages")
        );
    }

    #[test]
    fn cli_flags_beat_env_overrides() {
        let cli = Cli {
            port: Some(7777),
            blueprint_dir: Some("/cli/bp".into()),
            ..empty_cli()
        };
        let env = env_from(&[
            ("SUBMILLI_PORT", "9100"),
            ("SUBMILLI_BLUEPRINT_DIR", "/env/bp"),
        ]);
        let (addr, config) = merge(cli, FileConfig::default(), env).unwrap();
        assert_eq!(addr.port(), 7777);
        assert_eq!(config.blueprint_dir.unwrap(), PathBuf::from("/cli/bp"));
    }

    #[test]
    fn bind_and_port_walk_the_full_ladder() {
        // FileConfig isn't Clone, so each rung builds its own.
        let file = || FileConfig {
            bind: Some("10.0.0.1".parse().unwrap()),
            port: Some(9000),
            ..FileConfig::default()
        };
        let ambient = [("HOST", "192.168.0.1"), ("PORT", "10000")];
        let explicit_env = [
            ("SUBMILLI_BIND", "172.16.0.1"),
            ("SUBMILLI_PORT", "9500"),
            ("HOST", "192.168.0.1"),
            ("PORT", "10000"),
        ];
        let cli = || Cli {
            bind: Some("10.1.2.3".parse().unwrap()),
            port: Some(7777),
            ..empty_cli()
        };

        let rungs = [
            // (cli, file, env, expected) — each rung removes the tier above it.
            (
                cli(),
                file(),
                env_from(&explicit_env),
                "10.1.2.3:7777",
                "the flag",
            ),
            (
                empty_cli(),
                file(),
                env_from(&explicit_env),
                "172.16.0.1:9500",
                "the explicit SUBMILLI_* vars",
            ),
            (
                empty_cli(),
                file(),
                env_from(&ambient),
                "10.0.0.1:9000",
                "the config file",
            ),
            (
                empty_cli(),
                FileConfig::default(),
                env_from(&ambient),
                "192.168.0.1:10000",
                "ambient HOST/PORT",
            ),
            (
                empty_cli(),
                FileConfig::default(),
                EnvConfig::default(),
                "127.0.0.1:8128",
                "the built-in default",
            ),
        ];
        for (cli, file, env, expected, tier) in rungs {
            let (addr, _) = merge(cli, file, env).unwrap();
            assert_eq!(addr, expected.parse().unwrap(), "{tier} should have won");
        }
    }

    #[test]
    fn submilli_bind_and_port_outrank_ambient_host_and_port() {
        let env = env_from(&[
            ("SUBMILLI_BIND", "172.16.0.1"),
            ("SUBMILLI_PORT", "9500"),
            ("HOST", "192.168.0.1"),
            ("PORT", "10000"),
        ]);
        let (addr, _) = merge(empty_cli(), FileConfig::default(), env).unwrap();
        assert_eq!(addr, "172.16.0.1:9500".parse().unwrap());
    }

    #[test]
    fn env_allow_ip_unions_with_cli_and_file() {
        // Private addresses throughout: the base policy is deny-private, so a
        // public address would be permitted whether or not the grant landed.
        let cli = Cli {
            allow_ip: vec![parse_ip_or_cidr("10.1.1.1").unwrap()],
            ..empty_cli()
        };
        let file = FileConfig {
            network: NetworkFileConfig {
                allow_ip: vec!["172.16.5.0/24".into()],
                ..NetworkFileConfig::default()
            },
            ..FileConfig::default()
        };
        let env = env_from(&[("SUBMILLI_ALLOW_IP", "192.168.7.0/24, 10.2.2.2")]);
        let (_, config) = merge(cli, file, env).unwrap();

        for granted in ["10.1.1.1", "172.16.5.9", "192.168.7.9", "10.2.2.2"] {
            assert!(
                config.network_policy.permits(granted.parse().unwrap()),
                "{granted} should have been granted by one of the three sources"
            );
        }
        assert!(
            !config
                .network_policy
                .permits("192.168.99.99".parse().unwrap()),
            "an ungranted private address must stay blocked"
        );
    }

    #[test]
    fn env_allow_flags_grant_despite_a_config_file_false() {
        // Additive across every source: no source can revoke another's grant.
        let file = FileConfig {
            network: NetworkFileConfig {
                allow_localhost: Some(false),
                allow_private: Some(false),
                ..NetworkFileConfig::default()
            },
            ..FileConfig::default()
        };
        let env = env_from(&[
            ("SUBMILLI_ALLOW_LOCALHOST", "true"),
            ("SUBMILLI_ALLOW_PRIVATE", "1"),
        ]);
        let (_, config) = merge(empty_cli(), file, env).unwrap();
        assert!(config.network_policy.permits("127.0.0.1".parse().unwrap()));
        assert!(config.network_policy.permits("10.1.2.3".parse().unwrap()));
    }

    #[test]
    fn malformed_explicit_bind_fails_boot_instead_of_falling_through() {
        // The fail-open case. A discarded `SUBMILLI_BIND` drops to the ambient
        // tier, where `$PORT`'s presence alone means `0.0.0.0` — so a typo in an
        // operator's loopback bind would publish the server.
        let env = env_from(&[("SUBMILLI_BIND", "localhost"), ("PORT", "10000")]);
        let Err(err) = merge(empty_cli(), FileConfig::default(), env) else {
            panic!("a malformed SUBMILLI_BIND must not resolve to an address at all");
        };
        assert!(err.to_string().contains("SUBMILLI_BIND"), "got: {err}");
    }

    #[test]
    fn malformed_explicit_port_fails_boot_naming_the_variable() {
        let env = env_from(&[("SUBMILLI_PORT", "not-a-port")]);
        let Err(err) = merge(empty_cli(), FileConfig::default(), env) else {
            panic!("a malformed SUBMILLI_PORT should have failed boot");
        };
        assert!(err.to_string().contains("SUBMILLI_PORT"), "got: {err}");
    }

    #[test]
    fn malformed_explicit_grace_fails_boot_naming_the_variable() {
        let env = env_from(&[("SUBMILLI_SHUTDOWN_GRACE", "8s")]);
        let Err(err) = shutdown_grace(&empty_cli(), &FileConfig::default(), &env) else {
            panic!("a malformed SUBMILLI_SHUTDOWN_GRACE should have failed boot");
        };
        assert!(
            err.to_string().contains("SUBMILLI_SHUTDOWN_GRACE"),
            "got: {err}"
        );
    }

    #[test]
    fn ambient_host_and_port_keep_their_tolerance() {
        // A platform injects these; erroring on one would break a deployment
        // that never asked for it. Only the explicit tier is strict.
        let env = env_from(&[("HOST", "not-an-ip"), ("PORT", "not-a-port")]);
        let (addr, _) = merge(empty_cli(), FileConfig::default(), env)
            .expect("ambient values must never fail boot");
        // The unparseable HOST is ignored and the port falls back to the
        // default, but `$PORT` being *present* still implies a wildcard bind —
        // the long-standing PaaS inference, unchanged by this ladder.
        assert_eq!(addr, "0.0.0.0:8128".parse().unwrap());
    }

    #[test]
    fn shutdown_grace_walks_the_ladder() {
        let file = || FileConfig {
            shutdown_grace: Some(30),
            ..FileConfig::default()
        };
        let with_flag = Cli {
            shutdown_grace: Some(45),
            ..empty_cli()
        };
        let env = || env_from(&[("SUBMILLI_SHUTDOWN_GRACE", "20")]);

        for (cli, file, env, expected, tier) in [
            (with_flag, file(), env(), 45, "the flag"),
            (empty_cli(), file(), env(), 20, "the env var"),
            (
                empty_cli(),
                file(),
                EnvConfig::default(),
                30,
                "the config file",
            ),
            (
                empty_cli(),
                FileConfig::default(),
                EnvConfig::default(),
                DEFAULT_SHUTDOWN_GRACE_SECS,
                "the default",
            ),
        ] {
            let grace = shutdown_grace(&cli, &file, &env).unwrap();
            assert_eq!(
                grace,
                Duration::from_secs(expected),
                "{tier} should have won"
            );
        }
    }

    #[test]
    fn execution_timeout_configuration() {
        let file: FileConfig = serde_yml::from_str("max_execution_time: 30").unwrap();
        let mut cli = empty_cli();
        let mut env = EnvConfig::default();
        assert_eq!(
            max_execution_time(&cli, &FileConfig::default(), &env).unwrap(),
            None
        );
        assert_eq!(
            max_execution_time(&cli, &file, &env).unwrap(),
            Some(Duration::from_secs(30))
        );
        env = env_from(&[("SUBMILLI_MAX_EXECUTION_TIME", "20")]);
        assert_eq!(
            max_execution_time(&cli, &file, &env).unwrap(),
            Some(Duration::from_secs(20))
        );
        cli.max_execution_time = Some(10);
        assert_eq!(
            max_execution_time(&cli, &file, &env).unwrap(),
            Some(Duration::from_secs(10))
        );
        cli.max_execution_time = Some(0);
        assert_eq!(max_execution_time(&cli, &file, &env).unwrap(), None);
        cli.max_execution_time = None;
        env = env_from(&[("SUBMILLI_MAX_EXECUTION_TIME", "0")]);
        assert_eq!(max_execution_time(&cli, &file, &env).unwrap(), None);
        for value in ["-1", "1.5", "30s"] {
            env = env_from(&[("SUBMILLI_MAX_EXECUTION_TIME", value)]);
            assert!(
                max_execution_time(&cli, &file, &env)
                    .unwrap_err()
                    .to_string()
                    .contains("SUBMILLI_MAX_EXECUTION_TIME")
            );
            assert!(
                serde_yml::from_str::<FileConfig>(&format!("max_execution_time: {value}")).is_err()
            );
        }
    }

    #[test]
    fn max_execution_memory_walks_the_ladder() {
        const MB: u64 = 1024 * 1024;
        let file = || FileConfig {
            max_execution_memory: Some(300),
            ..FileConfig::default()
        };
        let with_flag = Cli {
            max_execution_memory: Some(400),
            ..empty_cli()
        };
        let env = || env_from(&[("SUBMILLI_MAX_EXECUTION_MEMORY", "200")]);

        for (cli, file, env, expected, tier) in [
            (with_flag, file(), env(), 400 * MB, "the flag"),
            (empty_cli(), file(), env(), 200 * MB, "the env var"),
            (
                empty_cli(),
                file(),
                EnvConfig::default(),
                300 * MB,
                "the config file",
            ),
            (
                empty_cli(),
                FileConfig::default(),
                EnvConfig::default(),
                DEFAULT_MAX_STORE_BYTES,
                "the default",
            ),
        ] {
            let bytes = max_execution_memory(&cli, &file, &env).unwrap();
            assert_eq!(bytes, expected, "{tier} should have won");
        }
    }

    #[test]
    fn max_execution_fuel_and_stack_walk_the_ladder() {
        let defaults = RuntimeConfig::default();
        let file = FileConfig {
            max_execution_fuel: Some(3_000),
            max_execution_stack: Some(300),
            ..FileConfig::default()
        };
        let env = env_from(&[
            ("SUBMILLI_MAX_EXECUTION_FUEL", "2000"),
            ("SUBMILLI_MAX_EXECUTION_STACK", "200"),
        ]);
        let flag = Cli {
            max_execution_fuel: Some(1_000),
            max_execution_stack: Some(100),
            ..empty_cli()
        };

        assert_eq!(max_execution_fuel(&flag, &file, &env).unwrap(), 1_000);
        assert_eq!(max_execution_stack(&flag, &file, &env).unwrap(), 100 * 1024);
        assert_eq!(
            max_execution_fuel(&empty_cli(), &file, &env).unwrap(),
            2_000
        );
        assert_eq!(
            max_execution_stack(&empty_cli(), &file, &env).unwrap(),
            200 * 1024
        );
        let no_env = EnvConfig::default();
        assert_eq!(
            max_execution_fuel(&empty_cli(), &file, &no_env).unwrap(),
            3_000
        );
        assert_eq!(
            max_execution_stack(&empty_cli(), &file, &no_env).unwrap(),
            300 * 1024
        );
        let none = FileConfig::default();
        assert_eq!(
            max_execution_fuel(&empty_cli(), &none, &no_env).unwrap(),
            defaults.fuel
        );
        assert_eq!(
            max_execution_stack(&empty_cli(), &none, &no_env).unwrap(),
            defaults.max_wasm_stack
        );

        let zero = FileConfig {
            max_execution_fuel: Some(0),
            max_execution_stack: Some(0),
            ..FileConfig::default()
        };
        assert!(max_execution_fuel(&empty_cli(), &zero, &no_env).is_err());
        assert!(max_execution_stack(&empty_cli(), &zero, &no_env).is_err());
        let huge = FileConfig {
            max_execution_stack: Some(MAX_EXECUTION_STACK_KIB + 1),
            ..FileConfig::default()
        };
        assert!(max_execution_stack(&empty_cli(), &huge, &no_env).is_err());
    }

    /// The same ladder, but unset stays `None` so the session manager's own
    /// default applies rather than being duplicated here.
    #[test]
    fn max_session_state_memory_walks_the_ladder() {
        const MB: u64 = 1024 * 1024;
        let file = || FileConfig {
            max_session_state_memory: Some(300),
            ..FileConfig::default()
        };
        let with_flag = Cli {
            max_session_state_memory: Some(400),
            ..empty_cli()
        };
        let env = || env_from(&[("SUBMILLI_MAX_SESSION_STATE_MEMORY", "200")]);

        for (cli, file, env, expected, tier) in [
            (with_flag, file(), env(), Some(400 * MB), "the flag"),
            (empty_cli(), file(), env(), Some(200 * MB), "the env var"),
            (
                empty_cli(),
                file(),
                EnvConfig::default(),
                Some(300 * MB),
                "the config file",
            ),
            (
                empty_cli(),
                FileConfig::default(),
                EnvConfig::default(),
                None,
                "unset",
            ),
        ] {
            let bytes = max_session_state_memory(&cli, &file, &env).unwrap();
            assert_eq!(bytes, expected, "{tier} should have won");
        }
    }

    /// The aggregate LLM ceiling walks the same rungs, in tokens rather than
    /// megabytes — no unit conversion, so the number an operator writes is the
    /// number the refusal names.
    #[test]
    fn max_llm_tokens_walks_the_ladder() {
        let file = || FileConfig {
            max_llm_tokens: Some(300),
            ..FileConfig::default()
        };
        let with_flag = Cli {
            max_llm_tokens: Some(400),
            ..empty_cli()
        };
        let env = || env_from(&[("SUBMILLI_MAX_LLM_TOKENS", "200")]);

        for (cli, file, env, expected, tier) in [
            (with_flag, file(), env(), Some(400), "the flag"),
            (empty_cli(), file(), env(), Some(200), "the env var"),
            (
                empty_cli(),
                file(),
                EnvConfig::default(),
                Some(300),
                "the config file",
            ),
            (
                empty_cli(),
                FileConfig::default(),
                EnvConfig::default(),
                None,
                "unset",
            ),
        ] {
            let tokens = max_llm_tokens(&cli, &file, &env).unwrap();
            assert_eq!(tokens, expected, "{tier} should have won");
        }
    }

    /// The per-execution ceiling is the one rung of `LlmLimits` an operator
    /// tunes, and unlike the aggregate it resolves to a concrete default rather
    /// than to `None` — so "unset" asserts the runtime's own constant.
    #[test]
    fn max_execution_llm_tokens_walks_the_ladder() {
        let file = || FileConfig {
            max_execution_llm_tokens: Some(300),
            ..FileConfig::default()
        };
        let with_flag = Cli {
            max_execution_llm_tokens: Some(400),
            ..empty_cli()
        };
        let env = || env_from(&[("SUBMILLI_MAX_EXECUTION_LLM_TOKENS", "200")]);

        for (cli, file, env, expected, tier) in [
            (with_flag, file(), env(), 400, "the flag"),
            (empty_cli(), file(), env(), 200, "the env var"),
            (
                empty_cli(),
                file(),
                EnvConfig::default(),
                300,
                "the config file",
            ),
            (
                empty_cli(),
                FileConfig::default(),
                EnvConfig::default(),
                DEFAULT_MAX_EXECUTION_TOKENS,
                "unset",
            ),
        ] {
            let tokens = max_execution_llm_tokens(&cli, &file, &env).unwrap();
            assert_eq!(tokens, expected, "{tier} should have won");
        }
    }

    /// The batch fan-out bound (KTD4) walks the same ladder.
    #[test]
    fn max_llm_concurrency_walks_the_ladder() {
        let file = || FileConfig {
            max_llm_concurrency: Some(3),
            ..FileConfig::default()
        };
        let with_flag = Cli {
            max_llm_concurrency: Some(9),
            ..empty_cli()
        };
        let env = || env_from(&[("SUBMILLI_MAX_LLM_CONCURRENCY", "6")]);

        for (cli, file, env, expected, tier) in [
            (with_flag, file(), env(), Some(9), "the flag"),
            (empty_cli(), file(), env(), Some(6), "the env var"),
            (
                empty_cli(),
                file(),
                EnvConfig::default(),
                Some(3),
                "the config file",
            ),
            (
                empty_cli(),
                FileConfig::default(),
                EnvConfig::default(),
                None,
                "unset",
            ),
        ] {
            let limit = max_llm_concurrency(&cli, &file, &env).unwrap();
            assert_eq!(limit, expected, "{tier} should have won");
        }
    }

    /// Zero is rejected on all three, rather than meaning "unlimited" (the
    /// budgets) or "no concurrency" (the bound, where it would deadlock).
    #[test]
    fn zero_llm_settings_are_rejected() {
        for (name, value) in [
            ("SUBMILLI_MAX_LLM_TOKENS", "0"),
            ("SUBMILLI_MAX_EXECUTION_LLM_TOKENS", "0"),
            ("SUBMILLI_MAX_LLM_CONCURRENCY", "0"),
        ] {
            let env = env_from(&[(name, value)]);
            let cli = empty_cli();
            let file = FileConfig::default();
            let failed = max_llm_tokens(&cli, &file, &env).is_err()
                || max_execution_llm_tokens(&cli, &file, &env).is_err()
                || max_llm_concurrency(&cli, &file, &env).is_err();
            assert!(failed, "${name}=0 should have failed boot");
        }
    }

    /// A set-but-unparseable value fails boot naming the variable, rather than
    /// falling through to a weaker source — a silently-ignored ceiling is how a
    /// server ends up spending more than the operator asked.
    #[test]
    fn malformed_llm_settings_fail_boot_naming_the_variable() {
        let env = env_from(&[("SUBMILLI_MAX_LLM_TOKENS", "lots")]);
        let Err(err) = max_llm_tokens(&empty_cli(), &FileConfig::default(), &env) else {
            panic!("a malformed SUBMILLI_MAX_LLM_TOKENS should have failed boot");
        };
        assert!(
            err.to_string().contains("SUBMILLI_MAX_LLM_TOKENS"),
            "the error must name the variable: {err}"
        );
    }

    #[test]
    fn zero_session_state_memory_is_rejected() {
        let env = env_from(&[("SUBMILLI_MAX_SESSION_STATE_MEMORY", "0")]);
        assert!(
            max_session_state_memory(&empty_cli(), &FileConfig::default(), &env).is_err(),
            "zero would refuse every session write, not mean unlimited"
        );
    }

    #[test]
    fn malformed_explicit_memory_fails_boot_naming_the_variable() {
        let env = env_from(&[("SUBMILLI_MAX_EXECUTION_MEMORY", "512mb")]);
        let Err(err) = max_execution_memory(&empty_cli(), &FileConfig::default(), &env) else {
            panic!("a malformed SUBMILLI_MAX_EXECUTION_MEMORY should have failed boot");
        };
        assert!(
            err.to_string().contains("SUBMILLI_MAX_EXECUTION_MEMORY"),
            "got: {err}"
        );
    }

    /// Zero is rejected rather than read as "unlimited": unbounded is the state
    /// this setting exists to end, so the permissive reading is the wrong one.
    #[test]
    fn a_zero_memory_budget_is_rejected() {
        let cli = Cli {
            max_execution_memory: Some(0),
            ..empty_cli()
        };
        let Err(err) = max_execution_memory(&cli, &FileConfig::default(), &EnvConfig::default())
        else {
            panic!("a zero budget should have failed boot");
        };
        assert!(err.to_string().contains("at least 1 MB"), "got: {err}");
    }

    #[test]
    fn malformed_env_allow_ip_names_the_variable() {
        // Unlike a bad port, a silently-dropped network grant is a security
        // surprise, so this one fails boot and says where it came from.
        let env = env_from(&[("SUBMILLI_ALLOW_IP", "10.0.0.0/24,nonsense")]);
        let Err(err) = merge(empty_cli(), FileConfig::default(), env) else {
            panic!("expected a parse error for the bad SUBMILLI_ALLOW_IP entry");
        };
        assert!(err.to_string().contains("SUBMILLI_ALLOW_IP"), "got: {err}");
    }

    #[test]
    fn empty_env_values_are_treated_as_unset() {
        let file = FileConfig {
            port: Some(9000),
            blueprint_dir: Some("/file/bp".into()),
            ..FileConfig::default()
        };
        let env = env_from(&[
            ("SUBMILLI_PORT", "  "),
            ("SUBMILLI_BLUEPRINT_DIR", ""),
            ("SUBMILLI_ALLOW_IP", ""),
        ]);
        let (addr, config) = merge(empty_cli(), file, env).unwrap();
        assert_eq!(addr.port(), 9000);
        assert_eq!(config.blueprint_dir.unwrap(), PathBuf::from("/file/bp"));
    }

    #[test]
    fn env_booleans_use_the_telemetry_vocabulary() {
        for truthy in ["1", "true", "TRUE", "yes", "on", " true "] {
            let env = env_from(&[("SUBMILLI_ALLOW_LOCALHOST", truthy)]);
            assert!(env.allow_localhost, "{truthy:?} should read as true");
        }
        for falsy in ["0", "false", "no", "off", "", "maybe"] {
            let env = env_from(&[("SUBMILLI_ALLOW_LOCALHOST", falsy)]);
            assert!(!env.allow_localhost, "{falsy:?} should read as false");
        }
    }

    #[test]
    fn secret_store_walks_the_ladder() {
        // Every other multi-source field has a precedence test; the store's
        // three variables only ever appeared as the sole source.
        let tmp = tempfile::tempdir().unwrap();
        let key = write_key_file(tmp.path(), 9);
        let cli_dir = tmp.path().join("from-cli");
        let env_dir = tmp.path().join("from-env");

        let env = env_from(&[
            ("SUBMILLI_SECRET_STORE_DIR", env_dir.to_str().unwrap()),
            ("SUBMILLI_SECRET_STORE_KEY_FILE", key.to_str().unwrap()),
        ]);
        let file = FileConfig {
            secret_store: SecretStoreFileConfig {
                dir: Some(tmp.path().join("from-file")),
                key_file: Some(key.clone()),
                ..SecretStoreFileConfig::default()
            },
            ..FileConfig::default()
        };
        // Env over file: the env dir is the one that gets created.
        resolve_secret_store(&empty_cli(), &file, &env)
            .unwrap()
            .expect("store enabled");
        assert!(env_dir.exists(), "env dir should have won over the file's");

        // CLI over env.
        let cli = Cli {
            secret_store_dir: Some(cli_dir.clone()),
            ..empty_cli()
        };
        resolve_secret_store(&cli, &file, &env)
            .unwrap()
            .expect("store enabled");
        assert!(cli_dir.exists(), "cli dir should have won over the env's");
    }

    #[test]
    fn env_secret_store_key_file_beats_key_env() {
        let tmp = tempfile::tempdir().unwrap();
        let env = env_from(&[
            (
                "SUBMILLI_SECRET_STORE_DIR",
                tmp.path().join("secrets").to_str().unwrap(),
            ),
            // Unset in the real environment, so it alone would leave the store off.
            ("SUBMILLI_SECRET_STORE_KEY_ENV", "SUB_TEST_SECRET_KEY_UNSET"),
            (
                "SUBMILLI_SECRET_STORE_KEY_FILE",
                write_key_file(tmp.path(), 7).to_str().unwrap(),
            ),
        ]);
        let store = resolve_secret_store(&empty_cli(), &FileConfig::default(), &env).unwrap();
        assert!(
            store.is_some(),
            "the key file should have enabled the store"
        );
    }

    #[test]
    fn env_config_supplies_the_config_file_path() {
        let tmp = tempfile::tempdir().unwrap();
        let from_env = tmp.path().join("env.yaml");
        let from_flag = tmp.path().join("flag.yaml");
        std::fs::write(&from_env, "port: 9100\n").unwrap();
        std::fs::write(&from_flag, "port: 9200\n").unwrap();

        let env = env_from(&[("SUBMILLI_CONFIG", from_env.to_str().unwrap())]);
        assert_eq!(
            load_config_file(&empty_cli(), &env).unwrap().port,
            Some(9100)
        );

        let cli = Cli {
            config: Some(from_flag),
            ..empty_cli()
        };
        assert_eq!(load_config_file(&cli, &env).unwrap().port, Some(9200));
    }

    #[test]
    fn egress_grants_names_every_widening_variable() {
        assert!(EnvConfig::default().egress_grants().is_empty());
        let env = env_from(&[
            ("SUBMILLI_ALLOW_LOCALHOST", "true"),
            ("SUBMILLI_ALLOW_PRIVATE", "on"),
            ("SUBMILLI_ALLOW_IP", "1.2.3.4"),
        ]);
        assert_eq!(
            env.egress_grants(),
            [
                "SUBMILLI_ALLOW_LOCALHOST",
                "SUBMILLI_ALLOW_PRIVATE",
                "SUBMILLI_ALLOW_IP"
            ]
        );
    }

    #[test]
    fn legacy_layout_marks_only_the_directories_left_at_their_defaults() {
        let tmp = tempfile::tempdir().unwrap();
        let cli = Cli {
            vfs_session_dir: Some(tmp.path().join("vfs")),
            // A key is configured, so only the explicit dir can keep `secrets`
            // out of the migration.
            secret_store_key_file: Some(write_key_file(tmp.path(), 3)),
            ..empty_cli()
        };
        let env = env_from(&[("SUBMILLI_BLUEPRINT_DIR", tmp.path().to_str().unwrap())]);
        let file = FileConfig {
            secret_store: SecretStoreFileConfig {
                dir: Some(tmp.path().join("secrets")),
                ..SecretStoreFileConfig::default()
            },
            ..FileConfig::default()
        };

        let layout = legacy_layout(&cli, &file, &env);

        assert!(!layout.blueprints, "env var names the blueprint dir");
        assert!(layout.sessions, "nothing names the session store");
        assert!(!layout.vfs_sessions, "flag names the VFS root");
        assert!(
            layout.secrets.is_none(),
            "an explicit secret dir is never migrated, key or no key"
        );
    }

    #[test]
    fn legacy_layout_marks_secrets_only_when_a_key_is_configured() {
        let tmp = tempfile::tempdir().unwrap();
        let unset = Cli {
            secret_store_key_env: Some("SUB_TEST_LEGACY_KEY_UNSET".into()),
            ..empty_cli()
        };
        let keyless = legacy_layout(&unset, &FileConfig::default(), &EnvConfig::default());
        assert!(keyless.secrets.is_none());
        assert!(!keyless.key_configured);
        assert!(
            keyless.blueprints,
            "the other directories are still eligible"
        );

        let key = write_key_file(tmp.path(), 3);
        let keyed_cli = Cli {
            secret_store_key_file: Some(key.clone()),
            ..empty_cli()
        };
        let keyed = legacy_layout(&keyed_cli, &FileConfig::default(), &EnvConfig::default());
        assert!(keyed.key_configured);
        assert!(matches!(keyed.secrets, Some(KeySource::File(path)) if path == key));
    }

    #[test]
    fn secret_key_source_prefers_a_key_file_over_the_env_var() {
        let tmp = tempfile::tempdir().unwrap();
        let key = write_key_file(tmp.path(), 5);
        let env = env_from(&[("SUBMILLI_SECRET_STORE_KEY_FILE", key.to_str().unwrap())]);
        let cli = Cli {
            // Even a key env var that is set loses to a configured key file.
            secret_store_key_env: Some("PATH".into()),
            ..empty_cli()
        };

        let source = secret_key_source(&cli, &FileConfig::default(), &env);

        assert!(matches!(source, Some(KeySource::File(path)) if path == key));
    }

    #[test]
    fn secret_key_source_uses_the_env_var_only_when_it_is_set() {
        // SAFETY (test-only): a name no other test reads, set and removed
        // within this test, matching `secret_store_constructs_from_env_key`.
        let var = "SUB_TEST_KEY_SOURCE_ENV";
        let cli = Cli {
            secret_store_key_env: Some(var.into()),
            ..empty_cli()
        };

        unsafe { std::env::remove_var(var) };
        assert!(secret_key_source(&cli, &FileConfig::default(), &EnvConfig::default()).is_none());

        unsafe { std::env::set_var(var, "anything") };
        let source = secret_key_source(&cli, &FileConfig::default(), &EnvConfig::default());
        unsafe { std::env::remove_var(var) };
        assert!(matches!(source, Some(KeySource::Env(name)) if name == var));
    }

    const ADMIN_TOKEN: &str = "admin-token-0123456789abcdef0123456789";
    const USER_TOKEN: &str = "user-token-0123456789abcdef01234567890";

    fn write_token_file(dir: &Path, name: &str, contents: &str) -> PathBuf {
        let path = dir.join(name);
        std::fs::write(&path, contents).unwrap();
        path
    }

    fn token_from_file(name: &str, role: Role, path: PathBuf) -> ApiTokenFileConfig {
        ApiTokenFileConfig {
            name: name.into(),
            role,
            token_file: path,
        }
    }

    /// The CLI as the binary sees it when no opt-out flag is passed.
    fn auth_required_cli() -> Cli {
        Cli {
            allow_unauthenticated: false,
            ..empty_cli()
        }
    }

    fn auth_of(tokens: Vec<ApiTokenFileConfig>) -> Result<AuthConfig> {
        let file = FileConfig {
            api_tokens: tokens,
            ..FileConfig::default()
        };
        resolve_auth(&auth_required_cli(), &file, &EnvConfig::default())
    }

    fn auth_err(tokens: Vec<ApiTokenFileConfig>) -> String {
        auth_of(tokens)
            .expect_err("auth should be refused")
            .to_string()
    }

    #[test]
    fn api_tokens_parse_from_the_config_file() {
        let yaml = "
api_tokens:
  - name: ops
    role: admin
    token_file: /run/secrets/admin
  - name: app
    role: user
    token_file: /run/secrets/user
";
        let cfg: FileConfig = serde_yml::from_str(yaml).unwrap();
        assert_eq!(cfg.api_tokens.len(), 2);
        assert_eq!(cfg.api_tokens[0].name, "ops");
        assert_eq!(cfg.api_tokens[0].role, Role::Admin);
        assert_eq!(
            cfg.api_tokens[0].token_file,
            PathBuf::from("/run/secrets/admin")
        );
        assert_eq!(cfg.api_tokens[1].role, Role::User);
        assert_eq!(
            cfg.api_tokens[1].token_file,
            PathBuf::from("/run/secrets/user")
        );
        assert!(FileConfig::default().api_tokens.is_empty());
    }

    #[test]
    fn github_token_file_parses_and_reaches_the_server_config() {
        let cfg: FileConfig =
            serde_yml::from_str("github_token_file: /run/secrets/github\n").unwrap();
        assert_eq!(
            cfg.github_token_file,
            Some(PathBuf::from("/run/secrets/github"))
        );
        assert_eq!(
            guarded_directories(&empty_cli(), &cfg, &EnvConfig::default()).github_token_file,
            Some(PathBuf::from("/run/secrets/github"))
        );
        assert!(FileConfig::default().github_token_file.is_none());
    }

    #[test]
    fn an_unknown_role_or_token_key_is_rejected() {
        for entry in [
            "{ name: a, role: root, token_file: /t }",
            "{ name: a, role: admin, token: literal }",
            "{ name: a, role: admin, token_env: T }",
            "{ name: a, role: admin }",
        ] {
            let yaml = format!("api_tokens:\n  - {entry}\n");
            assert!(serde_yml::from_str::<FileConfig>(&yaml).is_err(), "{entry}");
        }
    }

    #[test]
    fn a_server_with_no_tokens_is_refused_unless_it_opts_out() {
        let err = auth_err(vec![]);
        assert!(err.contains("api_tokens"), "got: {err}");
        assert!(err.contains("allow_unauthenticated"), "got: {err}");

        let file = FileConfig::default();
        let env = EnvConfig::default();
        let by_flag = Cli {
            allow_unauthenticated: true,
            ..auth_required_cli()
        };
        let by_flag = resolve_auth(&by_flag, &file, &env).unwrap();
        assert!(matches!(by_flag, AuthConfig::Disabled));

        let by_file = FileConfig {
            allow_unauthenticated: Some(true),
            ..FileConfig::default()
        };
        let by_file = resolve_auth(&auth_required_cli(), &by_file, &env).unwrap();
        assert!(matches!(by_file, AuthConfig::Disabled));

        let by_env = EnvConfig::from_lookup(|name| {
            (name == "SUBMILLI_ALLOW_UNAUTHENTICATED").then(|| "1".to_string())
        });
        let by_env = resolve_auth(&auth_required_cli(), &file, &by_env).unwrap();
        assert!(matches!(by_env, AuthConfig::Disabled));

        let file_says_no = FileConfig {
            allow_unauthenticated: Some(false),
            ..FileConfig::default()
        };
        assert!(resolve_auth(&auth_required_cli(), &file_says_no, &env).is_err());
    }

    #[test]
    fn an_opt_out_alongside_tokens_is_refused() {
        let tmp = tempfile::tempdir().unwrap();
        let file = FileConfig {
            api_tokens: vec![token_from_file(
                "ops",
                Role::Admin,
                write_token_file(tmp.path(), "admin", ADMIN_TOKEN),
            )],
            ..FileConfig::default()
        };
        let opted_out = Cli {
            allow_unauthenticated: true,
            ..auth_required_cli()
        };
        let err = resolve_auth(&opted_out, &file, &EnvConfig::default())
            .expect_err("tokens plus an opt-out should be refused")
            .to_string();
        assert!(err.contains("remove one"), "got: {err}");
    }

    #[test]
    fn tokens_are_read_from_files_with_trailing_whitespace_trimmed() {
        let tmp = tempfile::tempdir().unwrap();
        let auth = auth_of(vec![
            token_from_file(
                "ops",
                Role::Admin,
                write_token_file(tmp.path(), "admin", &format!("{ADMIN_TOKEN}\n")),
            ),
            token_from_file(
                "app",
                Role::User,
                write_token_file(tmp.path(), "user", USER_TOKEN),
            ),
        ])
        .unwrap();
        let AuthConfig::Tokens(tokens) = auth else {
            panic!("expected tokens");
        };
        assert_eq!(tokens.len(), 2);
        assert_eq!((tokens[0].name(), tokens[0].role()), ("ops", Role::Admin));
        assert_eq!((tokens[1].name(), tokens[1].role()), ("app", Role::User));
        let expected = ApiToken::new("x", Role::Admin, ADMIN_TOKEN).unwrap();
        assert!(tokens[0].same_token(&expected));
    }

    fn env_with_server_token(token: &str) -> EnvConfig {
        let token = token.to_string();
        EnvConfig::from_lookup(move |name| (name == SERVER_TOKEN_ENV).then(|| token.clone()))
    }

    #[test]
    fn the_server_token_variable_is_an_admin_token_with_no_config() {
        let file = FileConfig::default();
        let auth = resolve_auth(
            &auth_required_cli(),
            &file,
            &env_with_server_token(&format!(" {ADMIN_TOKEN}\n")),
        )
        .unwrap();
        let AuthConfig::Tokens(tokens) = auth else {
            panic!("expected tokens");
        };
        assert_eq!(tokens.len(), 1);
        assert_eq!(
            (tokens[0].name(), tokens[0].role()),
            ("SUBMILLI_SERVER_TOKEN", Role::Admin)
        );
        assert!(tokens[0].same_token(&ApiToken::new("x", Role::Admin, ADMIN_TOKEN).unwrap()));

        // Blank is unset, so the server is back to having no token at all.
        let blank = resolve_auth(&auth_required_cli(), &file, &env_with_server_token("  "));
        assert!(blank.is_err());
    }

    #[test]
    fn the_server_token_variable_combines_with_the_config_file() {
        let tmp = tempfile::tempdir().unwrap();
        let file = |token: &str| FileConfig {
            api_tokens: vec![token_from_file(
                "app",
                Role::User,
                write_token_file(tmp.path(), "user", token),
            )],
            ..FileConfig::default()
        };
        let env = env_with_server_token(ADMIN_TOKEN);

        let auth = resolve_auth(&auth_required_cli(), &file(USER_TOKEN), &env).unwrap();
        let AuthConfig::Tokens(tokens) = auth else {
            panic!("expected tokens");
        };
        let roles: Vec<_> = tokens.iter().map(|t| (t.name(), t.role())).collect();
        assert_eq!(
            roles,
            [("SUBMILLI_SERVER_TOKEN", Role::Admin), ("app", Role::User)]
        );

        let same = resolve_auth(&auth_required_cli(), &file(ADMIN_TOKEN), &env)
            .expect_err("one token under two names should be refused")
            .to_string();
        assert!(same.contains("same token"), "got: {same}");
    }

    #[test]
    fn a_bad_server_token_variable_is_refused_without_being_printed() {
        let file = FileConfig::default();
        let short = "too-short-to-accept";
        let err = resolve_auth(&auth_required_cli(), &file, &env_with_server_token(short))
            .expect_err("a short token should be refused")
            .to_string();
        assert!(err.contains("SUBMILLI_SERVER_TOKEN"), "got: {err}");
        assert!(err.contains("at least 32"), "got: {err}");
        assert!(!err.contains(short), "got: {err}");

        let not_unicode = EnvConfig {
            server_token_not_unicode: true,
            ..EnvConfig::default()
        };
        let err = resolve_auth(&auth_required_cli(), &file, &not_unicode)
            .expect_err("a non-Unicode token should be refused")
            .to_string();
        assert!(err.contains("valid Unicode"), "got: {err}");

        let opted_out = Cli {
            allow_unauthenticated: true,
            ..auth_required_cli()
        };
        let err = resolve_auth(&opted_out, &file, &env_with_server_token(ADMIN_TOKEN))
            .expect_err("a token plus an opt-out should be refused")
            .to_string();
        assert!(err.contains("remove one"), "got: {err}");
    }

    #[test]
    fn duplicate_token_names_and_values_are_refused() {
        let tmp = tempfile::tempdir().unwrap();
        let admin = write_token_file(tmp.path(), "admin", ADMIN_TOKEN);
        let user = write_token_file(tmp.path(), "user", USER_TOKEN);

        let err = auth_err(vec![
            token_from_file("ops", Role::Admin, admin.clone()),
            token_from_file("ops", Role::User, user),
        ]);
        assert!(err.contains("named `ops`"), "got: {err}");

        let err = auth_err(vec![
            token_from_file("ops", Role::Admin, admin.clone()),
            token_from_file("app", Role::User, admin),
        ]);
        assert!(err.contains("same token"), "got: {err}");
        assert!(err.contains("ops") && err.contains("app"), "got: {err}");
    }

    #[test]
    fn a_bad_token_is_refused_without_being_printed() {
        let tmp = tempfile::tempdir().unwrap();
        let short = "too-short-to-accept";
        let path = write_token_file(tmp.path(), "short", short);
        let err = auth_err(vec![token_from_file("ops", Role::Admin, path.clone())]);
        assert!(err.contains("at least 32"), "got: {err}");
        assert!(err.contains(&path.display().to_string()), "got: {err}");
        assert!(!err.contains(short), "got: {err}");

        let spaced = "a token with spaces 0123456789abcdef0123";
        let path = write_token_file(tmp.path(), "spaced", spaced);
        let err = auth_err(vec![token_from_file("ops", Role::Admin, path)]);
        assert!(err.contains("bearer token"), "got: {err}");
        assert!(!err.contains(spaced), "got: {err}");

        let missing = tmp.path().join("absent");
        let err = auth_of(vec![token_from_file("ops", Role::Admin, missing.clone())])
            .expect_err("a missing token file should be refused");
        assert!(format!("{err:#}").contains(&missing.display().to_string()));

        let empty = write_token_file(tmp.path(), "empty", "\n");
        let err = auth_err(vec![token_from_file("ops", Role::Admin, empty)]);
        assert!(err.contains("is empty"), "got: {err}");

        let blank = token_from_file(" ", Role::User, tmp.path().join("absent"));
        assert!(auth_err(vec![blank]).contains("name"));
    }

    #[test]
    fn merge_carries_the_tokens_and_guards_their_files() {
        let tmp = tempfile::tempdir().unwrap();
        let tokens_dir = tmp.path().join("tokens");
        std::fs::create_dir(&tokens_dir).unwrap();
        let file = |volumes: VolumeTable| FileConfig {
            api_tokens: vec![token_from_file(
                "ops",
                Role::Admin,
                write_token_file(&tokens_dir, "admin", ADMIN_TOKEN),
            )],
            secret_store: SecretStoreFileConfig {
                key_env: Some("SUB_TEST_SECRET_KEY_UNSET".into()),
                ..SecretStoreFileConfig::default()
            },
            volumes,
            ..FileConfig::default()
        };

        let (_, config) = merge(
            auth_required_cli(),
            file(VolumeTable::new()),
            EnvConfig::default(),
        )
        .map_err(|err| err.to_string())
        .expect("merge");
        assert!(matches!(config.auth, AuthConfig::Tokens(ref tokens) if tokens.len() == 1));

        let over_tokens = local_table([("work".to_string(), tokens_dir.clone())]);
        let err = match merge(auth_required_cli(), file(over_tokens), EnvConfig::default()) {
            Ok(_) => panic!("a volume over a token file should be refused"),
            Err(err) => err.to_string(),
        };
        assert!(err.contains("API token file"), "got: {err}");
    }
}
