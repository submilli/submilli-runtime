//! The lifecycle of a run, independent of its program and result.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RunStatus {
    Queued,
    Running,
    Stuck(StuckReason),
    Cancelled,
    Aborted,
    Completed,
    /// An interrupted unrecorded run whose outcome cannot be recovered.
    Indeterminate(IndeterminateReason),
    Expired,
}

impl RunStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Queued => "queued",
            Self::Running => "running",
            Self::Stuck(_) => "stuck",
            Self::Cancelled => "cancelled",
            Self::Aborted => "aborted",
            Self::Completed => "completed",
            Self::Indeterminate(_) => "indeterminate",
            Self::Expired => "expired",
        }
    }

    pub fn reason(self) -> Option<&'static str> {
        match self {
            Self::Stuck(reason) => Some(reason.as_str()),
            Self::Cancelled => Some("session_closed"),
            Self::Aborted => Some("never_dispatched"),
            Self::Indeterminate(reason) => Some(reason.as_str()),
            Self::Queued | Self::Running | Self::Completed | Self::Expired => None,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StuckReason {
    Credentials,
    Storage,
    Compatibility,
    History,
    Fuel,
    Timeout,
    Interrupted,
    Disabled,
    ReplayUnavailable,
}

impl StuckReason {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Credentials => "credentials_unavailable",
            Self::Storage => "storage_unavailable",
            Self::Compatibility => "incompatible_environment",
            Self::History => "invalid_history",
            Self::Fuel => "fuel_exhausted",
            Self::Timeout => "execution_timeout",
            Self::Interrupted => "execution_interrupted",
            Self::Disabled => "durability_disabled",
            Self::ReplayUnavailable => "replay_unavailable",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum IndeterminateReason {
    Interrupted,
    Legacy,
}

impl IndeterminateReason {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Interrupted => "interrupted_without_history",
            Self::Legacy => "legacy_without_history",
        }
    }
}
