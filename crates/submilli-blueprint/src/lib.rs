//! Shared blueprint types and YAML parser.

mod auth_proxy;
mod diag;
mod filter;
mod git;
mod llm;
mod mcp;
mod permissions;
mod secrets;
mod variables;
mod vfs_paths;

use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use std::marker::PhantomData;
use std::time::Duration;

use serde::ser::SerializeMap;
use serde::{Deserialize, Serialize, Serializer, de};

pub use auth_proxy::{
    AuthError, AuthProxyRule, AuthSpec, BasicAuth, Injections, SecretResolver, interpolate,
    resolve_injections, secret_refs, verify_secrets,
};
pub use diag::{Fault, PathSeg, YamlPath};
pub use filter::{FieldMatch, FilterExpr, VarBindings};
pub use git::{GitConfig, GitIdentity};
pub use llm::{LlmConfig, LlmModelDecl, LlmProviderDecl};
pub use mcp::{McpAuth, McpServer};
pub use permissions::{Action, DefaultAction, PermissionRule};
pub use secrets::{
    HarnessSecret, HarnessSecretBindings, HarnessSecretError, SecretSource,
    required_harness_secrets, resolve_harness_secrets,
};
pub use variables::{VariableDecl, VariableError, resolve_variables};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Blueprint {
    /// Document discriminator for kind-routed tooling (`submilli apply`).
    /// Optional for back-compat; when present it must be `blueprint`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub kind: Option<String>,
    pub name: String,
    /// Permit cleartext submilli:http traffic; auth-proxy rules must opt in separately.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub allow_insecure_http: bool,
    /// Idle window before a session is closed and reaped. Applies to every
    /// session regardless of VFS mode — a session always exists to hold the
    /// `lastRun` result; `per_session` mode additionally owns a directory the
    /// reaper wipes.
    #[serde(
        default = "default_idle_timeout",
        skip_serializing_if = "is_default_idle_timeout",
        serialize_with = "serialize_idle_timeout",
        deserialize_with = "deserialize_idle_timeout"
    )]
    pub idle_timeout: Duration,
    #[serde(default, skip_serializing_if = "VfsConfig::is_default")]
    pub vfs: VfsConfig,
    /// Declared secret names → where each value comes from. The allow-list that
    /// `${secrets.X}` references must name.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub secrets: BTreeMap<String, SecretSource>,
    /// Declared session variables → their `required` / `default` rules. The
    /// allow-list that `${vars.NAME}` filter references must name; caller-supplied
    /// values are bound per session and validated at init.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub variables: BTreeMap<String, VariableDecl>,
    /// Curated/registry packages the script may import. `submilli:*` host
    /// modules and `@mcp/*` virtual packages are configured elsewhere.
    #[serde(
        default,
        skip_serializing_if = "BTreeSet::is_empty",
        serialize_with = "serialize_packages",
        deserialize_with = "deserialize_packages"
    )]
    pub packages: BTreeSet<String>,
    /// Host-keyed outbound-auth injection rules.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub auth_proxy: Vec<AuthProxyRule>,
    /// Opt in to Git operations with an operator-controlled commit identity.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub git: Option<GitConfig>,
    /// Fall-through capability action when no `permissions` rule matches.
    /// Present (even just `default: deny`) means the operator configured a
    /// policy. Absent, it resolves to `deny` — a policy-free blueprint denies
    /// every capability (deny-by-default). Set `default: allow` to invert the
    /// posture to allow-by-default. See [`Blueprint::resolve_permission`].
    #[serde(default, rename = "default", skip_serializing_if = "Option::is_none")]
    pub default_action: Option<DefaultAction>,
    /// Per-caller capability rules. Keyed by caller id (`main` for the user
    /// script, package name for library code); each value is an ordered,
    /// first-match-wins rule list.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub permissions: BTreeMap<String, Vec<PermissionRule>>,
    /// Outbound MCP servers the script reaches via `@mcp/<server>` virtual
    /// packages. Keyed by local server identifier.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub mcp: BTreeMap<String, McpServer>,
    /// Model providers the script reaches through `submilli:llm`, and the models
    /// it may name. Unlike `mcp:`, which discovers its sub-entities live, this
    /// block *is* the model catalog: no provider SDK exposes a listing API, so
    /// declaration is authoritative and gates calling.
    #[serde(default, skip_serializing_if = "LlmConfig::is_empty")]
    pub llm: LlmConfig,
}

impl Default for Blueprint {
    fn default() -> Self {
        Blueprint {
            git: None,
            kind: None,
            name: String::new(),
            allow_insecure_http: false,
            idle_timeout: DEFAULT_IDLE_TIMEOUT,
            vfs: VfsConfig::default(),
            secrets: BTreeMap::new(),
            variables: BTreeMap::new(),
            packages: BTreeSet::new(),
            auth_proxy: Vec::new(),
            default_action: None,
            permissions: BTreeMap::new(),
            mcp: BTreeMap::new(),
            llm: LlmConfig::default(),
        }
    }
}

impl Blueprint {
    /// Whether the operator configured a capability policy: either explicit
    /// per-caller rules, or just a top-level `default:`. Reports only whether
    /// the blueprint declares a policy of its own (for tooling / lints) —
    /// enforcement always runs through [`Self::resolve_permission`], which is
    /// deny-by-default when no policy is declared.
    pub fn has_permission_policy(&self) -> bool {
        self.default_action.is_some() || !self.permissions.is_empty()
    }

    /// Resolve a capability `check()` for `caller` against this blueprint's
    /// permission rules. Pure — no parsing happens here (filter ASTs are parsed
    /// at registration), so it is cheap to call per check. The fall-through is
    /// `default:` when set, else `deny`.
    /// Resolve a permission and its zero-based index within the caller's rules.
    /// `None` identifies the blueprint's fall-through default.
    pub fn resolve_permission_with_rule(
        &self,
        caller: &str,
        capability: &str,
        context: &serde_json::Value,
        vars: &VarBindings,
    ) -> (Action, Option<usize>) {
        permissions::resolve_with_rule(
            &self.permissions,
            self.default_action.unwrap_or_default(),
            caller,
            capability,
            context,
            vars,
        )
    }

    pub fn resolve_permission(
        &self,
        caller: &str,
        capability: &str,
        context: &serde_json::Value,
        vars: &VarBindings,
    ) -> Action {
        permissions::resolve(
            &self.permissions,
            self.default_action.unwrap_or_default(),
            caller,
            capability,
            context,
            vars,
        )
    }
}

fn default_idle_timeout() -> Duration {
    DEFAULT_IDLE_TIMEOUT
}

fn is_default_idle_timeout(d: &Duration) -> bool {
    *d == DEFAULT_IDLE_TIMEOUT
}

// Durations serialize as `<secs>s` strings (which `parse_duration` reads back),
// so `to_yaml` output round-trips through `parse` when the file store re-reads
// it on boot.
fn serialize_idle_timeout<S>(d: &Duration, serializer: S) -> Result<S::Ok, S::Error>
where
    S: Serializer,
{
    serializer.serialize_str(&format!("{}s", d.as_secs()))
}

fn deserialize_idle_timeout<'de, D>(deserializer: D) -> Result<Duration, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let raw = String::deserialize(deserializer)?;
    parse_duration(&raw).map_err(de::Error::custom)
}

/// The `vfs:` block: which filesystem a script gets, and the per-mode settings.
/// Each mode's settings live on its variant, so invalid combinations — a
/// `volume` under `none`, a `grace_period` under `ephemeral`, `mounts` with no
/// root to mount them in — can't be represented; the parser rejects them at the
/// YAML boundary.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum VfsConfig {
    /// `submilli:fs.*` always fails. Pure-compute scripts.
    None,
    /// A fresh disk-backed scratch directory per execute, wiped at return. The
    /// default when `vfs:` is omitted — scripts get a filesystem without the
    /// operator having to opt in.
    Ephemeral {
        size_limit: Option<u64>,
        mounts: Mounts,
        cwd: Option<String>,
    },
    /// A disk-backed directory that persists across executes within one session
    /// and is wiped when the session ends.
    PerSession {
        size_limit: Option<u64>,
        mounts: Mounts,
        cwd: Option<String>,
    },
    /// A named volume the operator declared in the server config as the root;
    /// files persist across calls, sessions, and restarts, and other blueprints
    /// naming the same volume share them. Its size limit and the most access it
    /// allows are the operator's; `access` here can only narrow it. The
    /// blueprint names the volume — only the operator knows where it is stored.
    Named {
        volume: String,
        sub_path: Option<String>,
        access: Option<Access>,
        mounts: Mounts,
        cwd: Option<String>,
    },
}

/// Whether a script may change a named volume. A blueprint can only narrow
/// what the server's declaration of the volume allows.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Access {
    ReadOnly,
    ReadWrite,
}

impl Access {
    pub fn as_str(self) -> &'static str {
        match self {
            Access::ReadOnly => "read_only",
            Access::ReadWrite => "read_write",
        }
    }
}

/// A named volume mounted below the root, under `vfs.mounts`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MountConfig {
    pub volume: String,
    pub sub_path: Option<String>,
    /// `None` takes the access the server declares for the volume.
    pub access: Option<Access>,
}

/// Mounts keyed by their absolute guest path, such as `/memory`. The parser
/// admits only normalized paths that neither nest nor overlap.
pub type Mounts = BTreeMap<String, MountConfig>;

/// One use of a named volume by a blueprint: as the root (`mount: None`) or
/// mounted at a guest path.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct NamedReference<'a> {
    pub mount: Option<&'a str>,
    pub volume: &'a str,
    pub access: Option<Access>,
}

impl NamedReference<'_> {
    /// Where the reference sits in the blueprint, for diagnostics.
    pub fn yaml_path(&self, field: &str) -> YamlPath {
        match self.mount {
            None => yaml_path!["vfs", field],
            Some(mount) => yaml_path!["vfs", "mounts", mount, field],
        }
    }
}

const DEFAULT_IDLE_TIMEOUT: Duration = Duration::from_secs(24 * 60 * 60);

/// Bounds on a mount's guest path, so a hostile blueprint cannot make every
/// path lookup expensive.
const MAX_MOUNT_PATH_BYTES: usize = 4096;
const MAX_MOUNT_PATH_COMPONENTS: usize = 64;

/// The most mounts one blueprint may declare; the runtime routes every path
/// through all of them. Matches the interpreter's own cap
/// (`interpreter::runtime::vfs::MAX_MOUNTS`), which backs this one up.
pub const MAX_MOUNTS: usize = 16;

impl Default for VfsConfig {
    fn default() -> Self {
        VfsConfig::Ephemeral {
            size_limit: None,
            mounts: Mounts::new(),
            cwd: None,
        }
    }
}

impl VfsConfig {
    fn is_default(&self) -> bool {
        *self == Self::default()
    }

    /// The mode name as it appears in YAML and `fs.info()`.
    pub fn mode_str(&self) -> &'static str {
        match self {
            VfsConfig::None => "none",
            VfsConfig::Ephemeral { .. } => "ephemeral",
            VfsConfig::PerSession { .. } => "per_session",
            VfsConfig::Named { .. } => "named",
        }
    }

    /// Byte cap on the files a program keeps in the root, where the blueprint
    /// sets one (`ephemeral` / `per_session`). A named root's limit belongs to
    /// the server's declaration of the volume.
    pub fn size_limit(&self) -> Option<u64> {
        match self {
            VfsConfig::Ephemeral { size_limit, .. } | VfsConfig::PerSession { size_limit, .. } => {
                *size_limit
            }
            _ => None,
        }
    }

    /// The named volumes mounted below the root.
    pub fn mounts(&self) -> &Mounts {
        static EMPTY: Mounts = Mounts::new();
        match self {
            VfsConfig::None => &EMPTY,
            VfsConfig::Ephemeral { mounts, .. }
            | VfsConfig::PerSession { mounts, .. }
            | VfsConfig::Named { mounts, .. } => mounts,
        }
    }

    /// Every named volume the blueprint uses: the root first, when named, then
    /// each mount in path order.
    pub fn named_references(&self) -> Vec<NamedReference<'_>> {
        let mut references = Vec::new();
        if let VfsConfig::Named { volume, access, .. } = self {
            references.push(NamedReference {
                mount: None,
                volume,
                access: *access,
            });
        }
        for (path, mount) in self.mounts() {
            references.push(NamedReference {
                mount: Some(path),
                volume: &mount.volume,
                access: mount.access,
            });
        }
        references
    }
}

#[derive(Default, Clone, Copy, PartialEq, Eq)]
enum ModeTag {
    None,
    #[default]
    Ephemeral,
    PerSession,
    Named,
}

impl ModeTag {
    fn as_str(self) -> &'static str {
        match self {
            ModeTag::None => "none",
            ModeTag::Ephemeral => "ephemeral",
            ModeTag::PerSession => "per_session",
            ModeTag::Named => "named",
        }
    }

    /// Whether `field` belongs to this mode. `grace_period` and `path_limit` are
    /// accepted without doing anything: `grace_period` was a `per_session`
    /// setting before the connection-oriented transports were dropped, and
    /// `path_limit` a file-count cap that was never enforced. Stored blueprints
    /// carry both, so they keep parsing rather than failing on an unknown key.
    fn allows(self, field: &str) -> bool {
        match field {
            "volume" | "access" | "subPath" => matches!(self, ModeTag::Named),
            "mounts" | "cwd" => !matches!(self, ModeTag::None),
            "grace_period" => matches!(self, ModeTag::PerSession),
            "size_limit" | "path_limit" => {
                matches!(self, ModeTag::Ephemeral | ModeTag::PerSession)
            }
            _ => true,
        }
    }
}

/// What a blueprint still using the removed `persistent` mode is told, wherever
/// it names it.
const PERSISTENT_REMOVED: &str = "vfs mode `persistent` was removed: write `mode: named` and keep \
                                  the `volume:` line (`vfs: {mode: named, volume: <name>}`). A \
                                  named volume keeps its files across calls, sessions and \
                                  restarts as before; leave `access:` out to keep the access the \
                                  server declares for it";

impl<'de> Deserialize<'de> for ModeTag {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        deserializer.deserialize_str(ModeTagVisitor)
    }
}

struct ModeTagVisitor;

impl de::Visitor<'_> for ModeTagVisitor {
    type Value = ModeTag;

    fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("a vfs mode: `none`, `ephemeral`, `per_session` or `named`")
    }

    fn visit_str<E: de::Error>(self, mode: &str) -> Result<ModeTag, E> {
        match mode {
            "none" => Ok(ModeTag::None),
            "ephemeral" => Ok(ModeTag::Ephemeral),
            "per_session" => Ok(ModeTag::PerSession),
            "named" => Ok(ModeTag::Named),
            "persistent" => Err(E::custom(PERSISTENT_REMOVED)),
            other => Err(E::unknown_variant(
                other,
                &["none", "ephemeral", "per_session", "named"],
            )),
        }
    }
}

#[derive(Deserialize)]
#[serde(untagged)]
enum SizeRepr {
    Num(u64),
    Str(String),
}

#[derive(Default)]
struct Full {
    /// `None` means the block never wrote `mode:` — distinct from an explicit
    /// `mode: ephemeral`, because a field rejected under the *implicit* default
    /// must not name a mode the author never typed.
    mode: Option<ModeTag>,
    volume: Option<String>,
    access: Option<Access>,
    mounts: Option<Mounts>,
    sub_path: Option<String>,
    cwd: Option<String>,
    grace_period: Option<String>,
    size_limit: Option<SizeRepr>,
    has_legacy_path_limit: bool,
}

const VFS_KEYS: &str = "`mode`, `volume`, `access`, `size_limit`, `mounts`, `subPath`, `cwd` (`grace_period` under \
                        `mode: per_session` and `path_limit` are also accepted, but ignored)";

/// What a `volume:` written without a `mode:` beside it is told. The block
/// defaults to `ephemeral`, so the plain mode-conflict message would name a
/// mode that appears nowhere in the document.
const VOLUME_WITHOUT_MODE: &str = "`volume` is only valid under `mode: named`, and this vfs block \
                                   has no `mode:` (it defaults to `ephemeral`); add `mode: named` \
                                   to this vfs block, or move the volume under `mounts:` to keep \
                                   an ephemeral root";

/// What a blueprint still carrying the retired `path:` key is told. The key is
/// gone from every mode, so the message is the same wherever it appears.
const RETIRED_PATH: &str = "the `path` key is retired: a blueprint can no longer name a host \
                            directory. Use `volume: <name>` under `mode: named` — the operator \
                            declares each volume by name in the server config";

/// What `mounts:` under `mode: none` is told.
const MOUNTS_WITHOUT_ROOT: &str = "`mounts` needs a filesystem root to mount volumes in, and \
                                   `mode: none` has none; use `mode: ephemeral` (the default) or \
                                   `mode: per_session` for the root";

// Custom Deserialize accepts both the shorthand scalar (`vfs: per_session`) and
// the full map. Human-readable durations ("5m") and sizes ("100MB") are parsed
// here, and any field that doesn't belong to the chosen mode is rejected, so the
// result is always a well-formed variant.
//
// The map is walked by hand rather than deserialized into a `deny_unknown_fields`
// struct behind an untagged enum: untagged deserialization buffers the node and
// collapses every inner failure into "data did not match any variant", which
// names neither the offending key nor a usable YAML path.
impl<'de> Deserialize<'de> for VfsConfig {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        deserializer.deserialize_any(VfsVisitor)
    }
}

struct VfsVisitor;

impl<'de> de::Visitor<'de> for VfsVisitor {
    type Value = VfsConfig;

    fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(
            "a vfs mode name (`none`, `ephemeral`, `per_session`, `named`) or a vfs config map",
        )
    }

    fn visit_str<E: de::Error>(self, mode: &str) -> Result<Self::Value, E> {
        let mode = ModeTagVisitor.visit_str(mode)?;
        build_vfs(Full {
            mode: Some(mode),
            ..Full::default()
        })
        .map_err(de::Error::custom)
    }

    fn visit_map<A: de::MapAccess<'de>>(self, mut map: A) -> Result<Self::Value, A::Error> {
        let mut full = Full::default();
        let mut seen = BTreeSet::new();
        while let Some(key) = map.next_key::<String>()? {
            // A blueprint is a hand-edited policy document, and a repeated key
            // is what a bad merge produces: last-wins would silently deploy a
            // mode other than the one a reviewer reads at the top of the block.
            if !seen.insert(key.clone()) {
                map.next_value_seed(Reject::<()>::new(duplicate_key(&key, "vfs:")))?;
                continue;
            }
            // The mode as known *so far*: a field written above `mode:` can't
            // be checked here, and falls through to `build_vfs`.
            let mode = full.mode;
            match key.as_str() {
                "mode" => full.mode = Some(map.next_value()?),
                "volume" => {
                    full.volume =
                        Some(map.next_value_seed(Guarded::new("volume", mode, VolumeName))?);
                }
                "access" => {
                    let seed = Guarded::new("access", mode, PhantomData::<Access>);
                    full.access = Some(map.next_value_seed(seed)?);
                }
                "subPath" => {
                    full.sub_path = Some(map.next_value_seed(Guarded::new(
                        "subPath",
                        mode,
                        PhantomData::<String>,
                    ))?);
                }
                "cwd" => {
                    full.cwd = Some(map.next_value_seed(Guarded::new(
                        "cwd",
                        mode,
                        PhantomData::<String>,
                    ))?);
                }
                "mounts" => {
                    full.mounts =
                        Some(map.next_value_seed(Guarded::new("mounts", mode, MountsSeed))?);
                }
                "grace_period" => {
                    let seed = Guarded::new("grace_period", mode, PhantomData::<String>);
                    full.grace_period = Some(map.next_value_seed(seed)?);
                }
                "size_limit" => {
                    let seed = Guarded::new("size_limit", mode, PhantomData::<SizeRepr>);
                    full.size_limit = Some(map.next_value_seed(seed)?);
                }
                "path_limit" => {
                    let seed = Guarded::new("path_limit", mode, PhantomData::<u64>);
                    map.next_value_seed(seed)?;
                    full.has_legacy_path_limit = true;
                }
                "path" => map.next_value_seed(Reject::<()>::new(RETIRED_PATH))?,
                other => map.next_value_seed(Reject::<()>::new(format!(
                    "unknown field `{other}` under `vfs:`, expected one of {VFS_KEYS}"
                )))?,
            }
        }
        build_vfs(full).map_err(de::Error::custom)
    }
}

fn duplicate_key(key: &str, block: &str) -> String {
    format!(
        "duplicate key `{key}` under `{block}`: it is already set earlier in this block; \
         delete one of the two `{key}:` lines"
    )
}

/// Wraps a `vfs:` field's value seed with the mode check for that field, so a
/// field that doesn't belong to the declared mode fails from *inside* its value
/// and the diagnostic anchors on the offending key rather than on `vfs:`. When
/// `mode:` hasn't been read yet — it may come later in the block, or never —
/// the check falls through to [`build_vfs`], which reports at the block.
struct Guarded<S> {
    conflict: Option<String>,
    inner: S,
}

impl<S> Guarded<S> {
    fn new(field: &'static str, mode: Option<ModeTag>, inner: S) -> Self {
        Guarded {
            conflict: mode
                .filter(|mode| !mode.allows(field))
                .map(|mode| mode_conflict(field, mode).to_string()),
            inner,
        }
    }
}

impl<'de, S: de::DeserializeSeed<'de>> de::DeserializeSeed<'de> for Guarded<S> {
    type Value = S::Value;

    fn deserialize<D>(self, deserializer: D) -> Result<Self::Value, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        match self.conflict {
            Some(conflict) => Reject::new(conflict).deserialize(deserializer),
            None => self.inner.deserialize(deserializer),
        }
    }
}

/// A map value whose deserialization always fails, standing in for a value of
/// type `T` so it can replace any field's seed.
///
/// Raising the error from *inside* the value is what anchors the diagnostic at
/// that key: an error returned from [`VfsVisitor::visit_map`] itself carries
/// only the `vfs:` path, which is too coarse for an editor to squiggle the
/// offending line. Failing from within the visitor rather than after consuming
/// the node also keeps the YAML parser's mark on the value, so the reported
/// line is the offending one and not the first line of the block.
struct Reject<T>(String, PhantomData<T>);

impl<T> Reject<T> {
    fn new(message: impl Into<String>) -> Self {
        Reject(message.into(), PhantomData)
    }

    fn fail<E: de::Error>(self) -> Result<T, E> {
        Err(de::Error::custom(self.0))
    }
}

impl<'de, T> de::DeserializeSeed<'de> for Reject<T> {
    type Value = T;

    fn deserialize<D>(self, deserializer: D) -> Result<Self::Value, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        deserializer.deserialize_any(self)
    }
}

// Every shape of value gets the same message; the default `Visitor` methods
// would replace it with serde's own `invalid type` wording.
impl<'de, T> de::Visitor<'de> for Reject<T> {
    type Value = T;

    fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }

    fn visit_bool<E: de::Error>(self, _: bool) -> Result<T, E> {
        self.fail()
    }

    fn visit_i64<E: de::Error>(self, _: i64) -> Result<T, E> {
        self.fail()
    }

    fn visit_u64<E: de::Error>(self, _: u64) -> Result<T, E> {
        self.fail()
    }

    fn visit_f64<E: de::Error>(self, _: f64) -> Result<T, E> {
        self.fail()
    }

    fn visit_str<E: de::Error>(self, _: &str) -> Result<T, E> {
        self.fail()
    }

    fn visit_bytes<E: de::Error>(self, _: &[u8]) -> Result<T, E> {
        self.fail()
    }

    fn visit_unit<E: de::Error>(self) -> Result<T, E> {
        self.fail()
    }

    fn visit_seq<A: de::SeqAccess<'de>>(self, _: A) -> Result<T, A::Error> {
        self.fail()
    }

    fn visit_map<A: de::MapAccess<'de>>(self, _: A) -> Result<T, A::Error> {
        self.fail()
    }

    fn visit_enum<A: de::EnumAccess<'de>>(self, _: A) -> Result<T, A::Error> {
        self.fail()
    }
}

/// Reads `volume:` as a non-empty string, anchoring both the wrong-type and the
/// empty-name diagnostic at the `volume` key. Goes through `deserialize_any` so
/// a non-string scalar is refused rather than silently read as its YAML text:
/// `volume:` is a bare name, never a structure carrying a path or a subpath.
struct VolumeName;

impl<'de> de::DeserializeSeed<'de> for VolumeName {
    type Value = String;

    fn deserialize<D>(self, deserializer: D) -> Result<Self::Value, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        deserializer.deserialize_any(VolumeName)
    }
}

impl<'de> de::Visitor<'de> for VolumeName {
    type Value = String;

    fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(
            "a volume name as a string — the name of a volume the operator declared in the server \
             config (quote it if the name looks like a number)",
        )
    }

    fn visit_str<E: de::Error>(self, name: &str) -> Result<Self::Value, E> {
        if name.trim().is_empty() {
            return Err(de::Error::custom(
                "`volume` is empty: name a volume the operator declared in the server config",
            ));
        }
        Ok(name.to_string())
    }
}

/// Reads `mounts:`, a map from guest path to mount entry. A path that is
/// malformed, or that nests with or overlaps one written above it, is refused
/// from inside its entry so the diagnostic anchors there.
struct MountsSeed;

impl<'de> de::DeserializeSeed<'de> for MountsSeed {
    type Value = Mounts;

    fn deserialize<D>(self, deserializer: D) -> Result<Self::Value, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        deserializer.deserialize_map(MountsSeed)
    }
}

impl<'de> de::Visitor<'de> for MountsSeed {
    type Value = Mounts;

    fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(
            "a map from mount path to mount, such as `/memory: {mode: named, volume: memory}`",
        )
    }

    fn visit_map<A: de::MapAccess<'de>>(self, mut map: A) -> Result<Mounts, A::Error> {
        let mut mounts = Mounts::new();
        while let Some(path) = map.next_key::<String>()? {
            if let Err(problem) = check_mount_path(&path, &mounts) {
                map.next_value_seed(Reject::<()>::new(problem))?;
                continue;
            }
            if mounts.len() >= MAX_MOUNTS {
                map.next_value_seed(Reject::<()>::new(format!(
                    "more than {MAX_MOUNTS} mounts; mount fewer volumes"
                )))?;
                continue;
            }
            let mount = map.next_value_seed(MountSeed)?;
            mounts.insert(path, mount);
        }
        Ok(mounts)
    }
}

/// Why `path` cannot be a mount path beside the `mounts` already read.
fn check_mount_path(path: &str, mounts: &Mounts) -> Result<(), String> {
    if mounts.contains_key(path) {
        return Err(duplicate_key(path, "vfs.mounts"));
    }
    let Some(rest) = path.strip_prefix('/') else {
        return Err(format!(
            "mount path `{path}` must be absolute, such as `/memory`"
        ));
    };
    if rest.is_empty() {
        return Err(
            "a volume cannot be mounted at `/`: that is the root, which the vfs `mode:` chooses; \
             mount under a subdirectory such as `/memory`, or use `mode: named` for a named root"
                .into(),
        );
    }
    if path.len() > MAX_MOUNT_PATH_BYTES {
        return Err(format!(
            "mount path is longer than {MAX_MOUNT_PATH_BYTES} bytes"
        ));
    }
    // ASCII only: the runtime guards mount points with ASCII case folding, which
    // is only reliable for ASCII names on case-insensitive filesystems.
    if !path
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || matches!(c, '/' | '.' | '_' | '-'))
    {
        return Err(format!(
            "mount path `{}` may contain only ASCII letters, digits, `.`, `_` and `-` \
             between its `/` separators",
            path.escape_debug()
        ));
    }
    let parts: Vec<&str> = rest.split('/').collect();
    if parts.len() > MAX_MOUNT_PATH_COMPONENTS {
        return Err(format!(
            "mount path `{path}` has more than {MAX_MOUNT_PATH_COMPONENTS} components"
        ));
    }
    if parts
        .iter()
        .any(|part| part.is_empty() || *part == "." || *part == "..")
    {
        return Err(format!(
            "mount path `{path}` is not normalized: write it without empty, `.` or `..` \
             components and without a trailing `/`, such as `/data/memory`"
        ));
    }
    // Windows drops a trailing dot, which would make `memory.` and `memory` one
    // directory.
    if parts.iter().any(|part| part.ends_with('.')) {
        return Err(format!(
            "mount path `{path}` has a component ending in `.`; drop the trailing dot"
        ));
    }
    if parts.iter().any(|part| is_git_metadata(part)) {
        return Err(format!(
            "mount path `{path}` names Git metadata (`.git`); choose another directory"
        ));
    }
    for existing in mounts.keys() {
        let (outer, inner) = if path_contains(existing, path) {
            (existing.as_str(), path)
        } else if path_contains(path, existing) {
            (path, existing.as_str())
        } else {
            continue;
        };
        if outer.eq_ignore_ascii_case(inner) {
            return Err(format!(
                "mount path `{path}` differs from `{existing}` only in letter case; on a \
                 case-insensitive filesystem they are the same directory"
            ));
        }
        return Err(format!(
            "mount `{inner}` is inside mount `{outer}`; mounts may not nest — mount the volumes \
             side by side, such as `/a` and `/b`"
        ));
    }
    Ok(())
}

/// Whether the absolute path `outer` is `inner` or one of its ancestors,
/// comparing whole components and ignoring ASCII case, as a case-insensitive
/// filesystem would.
fn path_contains(outer: &str, inner: &str) -> bool {
    let mut inner_parts = inner.split('/');
    outer.split('/').all(|part| {
        inner_parts
            .next()
            .is_some_and(|other| other.eq_ignore_ascii_case(part))
    })
}

fn is_git_metadata(part: &str) -> bool {
    let lower = part.to_ascii_lowercase();
    lower.trim_end_matches([' ', '.']) == ".git" || lower.starts_with(".git-submilli-")
}

/// Reads one mount entry: `mode: named` (required — the only mount mode so
/// far), `volume`, and an optional `access`.
struct MountSeed;

impl<'de> de::DeserializeSeed<'de> for MountSeed {
    type Value = MountConfig;

    fn deserialize<D>(self, deserializer: D) -> Result<Self::Value, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        deserializer.deserialize_map(MountSeed)
    }
}

const MOUNT_KEYS: &str = "`mode`, `volume`, `access`, `subPath`";

impl<'de> de::Visitor<'de> for MountSeed {
    type Value = MountConfig;

    fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("a mount, such as `{mode: named, volume: memory, access: read_write}`")
    }

    fn visit_map<A: de::MapAccess<'de>>(self, mut map: A) -> Result<MountConfig, A::Error> {
        let mut seen = BTreeSet::new();
        let mut named = false;
        let mut volume = None;
        let mut access = None;
        let mut sub_path = None;
        while let Some(key) = map.next_key::<String>()? {
            if !seen.insert(key.clone()) {
                map.next_value_seed(Reject::<()>::new(duplicate_key(&key, "the mount")))?;
                continue;
            }
            match key.as_str() {
                "mode" => {
                    let mode: ModeTag = map.next_value()?;
                    if mode != ModeTag::Named {
                        return Err(de::Error::custom(format!(
                            "a mount's `mode` must be `named`, not `{}`: only named volumes \
                             can be mounted; the root's mode is set by the vfs `mode:`",
                            mode.as_str()
                        )));
                    }
                    named = true;
                }
                "volume" => volume = Some(map.next_value_seed(VolumeName)?),
                "access" => access = Some(map.next_value::<Access>()?),
                "subPath" => sub_path = Some(map.next_value::<String>()?),
                other => map.next_value_seed(Reject::<()>::new(format!(
                    "unknown field `{other}` in a mount, expected one of {MOUNT_KEYS}"
                )))?,
            }
        }
        if !named {
            return Err(de::Error::custom(
                "a mount needs `mode: named`: mounts are named volumes the operator declares in \
                 the server config",
            ));
        }
        let volume = volume.ok_or_else(|| {
            de::Error::custom(
                "a mount needs a `volume`: the name of a volume the operator declared in the \
                 server config",
            )
        })?;
        Ok(MountConfig {
            volume,
            access,
            sub_path,
        })
    }
}

// Hand-written so the output round-trips back through `parse`: the file store
// re-reads `to_yaml` output on boot. Durations serialize as `<secs>s` strings
// (which `parse_duration` reads back); sizes/counts as plain byte numbers.
impl Serialize for VfsConfig {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        let opt = |present: bool| usize::from(present);
        match self {
            VfsConfig::None => {
                let mut map = serializer.serialize_map(Some(1))?;
                map.serialize_entry("mode", "none")?;
                map.end()
            }
            VfsConfig::Ephemeral {
                size_limit,
                mounts,
                cwd,
            }
            | VfsConfig::PerSession {
                size_limit,
                mounts,
                cwd,
            } => {
                let len =
                    1 + opt(size_limit.is_some()) + opt(!mounts.is_empty()) + opt(cwd.is_some());
                let mut map = serializer.serialize_map(Some(len))?;
                map.serialize_entry("mode", self.mode_str())?;
                if let Some(n) = size_limit {
                    map.serialize_entry("size_limit", n)?;
                }
                if let Some(cwd) = cwd {
                    map.serialize_entry("cwd", cwd)?;
                }
                serialize_mounts(&mut map, mounts)?;
                map.end()
            }
            VfsConfig::Named {
                volume,
                access,
                mounts,
                sub_path,
                cwd,
            } => {
                let len = 2
                    + opt(access.is_some())
                    + opt(!mounts.is_empty())
                    + opt(sub_path.is_some())
                    + opt(cwd.is_some());
                let mut map = serializer.serialize_map(Some(len))?;
                map.serialize_entry("mode", "named")?;
                map.serialize_entry("volume", volume)?;
                if let Some(sub_path) = sub_path {
                    map.serialize_entry("subPath", sub_path)?;
                }
                if let Some(access) = access {
                    map.serialize_entry("access", access)?;
                }
                if let Some(cwd) = cwd {
                    map.serialize_entry("cwd", cwd)?;
                }
                serialize_mounts(&mut map, mounts)?;
                map.end()
            }
        }
    }
}

fn serialize_mounts<M: SerializeMap>(map: &mut M, mounts: &Mounts) -> Result<(), M::Error> {
    if mounts.is_empty() {
        return Ok(());
    }
    map.serialize_entry("mounts", &SerializedMounts(mounts))
}

struct SerializedMounts<'a>(&'a Mounts);

impl Serialize for SerializedMounts<'_> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut map = serializer.serialize_map(Some(self.0.len()))?;
        for (path, mount) in self.0 {
            map.serialize_entry(path, &SerializedMount(mount))?;
        }
        map.end()
    }
}

struct SerializedMount<'a>(&'a MountConfig);

impl Serialize for SerializedMount<'_> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let MountConfig {
            volume,
            access,
            sub_path,
        } = self.0;
        let mut map = serializer.serialize_map(Some(
            2 + usize::from(access.is_some()) + usize::from(sub_path.is_some()),
        ))?;
        map.serialize_entry("mode", "named")?;
        map.serialize_entry("volume", volume)?;
        if let Some(sub_path) = sub_path {
            map.serialize_entry("subPath", sub_path)?;
        }
        if let Some(access) = access {
            map.serialize_entry("access", access)?;
        }
        map.end()
    }
}

/// Convert the raw parsed fields into a well-formed variant, rejecting any field
/// that doesn't belong to the chosen mode.
///
/// The rejection sweep is a backstop: [`Guarded`] already refuses a field whose
/// mode was declared above it, with a diagnostic anchored on the field. What
/// reaches here is a field written *before* `mode:`, or under no `mode:` at all.
fn build_vfs(full: Full) -> Result<VfsConfig, BlueprintError> {
    let size_limit = match full.size_limit {
        Some(SizeRepr::Num(n)) => Some(n),
        Some(SizeRepr::Str(s)) => Some(parse_size(&s)?),
        None => None,
    };
    let grace = full
        .grace_period
        .as_deref()
        .map(parse_duration)
        .transpose()?;
    let mode = full.mode.unwrap_or_default();

    if full.volume.is_some() && !mode.allows("volume") {
        return Err(match full.mode {
            Some(mode) => mode_conflict("volume", mode),
            None => BlueprintError::InvalidVfs(VOLUME_WITHOUT_MODE.into()),
        });
    }
    for (field, present) in [
        ("access", full.access.is_some()),
        ("subPath", full.sub_path.is_some()),
        ("cwd", full.cwd.is_some()),
        ("mounts", full.mounts.is_some()),
        ("grace_period", grace.is_some()),
        ("size_limit", size_limit.is_some()),
        ("path_limit", full.has_legacy_path_limit),
    ] {
        if present && !mode.allows(field) {
            return Err(mode_conflict(field, mode));
        }
    }
    let mounts = full.mounts.unwrap_or_default();
    let cwd = full.cwd;

    match mode {
        ModeTag::None => Ok(VfsConfig::None),
        ModeTag::Ephemeral => Ok(VfsConfig::Ephemeral {
            size_limit,
            mounts,
            cwd,
        }),
        ModeTag::PerSession => Ok(VfsConfig::PerSession {
            size_limit,
            mounts,
            cwd,
        }),
        ModeTag::Named => {
            let volume = full.volume.ok_or_else(|| {
                BlueprintError::InvalidVfs(
                    "vfs mode 'named' requires a 'volume': the name of a volume the operator \
                     declared in the server config"
                        .into(),
                )
            })?;
            Ok(VfsConfig::Named {
                volume,
                access: full.access,
                sub_path: full.sub_path,
                cwd,
                mounts,
            })
        }
    }
}

fn mode_conflict(field: &str, mode: ModeTag) -> BlueprintError {
    let hint = match (field, mode) {
        ("mounts", ModeTag::None) => return BlueprintError::InvalidVfs(MOUNTS_WITHOUT_ROOT.into()),
        ("size_limit", ModeTag::Named) => {
            "; a named volume's size limit is set by the operator where the server declares it"
        }
        ("volume" | "access", _) => {
            "; `volume` and `access` belong to `mode: named`, or to an entry under `mounts:`"
        }
        _ => "",
    };
    BlueprintError::InvalidVfs(
        format!(
            "'{field}' is not valid for vfs mode '{}'{hint}",
            mode.as_str()
        )
        .into(),
    )
}

pub fn parse(yaml: &str) -> Result<Blueprint, BlueprintError> {
    if yaml.trim().is_empty() {
        return Err(BlueprintError::Empty);
    }
    let blueprint: Blueprint =
        serde_path_to_error::deserialize(serde_yml::Deserializer::from_str(yaml))
            .map_err(parse_fault)?;
    validate_kind(blueprint.kind.as_deref())?;
    validate_name(&blueprint.name)?;
    validate_packages(&blueprint.packages)?;
    auth_proxy::validate_auth_proxy(&blueprint)?;
    permissions::validate(&blueprint.permissions)?;
    variables::validate_variables(&blueprint)?;
    git::validate(&blueprint)?;
    vfs_paths::validate(&blueprint)?;
    mcp::validate_mcp(&blueprint)?;
    llm::validate_llm(&blueprint)?;
    Ok(blueprint)
}

/// Convert a deserialization failure into a `Parse` fault carrying the YAML
/// path serde got to (covers errors raised inside custom `Deserialize` impls —
/// filters, vfs, secret sources) and the parser's line/column when it has one.
fn parse_fault(err: serde_path_to_error::Error<serde_yml::Error>) -> BlueprintError {
    let path: YamlPath = err
        .path()
        .iter()
        .filter_map(|seg| match seg {
            serde_path_to_error::Segment::Map { key } => Some(PathSeg::Key(key.clone())),
            serde_path_to_error::Segment::Seq { index } => Some(PathSeg::Index(*index)),
            serde_path_to_error::Segment::Enum { variant } => Some(PathSeg::Key(variant.clone())),
            serde_path_to_error::Segment::Unknown => None,
        })
        .collect();
    let inner = err.into_inner();
    let location = inner.location().map(|l| (l.line(), l.column()));
    BlueprintError::Parse(Fault {
        message: inner.to_string(),
        path: (!path.is_empty()).then_some(path),
        location,
    })
}

/// Render a blueprint back to YAML. Used by `show` to echo a registered
/// blueprint; serialization of the value type is infallible.
pub fn to_yaml(blueprint: &Blueprint) -> String {
    serde_yml::to_string(blueprint).expect("blueprint serialization is infallible")
}

fn validate_kind(kind: Option<&str>) -> Result<(), BlueprintError> {
    match kind {
        None | Some("blueprint") => Ok(()),
        Some(other) => Err(BlueprintError::InvalidKind(Fault::at(
            yaml_path!["kind"],
            format!("unknown kind '{other}': expected `blueprint` (or omit `kind`)"),
        ))),
    }
}

fn validate_name(name: &str) -> Result<(), BlueprintError> {
    if name.is_empty() {
        return Err(BlueprintError::InvalidName(Fault::at(
            yaml_path!["name"],
            "blueprint name must not be empty",
        )));
    }
    if !name
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
    {
        return Err(BlueprintError::InvalidName(Fault::at(
            yaml_path!["name"],
            format!("blueprint name '{name}' contains characters outside [A-Za-z0-9_-]"),
        )));
    }
    Ok(())
}

fn serialize_packages<S>(packages: &BTreeSet<String>, serializer: S) -> Result<S::Ok, S::Error>
where
    S: Serializer,
{
    packages.serialize(serializer)
}

fn deserialize_packages<'de, D>(deserializer: D) -> Result<BTreeSet<String>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    #[derive(Deserialize)]
    #[serde(untagged)]
    enum Repr {
        List(Vec<String>),
        Map(BTreeMap<String, serde::de::IgnoredAny>),
    }

    let repr = Repr::deserialize(deserializer)?;
    let names: Vec<String> = match repr {
        Repr::List(names) => names,
        Repr::Map(map) => map.into_keys().collect(),
    };

    let mut packages = BTreeSet::new();
    for name in names {
        if !packages.insert(name.clone()) {
            return Err(de::Error::custom(format!(
                "duplicate package `{name}` in packages:"
            )));
        }
    }
    Ok(packages)
}

// The `packages:` path stops at the block — `BTreeSet` drops source order, so
// there is no honest per-item index; the message names the offending package.
fn validate_packages(packages: &BTreeSet<String>) -> Result<(), BlueprintError> {
    for name in packages {
        validate_package_name(name).map_err(|msg| {
            BlueprintError::InvalidPackages(Fault::at(yaml_path!["packages"], msg))
        })?;
    }
    Ok(())
}

fn validate_package_name(name: &str) -> Result<(), String> {
    if name.starts_with("submilli:") {
        return Err(format!(
            "`{name}` is a stdlib host module; do not list `submilli:*` modules in packages:"
        ));
    }
    if name.starts_with("@mcp/") {
        return Err(format!(
            "`{name}` is an MCP virtual package; declare the server in `mcp:` instead"
        ));
    }
    validate_scoped_name(name)?;
    Ok(())
}

/// Validate a scoped package name `@org/leaf` and return its parts.
///
/// Package namespaces are GitHub-org-rooted: the scope must be a GitHub-org
/// token (ASCII alphanumeric with single internal hyphens, no leading/trailing
/// hyphen, no dots, 1–39 chars). The leaf is permissive (`[A-Za-z0-9._-]`, not
/// `.`/`..`). The `Err` is an LLM-actionable message naming the fix. This is the
/// single source of truth for package-name shape — the blueprint, the package
/// store, and the build manifest all route through it.
pub fn validate_scoped_name(name: &str) -> Result<(&str, &str), String> {
    let Some(rest) = name.strip_prefix('@') else {
        return Err(format!(
            "package `{name}` must use scoped form `@org/name` (GitHub org as the scope)"
        ));
    };
    let mut parts = rest.split('/');
    let org = parts.next().unwrap_or_default();
    let leaf = parts.next().unwrap_or_default();
    if leaf.is_empty() || parts.next().is_some() {
        return Err(format!(
            "package `{name}` must use scoped form `@org/name` (exactly one `/`)"
        ));
    }
    if !is_valid_org(org) {
        return Err(format!(
            "package `{name}` scope `@{org}` must be a GitHub org: ASCII letters, digits, and single \
             internal hyphens only (no dots), 1–39 characters"
        ));
    }
    if !is_valid_leaf(leaf) {
        return Err(format!(
            "package `{name}` leaf `{leaf}` contains characters outside [A-Za-z0-9._-]"
        ));
    }
    Ok((org, leaf))
}

/// GitHub org/user rules: 1–39 chars, ASCII alphanumeric or single internal
/// hyphens, no leading/trailing hyphen.
fn is_valid_org(org: &str) -> bool {
    if org.is_empty() || org.len() > 39 {
        return false;
    }
    if org.starts_with('-') || org.ends_with('-') || org.contains("--") {
        return false;
    }
    org.chars().all(|c| c.is_ascii_alphanumeric() || c == '-')
}

fn is_valid_leaf(leaf: &str) -> bool {
    if leaf.is_empty() || leaf == "." || leaf == ".." {
        return false;
    }
    leaf.chars()
        .all(|c| c.is_ascii_alphanumeric() || c == '.' || c == '_' || c == '-')
}

/// Parse a human duration like `5m`, `30m`, `90s`, `1h`. Units: s, m, h.
fn parse_duration(raw: &str) -> Result<Duration, BlueprintError> {
    let s = raw.trim();
    let (digits, unit) = split_numeric(s);
    let value: u64 = digits.parse().map_err(|_| {
        BlueprintError::InvalidVfs(
            format!("invalid duration '{raw}': expected <number><s|m|h>").into(),
        )
    })?;
    let secs = match unit.to_ascii_lowercase().as_str() {
        "s" => Some(value),
        "m" => value.checked_mul(60),
        "h" => value.checked_mul(3600),
        "" => {
            return Err(BlueprintError::InvalidVfs(
                format!("duration '{raw}' needs a unit (s, m, or h)").into(),
            ));
        }
        other => {
            return Err(BlueprintError::InvalidVfs(
                format!("unknown duration unit '{other}' in '{raw}' (use s, m, or h)").into(),
            ));
        }
    }
    .ok_or_else(|| BlueprintError::InvalidVfs(format!("duration '{raw}' overflows").into()))?;
    Ok(Duration::from_secs(secs))
}

/// Parse a human byte size like `100MB`, `512KB`, `2GB`, or a bare `1048576`.
/// K/M/G/T are 1024-based; the `iB` spellings are accepted as synonyms.
pub fn parse_size(raw: &str) -> Result<u64, BlueprintError> {
    let s = raw.trim();
    let (digits, unit) = split_numeric(s);
    let value: u64 = digits.parse().map_err(|_| {
        BlueprintError::InvalidVfs(
            format!("invalid size '{raw}': expected <number><B|KB|MB|GB|TB>").into(),
        )
    })?;
    let mult: u64 = match unit.to_ascii_uppercase().as_str() {
        "" | "B" => 1,
        "K" | "KB" | "KIB" => 1024,
        "M" | "MB" | "MIB" => 1024 * 1024,
        "G" | "GB" | "GIB" => 1024 * 1024 * 1024,
        "T" | "TB" | "TIB" => 1024 * 1024 * 1024 * 1024,
        other => {
            return Err(BlueprintError::InvalidVfs(
                format!("unknown size unit '{other}' in '{raw}' (use B, KB, MB, GB, TB)").into(),
            ));
        }
    };
    value
        .checked_mul(mult)
        .ok_or_else(|| BlueprintError::InvalidVfs(format!("size '{raw}' overflows").into()))
}

/// Split a scalar into its leading ASCII-digit run and the trimmed remainder.
fn split_numeric(s: &str) -> (&str, &str) {
    let idx = s.find(|c: char| !c.is_ascii_digit()).unwrap_or(s.len());
    (&s[..idx], s[idx..].trim())
}

#[derive(Debug)]
pub enum BlueprintError {
    Empty,
    Parse(Fault),
    InvalidKind(Fault),
    InvalidName(Fault),
    InvalidVfs(Fault),
    InvalidPackages(Fault),
    InvalidSecrets(Fault),
    InvalidVariables(Fault),
    InvalidGit(Fault),
    InvalidAuthProxy(Fault),
    InvalidPermissions(Fault),
    InvalidMcp(Fault),
    InvalidLlm(Fault),
}

impl BlueprintError {
    /// The structured payload — message plus YAML path/location when known.
    pub fn fault(&self) -> Option<&Fault> {
        match self {
            BlueprintError::Empty => None,
            BlueprintError::Parse(fault)
            | BlueprintError::InvalidKind(fault)
            | BlueprintError::InvalidName(fault)
            | BlueprintError::InvalidVfs(fault)
            | BlueprintError::InvalidPackages(fault)
            | BlueprintError::InvalidSecrets(fault)
            | BlueprintError::InvalidVariables(fault)
            | BlueprintError::InvalidGit(fault)
            | BlueprintError::InvalidAuthProxy(fault)
            | BlueprintError::InvalidPermissions(fault)
            | BlueprintError::InvalidMcp(fault)
            | BlueprintError::InvalidLlm(fault) => Some(fault),
        }
    }
}

impl fmt::Display for BlueprintError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            BlueprintError::Empty => f.write_str("blueprint YAML is empty"),
            BlueprintError::Parse(fault) => write!(f, "blueprint parse error: {}", fault.message),
            BlueprintError::InvalidKind(fault) => {
                write!(f, "invalid blueprint kind: {}", fault.message)
            }
            BlueprintError::InvalidName(fault) => {
                write!(f, "invalid blueprint name: {}", fault.message)
            }
            BlueprintError::InvalidVfs(fault) => write!(f, "invalid vfs config: {}", fault.message),
            BlueprintError::InvalidPackages(fault) => {
                write!(f, "invalid packages config: {}", fault.message)
            }
            BlueprintError::InvalidSecrets(fault) => {
                write!(f, "invalid secrets config: {}", fault.message)
            }
            BlueprintError::InvalidVariables(fault) => {
                write!(f, "invalid variables config: {}", fault.message)
            }
            BlueprintError::InvalidGit(fault) => write!(f, "invalid git config: {}", fault.message),
            BlueprintError::InvalidAuthProxy(fault) => {
                write!(f, "invalid auth_proxy config: {}", fault.message)
            }
            BlueprintError::InvalidPermissions(fault) => {
                write!(f, "invalid permissions config: {}", fault.message)
            }
            BlueprintError::InvalidMcp(fault) => write!(f, "invalid mcp config: {}", fault.message),
            BlueprintError::InvalidLlm(fault) => write!(f, "invalid llm config: {}", fault.message),
        }
    }
}

impl std::error::Error for BlueprintError {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_minimal_blueprint() {
        let b = parse("name: production\n").expect("valid blueprint");
        assert_eq!(b.name, "production");
        assert_eq!(b.vfs, VfsConfig::default());
        assert_eq!(b.vfs.mode_str(), "ephemeral");
    }

    #[test]
    fn accepts_explicit_blueprint_kind() {
        let b = parse("kind: blueprint\nname: production\n").expect("valid blueprint");
        assert_eq!(b.kind.as_deref(), Some("blueprint"));
        assert_eq!(b.name, "production");
    }

    #[test]
    fn rejects_unknown_kind() {
        let err = parse("kind: deployment\nname: x\n").unwrap_err();
        assert!(err.to_string().contains("unknown kind 'deployment'"));
    }

    #[test]
    fn kindless_roundtrip_stays_kindless() {
        let b = parse("name: production\n").unwrap();
        assert_eq!(b.kind, None);
        assert!(!to_yaml(&b).contains("kind"));
    }

    #[test]
    fn name_with_dashes_and_underscores() {
        assert_eq!(parse("name: prod-1\n").unwrap().name, "prod-1");
        assert_eq!(parse("name: prod_2\n").unwrap().name, "prod_2");
    }

    #[test]
    fn rejects_empty() {
        assert!(matches!(parse(""), Err(BlueprintError::Empty)));
        assert!(matches!(parse("   \n"), Err(BlueprintError::Empty)));
    }

    #[test]
    fn rejects_missing_name() {
        let err = parse("other: 1\n").unwrap_err();
        assert!(matches!(err, BlueprintError::Parse(_)), "got {err:?}");
    }

    #[test]
    fn rejects_unknown_top_level_field() {
        let err = parse("name: production\nbogus: {}\n").unwrap_err();
        assert!(matches!(err, BlueprintError::Parse(_)), "got {err:?}");
    }

    #[test]
    fn rejects_unknown_field_inside_vfs() {
        let err = parse("name: production\nvfs:\n  mode: none\n  bogus: 1\n").unwrap_err();
        assert!(matches!(err, BlueprintError::Parse(_)), "got {err:?}");
        // Not the untagged-enum collapse ("data did not match any variant"):
        // the key is named and the fault anchors on it.
        let fault = err.fault().expect("fault");
        assert!(
            fault.message.contains("unknown field `bogus`"),
            "got {fault:?}"
        );
        assert_eq!(fault.path.as_deref(), Some(&yaml_path!["vfs", "bogus"][..]));
    }

    #[test]
    fn rejects_invalid_name_chars() {
        let err = parse("name: 'has space'\n").unwrap_err();
        assert!(matches!(err, BlueprintError::InvalidName(_)), "got {err:?}");
    }

    #[test]
    fn rejects_empty_name() {
        let err = parse("name: ''\n").unwrap_err();
        assert!(matches!(err, BlueprintError::InvalidName(_)), "got {err:?}");
    }

    #[test]
    fn shorthand_equals_map_form() {
        let short = parse("name: x\nvfs: per_session\n").unwrap();
        let long = parse("name: x\nvfs:\n  mode: per_session\n").unwrap();
        assert_eq!(short.vfs, long.vfs);
        assert!(matches!(short.vfs, VfsConfig::PerSession { .. }));
    }

    #[test]
    fn omitted_vfs_defaults_to_ephemeral() {
        assert!(matches!(
            parse("name: x\n").unwrap().vfs,
            VfsConfig::Ephemeral { .. }
        ));
    }

    #[test]
    fn explicit_none_mode() {
        assert_eq!(parse("name: x\nvfs: none\n").unwrap().vfs, VfsConfig::None);
    }

    #[test]
    fn omitted_idle_timeout_defaults_to_1d() {
        let b = parse("name: x\n").unwrap();
        assert_eq!(b.idle_timeout, Duration::from_secs(24 * 60 * 60));
    }

    const POLICY: &str = "\
name: prod
default: deny
permissions:
  main:
    - capability: stripe.com/charge
      filter: amount < 500 and currency == \"USD\"
      action: allow
    - capability: stripe.com/charge
      action: ask-human
    - capability: submilli/fs.read
      filter: path glob \"*.csv\"
      action: allow
  stripe.com/sdk:
    - capability: submilli/http.post
      filter: host == \"api.stripe.com\"
      action: allow
";

    #[test]
    fn parses_permissions_block() {
        let b = parse(POLICY).unwrap();
        assert_eq!(b.default_action, Some(DefaultAction::Deny));
        assert!(b.has_permission_policy());
        assert_eq!(b.permissions["main"].len(), 3);
        assert_eq!(b.permissions["main"][0].capability, "stripe.com/charge");
        assert_eq!(b.permissions["main"][0].action, Action::Allow);
        assert_eq!(b.permissions["main"][1].action, Action::AskHuman);
        assert_eq!(b.permissions["stripe.com/sdk"].len(), 1);
    }

    #[test]
    fn permissions_round_trip() {
        let parsed = parse(POLICY).unwrap();
        let reparsed = parse(&to_yaml(&parsed)).unwrap();
        assert_eq!(parsed, reparsed);
    }

    #[test]
    fn omitted_policy_is_inactive() {
        let b = parse("name: x\n").unwrap();
        assert_eq!(b.default_action, None);
        assert!(b.permissions.is_empty());
        assert!(!b.has_permission_policy());
    }

    #[test]
    fn omitted_packages_defaults_to_empty() {
        let b = parse("name: x\n").unwrap();
        assert!(b.packages.is_empty());
    }

    #[test]
    fn parses_packages_list_form() {
        let b =
            parse("name: x\npackages:\n  - \"@submilli/http\"\n  - \"@acme/stripe\"\n").unwrap();
        assert!(b.packages.contains("@submilli/http"));
        assert!(b.packages.contains("@acme/stripe"));
    }

    #[test]
    fn parses_packages_map_form_and_ignores_versions() {
        let b = parse(
            "name: x\npackages:\n  \"@submilli/http\": \"1.0.0\"\n  \"@acme/stripe\": \"2.0.0\"\n",
        )
        .unwrap();
        assert_eq!(b.packages.len(), 2);
        assert!(b.packages.contains("@submilli/http"));
        assert!(b.packages.contains("@acme/stripe"));
    }

    #[test]
    fn packages_round_trip_as_list_form() {
        let parsed = parse("name: x\npackages:\n  \"@submilli/http\": \"1.0.0\"\n").unwrap();
        let yaml = to_yaml(&parsed);
        assert!(yaml.contains("- '@submilli/http'") || yaml.contains("- \"@submilli/http\""));
        let reparsed = parse(&yaml).unwrap();
        assert_eq!(parsed, reparsed);
    }

    #[test]
    fn rejects_duplicate_packages_in_list_form() {
        let err = parse("name: x\npackages:\n  - \"@submilli/http\"\n  - \"@submilli/http\"\n")
            .unwrap_err();
        assert!(matches!(err, BlueprintError::Parse(_)), "got {err:?}");
    }

    #[test]
    fn rejects_stdlib_host_module_in_packages() {
        let err = parse("name: x\npackages:\n  - \"submilli:http\"\n").unwrap_err();
        assert!(
            matches!(err, BlueprintError::InvalidPackages(_)),
            "got {err:?}"
        );
    }

    #[test]
    fn rejects_mcp_package_in_packages() {
        let err = parse("name: x\npackages:\n  - \"@mcp/linear\"\n").unwrap_err();
        assert!(
            matches!(err, BlueprintError::InvalidPackages(_)),
            "got {err:?}"
        );
    }

    #[test]
    fn rejects_unscoped_package_in_packages() {
        let err = parse("name: x\npackages:\n  - \"stripe\"\n").unwrap_err();
        assert!(
            matches!(err, BlueprintError::InvalidPackages(_)),
            "got {err:?}"
        );
    }

    #[test]
    fn bare_default_activates_policy() {
        // `default: deny` with no rules is still a configured policy.
        let b = parse("name: x\ndefault: deny\n").unwrap();
        assert_eq!(b.default_action, Some(DefaultAction::Deny));
        assert!(b.has_permission_policy());
    }

    #[test]
    fn accepts_default_allow() {
        let b = parse("name: x\ndefault: allow\n").unwrap();
        assert_eq!(b.default_action, Some(DefaultAction::Allow));
        assert!(b.has_permission_policy());
        // Allow-by-default: a capability with no matching rule resolves to allow.
        assert_eq!(
            b.resolve_permission(
                "main",
                "http.get",
                &serde_json::json!({}),
                &VarBindings::new()
            ),
            Action::Allow
        );
    }

    #[test]
    fn rejects_malformed_filter() {
        let yaml = "\
name: x
permissions:
  main:
    - capability: c
      filter: amount <> 5
      action: allow
";
        let err = parse(yaml).unwrap_err();
        assert!(matches!(err, BlueprintError::Parse(_)), "got {err:?}");
    }

    #[test]
    fn rejects_empty_capability() {
        let yaml = "\
name: x
permissions:
  main:
    - capability: ''
      action: allow
";
        let err = parse(yaml).unwrap_err();
        assert!(
            matches!(err, BlueprintError::InvalidPermissions(_)),
            "got {err:?}"
        );
    }

    #[test]
    fn resolve_permission_uses_default_when_caller_absent() {
        let b = parse(POLICY).unwrap();
        assert_eq!(
            b.resolve_permission(
                "unknown-pkg",
                "stripe.com/charge",
                &serde_json::json!({}),
                &VarBindings::new()
            ),
            Action::Deny
        );
        assert_eq!(
            b.resolve_permission(
                "main",
                "stripe.com/charge",
                &serde_json::json!({ "amount": 100, "currency": "USD" }),
                &VarBindings::new()
            ),
            Action::Allow
        );
    }

    #[test]
    fn resolve_permission_evaluates_combinators() {
        let b = parse(POLICY).unwrap();
        // Both conjuncts hold → the first charge rule allows.
        assert_eq!(
            b.resolve_permission(
                "main",
                "stripe.com/charge",
                &serde_json::json!({ "amount": 100, "currency": "USD" }),
                &VarBindings::new()
            ),
            Action::Allow
        );
        // Wrong currency fails the `and`, so the rule misses and the next
        // (filterless) charge rule asks a human.
        assert_eq!(
            b.resolve_permission(
                "main",
                "stripe.com/charge",
                &serde_json::json!({ "amount": 100, "currency": "EUR" }),
                &VarBindings::new()
            ),
            Action::AskHuman
        );
        // Over the limit also misses the first rule.
        assert_eq!(
            b.resolve_permission(
                "main",
                "stripe.com/charge",
                &serde_json::json!({ "amount": 900, "currency": "USD" }),
                &VarBindings::new()
            ),
            Action::AskHuman
        );
    }

    #[test]
    fn top_level_idle_timeout_applies_to_any_mode() {
        let b = parse("name: x\nidle_timeout: 1h\nvfs: none\n").unwrap();
        assert_eq!(b.idle_timeout, Duration::from_secs(3600));
        assert_eq!(b.vfs, VfsConfig::None);
    }

    #[test]
    fn idle_timeout_under_vfs_is_rejected() {
        let err = parse("name: x\nvfs:\n  mode: per_session\n  idle_timeout: 1h\n").unwrap_err();
        assert!(matches!(err, BlueprintError::Parse(_)), "got {err:?}");
    }

    #[test]
    fn per_session_full_block() {
        let b = parse("name: x\nvfs:\n  mode: per_session\n  size_limit: 100MB\n").unwrap();
        assert_eq!(b.vfs.size_limit(), Some(100 * 1024 * 1024));
    }

    #[test]
    fn per_session_tolerates_legacy_path_limit() {
        // Stored blueprints carried the never-enforced `path_limit`; it must still
        // parse (ignored), and it isn't written back.
        let b =
            parse("name: x\nvfs:\n  mode: per_session\n  size_limit: 1MB\n  path_limit: 1000\n")
                .unwrap();
        assert_eq!(b.vfs.size_limit(), Some(1024 * 1024));
        assert!(!to_yaml(&b).contains("path_limit"));
        let err =
            parse("name: x\nvfs:\n  mode: named\n  volume: w\n  path_limit: 10\n").unwrap_err();
        assert!(
            err.to_string().contains("'path_limit' is not valid"),
            "got {err}"
        );
    }

    #[test]
    fn per_session_tolerates_legacy_grace_period() {
        // Old blueprints carried `grace_period`; it must still parse (ignored)
        // rather than failing as an unknown field.
        let b = parse("name: x\nvfs:\n  mode: per_session\n  grace_period: 10m\n").unwrap();
        assert!(matches!(b.vfs, VfsConfig::PerSession { .. }));
        assert!(!to_yaml(&b).contains("grace_period"));
    }

    #[test]
    fn named_requires_volume() {
        let err = parse("name: x\nvfs:\n  mode: named\n").unwrap_err();
        assert!(matches!(err, BlueprintError::Parse(_)), "got {err:?}");
        assert!(err.to_string().contains("'volume'"), "got {err}");
        let ok = parse("name: x\nvfs:\n  mode: named\n  volume: workspace\n").unwrap();
        assert_eq!(
            ok.vfs,
            VfsConfig::Named {
                volume: "workspace".into(),
                access: None,
                mounts: Mounts::new(),
                cwd: None,
                sub_path: None,
            }
        );
    }

    #[test]
    fn named_rejects_the_retired_path_key() {
        let err = parse("name: x\nvfs:\n  mode: named\n  path: /data\n").unwrap_err();
        let fault = err.fault().expect("fault");
        assert!(fault.message.contains("`volume: <name>`"), "got {fault:?}");
        assert!(fault.message.contains("operator"), "got {fault:?}");
        assert_eq!(fault.path.as_deref(), Some(&yaml_path!["vfs", "path"][..]));
    }

    #[test]
    fn named_rejects_volume_and_path_together() {
        let err = parse("name: x\nvfs:\n  mode: named\n  volume: w\n  path: /data\n").unwrap_err();
        let fault = err.fault().expect("fault");
        assert!(fault.message.contains("retired"), "got {fault:?}");
        assert_eq!(fault.path.as_deref(), Some(&yaml_path!["vfs", "path"][..]));
    }

    #[test]
    fn the_retired_path_key_is_rejected_under_every_mode() {
        for mode in ["none", "ephemeral", "per_session"] {
            let err =
                parse(&format!("name: x\nvfs:\n  mode: {mode}\n  path: /data\n")).unwrap_err();
            let fault = err.fault().expect("fault");
            assert!(fault.message.contains("retired"), "{mode}: got {fault:?}");
            assert_eq!(
                fault.path.as_deref(),
                Some(&yaml_path!["vfs", "path"][..]),
                "{mode}"
            );
        }
    }

    #[test]
    fn a_volume_that_is_not_a_bare_string_is_rejected() {
        for value in ["7", "true", "null", "[a]", "{ name: a, subpath: b }"] {
            let err = parse(&format!(
                "name: x\nvfs:\n  mode: named\n  volume: {value}\n"
            ))
            .unwrap_err();
            let fault = err.fault().expect("fault");
            assert_eq!(
                fault.path.as_deref(),
                Some(&yaml_path!["vfs", "volume"][..]),
                "{value}"
            );
        }
        // Quoting makes a number-looking name usable, matching the config side,
        // where the same name is an ordinary map key.
        let ok = parse("name: x\nvfs:\n  mode: named\n  volume: \"7\"\n").unwrap();
        assert_eq!(
            ok.vfs,
            VfsConfig::Named {
                volume: "7".into(),
                access: None,
                mounts: Mounts::new(),
                cwd: None,
                sub_path: None,
            }
        );
    }

    #[test]
    fn empty_volume_is_rejected() {
        let err = parse("name: x\nvfs:\n  mode: named\n  volume: ''\n").unwrap_err();
        let fault = err.fault().expect("fault");
        assert!(fault.message.contains("empty"), "got {fault:?}");
        assert_eq!(
            fault.path.as_deref(),
            Some(&yaml_path!["vfs", "volume"][..])
        );
    }

    #[test]
    fn volume_is_rejected_outside_named_mode() {
        for mode in ["none", "ephemeral", "per_session"] {
            let err = parse(&format!("name: x\nvfs:\n  mode: {mode}\n  volume: w\n")).unwrap_err();
            let fault = err.fault().expect("fault");
            assert!(
                fault
                    .message
                    .contains(&format!("'volume' is not valid for vfs mode '{mode}'")),
                "{mode}: got {fault:?}"
            );
            // The fault anchors on `volume:`, not on the block or on `mode:`.
            assert_eq!(
                fault.path.as_deref(),
                Some(&yaml_path!["vfs", "volume"][..]),
                "{mode}: got {fault:?}"
            );
            assert_eq!(fault_line(fault), Some(4), "{mode}: got {fault:?}");
        }
    }

    #[test]
    fn shorthand_named_still_needs_a_volume() {
        let err = parse("name: x\nvfs: named\n").unwrap_err();
        assert!(err.to_string().contains("'volume'"), "got {err}");
    }

    #[test]
    fn named_rejects_limits() {
        for (key, line) in [
            ("size_limit", "size_limit: 1MB"),
            ("grace_period", "grace_period: 5m"),
        ] {
            let err = parse(&format!(
                "name: x\nvfs:\n  mode: named\n  volume: w\n  {line}\n"
            ))
            .unwrap_err();
            let fault = err.fault().expect("fault");
            assert!(
                fault
                    .message
                    .contains(&format!("'{key}' is not valid for vfs mode 'named'")),
                "{key}: got {fault:?}"
            );
            // The fault anchors on the offending key, not on the block or on
            // `mode:` at the top of it.
            assert_eq!(
                fault.path.as_deref(),
                Some(&yaml_path!["vfs", key][..]),
                "{key}: got {fault:?}"
            );
            assert_eq!(fault_line(fault), Some(5), "{key}: got {fault:?}");
        }
    }

    #[test]
    fn none_rejects_subfields() {
        let err = parse("name: x\nvfs:\n  mode: none\n  size_limit: 1MB\n").unwrap_err();
        let fault = err.fault().expect("fault");
        assert!(
            fault.message.contains("'size_limit' is not valid"),
            "got {fault:?}"
        );
        assert_eq!(
            fault.path.as_deref(),
            Some(&yaml_path!["vfs", "size_limit"][..])
        );
    }

    #[test]
    fn ephemeral_rejects_grace_period() {
        let err = parse("name: x\nvfs:\n  mode: ephemeral\n  grace_period: 5m\n").unwrap_err();
        let fault = err.fault().expect("fault");
        assert!(
            fault.message.contains("'grace_period' is not valid"),
            "got {fault:?}"
        );
        assert_eq!(
            fault.path.as_deref(),
            Some(&yaml_path!["vfs", "grace_period"][..])
        );
    }

    #[test]
    fn ephemeral_allows_limits() {
        let b = parse("name: x\nvfs:\n  mode: ephemeral\n  size_limit: 10MB\n").unwrap();
        assert_eq!(b.vfs.size_limit(), Some(10 * 1024 * 1024));
    }

    #[test]
    fn bad_mode_string_is_parse_error() {
        let err = parse("name: x\nvfs: bogus\n").unwrap_err();
        assert!(matches!(err, BlueprintError::Parse(_)), "got {err:?}");
    }

    #[test]
    fn size_accepts_bare_byte_count() {
        let b = parse("name: x\nvfs:\n  mode: ephemeral\n  size_limit: 4096\n").unwrap();
        assert_eq!(b.vfs.size_limit(), Some(4096));
    }

    #[test]
    fn round_trips_through_yaml() {
        let original =
            parse("name: x\nidle_timeout: 1h\nvfs:\n  mode: per_session\n  size_limit: 100MB\n")
                .unwrap();
        let reparsed = parse(&to_yaml(&original)).unwrap();
        assert_eq!(original, reparsed);
        assert_eq!(reparsed.idle_timeout, Duration::from_secs(3600));
    }

    #[test]
    fn default_idle_timeout_omitted_from_yaml() {
        let b = parse("name: x\n").unwrap();
        assert!(
            !to_yaml(&b).contains("idle_timeout"),
            "default idle_timeout should be omitted: {}",
            to_yaml(&b)
        );
    }

    #[test]
    fn round_trips_named() {
        let original = parse("name: x\nvfs:\n  mode: named\n  volume: workspaces\n").unwrap();
        assert!(to_yaml(&original).contains("volume: workspaces"));
        let reparsed = parse(&to_yaml(&original)).unwrap();
        assert_eq!(original, reparsed);
    }

    #[test]
    fn round_trips_every_unnamed_mode() {
        for src in [
            "name: x\nvfs: none\n",
            "name: x\nvfs: ephemeral\n",
            "name: x\nvfs: per_session\n",
            "name: x\nvfs:\n  mode: ephemeral\n  size_limit: 10MB\n",
            "name: x\nvfs:\n  mode: per_session\n  size_limit: 10MB\n",
        ] {
            let original = parse(src).unwrap();
            let reparsed = parse(&to_yaml(&original)).unwrap();
            assert_eq!(original, reparsed, "{src}");
        }
    }

    #[test]
    fn round_trips_none() {
        let original = parse("name: x\nvfs: none\n").unwrap();
        let reparsed = parse(&to_yaml(&original)).unwrap();
        assert_eq!(original, reparsed);
    }

    #[test]
    fn name_only_omits_vfs_in_yaml() {
        let b = parse("name: x\n").unwrap();
        let yaml = to_yaml(&b);
        assert!(
            !yaml.contains("vfs"),
            "default vfs should be omitted: {yaml}"
        );
    }

    #[test]
    fn parse_duration_table() {
        assert_eq!(parse_duration("5m").unwrap(), Duration::from_secs(300));
        assert_eq!(parse_duration("30m").unwrap(), Duration::from_secs(1800));
        assert_eq!(parse_duration("90s").unwrap(), Duration::from_secs(90));
        assert_eq!(parse_duration("1h").unwrap(), Duration::from_secs(3600));
        assert!(parse_duration("5").is_err(), "missing unit");
        assert!(parse_duration("5d").is_err(), "unknown unit");
        assert!(parse_duration("xm").is_err(), "non-numeric");
    }

    #[test]
    fn parse_size_table() {
        assert_eq!(parse_size("100MB").unwrap(), 100 * 1024 * 1024);
        assert_eq!(parse_size("512KB").unwrap(), 512 * 1024);
        assert_eq!(parse_size("2GB").unwrap(), 2 * 1024 * 1024 * 1024);
        assert_eq!(parse_size("4096").unwrap(), 4096);
        assert_eq!(parse_size("4096B").unwrap(), 4096);
        assert_eq!(parse_size("1MiB").unwrap(), 1024 * 1024);
        assert!(parse_size("10XB").is_err(), "unknown unit");
    }

    #[test]
    fn omitted_secrets_and_auth_proxy_default_empty() {
        let b = parse("name: x\n").unwrap();
        assert!(b.secrets.is_empty());
        assert!(b.auth_proxy.is_empty());
    }

    #[test]
    fn parses_store_secret() {
        let b = parse("name: x\nsecrets:\n  A: { store: prod/a }\n").unwrap();
        assert_eq!(b.secrets["A"], SecretSource::Store("prod/a".into()));
    }

    #[test]
    fn parses_structured_harness_secret() {
        let b = parse("name: x\nsecrets:\n  A:\n    harness:\n      required: true\n").unwrap();
        assert_eq!(
            b.secrets["A"],
            SecretSource::Harness(HarnessSecret { required: true })
        );
    }

    #[test]
    fn rejects_boolean_harness_secret() {
        let err = parse("name: x\nsecrets:\n  A: { harness: true }\n").unwrap_err();
        assert!(matches!(err, BlueprintError::Parse(_)), "got {err:?}");
    }

    #[test]
    fn unknown_secret_source_is_parse_error() {
        for source in ["vault", "env", "file"] {
            let yaml = format!("name: x\nsecrets:\n  A: {{ {source}: x }}\n");
            let err = parse(&yaml).unwrap_err();
            let fault = err.fault().unwrap();
            assert!(fault.message.contains("unknown field"), "{err}");
            assert_eq!(fault.path, Some(yaml_path!["secrets", "A", source]));
            assert!(fault.location.is_some());
        }
    }

    #[test]
    fn secret_needs_exactly_one_source() {
        for declaration in [
            "{}",
            "{ store: null }",
            "{ harness: null }",
            "{ store: key, harness: {} }",
        ] {
            let yaml = format!("name: x\nsecrets:\n  A: {declaration}\n");
            let err = parse(&yaml).unwrap_err();
            assert!(
                err.to_string()
                    .contains("exactly one source: store / harness"),
                "{err}"
            );
        }
    }

    #[test]
    fn parses_auth_proxy_rule() {
        let b = parse(
            "name: x\nsecrets:\n  K: { store: K }\nauth_proxy:\n  - host: api.example.com\n    headers:\n      Authorization: \"Bearer ${secrets.K}\"\n",
        )
        .unwrap();
        assert_eq!(b.auth_proxy.len(), 1);
        assert_eq!(b.auth_proxy[0].host, "api.example.com");
    }

    #[test]
    fn auth_proxy_undeclared_secret_rejected() {
        let err = parse(
            "name: x\nauth_proxy:\n  - host: api.example.com\n    headers:\n      Authorization: \"Bearer ${secrets.NOPE}\"\n",
        )
        .unwrap_err();
        assert!(
            matches!(err, BlueprintError::InvalidAuthProxy(_)),
            "got {err:?}"
        );
    }

    #[test]
    fn auth_proxy_rule_needs_headers_or_query() {
        let err = parse("name: x\nauth_proxy:\n  - host: api.example.com\n").unwrap_err();
        assert!(
            matches!(err, BlueprintError::InvalidAuthProxy(_)),
            "got {err:?}"
        );
    }

    #[test]
    fn auth_proxy_round_trips_through_yaml() {
        let original = parse(
            "name: x\nsecrets:\n  K: { store: K_VAR }\nauth_proxy:\n  - host: api.example.com\n    headers:\n      Authorization: \"Bearer ${secrets.K}\"\n    query:\n      appid: \"${secrets.K}\"\n",
        )
        .unwrap();
        let reparsed = parse(&to_yaml(&original)).unwrap();
        assert_eq!(original, reparsed);
    }

    #[track_caller]
    fn fault_path(yaml: &str) -> YamlPath {
        parse(yaml)
            .unwrap_err()
            .fault()
            .expect("fault")
            .path
            .clone()
            .expect("path")
    }

    #[test]
    fn kind_name_and_packages_faults_carry_paths() {
        assert_eq!(
            fault_path("kind: deployment\nname: x\n"),
            yaml_path!["kind"]
        );
        assert_eq!(fault_path("name: 'has space'\n"), yaml_path!["name"]);
        assert_eq!(
            fault_path("name: x\npackages:\n  - unscoped\n"),
            yaml_path!["packages"]
        );
    }

    #[test]
    fn empty_capability_fault_points_at_the_rule() {
        let yaml = "\
name: x
permissions:
  main:
    - capability: fs.read
      action: allow
    - capability: \"\"
      action: allow
";
        assert_eq!(
            fault_path(yaml),
            yaml_path!["permissions", "main", 1_usize, "capability"]
        );
    }

    #[test]
    fn undeclared_variable_fault_points_at_the_filter() {
        let yaml = "\
name: x
permissions:
  main:
    - capability: db.query
      filter: userId == ${vars.tenant}
      action: allow
";
        assert_eq!(
            fault_path(yaml),
            yaml_path!["permissions", "main", 0_usize, "filter"]
        );
    }

    #[test]
    fn contradictory_variable_fault_points_at_the_declaration() {
        assert_eq!(
            fault_path("name: x\nvariables:\n  t:\n    required: true\n    default: d\n"),
            yaml_path!["variables", "t"]
        );
    }

    #[test]
    fn mcp_faults_carry_paths() {
        assert_eq!(
            fault_path("name: x\nmcp:\n  linear:\n    transport: stdio\n    url: https://x\n"),
            yaml_path!["mcp", "linear", "transport"]
        );
        let dangling = "\
name: x
permissions:
  main:
    - capability: mcp.ghost
      action: allow
";
        assert_eq!(
            fault_path(dangling),
            yaml_path!["permissions", "main", 0_usize, "capability"]
        );
    }

    #[test]
    fn auth_proxy_faults_carry_paths() {
        assert_eq!(
            fault_path("name: x\nauth_proxy:\n  - host: h\n"),
            yaml_path!["auth_proxy", 0_usize]
        );
        assert_eq!(
            fault_path(
                "name: x\nauth_proxy:\n  - host: h\n    auth:\n      basic:\n        \
                 username: alice\n        password: MISSING\n"
            ),
            yaml_path!["auth_proxy", 0_usize, "auth", "basic", "password"]
        );
        assert_eq!(
            fault_path(
                "name: x\nauth_proxy:\n  - host: h\n    query:\n      appid: \"${secrets.NOPE}\"\n"
            ),
            yaml_path!["auth_proxy", 0_usize, "query", "appid"]
        );
    }

    #[test]
    fn deserialize_faults_carry_serde_paths() {
        let filter = "\
name: x
permissions:
  main:
    - capability: c
      filter: amount <> 5
      action: allow
";
        assert_eq!(
            fault_path(filter),
            yaml_path!["permissions", "main", 0_usize, "filter"]
        );
        assert_eq!(
            fault_path("name: x\nsecrets:\n  A: { vault: x }\n"),
            yaml_path!["secrets", "A", "vault"]
        );
        assert_eq!(fault_path("name: x\nvfs: bogus\n"), yaml_path!["vfs"]);
    }

    #[test]
    fn yaml_syntax_error_fault_carries_location() {
        let err = parse("name: x\nvfs: [\n").unwrap_err();
        let fault = err.fault().expect("fault");
        assert!(fault.location.is_some(), "got {fault:?}");
    }

    #[test]
    fn multi_document_yaml_is_still_rejected() {
        let err = parse("name: x\n---\nname: y\n").unwrap_err();
        assert!(matches!(err, BlueprintError::Parse(_)), "got {err:?}");
    }

    /// The 1-based line a fault points at. Only the line is asserted: the
    /// column is the YAML parser's mark for the offending *value*, which moves
    /// with the key's length.
    fn fault_line(fault: &Fault) -> Option<usize> {
        fault.location.map(|(line, _)| line)
    }

    #[test]
    fn duplicate_vfs_key_is_rejected_at_the_repeat() {
        let err =
            parse("name: x\nvfs:\n  mode: ephemeral\n  mode: named\n  volume: w\n").unwrap_err();
        let fault = err.fault().expect("fault");
        assert!(
            fault.message.contains("duplicate key `mode`"),
            "got {fault:?}"
        );
        assert_eq!(fault.path.as_deref(), Some(&yaml_path!["vfs", "mode"][..]));
        assert_eq!(fault_line(fault), Some(4), "got {fault:?}");
    }

    #[test]
    fn every_duplicated_vfs_key_is_rejected() {
        // Each key is duplicated under a mode that accepts it, so what fails
        // is the repetition and not a mode conflict.
        for (key, mode, line) in [
            ("mode", "per_session", "mode: per_session"),
            ("volume", "named", "volume: w"),
            ("size_limit", "per_session", "size_limit: 1MB"),
            ("grace_period", "per_session", "grace_period: 5m"),
        ] {
            let yaml = format!("name: x\nvfs:\n  mode: {mode}\n  {line}\n  {line}\n");
            let err = parse(&yaml).unwrap_err();
            let fault = err.fault().expect("fault");
            assert!(
                fault.message.contains(&format!("duplicate key `{key}`")),
                "{key}: got {fault:?}"
            );
            assert_eq!(
                fault.path.as_deref(),
                Some(&yaml_path!["vfs", key][..]),
                "{key}"
            );
        }
    }

    #[test]
    fn unknown_vfs_key_message_admits_grace_period() {
        let err = parse("name: x\nvfs:\n  mode: none\n  bogus: 1\n").unwrap_err();
        let fault = err.fault().expect("fault");
        assert!(
            fault.message.contains("grace_period"),
            "the accepted-key list must not hide the key the parser tolerates: {fault:?}"
        );
        assert!(fault.message.contains("ignored"), "got {fault:?}");
    }

    #[test]
    fn volume_without_an_explicit_mode_names_the_missing_mode() {
        let err = parse("name: x\nvfs:\n  volume: w\n").unwrap_err();
        let fault = err.fault().expect("fault");
        assert!(
            fault.message.contains("mode: named"),
            "the message must name the edit: {fault:?}"
        );
        assert!(
            fault.message.contains("no `mode:`"),
            "the message must say the mode was never written: {fault:?}"
        );
    }

    #[test]
    fn persistent_is_removed_with_a_migration_hint() {
        for yaml in [
            "name: x\nvfs:\n  mode: persistent\n  volume: notes\n",
            "name: x\nvfs: persistent\n",
        ] {
            let err = parse(yaml).unwrap_err();
            let fault = err.fault().expect("fault");
            assert!(
                fault.message.contains("`persistent` was removed")
                    && fault.message.contains("mode: named"),
                "{yaml}: got {fault:?}"
            );
        }
        let err = parse("name: x\nvfs:\n  mode: persistent\n  volume: notes\n").unwrap_err();
        let fault = err.fault().expect("fault");
        assert_eq!(fault.path.as_deref(), Some(&yaml_path!["vfs", "mode"][..]));
        assert_eq!(fault_line(fault), Some(3), "anchors at `mode:`");
    }

    #[test]
    fn named_roots_take_an_optional_access() {
        let b =
            parse("name: x\nvfs:\n  mode: named\n  volume: notes\n  access: read_only\n").unwrap();
        assert_eq!(
            b.vfs,
            VfsConfig::Named {
                volume: "notes".into(),
                access: Some(Access::ReadOnly),
                mounts: Mounts::new(),
                cwd: None,
                sub_path: None,
            }
        );
        assert_eq!(b.vfs.mode_str(), "named");
        let err =
            parse("name: x\nvfs:\n  mode: named\n  volume: notes\n  access: write\n").unwrap_err();
        assert_eq!(
            err.fault().expect("fault").path.as_deref(),
            Some(&yaml_path!["vfs", "access"][..])
        );
        for mode in ["ephemeral", "per_session", "none"] {
            let err = parse(&format!(
                "name: x\nvfs:\n  mode: {mode}\n  access: read_only\n"
            ))
            .unwrap_err();
            assert!(
                err.to_string()
                    .contains(&format!("'access' is not valid for vfs mode '{mode}'")),
                "{mode}: got {err}"
            );
        }
    }

    #[test]
    fn mounts_parse_under_every_mode_with_a_root() {
        let yaml = "name: x\nvfs:\n  mode: per_session\n  size_limit: 1MB\n  mounts:\n    /memory:\n      mode: named\n      volume: project-memory\n      access: read_write\n    /handbook:\n      mode: named\n      volume: company-handbook\n";
        let b = parse(yaml).unwrap();
        let VfsConfig::PerSession {
            size_limit, mounts, ..
        } = &b.vfs
        else {
            panic!("per_session: {:?}", b.vfs);
        };
        assert_eq!(*size_limit, Some(1024 * 1024));
        assert_eq!(
            mounts.get("/memory"),
            Some(&MountConfig {
                volume: "project-memory".into(),
                access: Some(Access::ReadWrite),
                sub_path: None,
            })
        );
        assert_eq!(
            mounts.get("/handbook"),
            Some(&MountConfig {
                volume: "company-handbook".into(),
                access: None,
                sub_path: None,
            })
        );
        let references = b.vfs.named_references();
        assert_eq!(references.len(), 2);
        assert_eq!(references[0].mount, Some("/handbook"));
        assert_eq!(
            references[1].yaml_path("volume"),
            yaml_path!["vfs", "mounts", "/memory", "volume"]
        );
        // The default root accepts mounts without writing `mode:`.
        let b = parse("name: x\nvfs:\n  mounts:\n    /m: {mode: named, volume: m}\n").unwrap();
        assert!(matches!(b.vfs, VfsConfig::Ephemeral { .. }));
        assert_eq!(b.vfs.mounts().len(), 1);
        // A named root takes mounts too, and lists itself first.
        let b = parse(
            "name: x\nvfs:\n  mode: named\n  volume: root\n  mounts:\n    /m: {mode: named, volume: m}\n",
        )
        .unwrap();
        assert_eq!(b.vfs.named_references()[0].mount, None);
        assert_eq!(b.vfs.named_references()[1].mount, Some("/m"));
    }

    #[test]
    fn mounts_round_trip() {
        for src in [
            "name: x\nvfs:\n  mounts:\n    /m: {mode: named, volume: m, access: read_only}\n",
            "name: x\nvfs:\n  mode: per_session\n  mounts:\n    /a/b: {mode: named, volume: m}\n",
            "name: x\nvfs:\n  mode: named\n  volume: r\n  access: read_only\n  mounts:\n    /m: {mode: named, volume: m}\n",
        ] {
            let original = parse(src).unwrap();
            let yaml = to_yaml(&original);
            let reparsed = parse(&yaml).unwrap();
            assert_eq!(original, reparsed, "{src}\n{yaml}");
        }
    }

    #[test]
    fn mounts_need_a_root() {
        let err =
            parse("name: x\nvfs:\n  mode: none\n  mounts:\n    /m: {mode: named, volume: m}\n")
                .unwrap_err();
        let fault = err.fault().expect("fault");
        assert!(
            fault.message.contains("needs a filesystem root"),
            "got {fault:?}"
        );
        assert_eq!(
            fault.path.as_deref(),
            Some(&yaml_path!["vfs", "mounts"][..])
        );
    }

    #[test]
    fn mount_paths_must_be_normalized_absolute_and_disjoint() {
        for (path, expected) in [
            ("memory", "must be absolute"),
            ("/", "cannot be mounted at `/`"),
            ("/a/", "not normalized"),
            ("/a//b", "not normalized"),
            ("/a/./b", "not normalized"),
            ("/a/../b", "not normalized"),
            ("/a/.git", "Git metadata"),
            ("/.GIT/x", "Git metadata"),
            ("/a\\b", "only ASCII letters"),
            ("/mémoire", "only ASCII letters"),
            ("/memory.", "ending in `.`"),
            ("/a b", "only ASCII letters"),
        ] {
            let yaml = format!(
                "name: x\nvfs:\n  mounts:\n    \"{}\": {{mode: named, volume: m}}\n",
                path.replace('\\', "\\\\")
            );
            let err = parse(&yaml).unwrap_err();
            let fault = err.fault().expect("fault");
            assert!(fault.message.contains(expected), "{path}: got {fault:?}");
            assert_eq!(
                fault.path.as_deref(),
                Some(&yaml_path!["vfs", "mounts", path][..]),
                "{path}"
            );
        }
        for (first, second, expected) in [
            ("/a", "/a/b", "inside mount `/a`"),
            ("/a/b", "/a", "inside mount `/a`"),
            ("/Memory", "/memory", "only in letter case"),
        ] {
            let yaml = format!(
                "name: x\nvfs:\n  mounts:\n    {first}: {{mode: named, volume: one}}\n    {second}: {{mode: named, volume: two}}\n"
            );
            let err = parse(&yaml).unwrap_err();
            let fault = err.fault().expect("fault");
            assert!(
                fault.message.contains(expected),
                "{first} {second}: got {fault:?}"
            );
            assert_eq!(fault_line(fault), Some(5), "anchors at the later mount");
        }
        let ok = parse(
            "name: x\nvfs:\n  mounts:\n    /a: {mode: named, volume: one}\n    /ab: {mode: named, volume: two}\n",
        )
        .unwrap();
        assert_eq!(ok.vfs.mounts().len(), 2, "a shared prefix is not nesting");
    }

    #[test]
    fn mount_entries_are_checked() {
        for (entry, expected) in [
            ("{mode: ephemeral, volume: m}", "must be `named`"),
            ("{mode: persistent, volume: m}", "`persistent` was removed"),
            ("{volume: m}", "needs `mode: named`"),
            ("{mode: named}", "needs a `volume`"),
            (
                "{mode: named, volume: m, size_limit: 1MB}",
                "unknown field `size_limit`",
            ),
            ("{mode: named, volume: m, access: all}", "unknown variant"),
            ("{mode: named, volume: ''}", "empty"),
            ("/data", "a mount"),
        ] {
            let yaml = format!("name: x\nvfs:\n  mounts:\n    /m: {entry}\n");
            let err = parse(&yaml).unwrap_err();
            assert!(err.to_string().contains(expected), "{entry}: got {err}");
        }
    }

    #[test]
    fn a_volume_can_be_used_more_than_once() {
        for yaml in [
            "name: x\nvfs:\n  mounts:\n    /a: {mode: named, volume: v}\n    /b: {mode: named, volume: v}\n",
            "name: x\nvfs:\n  mode: named\n  volume: v\n  mounts:\n    /b: {mode: named, volume: v}\n",
        ] {
            let blueprint = parse(yaml).unwrap();
            assert_eq!(parse(&to_yaml(&blueprint)).unwrap().vfs, blueprint.vfs);
        }
    }

    #[test]
    fn too_many_mounts_are_refused() {
        let mut yaml = String::from("name: x\nvfs:\n  mounts:\n");
        for index in 0..=MAX_MOUNTS {
            yaml.push_str(&format!(
                "    /m{index}: {{mode: named, volume: v{index}}}\n"
            ));
        }
        let err = parse(&yaml).unwrap_err();
        assert!(err.to_string().contains("more than"), "got {err}");
    }

    #[test]
    fn a_size_limit_under_a_named_root_points_at_the_server() {
        let err =
            parse("name: x\nvfs:\n  mode: named\n  volume: v\n  size_limit: 1MB\n").unwrap_err();
        assert!(err.to_string().contains("set by the operator"), "got {err}");
    }
}

#[cfg(test)]
mod insecure_http_tests {
    use super::*;

    #[test]
    fn insecure_http_flags_default_false_and_round_trip_independently() {
        let base =
            "name: transport\nauth_proxy:\n- host: localhost\n  headers: { X-Test: value }\n";
        let omitted = parse(base).unwrap();
        assert!(!omitted.allow_insecure_http);
        assert!(!omitted.auth_proxy[0].allow_insecure_http);
        assert!(
            !serde_yml::to_string(&omitted)
                .unwrap()
                .contains("allow_insecure_http")
        );
        for blueprint in [false, true] {
            for rule in [false, true] {
                let yaml = format!(
                    "{base}  allow_insecure_http: {rule}\nallow_insecure_http: {blueprint}\n"
                );
                let parsed = parse(&yaml).unwrap();
                assert_eq!(parsed.allow_insecure_http, blueprint);
                assert_eq!(parsed.auth_proxy[0].allow_insecure_http, rule);
                assert_eq!(
                    parse(&serde_yml::to_string(&parsed).unwrap()).unwrap(),
                    parsed
                );
            }
        }
        assert!(parse(&format!("{base}allow_insecure_http: sometimes\n")).is_err());
        assert!(parse(&format!("{base}  allow_insecure_http: sometimes\n")).is_err());
    }
}
