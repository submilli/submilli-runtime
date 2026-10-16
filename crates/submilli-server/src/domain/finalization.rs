//! Facts about cleanup attempts. Errors are terminal; interrupted attempts remain recoverable.
#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum FinalizerFailure {
    Execution,
    AuthorityUnavailable,
    ArtifactUnavailable,
    BudgetExhausted,
}
impl FinalizerFailure {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Execution => "execution_failed",
            Self::AuthorityUnavailable => "authority_unavailable",
            Self::ArtifactUnavailable => "artifact_unavailable",
            Self::BudgetExhausted => "budget_exhausted",
        }
    }
}

/// One engine-owned registration, already claimed for an attempt.
pub(crate) struct FinalizerInvocation {
    pub id: String,
    pub session_id: String,
    pub package: String,
    pub key: Vec<u16>,
    pub value: Result<Vec<u16>, FinalizerFailure>,
    pub package_versions: Result<std::collections::BTreeMap<String, String>, FinalizerFailure>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum FinalizationStatus {
    Registered,
    Pending,
    Running,
    Succeeded,
    Failed,
}

/// Terminal errors are deliberately distinct from an interrupted attempt: only
/// the latter can become pending again during recovery.
pub(crate) struct Finalization {
    status: FinalizationStatus,
    failure: Option<FinalizerFailure>,
}
impl Finalization {
    pub fn restore(status: FinalizationStatus) -> Self {
        Self {
            status,
            failure: None,
        }
    }
    pub fn status(&self) -> FinalizationStatus {
        self.status
    }
    pub fn failure(&self) -> Option<FinalizerFailure> {
        self.failure
    }
    pub fn terminate(&mut self) -> bool {
        if self.status != FinalizationStatus::Registered {
            return false;
        }
        self.status = FinalizationStatus::Pending;
        true
    }
    pub fn claim(&mut self) -> bool {
        if self.status != FinalizationStatus::Pending {
            return false;
        }
        self.status = FinalizationStatus::Running;
        true
    }
    pub fn complete(&mut self, outcome: Result<(), FinalizerFailure>) -> bool {
        if self.status != FinalizationStatus::Running {
            return false;
        }
        self.failure = outcome.err();
        self.status = if self.failure.is_some() {
            FinalizationStatus::Failed
        } else {
            FinalizationStatus::Succeeded
        };
        true
    }
    pub fn recover(&mut self) -> bool {
        if self.status != FinalizationStatus::Running {
            return false;
        }
        self.status = FinalizationStatus::Pending;
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn only_interruption_can_redeliver_an_attempt() {
        for outcome in [Ok(()), Err(FinalizerFailure::Execution)] {
            let mut registration = Finalization::restore(FinalizationStatus::Registered);
            assert!(registration.terminate());
            assert!(!registration.terminate());
            assert!(registration.claim());
            assert!(registration.recover());
            assert!(registration.claim());
            assert!(registration.complete(outcome));
            assert!(!registration.recover());
            assert!(!registration.claim());
            assert!(!registration.complete(Ok(())));
        }
    }
}
