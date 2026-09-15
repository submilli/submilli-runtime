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
//! server with no inbound authentication, exposed by ambient platform noise.
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
    default_package_store_dir, default_secret_store_dir, default_session_storage_root,
    default_session_store_dir, validate_volumes,
};
use submilli_server::{
    DEFAULT_MAX_STORE_BYTES, FileSecretStore, KeySource, NetworkPolicy, RuntimeConfig, ServerConfig,
};
use submilli_shared::secret_store::SecretStore;

use crate::Cli;

const DEFAULT_BIND: IpAddr = IpAddr::V4(Ipv4Addr::LOCALHOST);
const DEFAULT_PORT: u16 = 8128;
const DEFAULT_SECRET_KEY_ENV: &str = "SUBMILLI_SECRET_KEY";

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
    pub blueprint_dir: Option<PathBuf>,
    pub blueprint_seed_dir: Option<PathBuf>,
    pub session_store_dir: Option<PathBuf>,
    pub vfs_session_dir: Option<PathBuf>,
    pub vfs_ephemeral_dir: Option<PathBuf>,
    pub package_store_dir: Option<PathBuf>,
    /// Seconds. `deny_unknown_fields` means omitting this would turn a
    /// `shutdown_grace:` key into a boot failure, contradicting `--config`'s
    /// promise to supply "values for the options below".
    pub shutdown_grace: Option<u64>,
    /// Megabytes of memory one execution may hold live.
    pub max_execution_memory: Option<u64>,
    /// Megabytes of `submilli:session` state every live session may hold in
    /// total. Bounds the process against session count, where
    /// `max_execution_memory` bounds a single execution.
    pub max_session_state_memory: Option<u64>,
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
    /// Volumes a blueprint's `vfs: { mode: persistent, volume: <name> }` may
    /// name, as `name: /absolute/host/dir`. File-only, like `mcp_oauth`: the
    /// mapping from a name a blueprint can write to a directory on the host is
    /// the whole security boundary, so it stays in one reviewable place rather
    /// than spreading across flags and environment variables.
    #[serde(default)]
    pub volumes: VolumeTable,
    /// Sentry crash-reporting + metrics. Defaults to off; set `true` to opt in.
    /// `SUBMILLI_TELEMETRY` can also opt in. A config-file `false` or a supplied
    /// environment value other than `1`/`true`/`yes`/`on` disables telemetry.
    pub telemetry: Option<bool>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct McpOAuthFileConfig {
    #[serde(default)]
    pub providers: Vec<OAuthProviderFileConfig>,
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
/// operator's `SUBMILLI_BIND=127.0.0.1` would silently publish a server that
/// has no inbound authentication. Ambient `HOST`/`PORT` keep the tolerance
/// they have always had — a platform injects those, and erroring on one would
/// break deployments that never asked for it.
#[derive(Debug, Default)]
pub(crate) struct EnvConfig {
    config: Option<PathBuf>,
    bind: Option<String>,
    port: Option<String>,
    shutdown_grace: Option<String>,
    max_execution_memory: Option<String>,
    max_session_state_memory: Option<String>,
    blueprint_dir: Option<PathBuf>,
    blueprint_seed_dir: Option<PathBuf>,
    session_store_dir: Option<PathBuf>,
    vfs_session_dir: Option<PathBuf>,
    vfs_ephemeral_dir: Option<PathBuf>,
    secret_store_dir: Option<PathBuf>,
    package_store_dir: Option<PathBuf>,
    secret_store_key_env: Option<String>,
    secret_store_key_file: Option<PathBuf>,
    allow_localhost: bool,
    allow_private: bool,
    allow_ip: Vec<String>,
    mcp_allowed_hosts: Vec<String>,
    /// Tier 4. Injected by Render and similar hosts, which route external
    /// traffic in and expect the service on all interfaces — so `PORT` being
    /// set at all implies a `0.0.0.0` bind.
    ambient_bind: Option<IpAddr>,
    ambient_port: Option<u16>,
}

impl EnvConfig {
    fn from_env() -> Self {
        Self::from_lookup(|name| std::env::var(name).ok())
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
            bind: var("SUBMILLI_BIND"),
            port: var("SUBMILLI_PORT"),
            shutdown_grace: var("SUBMILLI_SHUTDOWN_GRACE"),
            max_execution_memory: var("SUBMILLI_MAX_EXECUTION_MEMORY"),
            max_session_state_memory: var("SUBMILLI_MAX_SESSION_STATE_MEMORY"),
            blueprint_dir: path("SUBMILLI_BLUEPRINT_DIR"),
            blueprint_seed_dir: path("SUBMILLI_BLUEPRINT_SEED_DIR"),
            session_store_dir: path("SUBMILLI_SESSION_STORE_DIR"),
            vfs_session_dir: path("SUBMILLI_VFS_SESSION_DIR"),
            vfs_ephemeral_dir: path("SUBMILLI_VFS_EPHEMERAL_DIR"),
            secret_store_dir: path("SUBMILLI_SECRET_STORE_DIR"),
            package_store_dir: path("SUBMILLI_PACKAGE_STORE_DIR"),
            secret_store_key_env: var("SUBMILLI_SECRET_STORE_KEY_ENV"),
            secret_store_key_file: path("SUBMILLI_SECRET_STORE_KEY_FILE"),
            allow_localhost: flag("SUBMILLI_ALLOW_LOCALHOST"),
            allow_private: flag("SUBMILLI_ALLOW_PRIVATE"),
            allow_ip: list("SUBMILLI_ALLOW_IP"),
            mcp_allowed_hosts: list("SUBMILLI_MCP_ALLOWED_HOSTS"),
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
/// the CLI flags on top.
pub(crate) fn resolve(cli: Cli) -> Result<Resolved> {
    let env = EnvConfig::from_env();
    let file = load_config_file(&cli, &env)?;
    let telemetry = combine_telemetry(
        std::env::var("SUBMILLI_TELEMETRY").ok().as_deref(),
        file.telemetry,
    );
    let shutdown_grace = shutdown_grace(&cli, &file, &env)?;
    let (addr, config) = merge(cli, file, env)?;
    Ok(Resolved {
        addr,
        config,
        telemetry,
        shutdown_grace,
    })
}

/// Everything the binary needs from the three configuration sources.
pub(crate) struct Resolved {
    pub addr: SocketAddr,
    pub config: ServerConfig,
    pub telemetry: bool,
    pub shutdown_grace: Duration,
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
    // The path the config was read from, so a volume cannot be declared over the file
    // that declares volumes. Resolution consumes the file and `ServerConfig` never
    // carries the path, so this is the only point it is in scope.
    let config_file = cli.config.clone().or_else(|| env.config.clone());
    let network_policy = resolve_network_policy(&cli, &file, &env)?;
    let secrets = resolve_secret_store(&cli, &file, &env)?;
    let mcp_allowed_hosts = resolve_mcp_allowed_hosts(&cli, &file, &env);
    let runtime = RuntimeConfig {
        max_store_bytes: max_execution_memory(&cli, &file, &env)?,
        ..RuntimeConfig::default()
    };
    let max_session_state_memory = max_session_state_memory(&cli, &file, &env)?;

    let blueprint_dir = explicit(cli.blueprint_dir, env.blueprint_dir, file.blueprint_dir)
        .unwrap_or_else(default_blueprint_dir);
    // No default: seeding is opt-in, and a default path would silently revert
    // API-managed blueprints the moment someone created that directory.
    let blueprint_seed_dir = explicit(
        cli.blueprint_seed_dir,
        env.blueprint_seed_dir,
        file.blueprint_seed_dir,
    );
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

    validate_volumes(
        &file.volumes,
        &ServerDirectories {
            blueprint_dir: Some(blueprint_dir.clone()),
            blueprint_seed_dir: blueprint_seed_dir.clone(),
            package_store_root: Some(
                package_store_root
                    .clone()
                    .unwrap_or_else(default_package_store_dir),
            ),
            secret_store_dir: Some(secrets.dir),
            secret_store_key_file: secrets.key_file,
            session_storage_root: Some(session_storage_root.clone()),
            session_store_dir: Some(session_store_dir.clone()),
            ephemeral_storage_root: ephemeral_storage_root.clone(),
            config_file: config_file.clone(),
        },
    )?;

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
        runtime,
        blueprint_dir: Some(blueprint_dir),
        blueprint_seed_dir,
        session_store_dir: Some(session_store_dir),
        session_storage_root: Some(session_storage_root),
        ephemeral_storage_root,
        package_store_root,
        network_policy,
        secret_store: secrets.store,
        mcp_allowed_hosts,
        mcp_oauth_providers,
        max_session_state_memory,
        volumes: file.volumes,
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
fn resolve_secret_store(cli: &Cli, file: &FileConfig, env: &EnvConfig) -> Result<SecretStoreSetup> {
    let dir = explicit(
        cli.secret_store_dir.clone(),
        env.secret_store_dir.clone(),
        file.secret_store.dir.clone(),
    )
    .unwrap_or_else(default_secret_store_dir);

    let key_file = explicit(
        cli.secret_store_key_file.clone(),
        env.secret_store_key_file.clone(),
        file.secret_store.key_file.clone(),
    );
    let key_env = explicit(
        cli.secret_store_key_env.clone(),
        env.secret_store_key_env.clone(),
        file.secret_store.key_env.clone(),
    )
    .unwrap_or_else(|| DEFAULT_SECRET_KEY_ENV.into());

    let mut setup = SecretStoreSetup {
        store: None,
        dir: dir.clone(),
        key_file: key_file.clone(),
    };
    let key_source = match key_file {
        Some(path) => KeySource::File(path),
        None if std::env::var_os(&key_env).is_some() => KeySource::Env(key_env),
        // No key file and the env var isn't set: leave the store off.
        None => return Ok(setup),
    };

    let store = FileSecretStore::open(dir, &key_source)
        .map_err(|e| anyhow::anyhow!("opening secret store: {e}"))?;
    setup.store = Some(Arc::new(store));
    Ok(setup)
}

/// The secret store plus the paths it was resolved from. `ServerConfig` carries
/// only the opened store, but the volume-overlap check has to know which
/// directory and key file to keep a volume away from.
struct SecretStoreSetup {
    store: Option<Arc<dyn SecretStore>>,
    dir: PathBuf,
    key_file: Option<PathBuf>,
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

    fn empty_cli() -> Cli {
        Cli {
            config: None,
            bind: None,
            port: None,
            blueprint_dir: None,
            blueprint_seed_dir: None,
            session_store_dir: None,
            vfs_session_dir: None,
            vfs_ephemeral_dir: None,
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
            max_session_state_memory: None,
            health_check: false,
        }
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
            blueprint_seed_dir: Some(root.join("seed")),
            package_store_root: Some(root.join("packages")),
            secret_store_dir: Some(root.join("secrets")),
            secret_store_key_file: Some(root.join("keys/secret.b64")),
            session_storage_root: Some(root.join("vfs/sessions")),
            session_store_dir: Some(root.join("sessions")),
            ephemeral_storage_root: Some(root.join("scratch")),
            config_file: Some(root.join("etc/submilli.yaml")),
        }
    }

    /// The directories [`dirs_under`] filled in, each paired with the words its
    /// refusal uses. Destructured exhaustively, so a directory added to
    /// `ServerDirectories` fails to compile here until it is listed — and then
    /// every guard test below covers it without a second list to maintain.
    fn guarded_paths(dirs: &ServerDirectories) -> Vec<(PathBuf, &'static str)> {
        let ServerDirectories {
            blueprint_dir,
            blueprint_seed_dir,
            package_store_root,
            secret_store_dir,
            secret_store_key_file,
            session_storage_root,
            session_store_dir,
            ephemeral_storage_root,
            config_file,
        } = dirs.clone();
        [
            (blueprint_dir, "blueprint store"),
            (blueprint_seed_dir, "blueprint seed directory"),
            (package_store_root, "package store"),
            (secret_store_dir, "secret store"),
            (secret_store_key_file, "secret-store key file"),
            (session_storage_root, "per-session VFS root"),
            (session_store_dir, "durable session store"),
            (ephemeral_storage_root, "ephemeral storage root"),
            (config_file, "server config file"),
        ]
        .into_iter()
        .filter_map(|(path, owned)| Some((path?, owned)))
        .collect()
    }

    fn refusal(volume: &str, target: PathBuf, dirs: &ServerDirectories) -> String {
        validate_volumes(&VolumeTable::from([(volume.to_string(), target)]), dirs)
            .expect_err("volume should be refused")
            .to_string()
    }

    #[test]
    fn volumes_resolve_from_the_config_file() {
        let config = merge_file(FileConfig {
            volumes: VolumeTable::from([
                ("work".to_string(), PathBuf::from("/srv/work")),
                ("data".to_string(), PathBuf::from("/srv/data")),
            ]),
            ..FileConfig::default()
        })
        .expect("merge");
        assert_eq!(config.volumes.len(), 2);
        assert_eq!(config.volumes["work"], PathBuf::from("/srv/work"));
        assert_eq!(config.volumes["data"], PathBuf::from("/srv/data"));
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
            volumes: VolumeTable::from([("work".to_string(), PathBuf::from("relative/dir"))]),
            ..FileConfig::default()
        });
        assert!(err.contains("absolute"), "got: {err}");
        assert!(err.contains("work"), "got: {err}");
    }

    #[test]
    fn an_empty_volume_name_is_refused() {
        let err = merge_err(FileConfig {
            volumes: VolumeTable::from([(String::new(), PathBuf::from("/srv/work"))]),
            ..FileConfig::default()
        });
        assert!(err.contains("empty"), "got: {err}");
    }

    #[test]
    fn a_volume_name_containing_a_newline_is_refused() {
        let err = merge_err(FileConfig {
            volumes: VolumeTable::from([("work\nfake".to_string(), PathBuf::from("/srv/work"))]),
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
            &VolumeTable::from([("work".to_string(), inside.path().to_path_buf())]),
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
            &VolumeTable::from([("outer".to_string(), outer), ("inner".to_string(), alias)]),
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
        let volumes = VolumeTable::from([("work".to_string(), root.path().join("STATE"))]);
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
            volumes: VolumeTable::from([("work".to_string(), root.path().to_path_buf())]),
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
            volumes: VolumeTable::from([("work".to_string(), root.path().join("sessions"))]),
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
            volumes: VolumeTable::from([(
                "work".to_string(),
                root.path().join("sessions/idempotency"),
            )]),
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
            volumes: VolumeTable::from([("work".to_string(), root.path().join("packages/@acme"))]),
            ..FileConfig::default()
        });
        assert!(err.contains("package store"), "got: {err}");
        assert!(err.contains("is inside"), "got: {err}");
    }

    #[test]
    fn two_volumes_that_overlap_are_refused() {
        let err = merge_err(FileConfig {
            volumes: VolumeTable::from([
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
            volumes: VolumeTable::from([
                ("data".to_string(), PathBuf::from("/srv/data")),
                ("alias".to_string(), PathBuf::from("/srv/data")),
            ]),
            ..FileConfig::default()
        });
        assert!(err.contains("alias"), "got: {err}");
        assert!(err.contains("data"), "got: {err}");
        assert!(err.contains("both point at"), "got: {err}");
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
                .store
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
        assert!(store.store.is_some());
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
        assert!(store.store.is_some());
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
        assert!(store.store.is_some());
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
        // implies a `0.0.0.0` bind, and this server has no inbound auth — so a
        // config file that deliberately says loopback must not be flipped open
        // by a platform that merely happens to inject `$PORT`.
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
    fn blueprint_seed_dir_walks_the_ladder() {
        let file = FileConfig {
            blueprint_seed_dir: Some("/file/seed".into()),
            ..FileConfig::default()
        };
        let (_, config) = merge(empty_cli(), file, EnvConfig::default()).unwrap();
        assert_eq!(
            config.blueprint_seed_dir.unwrap(),
            PathBuf::from("/file/seed")
        );

        let file = FileConfig {
            blueprint_seed_dir: Some("/file/seed".into()),
            ..FileConfig::default()
        };
        let env = env_from(&[("SUBMILLI_BLUEPRINT_SEED_DIR", "/env/seed")]);
        let (_, config) = merge(empty_cli(), file, env).unwrap();
        assert_eq!(
            config.blueprint_seed_dir.unwrap(),
            PathBuf::from("/env/seed")
        );

        let cli = Cli {
            blueprint_seed_dir: Some("/cli/seed".into()),
            ..empty_cli()
        };
        let env = env_from(&[("SUBMILLI_BLUEPRINT_SEED_DIR", "/env/seed")]);
        let (_, config) = merge(cli, FileConfig::default(), env).unwrap();
        assert_eq!(
            config.blueprint_seed_dir.unwrap(),
            PathBuf::from("/cli/seed")
        );
    }

    #[test]
    fn blueprint_seed_dir_has_no_default() {
        let (_, config) = merge(empty_cli(), FileConfig::default(), EnvConfig::default()).unwrap();
        assert!(config.blueprint_seed_dir.is_none());
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
        // operator's loopback bind would publish a server with no inbound auth.
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
            .store
            .expect("store enabled");
        assert!(env_dir.exists(), "env dir should have won over the file's");

        // CLI over env.
        let cli = Cli {
            secret_store_dir: Some(cli_dir.clone()),
            ..empty_cli()
        };
        resolve_secret_store(&cli, &file, &env)
            .unwrap()
            .store
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
            store.store.is_some(),
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
}
