//! Session consistency rules. No storage, transport, clock reads, or filesystem IO.
use std::collections::BTreeMap;
use std::path::{Component, Path, PathBuf};
use std::time::{Duration, SystemTime};

use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RootVfsType {
    #[default]
    None,
    Ephemeral,
    PerSession,
    Named,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ClosedReason {
    Unknown,
    Deleted,
    Expired,
    BlueprintRemoved,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum SessionStatus {
    #[default]
    Active,
    Closed(ClosedReason),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SessionId(String);

impl SessionId {
    pub fn parse(value: String) -> Result<Self, SessionRuleError> {
        let mut parts = Path::new(&value).components();
        if value.contains(['\0', '\\'])
            || !matches!(parts.next(), Some(Component::Normal(part)) if part == std::ffi::OsStr::new(&value))
            || parts.next().is_some()
        {
            return Err(SessionRuleError::InvalidIdentity);
        }
        Ok(Self(value))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SessionBinding {
    blueprint: String,
    variables: BTreeMap<String, String>,
}

impl SessionBinding {
    pub fn new(
        blueprint: String,
        variables: BTreeMap<String, String>,
    ) -> Result<Self, SessionRuleError> {
        if blueprint.is_empty() {
            return Err(SessionRuleError::MissingBlueprint);
        }
        Ok(Self {
            blueprint,
            variables,
        })
    }
    pub fn blueprint(&self) -> &str {
        &self.blueprint
    }
    pub fn variables(&self) -> &BTreeMap<String, String> {
        &self.variables
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RootVfs {
    None,
    Ephemeral,
    PerSession {
        path: PathBuf,
    },
    Named {
        path: PathBuf,
    },
    /// Old JSON recorded only an ownership boolean. Resolve before using a root.
    LegacyUnresolved,
}

impl RootVfs {
    pub fn kind(&self) -> Option<RootVfsType> {
        match self {
            Self::None => Some(RootVfsType::None),
            Self::Ephemeral => Some(RootVfsType::Ephemeral),
            Self::PerSession { .. } => Some(RootVfsType::PerSession),
            Self::Named { .. } => Some(RootVfsType::Named),
            Self::LegacyUnresolved => None,
        }
    }
    pub fn path(&self) -> Option<&Path> {
        match self {
            Self::PerSession { path } | Self::Named { path } => Some(path),
            Self::None | Self::Ephemeral | Self::LegacyUnresolved => None,
        }
    }
    fn validate(&self) -> Result<(), SessionRuleError> {
        if self.path().is_some_and(|path| path.as_os_str().is_empty()) {
            return Err(SessionRuleError::InvalidRoot);
        }
        Ok(())
    }
    pub fn owned_path(&self) -> Option<&Path> {
        match self {
            Self::PerSession { path } => Some(path),
            _ => None,
        }
    }
}

/// Persisted credentials can remain unresolved during lifecycle operations.
/// Their encrypted representation belongs to the persistence adapter.
#[derive(Clone, Default)]
pub enum CredentialBinding {
    #[default]
    Absent,
    Persisted,
    Supplied(std::sync::Arc<submilli_blueprint::HarnessSecretBindings>),
}

impl std::fmt::Debug for CredentialBinding {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("CredentialBinding([redacted])")
    }
}

#[derive(Clone, Debug)]
pub struct SessionLifetime {
    idle_timeout: Duration,
    last_activity: SystemTime,
}

impl SessionLifetime {
    pub fn new(idle_timeout: Duration, last_activity: SystemTime) -> Self {
        Self {
            idle_timeout,
            last_activity,
        }
    }
    pub fn idle_timeout(&self) -> Duration {
        self.idle_timeout
    }
    pub fn last_activity(&self) -> SystemTime {
        self.last_activity
    }
    pub fn expired_at(&self, now: SystemTime) -> bool {
        now.duration_since(self.last_activity).unwrap_or_default() > self.idle_timeout
    }
}

#[derive(Clone, Debug)]
pub struct Session {
    id: SessionId,
    binding: SessionBinding,
    root: RootVfs,
    lifetime: SessionLifetime,
    status: SessionStatus,
    credentials: CredentialBinding,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SessionRuleError {
    InvalidIdentity,
    MissingBlueprint,
    Closed,
    Expired,
    BindingChanged,
    RootChanged,
    UnresolvedRoot,
    InvalidRoot,
}

impl std::fmt::Display for SessionRuleError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::InvalidIdentity => "invalid session identity",
            Self::MissingBlueprint => "session blueprint is missing",
            Self::Closed => "session is closed",
            Self::Expired => "session has expired",
            Self::BindingChanged => "session blueprint and variables cannot change",
            Self::RootChanged => "session root filesystem cannot change",
            Self::InvalidRoot => "session root path is empty",
            Self::UnresolvedRoot => "session root filesystem must be resolved before creation",
        })
    }
}
impl std::error::Error for SessionRuleError {}

impl Session {
    pub fn create(
        id: SessionId,
        binding: SessionBinding,
        root: RootVfs,
        lifetime: SessionLifetime,
    ) -> Result<Self, SessionRuleError> {
        if matches!(root, RootVfs::LegacyUnresolved) {
            return Err(SessionRuleError::UnresolvedRoot);
        }
        Self::restore(
            id,
            binding,
            root,
            lifetime,
            SessionStatus::Active,
            CredentialBinding::Absent,
        )
    }

    /// Restore historical state without replaying creation or emitting a new fact.
    pub fn restore(
        id: SessionId,
        binding: SessionBinding,
        root: RootVfs,
        lifetime: SessionLifetime,
        status: SessionStatus,
        credentials: CredentialBinding,
    ) -> Result<Self, SessionRuleError> {
        root.validate()?;
        Ok(Self {
            id,
            binding,
            root,
            lifetime,
            status,
            credentials,
        })
    }
    pub fn id(&self) -> &SessionId {
        &self.id
    }
    pub fn binding(&self) -> &SessionBinding {
        &self.binding
    }
    pub fn root(&self) -> &RootVfs {
        &self.root
    }
    pub fn lifetime(&self) -> &SessionLifetime {
        &self.lifetime
    }
    pub fn status(&self) -> SessionStatus {
        self.status
    }
    pub fn closed_reason(&self) -> Option<ClosedReason> {
        match self.status {
            SessionStatus::Closed(reason) => Some(reason),
            _ => None,
        }
    }
    pub fn require_active(&self) -> Result<(), SessionRuleError> {
        match self.status {
            SessionStatus::Active => Ok(()),
            SessionStatus::Closed(_) => Err(SessionRuleError::Closed),
        }
    }
    pub fn require_available(&self, now: SystemTime) -> Result<(), SessionRuleError> {
        self.require_active()?;
        if self.lifetime.expired_at(now) {
            return Err(SessionRuleError::Expired);
        }
        Ok(())
    }
    pub fn verify_blueprint(&self, name: &str) -> Result<(), SessionRuleError> {
        self.require_active()?;
        if self.binding.blueprint() != name {
            return Err(SessionRuleError::BindingChanged);
        }
        Ok(())
    }
    pub fn verify_binding(&self, binding: &SessionBinding) -> Result<(), SessionRuleError> {
        self.require_active()?;
        if &self.binding != binding {
            return Err(SessionRuleError::BindingChanged);
        }
        Ok(())
    }
    pub fn resolve_root(&mut self, root: RootVfs) -> Result<(), SessionRuleError> {
        self.require_active()?;
        if matches!(root, RootVfs::LegacyUnresolved) {
            return Err(SessionRuleError::UnresolvedRoot);
        }
        root.validate()?;
        if !matches!(self.root, RootVfs::LegacyUnresolved) && self.root != root {
            return Err(SessionRuleError::RootChanged);
        }
        self.root = root;
        Ok(())
    }
    pub fn record_activity(&mut self, now: SystemTime) -> Result<(), SessionRuleError> {
        self.require_active()?;
        self.lifetime.last_activity = self.lifetime.last_activity.max(now);
        Ok(())
    }
    pub fn rebind(
        &mut self,
        binding: SessionBinding,
        root: RootVfs,
        credentials: std::sync::Arc<submilli_blueprint::HarnessSecretBindings>,
        now: SystemTime,
    ) -> Result<(), SessionRuleError> {
        self.require_available(now)?;
        self.verify_binding(&binding)?;
        self.resolve_root(root)?;
        self.replace_credentials(credentials)
    }

    pub fn credentials(&self) -> &CredentialBinding {
        &self.credentials
    }

    pub fn replace_credentials(
        &mut self,
        credentials: std::sync::Arc<submilli_blueprint::HarnessSecretBindings>,
    ) -> Result<(), SessionRuleError> {
        self.require_active()?;
        self.credentials = if credentials.is_empty() {
            CredentialBinding::Absent
        } else {
            CredentialBinding::Supplied(credentials)
        };
        Ok(())
    }

    pub fn record_execution_started(&mut self, now: SystemTime) -> Result<(), SessionRuleError> {
        self.require_available(now)?;
        self.record_activity(now)
    }

    pub fn record_execution_completed(&mut self, now: SystemTime) -> Result<(), SessionRuleError> {
        self.record_activity(now)
    }

    pub fn expire(&mut self, now: SystemTime) -> bool {
        if matches!(self.status, SessionStatus::Closed(_)) || !self.lifetime.expired_at(now) {
            return false;
        }
        self.close(ClosedReason::Expired)
    }

    /// Reconcile persisted lifecycle facts after restart without reopening sessions.
    pub fn recover(&mut self, now: SystemTime) -> bool {
        self.expire(now)
    }

    /// Repeated closure preserves its cause. Resource disposal follows persistence.
    pub fn close(&mut self, reason: ClosedReason) -> bool {
        if matches!(self.status, SessionStatus::Closed(_)) {
            return false;
        }
        self.status = SessionStatus::Closed(reason);
        self.credentials = CredentialBinding::Absent;
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::UNIX_EPOCH;

    fn session(root: RootVfs) -> Session {
        Session::create(
            SessionId::parse("one".into()).unwrap(),
            SessionBinding::new(
                "blueprint".into(),
                BTreeMap::from([("tenant".into(), "ada".into())]),
            )
            .unwrap(),
            root,
            SessionLifetime::new(Duration::from_secs(60), UNIX_EPOCH),
        )
        .unwrap()
    }

    #[test]
    fn execution_activity_does_not_override_expiry_or_closure() {
        let mut active = session(RootVfs::None);
        let late = UNIX_EPOCH + Duration::from_secs(61);
        assert_eq!(
            active.record_execution_started(late),
            Err(SessionRuleError::Expired)
        );
        active
            .record_execution_started(UNIX_EPOCH + Duration::from_secs(30))
            .unwrap();
        active.record_execution_completed(UNIX_EPOCH).unwrap();
        assert_eq!(
            active.lifetime().last_activity(),
            UNIX_EPOCH + Duration::from_secs(30)
        );
        assert!(!active.expire(late));
        assert!(active.recover(late + Duration::from_secs(30)));
        assert_eq!(active.closed_reason(), Some(ClosedReason::Expired));
        assert!(!active.recover(late));
        assert_eq!(
            active.record_execution_completed(late),
            Err(SessionRuleError::Closed)
        );
    }

    #[test]
    fn credentials_are_replaced_and_cleared_by_the_aggregate() {
        let mut active = session(RootVfs::None);
        let values = std::sync::Arc::new(BTreeMap::from([(
            "TOKEN".into(),
            "domain-secret-canary".into(),
        )]));
        active.replace_credentials(values.clone()).unwrap();
        assert!(
            matches!(active.credentials(), CredentialBinding::Supplied(bound) if bound == &values)
        );
        assert!(!format!("{active:?}").contains("domain-secret-canary"));
        active.close(ClosedReason::Deleted);
        assert!(matches!(active.credentials(), CredentialBinding::Absent));
        assert_eq!(
            active.replace_credentials(values),
            Err(SessionRuleError::Closed)
        );
    }

    #[test]
    fn closure_is_terminal_and_preserves_original_reason() {
        let mut session = session(RootVfs::None);
        assert!(session.close(ClosedReason::Expired));
        assert!(!session.close(ClosedReason::Deleted));
        assert_eq!(session.closed_reason(), Some(ClosedReason::Expired));
        assert_eq!(
            session.record_activity(UNIX_EPOCH),
            Err(SessionRuleError::Closed)
        );
        assert_eq!(
            session.require_available(UNIX_EPOCH),
            Err(SessionRuleError::Closed)
        );
    }

    #[test]
    fn activity_is_monotonic_and_expiry_uses_persisted_activity() {
        let mut session = session(RootVfs::None);
        session
            .record_activity(UNIX_EPOCH + Duration::from_secs(30))
            .unwrap();
        session.record_activity(UNIX_EPOCH).unwrap();
        assert_eq!(
            session.lifetime().last_activity(),
            UNIX_EPOCH + Duration::from_secs(30)
        );
        assert!(
            session
                .require_available(UNIX_EPOCH + Duration::from_secs(90))
                .is_ok()
        );
        assert_eq!(
            session.require_available(UNIX_EPOCH + Duration::from_secs(91)),
            Err(SessionRuleError::Expired)
        );
    }

    #[test]
    fn binding_and_root_cannot_change() {
        let mut session = session(RootVfs::PerSession {
            path: "/sessions/one".into(),
        });
        assert_eq!(
            session
                .verify_binding(&SessionBinding::new("another".into(), BTreeMap::new()).unwrap()),
            Err(SessionRuleError::BindingChanged)
        );
        for root in [
            RootVfs::None,
            RootVfs::Ephemeral,
            RootVfs::Named {
                path: "/volume".into(),
            },
            RootVfs::PerSession {
                path: "/sessions/two".into(),
            },
        ] {
            assert_eq!(
                session.resolve_root(root),
                Err(SessionRuleError::RootChanged)
            );
        }
        assert_eq!(
            session.root().owned_path(),
            Some(Path::new("/sessions/one"))
        );
    }

    #[test]
    fn historical_closure_stays_unknown_and_legacy_root_can_resolve_once() {
        let mut session = session(RootVfs::None);
        session.status = SessionStatus::Closed(ClosedReason::Unknown);
        assert!(!session.close(ClosedReason::Deleted));
        assert_eq!(
            session.status(),
            SessionStatus::Closed(ClosedReason::Unknown)
        );
        session.status = SessionStatus::Active;
        session.root = RootVfs::LegacyUnresolved;
        session
            .resolve_root(RootVfs::Named {
                path: "/volume".into(),
            })
            .unwrap();
        assert_eq!(
            session.resolve_root(RootVfs::None),
            Err(SessionRuleError::RootChanged)
        );
    }

    #[test]
    fn identity_rejects_directory_traversal() {
        for id in [
            "",
            ".",
            "..",
            "../session",
            "/session",
            "a/b",
            "one/",
            "one/.",
            "./one",
            "a//b",
        ] {
            assert_eq!(
                SessionId::parse(id.into()),
                Err(SessionRuleError::InvalidIdentity)
            );
        }
    }
}
