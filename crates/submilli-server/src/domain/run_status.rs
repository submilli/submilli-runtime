//! The lifecycle of a run, independent of its program and result.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RunStatus {
    Queued,
    Running,
    Stuck,
    Cancelled,
    Aborted,
    Completed,
    /// An interrupted unrecorded run whose outcome cannot be recovered.
    Indeterminate,
    Expired,
}

impl RunStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Queued => "queued",
            Self::Running => "running",
            Self::Stuck => "stuck",
            Self::Cancelled => "cancelled",
            Self::Aborted => "aborted",
            Self::Completed => "completed",
            Self::Indeterminate => "indeterminate",
            Self::Expired => "expired",
        }
    }
}

#[derive(Clone, Copy, Debug)]
pub enum StuckReason {
    Credentials,
    Storage,
    Compatibility,
    History,
    Fuel,
    Timeout,
    Interrupted,
    Disabled,
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
        }
    }
}
