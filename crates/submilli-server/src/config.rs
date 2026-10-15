use std::collections::BTreeMap;
use std::fmt;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use interpreter::runtime::{
    EmbeddingLimits, LlmLimits, NetworkPolicy, RuntimeConfig, SessionKvLimits,
};

use submilli_shared::embedding::EmbeddingDispatch;
use submilli_shared::llm::ModelDispatch;
use submilli_shared::secret_store::SecretStore;

use crate::auth::AuthConfig;
use crate::blueprint::BlueprintStore;
use crate::session::SessionStore;

#[derive(Clone, Default)]
pub struct ServerConfig {
    pub audit: crate::audit::AuditConfig,
    pub audit_log: Option<crate::audit::AuditLog>,
    pub runtime: RuntimeConfig,
    /// TLS configuration for the listener. None keeps plain HTTP.
    pub tls: Option<Arc<rustls::ServerConfig>>,
    /// Who may call the HTTP API. Defaults to [`AuthConfig::Disabled`]; the
    /// `submilli-server` binary requires tokens unless the operator opts out.
    pub auth: AuthConfig,
    /// Outbound HTTP egress policy (SSRF guard). Defaults to allow-all; the
    /// `submilli-server` binary installs `deny_private` via its CLI flags.
    pub network_policy: NetworkPolicy,
    /// When `None`, `AppState::new` installs an in-memory store.
    pub sessions: Option<Arc<dyn SessionStore>>,
    /// Optional startup blueprint source. Stores on the shared database are reused;
    /// other sources are copied into SQLite before requests are admitted.
    pub blueprints: Option<Arc<dyn BlueprintStore>>,
    /// Legacy blueprint directory to import into a persistent database.
    pub blueprint_dir: Option<PathBuf>,
    /// Legacy session directory to import into a persistent database.
    /// Idempotency files are not imported.
    pub session_store_dir: Option<PathBuf>,
    /// Persistent SQLite path. With neither path nor database supplied, startup
    /// creates an in-memory SQLite database; directory imports require persistence.
    pub database_path: Option<PathBuf>,
    /// Shared database owner; takes precedence over database_path.
    pub database: Option<Arc<crate::database::ServerDatabase>>,
    /// A secret store an embedder supplies in place of the database, backing
    /// the blueprint `store:` secret source, the `submilli server secret` CLI
    /// and MCP OAuth grants. `None` (the default) keeps secrets and grants in
    /// the database, sealed with `session_cipher`; without a cipher they are off:
    /// `store:` secrets fail to resolve, while `env:`/`file:` are unaffected.
    pub secret_store: Option<Arc<dyn SecretStore>>,
    /// The directory of a secret store from before secrets were kept in the
    /// database. Boot imports it, sealed with `session_cipher`'s key, and moves
    /// it to `<parent>/archive/secrets`.
    pub secret_store_dir: Option<PathBuf>,
    pub session_cipher: Option<Arc<submilli_shared::secret_store::SecretCipher>>,
    /// Root of every VFS directory the server allocates: `managed-local`
    /// volumes, `per_session` workspaces and `ephemeral` scratch (see
    /// [`VolumeRoot`]). Mount it on **durable** storage (a PersistentVolume): a
    /// named volume outlives sessions, and a client that reconnects with the
    /// same session id after a restart finds its files intact. `None` uses
    /// [`default_volume_root`], created at startup. A path given here must
    /// already exist, so a missing mount fails startup instead of being
    /// replaced by an empty local directory.
    pub volume_root: Option<PathBuf>,
    /// Root of the package store the server owns: installs and uninstalls go
    /// here, and reads check it first. Defaults to [`default_package_store_dir`].
    pub package_store_root: Option<PathBuf>,
    /// A read-only package root searched after [`Self::package_store_root`].
    /// The binary points it at the CLI's store so a locally published package
    /// resolves without a second install; `None` means no fallback.
    pub package_fallback_root: Option<PathBuf>,
    /// `Host` headers the MCP streamable-HTTP endpoint accepts (rmcp's
    /// DNS-rebinding guard). `None` keeps rmcp's loopback-only default; `Some`
    /// replaces it wholesale, so the binary boundary pre-composes the loopback
    /// defaults with any operator-supplied hosts.
    pub mcp_allowed_hosts: Option<Vec<String>>,
    /// OAuth client apps for MCP servers, keyed by authorization-server host. The
    /// server uses these to build authorize URLs and exchange/refresh tokens, so
    /// the `client_secret` lives only here — never on the CLI or in a blueprint.
    pub mcp_oauth_providers: Vec<OAuthProvider>,
    /// Per-session bounds on `submilli:session` storage (value size, entry
    /// count, key length, retained bytes per session). Defaults to
    /// [`SessionKvLimits::default`].
    pub session_kv_limits: SessionKvLimits,
    /// Server-wide ceiling on retained `submilli:session` bytes summed across
    /// every live session, reserved atomically so two sessions cannot both
    /// claim the same headroom. `None` uses
    /// [`crate::session_manager::DEFAULT_TOTAL_SESSION_KV_BYTES`]. Unlike the
    /// per-session limits, this
    /// bounds the *process*: it is the only aggregate memory knob the server
    /// has, `max_store_bytes` being per-execute.
    pub max_session_state_memory: Option<u64>,
    /// Per-execution bounds on `submilli:llm` (token ceiling, indeterminate-spend
    /// ceiling, default output cap, prompt count and size). Embedder-only, like
    /// [`Self::session_kv_limits`]: the aggregate below is the knob an operator
    /// reasons about. Defaults to [`LlmLimits::default`].
    pub llm_limits: LlmLimits,
    /// Server-wide ceiling on `submilli:llm` tokens summed across every live
    /// execution, reserved atomically so two executions cannot both claim the
    /// same headroom. `None` uses
    /// [`crate::session_manager::DEFAULT_MAX_ALL_EXECUTIONS_TOKENS`]. Where
    /// `llm_limits.per_execution_tokens` bounds one run, this bounds the spend
    /// against the operator's provider credential across the whole process.
    pub max_llm_tokens: Option<u64>,
    /// Elements one `batch` dispatches at once. `None` uses
    /// [`crate::session_manager::DEFAULT_MAX_CONCURRENCY`]. Bounded because
    /// unbounded fan-out manufactures the 429s it then cannot back off from.
    pub max_llm_concurrency: Option<usize>,
    /// Runs that may wait on the harness at once, server-wide. `None` uses
    /// [`crate::runs::DEFAULT_MAX_SUSPENDED_RUNS`].
    pub max_suspended_runs: Option<usize>,
    /// How long a keyed execute's response is kept for replay after it
    /// completes. `None` uses [`ResponseRetention::default`]'s.
    ///
    /// [`ResponseRetention::default`]: crate::domain::idempotent_request::ResponseRetention
    pub idempotency_retention: Option<std::time::Duration>,
    /// How many of a session's newest keyed-execute responses are kept for
    /// replay whatever their age. `None` uses [`ResponseRetention::default`]'s.
    ///
    /// [`ResponseRetention::default`]: crate::domain::idempotent_request::ResponseRetention
    pub idempotency_keep_last: Option<usize>,
    /// An override for the outbound model dispatch every `submilli:llm` call
    /// rides.
    ///
    /// `None` — the default — means the **real** HTTP dispatch, built per
    /// execute against the executing blueprint and this server's secret store.
    /// A value here replaces it for every blueprint, which is what the tests use
    /// to drive `llm.call` without a socket; an embedder can also use it to
    /// supply its own SDK.
    ///
    /// It has no CLI flag, env var, or config-file key: the choice of dispatch
    /// is a compile-time dependency, not an operator setting, and the
    /// credentials it uses already resolve from the blueprint's `llm:` block.
    /// A deployment that configures no model still gets a catchable error (R12),
    /// raised from the blueprint as the undeclared-model refusal.
    pub llm_dispatch: Option<Arc<dyn ModelDispatch>>,
    /// Per-execution bounds on `submilli:embedding` (tokens, held tokens,
    /// outbound requests, texts and bytes per call). Embedder-only except for
    /// the token and request ceilings, which the operator sets by flag.
    /// Defaults to [`EmbeddingLimits::default`].
    pub embedding_limits: EmbeddingLimits,
    /// Server-wide ceiling on `submilli:embedding` tokens summed across every
    /// live execution. `None` uses
    /// [`crate::session_manager::DEFAULT_MAX_ALL_EXECUTIONS_EMBEDDING_TOKENS`].
    /// Kept separate from the `submilli:llm` ceiling so neither can starve the
    /// other.
    pub max_embedding_tokens: Option<u64>,
    /// Sub-batches one embedding call sends at once. `None` uses
    /// [`crate::session_manager::DEFAULT_MAX_EMBEDDING_CONCURRENCY`].
    pub max_embedding_concurrency: Option<usize>,
    /// An override for the outbound embedding dispatch, mirroring
    /// [`Self::llm_dispatch`]: `None` builds the real HTTP dispatch per
    /// execute. Not an operator setting.
    pub embedding_dispatch: Option<Arc<dyn EmbeddingDispatch>>,
    /// Operator-declared named volumes a blueprint's `vfs` root or `mounts`
    /// resolve through, by name. Config-file only: no CLI flag and no
    /// environment variable, so the declarations live in one reviewable place.
    /// [`validate_volumes`] refuses a declaration that overlaps a server-owned
    /// directory; it runs on the config-file path, not here.
    pub volumes: VolumeTable,
    /// The file holding the GitHub token package installs send. Kept as a
    /// path and read on every install, so replacing the file rotates the
    /// token without a restart.
    pub github_token_file: Option<PathBuf>,
    /// Records every program the server runs, for an embedder such as the playground.
    /// Code-only, like `llm_dispatch`: no flag, env var, or config-file key. `None` (the
    /// default) records nothing and changes nothing.
    pub run_recorder: Option<Arc<dyn crate::record::RunRecorderFactory>>,
    /// Whether runs feed the process's Sentry client: a failed run's report and the
    /// runtime metrics sink. On by default, so the server binary keeps its
    /// opt-in telemetry; an embedder whose runs must stay on the machine, such as
    /// the playground, turns it off whatever the process's own telemetry setting.
    /// Code-only: no flag, env var, or config-file key.
    pub run_telemetry: RunTelemetry,
    /// Called before every program is compiled, whoever sent it, so an embedder can
    /// bring the packages the run imports up to date first; the playground rebuilds a
    /// package whose source changed since it was installed. A refusal fails the run
    /// as a package-resolution error. Code-only, like `run_recorder`. `None` (the
    /// default) checks nothing.
    pub pre_execute: Option<Arc<dyn PreExecuteHook>>,
}

/// What a [`PreExecuteHook`] is told about the run about to be compiled.
pub struct PreExecute<'a> {
    pub state: &'a crate::AppState,
    pub blueprint_name: &'a str,
    pub blueprint: &'a Arc<submilli_blueprint::Blueprint>,
    /// The registry packages the program imports that the blueprint lists: what the
    /// run will load from the package store, before their dependencies.
    pub packages: &'a std::collections::BTreeSet<String>,
}

/// Why a [`PreExecuteHook`] stopped a run: reported to the caller, and recorded, as a
/// package-resolution error with this message, which should name the package and how
/// to fix it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PreExecuteRefusal {
    pub message: String,
}

impl fmt::Display for PreExecuteRefusal {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.message)
    }
}

impl std::error::Error for PreExecuteRefusal {}

/// See [`ServerConfig::pre_execute`]. Called from the async worker running the
/// request: work that blocks belongs on a blocking thread.
#[async_trait::async_trait]
pub trait PreExecuteHook: Send + Sync {
    async fn before_execute(&self, run: PreExecute<'_>) -> Result<(), PreExecuteRefusal>;
}

/// See [`ServerConfig::run_telemetry`].
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum RunTelemetry {
    /// Report failed runs and runtime metrics to Sentry when a client is bound.
    #[default]
    Report,
    /// Never hand run data to Sentry, even when a client is bound.
    Off,
}

pub use submilli_shared::OAuthProvider;

/// Base directory for Submilli's on-disk state when no explicit path is given.
/// Resolves to `$SUBMILLI_HOME` if set, else `$HOME/.submilli`, else a temp-dir
/// fallback so the server still boots in a bare environment. A single `~/.submilli`
/// dotdir (à la `~/.cargo`, `~/.aws`, `~/.docker`) so the out-of-the-box defaults
/// need no root and land somewhere a developer expects; production deployments set
/// explicit paths (CLI flags / config file) pointing at their mounted volumes.
pub fn default_data_root() -> PathBuf {
    submilli_build::default_data_root()
}

/// The subtree of the data root that belongs to the server: `<root>/server`.
/// Every state directory the server defaults lands under it, so the CLI's own
/// `packages/`, `secrets/`, and `mcp_oauth.yaml` siblings are never written by
/// a running server (`packages/` is read, as the fallback package store) and
/// the two never share a directory (the CLI's plaintext secret store and the
/// server's sealed one share a file-name scheme, so a shared directory would
/// let each overwrite the other's entries).
pub fn default_server_root() -> PathBuf {
    default_data_root().join("server")
}

/// Default directory the file-backed blueprint store persists to.
pub fn default_blueprint_dir() -> PathBuf {
    default_server_root().join("blueprints")
}

/// Default root of the server's VFS directories: `<server root>/volumes`,
/// created at startup.
pub fn default_volume_root() -> PathBuf {
    default_server_root().join("volumes")
}

/// Where the server's VFS directories live:
///
/// ```text
/// <root>/managed-local/<volume name>
/// <root>/sessions/<session id>
/// <root>/ephemeral/submilli-vfs-*   one per execute, removed when it ends
/// ```
///
/// Ephemeral scratch sits on the same storage as the rest so that, once runs
/// can pause and resume elsewhere, their files are where another worker can
/// reach them.
///
/// [`Self::prepare`] runs once at startup and is the only place the root and
/// its three directories are created. After it nothing recreates them, so a
/// mount that disappears fails the work that needs it rather than filling an
/// empty local directory at the mountpoint. A root the operator configured is
/// a mount the server must not stand in for, so it must already exist.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VolumeRoot {
    path: PathBuf,
    /// Whether the operator named this root, rather than it being the default.
    configured: bool,
}

impl VolumeRoot {
    /// The root [`ServerConfig::volume_root`] names, else the default.
    pub fn from_config(configured: Option<PathBuf>) -> Self {
        match configured {
            Some(path) => Self::configured(path),
            None => Self::default_root(default_volume_root()),
        }
    }

    /// A root the operator configured, which must already exist.
    pub fn configured(path: impl Into<PathBuf>) -> Self {
        Self {
            path: path.into(),
            configured: true,
        }
    }

    /// A root the server creates at startup.
    pub fn default_root(path: impl Into<PathBuf>) -> Self {
        Self {
            path: path.into(),
            configured: false,
        }
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Where `managed-local` volumes live, one directory per volume name.
    pub fn managed_dir(&self) -> PathBuf {
        self.path.join("managed-local")
    }

    /// Where `per_session` workspaces live, one directory per session id.
    pub fn sessions_dir(&self) -> PathBuf {
        self.path.join("sessions")
    }

    /// Where `ephemeral` scratch lives, one directory per execute.
    pub fn ephemeral_dir(&self) -> PathBuf {
        self.path.join("ephemeral")
    }

    /// Make the root ready at startup. A configured root must be an existing
    /// directory; the default one is created. Then `managed-local/`,
    /// `sessions/` and `ephemeral/` are created beneath it when missing.
    pub fn prepare(&self) -> Result<(), VolumeRootError> {
        let unusable = |source: std::io::Error| VolumeRootError::Root {
            path: self.path.clone(),
            configured: self.configured,
            source,
        };
        if !self.configured && !self.path.exists() {
            std::fs::create_dir_all(&self.path).map_err(unusable)?;
        }
        let meta = std::fs::metadata(&self.path).map_err(unusable)?;
        if !meta.is_dir() {
            return Err(unusable(std::io::Error::other("it is not a directory")));
        }
        for dir in [
            self.managed_dir(),
            self.sessions_dir(),
            self.ephemeral_dir(),
        ] {
            create_plain_dir(&dir, false)
                .map_err(|source| VolumeRootError::Subdirectory { path: dir, source })?;
        }
        Ok(())
    }
}

/// Create `dir`, one managed volume's or session's directory directly under
/// `managed-local/` or `sessions/`, owner-only on Unix, unless a plain
/// directory is already there. The parent is never created: after
/// [`VolumeRoot::prepare`] a missing one means a lost mount. A link at `dir` is
/// refused: the directory is opened with the server's full authority, so a link
/// swapped in would aim it anywhere.
pub(crate) fn create_owned_dir(dir: &Path) -> std::io::Result<()> {
    create_plain_dir(dir, true)
}

/// Create `dir` (owner-only on Unix when `private`) unless a plain directory is
/// already there. The parent must exist.
fn create_plain_dir(dir: &Path, private: bool) -> std::io::Result<()> {
    match std::fs::symlink_metadata(dir) {
        Ok(meta) => require_plain_dir(&meta),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => {
            let mut builder = std::fs::DirBuilder::new();
            #[cfg(unix)]
            if private {
                std::os::unix::fs::DirBuilderExt::mode(&mut builder, 0o700);
            }
            #[cfg(not(unix))]
            let _ = private;
            match builder.create(dir) {
                Ok(()) => Ok(()),
                // Another first user created it in between; check what it made.
                Err(err) if err.kind() == std::io::ErrorKind::AlreadyExists => {
                    require_plain_dir(&std::fs::symlink_metadata(dir)?)
                }
                Err(err) => Err(err),
            }
        }
        Err(err) => Err(err),
    }
}

fn require_plain_dir(meta: &std::fs::Metadata) -> std::io::Result<()> {
    if meta.is_dir() {
        return Ok(());
    }
    Err(std::io::Error::other(
        "the path exists and is not a plain directory",
    ))
}

/// Why the volume root could not be made ready at startup.
#[derive(Debug)]
pub enum VolumeRootError {
    /// The root itself is missing, not a directory, or could not be created.
    Root {
        path: PathBuf,
        /// Whether the operator configured it, so it must exist already.
        configured: bool,
        source: std::io::Error,
    },
    /// One of the three directories under it could not be created or is not a
    /// plain directory.
    Subdirectory {
        path: PathBuf,
        source: std::io::Error,
    },
}

impl fmt::Display for VolumeRootError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            VolumeRootError::Root {
                path,
                configured: true,
                source,
            } => write!(
                f,
                "the volume root {} is not usable ({source}). Mount it or create it, then \
                 restart. The server never creates a configured `volume_dir`, so a missing \
                 mount is not replaced by an empty local directory",
                path.display(),
            ),
            VolumeRootError::Root {
                path,
                configured: false,
                source,
            } => write!(
                f,
                "the default volume root {} is not usable ({source}). Make sure it is a \
                 directory the server can create and write, or set `volume_dir` to an \
                 existing directory",
                path.display(),
            ),
            VolumeRootError::Subdirectory { path, source } => write!(
                f,
                "{} under the volume root is not usable ({source}). It must be a plain \
                 directory the server can write, not a link or a file",
                path.display(),
            ),
        }
    }
}

impl std::error::Error for VolumeRootError {}

/// Default directory the file-backed session store persists lifecycle metadata
/// to — sibling to the VFS directories under the server root.
pub fn default_session_store_dir() -> PathBuf {
    default_server_root().join("sessions")
}

/// Default server-owned SQLite database file, under a separately mountable directory.
pub fn default_database_path() -> PathBuf {
    default_server_root().join("db/submilli.db")
}

/// Default directory backing the encrypted secret store.
pub fn default_secret_store_dir() -> PathBuf {
    default_server_root().join("secrets")
}

/// Default root of the package store the server writes to. Not the CLI's
/// store (`submilli_build::default_package_store_dir`), which the server only
/// reads through [`default_cli_package_store_dir`].
pub fn default_package_store_dir() -> PathBuf {
    default_server_root().join("packages")
}

/// The CLI's package store, `<root>/packages`: where `submilli build
/// publish-local` and `submilli install` put artifacts. The server reads it as
/// a fallback so locally published packages resolve without a second install,
/// and never writes to it.
pub fn default_cli_package_store_dir() -> PathBuf {
    submilli_build::default_package_store_dir()
}

/// An operator-declared volume table: name → declaration. A blueprint's
/// `vfs: { mode: named, volume: <name> }` and every entry under `vfs.mounts`
/// resolve through this table, so a blueprint never names a host directory of
/// its own.
pub type VolumeTable = BTreeMap<String, VolumeSpec>;

pub use submilli_blueprint::Access;

/// One named volume the server declares.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VolumeSpec {
    pub kind: VolumeKind,
    /// The most a blueprint may do with the volume; a blueprint's own `access`
    /// can only narrow it.
    pub access: Access,
    /// One limit shared by every session and blueprint that uses the volume.
    pub size_limit: SizeLimit,
}

impl VolumeSpec {
    /// An operator-owned directory, read-write, with no size limit.
    pub fn local_path(path: impl Into<PathBuf>) -> Self {
        Self {
            kind: VolumeKind::LocalPath { path: path.into() },
            access: Access::ReadWrite,
            size_limit: SizeLimit::Unlimited,
        }
    }

    /// A directory Submilli allocates under the volume root's `managed-local/`.
    pub fn managed(size_limit: SizeLimit) -> Self {
        Self {
            kind: VolumeKind::ManagedLocal,
            access: Access::ReadWrite,
            size_limit,
        }
    }

    pub fn with_access(mut self, access: Access) -> Self {
        self.access = access;
        self
    }
}

/// Reads the config file's `volumes:` table. Hand-written so each refusal names
/// the volume and the edit that fixes it, including the retired
/// `name: /host/dir` form, which needs a `kind` and a `size_limit` now.
pub fn deserialize_volume_table<'de, D>(deserializer: D) -> Result<VolumeTable, D::Error>
where
    D: serde::Deserializer<'de>,
{
    deserializer.deserialize_map(VolumeTableVisitor)
}

struct VolumeTableVisitor;

impl<'de> serde::de::Visitor<'de> for VolumeTableVisitor {
    type Value = VolumeTable;

    fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("a map from volume name to `{kind, path?, access?, size_limit}`")
    }

    fn visit_unit<E: serde::de::Error>(self) -> Result<VolumeTable, E> {
        Ok(VolumeTable::new())
    }

    fn visit_map<A: serde::de::MapAccess<'de>>(self, mut map: A) -> Result<VolumeTable, A::Error> {
        let mut table = VolumeTable::new();
        while let Some(name) = map.next_key::<String>()? {
            if table.contains_key(&name) {
                return Err(serde::de::Error::custom(format!(
                    "volume '{name}' is declared twice under `volumes:`; delete one"
                )));
            }
            let spec = map.next_value_seed(VolumeSpecSeed { name: &name })?;
            table.insert(name, spec);
        }
        Ok(table)
    }
}

struct VolumeSpecSeed<'a> {
    name: &'a str,
}

impl<'de> serde::de::DeserializeSeed<'de> for VolumeSpecSeed<'_> {
    type Value = VolumeSpec;

    fn deserialize<D: serde::Deserializer<'de>>(
        self,
        deserializer: D,
    ) -> Result<VolumeSpec, D::Error> {
        deserializer.deserialize_any(self)
    }
}

/// A size in bytes, or a size string such as `10GB`, or `unlimited`.
#[derive(serde::Deserialize)]
#[serde(untagged)]
enum SizeLimitRepr {
    Bytes(u64),
    Text(String),
}

impl<'de> serde::de::Visitor<'de> for VolumeSpecSeed<'_> {
    type Value = VolumeSpec;

    fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "a declaration for volume '{}', such as `{{kind: managed-local, size_limit: 1GB}}`",
            self.name
        )
    }

    fn visit_str<E: serde::de::Error>(self, path: &str) -> Result<VolumeSpec, E> {
        let name = self.name;
        Err(E::custom(format!(
            "volume '{name}' uses the retired `{name}: {path}` form; write `{name}: {{kind: \
             local-path, path: {path}, size_limit: unlimited}}` to keep using that directory, or \
             `{name}: {{kind: managed-local, size_limit: <size>}}` to let Submilli store the \
             volume under `volume_dir`"
        )))
    }

    fn visit_map<A: serde::de::MapAccess<'de>>(self, mut map: A) -> Result<VolumeSpec, A::Error> {
        use serde::de::Error;
        let name = self.name;
        let mut kind: Option<String> = None;
        let mut path: Option<PathBuf> = None;
        let mut access: Option<Access> = None;
        let mut size_limit: Option<SizeLimitRepr> = None;
        let mut seen = std::collections::BTreeSet::new();
        while let Some(key) = map.next_key::<String>()? {
            if !seen.insert(key.clone()) {
                return Err(A::Error::custom(format!(
                    "volume '{name}' sets `{key}` twice; delete one"
                )));
            }
            match key.as_str() {
                "kind" => kind = Some(map.next_value()?),
                "path" => path = Some(map.next_value()?),
                "access" => access = Some(map.next_value()?),
                "size_limit" => size_limit = Some(map.next_value()?),
                other => {
                    return Err(A::Error::custom(format!(
                        "unknown field `{other}` in volume '{name}', expected `kind`, `path`, \
                         `access` or `size_limit`"
                    )));
                }
            }
        }
        let kind = match (kind.as_deref(), path) {
            (None, _) => {
                return Err(A::Error::custom(format!(
                    "volume '{name}' needs a `kind`: `managed-local` (stored by Submilli under \
                     `volume_dir`) or `local-path` (a directory you name with `path`)"
                )));
            }
            (Some("managed-local"), None) => VolumeKind::ManagedLocal,
            (Some("managed-local"), Some(_)) => {
                return Err(A::Error::custom(format!(
                    "`path` is only valid for `kind: local-path`; managed-local volume '{name}' \
                     is stored under `volume_dir`/{name}"
                )));
            }
            (Some("local-path"), Some(path)) => VolumeKind::LocalPath { path },
            (Some("local-path"), None) => {
                return Err(A::Error::custom(format!(
                    "local-path volume '{name}' needs a `path`: the absolute host directory it \
                     exposes"
                )));
            }
            (Some(other), _) => {
                return Err(A::Error::custom(format!(
                    "volume '{name}' has unknown kind `{other}`; use `managed-local` or \
                     `local-path`"
                )));
            }
        };
        let size_limit = match size_limit {
            None => {
                return Err(A::Error::custom(format!(
                    "volume '{name}' needs an explicit `size_limit`: a size such as `10GB`, or \
                     `unlimited`. The limit is shared by every session and blueprint using it"
                )));
            }
            Some(SizeLimitRepr::Bytes(bytes)) => SizeLimit::Bytes(bytes),
            Some(SizeLimitRepr::Text(text)) if text == "unlimited" => SizeLimit::Unlimited,
            Some(SizeLimitRepr::Text(text)) => submilli_blueprint::parse_size(&text)
                .map(SizeLimit::Bytes)
                .map_err(|err| A::Error::custom(format!("volume '{name}' `size_limit`: {err}")))?,
        };
        Ok(VolumeSpec {
            kind,
            access: access.unwrap_or(Access::ReadWrite),
            size_limit,
        })
    }
}

/// Where a named volume's files live.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum VolumeKind {
    /// `<volume root>/managed-local/<name>`, created by the server on first use
    /// and never deleted by it.
    ManagedLocal,
    /// A directory the operator owns. The server neither creates nor deletes it.
    LocalPath { path: PathBuf },
}

/// A volume's size limit. Declared explicitly, so no volume is unbounded by
/// accident.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SizeLimit {
    Bytes(u64),
    Unlimited,
}

/// The directories the server reads or writes that [`validate_volumes`]
/// guards. Callers pass
/// *effective* values — after defaults are applied — because a volume that
/// overlaps the directory the server actually uses is the hazard, not one that
/// overlaps the value the operator happened to type.
///
/// The secret-store directory and key file are deliberately here rather than
/// read off [`ServerConfig`]: that struct carries the opened store, not the
/// paths it was opened from, so a caller that skipped these fields would let a
/// volume swallow the decryption key unnoticed. The same holds for the API
/// token files.
#[derive(Clone, Debug, Default)]
pub struct ServerDirectories {
    pub blueprint_dir: Option<PathBuf>,
    pub package_store_root: Option<PathBuf>,
    /// The read-only package root the server falls back to; executable
    /// artifacts are loaded from it just like from the owned store.
    pub package_fallback_root: Option<PathBuf>,
    pub secret_store_dir: Option<PathBuf>,
    pub secret_store_key_file: Option<PathBuf>,
    /// The files `api_tokens` entries read their tokens from. Like the key
    /// file, they never reach [`ServerConfig`], which holds only digests.
    pub api_token_files: Vec<PathBuf>,
    /// The file the GitHub token for package installs is read from.
    pub github_token_file: Option<PathBuf>,
    /// The TLS private key must never be reachable through a named volume.
    pub tls_key_file: Option<PathBuf>,
    pub tls_cert_file: Option<PathBuf>,
    /// The durable session store — lifecycle records plus the idempotency
    /// ledger in a subdirectory of it. A different directory from the
    /// `per_session` workspaces under [`Self::volume_root`].
    pub session_store_dir: Option<PathBuf>,
    pub database_path: Option<PathBuf>,
    /// The root of the server's VFS directories ([`VolumeRoot`]): `managed-local`
    /// volumes, `per_session` workspaces and `ephemeral` scratch. `None` means
    /// [`default_volume_root`].
    pub volume_root: Option<PathBuf>,
    /// The config file this server was started from, when it was started from one.
    /// It is not reachable from [`ServerConfig`] — the file is consumed during
    /// resolution and nothing keeps the path — so only the config-file channel can
    /// supply it.
    pub config_file: Option<PathBuf>,
}

impl ServerDirectories {
    /// Every guarded directory [`ServerConfig`] can supply, with the defaults
    /// applied. The secret-store fields are left empty — fill them in from
    /// wherever the store was opened before validating.
    pub fn from_config(config: &ServerConfig) -> Self {
        Self {
            blueprint_dir: Some(
                config
                    .blueprint_dir
                    .clone()
                    .unwrap_or_else(default_blueprint_dir),
            ),
            package_store_root: Some(
                config
                    .package_store_root
                    .clone()
                    .unwrap_or_else(default_package_store_dir),
            ),
            package_fallback_root: config.package_fallback_root.clone(),
            secret_store_dir: config.secret_store_dir.clone(),
            secret_store_key_file: None,
            api_token_files: Vec::new(),
            github_token_file: config.github_token_file.clone(),
            tls_key_file: None,
            tls_cert_file: None,
            // Neither the secret-store key's path, the token files', nor the
            // config file's survive into `ServerConfig`; an embedder that wants
            // them guarded fills them in.
            config_file: None,
            session_store_dir: Some(
                config
                    .session_store_dir
                    .clone()
                    .unwrap_or_else(default_session_store_dir),
            ),
            database_path: config
                .database
                .as_ref()
                .map(|database| database.path().to_path_buf())
                .or_else(|| config.database_path.clone()),
            volume_root: Some(
                config
                    .volume_root
                    .clone()
                    .unwrap_or_else(default_volume_root),
            ),
        }
    }
}

/// Why a volume declaration was refused. Every variant names the volume, so an
/// operator reading the boot failure knows which line of the config to edit.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum VolumeError {
    /// A blank map key. An unnamed volume can never be referenced.
    EmptyName,
    /// A control character (a newline, most plausibly) in a name. Names are
    /// echoed into log lines and client-facing errors.
    ControlCharInName { name: String },
    /// A relative target. Every other path option is resolved against the
    /// process working directory, so a container `WORKDIR` change would
    /// silently repoint the volume at a different host directory.
    RelativeTarget { name: String, target: PathBuf },
    /// The volume overlaps a directory the server owns. See [`Overlap`].
    Overlap(Overlap),
    /// One declared volume resolves inside another. Mounting opens the target
    /// with ambient authority and follows symlinks, so a guest holding the
    /// outer volume can replace the inner volume's root with a link and
    /// redirect it anywhere on the host.
    NestedVolumes {
        outer: String,
        outer_target: PathBuf,
        inner: String,
        inner_target: PathBuf,
    },
    /// Two volume names resolving to one directory: each blueprint would
    /// silently share the other's files.
    SharedTarget {
        first: String,
        second: String,
        target: PathBuf,
    },
    /// A `managed-local` volume name that cannot be used as one directory name
    /// under the volume root's `managed-local/`.
    BadManagedName { name: String },
    /// The volume root overlaps a directory the server owns. Managed volumes and
    /// session workspaces are created by name beneath it, so either direction is
    /// unsafe.
    VolumeRootOverlap(VolumeRootOverlap),
}

/// One volume declaration overlapping one server-owned directory, in the
/// direction that is unsafe for that directory.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Overlap {
    pub name: String,
    /// The declared target, as the operator wrote it.
    pub target: PathBuf,
    /// What the volume collides with, in words an operator recognises.
    pub owned: &'static str,
    /// The server-owned directory, as the server resolved it.
    pub owned_path: PathBuf,
    pub direction: Direction,
    pub reason: &'static str,
}

/// The volume root overlapping one server-owned directory.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VolumeRootOverlap {
    pub root: PathBuf,
    pub owned: &'static str,
    pub owned_path: PathBuf,
    /// Whether the root contains the owned directory or sits inside it.
    pub direction: Direction,
    pub reason: &'static str,
}

/// Which way containment is unsafe. The two directions fail for different
/// reasons, and a directory can be guarded in both: the volume root
/// leaks outward and loses data inward, while the package store and the durable
/// session store have nested layouts, so a volume one level down still lands on
/// server-owned files.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Direction {
    /// The volume is at or above the server-owned directory.
    VolumeContains,
    /// The volume is at or below the server-owned directory.
    VolumeInside,
}

impl Direction {
    /// How a refusal names the containment, with the volume or root as subject.
    fn verb(self) -> &'static str {
        match self {
            Direction::VolumeContains => "contains",
            Direction::VolumeInside => "is inside",
        }
    }
}

impl fmt::Display for VolumeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            VolumeError::EmptyName => f.write_str(
                "a volume name is empty; give every entry under `volumes:` a name a blueprint can \
                 reference",
            ),
            VolumeError::ControlCharInName { name } => write!(
                f,
                "volume name {name:?} contains a control character; use a plain single-line name"
            ),
            VolumeError::RelativeTarget { name, target } => write!(
                f,
                "volume '{name}' points at the relative path {}; give an absolute path — a \
                 relative one moves with the process working directory",
                target.display()
            ),
            VolumeError::Overlap(o) => write!(
                f,
                "volume '{name}' ({target}) {direction} the {owned} ({owned_path}): {reason}. \
                 Point the volume at a directory outside it, or move the {owned} elsewhere",
                name = o.name,
                target = o.target.display(),
                direction = o.direction.verb(),
                owned = o.owned,
                owned_path = o.owned_path.display(),
                reason = o.reason,
            ),
            VolumeError::NestedVolumes {
                outer,
                outer_target,
                inner,
                inner_target,
            } => write!(
                f,
                "volume '{inner}' ({inner_path}) is inside volume '{outer}' ({outer_path}): a \
                 guest holding '{outer}' can replace the root of '{inner}' with a symlink and \
                 redirect it anywhere on the host. Point one of them at a directory outside the \
                 other",
                inner_path = inner_target.display(),
                outer_path = outer_target.display(),
            ),
            VolumeError::SharedTarget {
                first,
                second,
                target,
            } => write!(
                f,
                "volumes '{first}' and '{second}' both point at {target}: give each volume its \
                 own directory, or declare one name and reference it from both blueprints",
                target = target.display(),
            ),
            VolumeError::BadManagedName { name } => write!(
                f,
                "managed-local volume name {name:?} is stored as a directory of that name, so it \
                 must be 1-64 characters of letters, digits, `.`, `_` and `-`, starting with a \
                 letter or digit"
            ),
            VolumeError::VolumeRootOverlap(o) => write!(
                f,
                "the volume root ({root}) {direction} the {owned} ({owned_path}): \
                 {reason}. Set `volume_dir` to a directory outside it",
                root = o.root.display(),
                direction = o.direction.verb(),
                owned = o.owned,
                owned_path = o.owned_path.display(),
                reason = o.reason,
            ),
        }
    }
}

impl std::error::Error for VolumeError {}

/// Refuse a volume table that a guest could use to reach the server's own
/// state. Called automatically on the config-file path; an embedder that builds
/// [`ServerConfig`] programmatically gets no such check and should call this
/// itself before serving.
///
/// Volumes are also refused when they overlap *each other*: nesting one inside
/// another hands the outer volume's holder the inner one's root.
///
/// Comparison runs over both the written and the symlink-resolved target (see
/// [`Shape`]), so an overlap that appears only once a link is followed and one
/// that appears only in the operator's spelling are both refused.
///
/// A `managed-local` volume lives at `<volume root>/managed-local/<name>`: its
/// name must be a plain directory name, and the volume root itself must stay
/// clear of every other server-owned directory.
pub fn validate_volumes(
    volumes: &VolumeTable,
    dirs: &ServerDirectories,
) -> Result<(), VolumeError> {
    let root = VolumeRoot::from_config(dirs.volume_root.clone());
    let guarded = guarded_dirs(&ServerDirectories {
        volume_root: Some(root.path().to_path_buf()),
        ..dirs.clone()
    });
    // Every guarded directory but the volume root itself.
    let others = guarded_dirs(&ServerDirectories {
        volume_root: None,
        ..dirs.clone()
    });
    check_volume_root(root.path(), &others)?;
    let managed_root = root.managed_dir();
    let mut checked: Vec<Volume> = Vec::new();
    for (name, spec) in volumes {
        let volume = match &spec.kind {
            VolumeKind::LocalPath { path } => {
                let volume = prepare_volume(name, path)?;
                check_against_guarded_dirs(&volume, &guarded)?;
                volume
            }
            VolumeKind::ManagedLocal => {
                validate_name(name)?;
                if !is_managed_name(name) {
                    return Err(VolumeError::BadManagedName {
                        name: name.to_string(),
                    });
                }
                let target = managed_root.join(name);
                Volume {
                    name: name.to_string(),
                    shape: Shape::of(&target),
                    target,
                }
            }
        };
        check_against_other_volumes(&volume, &checked)?;
        checked.push(volume);
    }
    Ok(())
}

/// Whether `name` can be a managed volume's directory name on every platform.
pub fn is_managed_name(name: &str) -> bool {
    let mut chars = name.chars();
    chars.next().is_some_and(|c| c.is_ascii_alphanumeric())
        && name.len() <= 64
        && chars.all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-'))
}

/// Refuse a volume root that overlaps a server-owned directory in either
/// direction.
fn check_volume_root(root: &Path, guarded: &[GuardedDir]) -> Result<(), VolumeError> {
    let shape = Shape::of(root);
    for guard in guarded {
        let direction = if guard.shape.beneath(&shape) {
            Direction::VolumeContains
        } else if shape.beneath(&guard.shape) {
            Direction::VolumeInside
        } else {
            continue;
        };
        return Err(VolumeError::VolumeRootOverlap(VolumeRootOverlap {
            root: root.to_path_buf(),
            owned: guard.owned,
            owned_path: guard.path.clone(),
            direction,
            reason: guard.reason,
        }));
    }
    Ok(())
}

/// One declaration, validated far enough to compare: an absolute target with a
/// usable name, plus the shapes that target is compared in.
struct Volume {
    name: String,
    target: PathBuf,
    shape: Shape,
}

/// The two shapes a directory is compared in: the path as it is written, and
/// where that path lands once symlinks are followed.
///
/// Both are load-bearing, and neither subsumes the other. The resolved shape
/// catches a target that only overlaps once a link is followed. The written
/// shape catches the reverse — a target that *is* a link, resolving away from
/// where it sits. `volume inner: /data/outer/alias`, where `alias` is a link
/// pointing somewhere else entirely, shows no resolved overlap with `volume
/// outer: /data/outer`; but the entry named `alias` lives in a directory a
/// guest can write, so that guest can repoint it and the next mount of `inner`
/// opens wherever the guest chose, with ambient authority. Comparing what the
/// operator wrote keeps a guest-reachable pathname out of the table to begin
/// with, which is the only durable fix: a check on the resolved target is a
/// check on a value that can change after it is read.
struct Shape {
    written: PathBuf,
    resolved: PathBuf,
}

impl Shape {
    fn of(path: &Path) -> Self {
        Self {
            written: path.to_path_buf(),
            resolved: resolve_links(path),
        }
    }

    /// Whether this directory is `other` or sits beneath it, in either shape.
    fn beneath(&self, other: &Self) -> bool {
        starts_with(&self.written, &other.written) || starts_with(&self.resolved, &other.resolved)
    }

    /// Whether the two name the same directory, in either shape.
    fn same_as(&self, other: &Self) -> bool {
        same(&self.written, &other.written) || same(&self.resolved, &other.resolved)
    }
}

fn same(a: &Path, b: &Path) -> bool {
    starts_with(a, b) && starts_with(b, a)
}

/// Component-wise prefix test that honours the platform's case sensitivity.
///
/// `Path::starts_with` is always case-sensitive. On the default macOS and
/// Windows filesystems that under-refuses: `resolve_links` folds the case of
/// the part of a target that already exists — `canonicalize` returns the
/// on-disk spelling — but a directory that does not exist yet keeps whatever
/// the operator typed, so `/srv/STATE` and `/srv/state` name the same future
/// directory while an exact comparison calls them unrelated. Folding ASCII case
/// there refuses a pair that would collide once created; it never accepts one
/// an exact comparison would have refused.
fn starts_with(path: &Path, prefix: &Path) -> bool {
    let mut have = path.components();
    prefix.components().all(|want| {
        have.next()
            .is_some_and(|got| same_component(got.as_os_str(), want.as_os_str()))
    })
}

#[cfg(any(target_os = "macos", windows))]
fn same_component(a: &std::ffi::OsStr, b: &std::ffi::OsStr) -> bool {
    a.as_encoded_bytes()
        .eq_ignore_ascii_case(b.as_encoded_bytes())
}

#[cfg(not(any(target_os = "macos", windows)))]
fn same_component(a: &std::ffi::OsStr, b: &std::ffi::OsStr) -> bool {
    a == b
}

fn prepare_volume(name: &str, target: &Path) -> Result<Volume, VolumeError> {
    validate_name(name)?;
    if !target.is_absolute() {
        return Err(VolumeError::RelativeTarget {
            name: name.to_string(),
            target: target.to_path_buf(),
        });
    }
    Ok(Volume {
        name: name.to_string(),
        target: target.to_path_buf(),
        shape: Shape::of(target),
    })
}

fn check_against_guarded_dirs(volume: &Volume, guarded: &[GuardedDir]) -> Result<(), VolumeError> {
    for guard in guarded {
        let overlaps = match guard.direction {
            Direction::VolumeContains => guard.shape.beneath(&volume.shape),
            Direction::VolumeInside => volume.shape.beneath(&guard.shape),
        };
        if overlaps {
            return Err(VolumeError::Overlap(Overlap {
                name: volume.name.clone(),
                target: volume.target.clone(),
                owned: guard.owned,
                owned_path: guard.path.clone(),
                direction: guard.direction,
                reason: guard.reason,
            }));
        }
    }
    Ok(())
}

fn check_against_other_volumes(volume: &Volume, declared: &[Volume]) -> Result<(), VolumeError> {
    for other in declared {
        if volume.shape.same_as(&other.shape) {
            return Err(VolumeError::SharedTarget {
                first: other.name.clone(),
                second: volume.name.clone(),
                target: volume.target.clone(),
            });
        }
        let (outer, inner) = if volume.shape.beneath(&other.shape) {
            (other, volume)
        } else if other.shape.beneath(&volume.shape) {
            (volume, other)
        } else {
            continue;
        };
        return Err(VolumeError::NestedVolumes {
            outer: outer.name.clone(),
            outer_target: outer.target.clone(),
            inner: inner.name.clone(),
            inner_target: inner.target.clone(),
        });
    }
    Ok(())
}

fn validate_name(name: &str) -> Result<(), VolumeError> {
    if name.trim().is_empty() {
        return Err(VolumeError::EmptyName);
    }
    if name.chars().any(char::is_control) {
        return Err(VolumeError::ControlCharInName {
            name: name.to_string(),
        });
    }
    Ok(())
}

struct GuardedDir {
    owned: &'static str,
    path: PathBuf,
    shape: Shape,
    direction: Direction,
    reason: &'static str,
}

/// The refusal table: one row per guarded directory, with the *one*
/// direction that is unsafe for it and why. Ordered so the most damaging
/// collisions are reported first; leave the order alone.
fn guarded_dirs(dirs: &ServerDirectories) -> Vec<GuardedDir> {
    let rows: [(&'static str, Option<PathBuf>, Direction, &'static str); 16] = [
        (
            "secret store",
            dirs.secret_store_dir.clone(),
            Direction::VolumeContains,
            "a guest read would reach the store's decryption key",
        ),
        (
            "secret-store key file",
            dirs.secret_store_key_file.clone(),
            Direction::VolumeContains,
            "a guest read would reach the store's decryption key",
        ),
        (
            "TLS certificate file",
            dirs.tls_cert_file.clone(),
            Direction::VolumeContains,
            "a program could replace the server's TLS certificate",
        ),
        (
            "TLS private key",
            dirs.tls_key_file.clone(),
            Direction::VolumeContains,
            "a program could read or replace the server's TLS identity",
        ),
        (
            "GitHub token file",
            dirs.github_token_file.clone(),
            Direction::VolumeContains,
            "a guest read would reach the token the server fetches private packages with",
        ),
        (
            "blueprint store",
            dirs.blueprint_dir.clone(),
            Direction::VolumeContains,
            "a guest write would reach the blueprint index, letting a program grant itself \
             capabilities",
        ),
        (
            "package store",
            dirs.package_store_root.clone(),
            Direction::VolumeContains,
            "a guest write would reach executable package artifacts, running code as another \
             package",
        ),
        (
            "package store",
            dirs.package_store_root.clone(),
            Direction::VolumeInside,
            "the store nests artifacts under <scope>/<package>, so a volume one level down still \
             reaches executable package artifacts, running code as another package",
        ),
        (
            "fallback package store",
            dirs.package_fallback_root.clone(),
            Direction::VolumeContains,
            "a guest write would reach executable package artifacts the server loads, running \
             code as another package",
        ),
        (
            "fallback package store",
            dirs.package_fallback_root.clone(),
            Direction::VolumeInside,
            "the store nests artifacts under <scope>/<package>, so a volume one level down still \
             reaches executable package artifacts the server loads",
        ),
        (
            "durable session store",
            dirs.session_store_dir.clone(),
            Direction::VolumeContains,
            "a guest read would reach every session's stored variables and the recorded response \
             body of every idempotent execute, and a guest write could repoint a session at \
             another blueprint — running it under that blueprint's permissions",
        ),
        (
            "server database",
            dirs.database_path.clone(),
            Direction::VolumeContains,
            "a guest could read or replace persistent server metadata",
        ),
        (
            "durable session store",
            dirs.session_store_dir.clone(),
            Direction::VolumeInside,
            "the idempotency ledger lives in a subdirectory of that root, so a guest write would \
             forge the recorded outcome of a keyed execute",
        ),
        (
            "volume root",
            dirs.volume_root.clone(),
            Direction::VolumeContains,
            "the volume would expose every session's files, every execute's scratch and every \
             managed volume, whatever access each declares",
        ),
        (
            "volume root",
            dirs.volume_root.clone(),
            Direction::VolumeInside,
            "managed volumes and session workspaces are created by name under that root, and \
             orphan reconciliation deletes unclaimed directories under its sessions/ on every \
             boot, so the volume's contents could be shared or wiped",
        ),
        (
            "server config file",
            dirs.config_file.clone(),
            Direction::VolumeContains,
            "a guest write would reach the server's own configuration — rewriting the `volumes:` \
             table, the network policy, or the secret-store key path, all of which take effect \
             at the next restart",
        ),
    ];

    let token_files = dirs.api_token_files.iter().map(|path| {
        (
            "API token file",
            Some(path.clone()),
            Direction::VolumeContains,
            "a guest read would reach a token that authenticates to this server's API",
        )
    });

    rows.into_iter()
        .chain(token_files)
        .filter_map(|(owned, path, direction, reason)| {
            let path = path?;
            Some(GuardedDir {
                owned,
                shape: Shape::of(&path),
                path,
                direction,
                reason,
            })
        })
        .collect()
}

/// Resolve symlinks as far as the path exists, keeping the components that do
/// not yet. A volume target is allowed not to exist at start; what matters is
/// where it would land once created.
fn resolve_links(path: &Path) -> PathBuf {
    let mut missing: Vec<std::ffi::OsString> = Vec::new();
    let mut head = path.to_path_buf();
    loop {
        if let Ok(real) = std::fs::canonicalize(&head) {
            return missing.iter().rev().fold(real, |acc, seg| acc.join(seg));
        }
        let Some(name) = head.file_name().map(std::ffi::OsStr::to_os_string) else {
            return path.to_path_buf();
        };
        if !head.pop() {
            return path.to_path_buf();
        }
        missing.push(name);
    }
}

#[cfg(test)]
pub(crate) fn test_config() -> ServerConfig {
    ServerConfig {
        volume_root: Some(tempfile::tempdir().unwrap().keep()),
        blueprints: Some(Arc::new(crate::blueprint::InMemoryBlueprintStore::default())),
        ..Default::default()
    }
}

impl ServerConfig {
    pub(crate) async fn resolve_database(
        &self,
    ) -> Result<Arc<crate::database::ServerDatabase>, crate::database::DatabaseError> {
        if let Some(database) = self
            .database
            .clone()
            .or_else(|| self.blueprints.as_ref().and_then(|store| store.database()))
        {
            return Ok(database);
        }
        let database = match self.database_path.as_deref() {
            Some(path) => crate::database::ServerDatabase::open(path).await?,
            None => crate::database::ServerDatabase::open_ephemeral().await?,
        };
        Ok(Arc::new(database))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_configured_root_that_is_missing_is_refused_and_not_created() {
        let parent = tempfile::tempdir().unwrap();
        let root = parent.path().join("mnt/volumes");
        let err = VolumeRoot::configured(&root)
            .prepare()
            .expect_err("a missing configured root must be refused");
        assert!(err.to_string().contains("Mount it or create it"), "{err}");
        assert!(!root.exists());
        assert!(!parent.path().join("mnt").exists());
    }

    #[test]
    fn a_configured_root_gets_its_directories() {
        let root = tempfile::tempdir().unwrap();
        let volume_root = VolumeRoot::configured(root.path());
        volume_root.prepare().unwrap();
        for dir in [
            volume_root.managed_dir(),
            volume_root.sessions_dir(),
            volume_root.ephemeral_dir(),
        ] {
            assert!(dir.is_dir(), "{}", dir.display());
        }
        // Preparing again keeps what is there.
        std::fs::write(volume_root.sessions_dir().join("kept"), b"x").unwrap();
        volume_root.prepare().unwrap();
        assert!(volume_root.sessions_dir().join("kept").exists());
    }

    #[test]
    fn a_configured_root_that_is_a_file_is_refused() {
        let parent = tempfile::tempdir().unwrap();
        let root = parent.path().join("volumes");
        std::fs::write(&root, b"x").unwrap();
        let err = VolumeRoot::configured(&root)
            .prepare()
            .expect_err("a file is no root");
        assert!(err.to_string().contains("not a directory"), "{err}");
    }

    #[cfg(unix)]
    #[test]
    fn a_link_in_place_of_a_root_directory_is_refused() {
        let root = tempfile::tempdir().unwrap();
        let elsewhere = tempfile::tempdir().unwrap();
        std::os::unix::fs::symlink(elsewhere.path(), root.path().join("sessions")).unwrap();
        let err = VolumeRoot::configured(root.path())
            .prepare()
            .expect_err("a link at sessions/ must be refused");
        let err = err.to_string();
        assert!(err.contains("sessions"), "{err}");
        assert!(err.contains("not a link or a file"), "{err}");
    }

    #[test]
    fn the_default_root_is_created_with_its_directories() {
        let parent = tempfile::tempdir().unwrap();
        let root = parent.path().join("server/volumes");
        let volume_root = VolumeRoot::default_root(&root);
        volume_root.prepare().unwrap();
        for dir in [
            volume_root.managed_dir(),
            volume_root.sessions_dir(),
            volume_root.ephemeral_dir(),
        ] {
            assert!(dir.is_dir(), "{}", dir.display());
        }
    }

    #[test]
    fn a_file_at_the_default_root_is_refused() {
        let parent = tempfile::tempdir().unwrap();
        let root = parent.path().join("volumes");
        std::fs::write(&root, b"x").unwrap();
        let err = VolumeRoot::default_root(&root)
            .prepare()
            .expect_err("a file is no root");
        assert!(err.to_string().contains("not a directory"), "{err}");
    }

    #[test]
    fn a_workspace_under_a_lost_parent_is_not_recreated() {
        let root = tempfile::tempdir().unwrap();
        let sessions = root.path().join("sessions");
        let err = create_owned_dir(&sessions.join("s1")).expect_err("a missing parent must fail");
        assert_eq!(err.kind(), std::io::ErrorKind::NotFound);
        assert!(!sessions.exists());
    }
}
