use crate::application::error::StoreError;
use crate::domain::session::SessionRuleError;

#[derive(Debug)]
pub enum BootError {
    Sessions(StoreError),
    Idempotency(StoreError),
}

impl std::fmt::Display for BootError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Sessions(_) => f.write_str("cannot enumerate persisted sessions"),
            Self::Idempotency(_) => f.write_str("cannot enumerate idempotency sessions"),
        }
    }
}

impl std::error::Error for BootError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Sessions(error) | Self::Idempotency(error) => Some(error),
        }
    }
}

#[derive(Debug)]
pub enum SessionError {
    UnknownSession,
    CleanupPending,
    Storage(StoreError),
    Secrets(String),
    Io(String),
    InvalidVfs(String),
    /// The blueprint names a volume this server does not declare — the operator
    /// removed or renamed it since the blueprint was registered.
    UnknownVolume(String),
    /// A declared volume could not be mounted: its directory is gone, is not a
    /// directory, or is unreadable. The host path is deliberately absent — it is
    /// logged server-side instead, so a client learns only the volume name.
    VolumeUnavailable(String),
    /// A volume could not be grafted at its mount path in the root, such as
    /// when the root holds a file there.
    MountFailed {
        volume: String,
        reason: String,
    },
}

impl std::fmt::Display for SessionError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            SessionError::CleanupPending => f.write_str("session cleanup is pending"),
            SessionError::Storage(error) => write!(f, "session storage unavailable: {error}"),
            SessionError::Secrets(error) => f.write_str(error),
            SessionError::UnknownSession => f.write_str("unknown or expired session"),
            SessionError::InvalidVfs(msg) => write!(f, "invalid session filesystem: {msg}"),
            SessionError::Io(msg) => write!(f, "session vfs io: {msg}"),
            SessionError::UnknownVolume(name) => write!(
                f,
                "volume '{name}' is not declared on this server; ask the operator to declare it \
                 under `volumes:` in the server config"
            ),
            SessionError::VolumeUnavailable(name) => write!(
                f,
                "volume '{name}' is declared but unavailable; the server log has the details"
            ),
            SessionError::MountFailed { volume, reason } => {
                write!(f, "volume '{volume}' could not be mounted: {reason}")
            }
        }
    }
}

impl std::error::Error for SessionError {}
impl From<SessionRuleError> for SessionError {
    fn from(error: SessionRuleError) -> Self {
        match error {
            SessionRuleError::Closed | SessionRuleError::Expired => Self::UnknownSession,
            other => Self::InvalidVfs(other.to_string()),
        }
    }
}

impl From<StoreError> for SessionError {
    fn from(error: StoreError) -> Self {
        match error {
            StoreError::Credentials(message) => Self::Secrets(message),
            error => Self::Storage(error),
        }
    }
}
