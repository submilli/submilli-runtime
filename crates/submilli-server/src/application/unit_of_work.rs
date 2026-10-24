//! Transactional queries and writes for application use cases.
use crate::application::error::StoreError;
use crate::application::sessions::ports::StagedLastRun;
use crate::domain::mcp_grant::{GrantOverview, GrantVersion, McpGrant};
use crate::domain::secret::{Secret, SecretName};
use crate::domain::session::Session;
use std::time::SystemTime;

#[async_trait::async_trait]
pub(crate) trait UnitOfWorkFactory: Send + Sync {
    async fn begin(&self) -> Result<Box<dyn UnitOfWork>, StoreError>;
}

/// Reads see this unit's pending writes. Commit consumes the unit; dropping it
/// discards uncommitted work. Compatibility adapters document weaker commit
/// guarantees. Do not open another database operation while this scope holds
/// the transaction's connection.
#[async_trait::async_trait]
pub(crate) trait UnitOfWork: Send {
    async fn get_session(&mut self, id: &str) -> Result<Option<Session>, StoreError>;
    async fn sessions_due_for_expiry(
        &mut self,
        now: SystemTime,
    ) -> Result<Vec<Session>, StoreError>;
    async fn list_sessions(&mut self) -> Result<Vec<Session>, StoreError>;
    async fn sessions_for_blueprint(&mut self, name: &str) -> Result<Vec<Session>, StoreError>;
    async fn save_session(&mut self, session: Session) -> Result<(), StoreError>;
    async fn blueprint_exists(&mut self, name: &str) -> Result<bool, StoreError>;
    async fn remove_blueprint(&mut self, name: &str) -> Result<bool, StoreError>;
    async fn list_runs(
        &mut self,
        query: super::sessions::recovery::RunQuery,
    ) -> Result<Vec<crate::domain::run::Run>, StoreError>;
    async fn has_retained_run(&mut self, session: &str) -> Result<bool, StoreError>;
    async fn has_run_predecessor(&mut self, id: &str) -> Result<bool, StoreError>;
    async fn retire_run_resources(&mut self, id: &str) -> Result<(), StoreError>;
    async fn run_result(
        &mut self,
        id: &str,
    ) -> Result<Option<crate::domain::run_result::RunResult>, StoreError>;
    async fn get_run(&mut self, id: &str) -> Result<Option<crate::domain::run::Run>, StoreError>;
    async fn run_for_key(
        &mut self,
        session_id: &str,
        key: &str,
    ) -> Result<Option<crate::domain::run::Run>, StoreError>;
    async fn save_run(&mut self, request: crate::domain::run::Run) -> Result<(), StoreError>;

    async fn get_run_program(
        &mut self,
        id: &str,
    ) -> Result<Option<crate::domain::run_program::RunProgram>, StoreError>;
    async fn save_run_program(
        &mut self,
        id: &str,
        program: crate::domain::run_program::RunProgram,
    ) -> Result<(), StoreError>;
    async fn save_run_result(
        &mut self,
        result: crate::domain::run_result::RunResult,
    ) -> Result<(), StoreError>;

    async fn unfinished_runs(&mut self) -> Result<Vec<crate::domain::run::Run>, StoreError>;
    /// The sessions holding more than `kept` completed responses, at least one
    /// of them completed at or before `cutoff`.
    async fn sessions_with_responses_to_expire(
        &mut self,
        cutoff: SystemTime,
        kept: usize,
    ) -> Result<Vec<String>, StoreError>;
    /// A session's completed responses, without their bodies.
    async fn completed_responses(
        &mut self,
        session_id: &str,
    ) -> Result<Vec<crate::domain::idempotent_request::CompletedResponse>, StoreError>;
    /// Drop the bodies of `run_ids`, leaving tombstones; a request that is no longer
    /// completed is left alone. Returns how many expired. A bulk transition in
    /// storage, deliberately: loading the requests would read the very bodies
    /// this drops.
    async fn expire_responses(
        &mut self,
        session_id: &str,
        run_ids: Vec<String>,
    ) -> Result<u64, StoreError>;
    async fn get_secret(&mut self, name: &SecretName) -> Result<Option<Secret>, StoreError>;
    async fn save_secret(&mut self, secret: Secret, now: SystemTime) -> Result<(), StoreError>;
    async fn remove_secret(&mut self, name: &SecretName) -> Result<bool, StoreError>;
    /// Secret names starting with `prefix`, sorted; no value is opened.
    async fn secret_names(&mut self, prefix: Option<&str>) -> Result<Vec<String>, StoreError>;
    async fn get_grant(
        &mut self,
        blueprint: &str,
        server: &str,
    ) -> Result<Option<McpGrant>, StoreError>;
    /// Persist `grant` over the version it was changed from, or
    /// [`StoreError::Conflict`] when the grant on file moved meanwhile.
    async fn save_grant(&mut self, grant: McpGrant) -> Result<(), StoreError>;
    async fn remove_grant(&mut self, blueprint: &str, server: &str) -> Result<bool, StoreError>;
    /// The version on file, without opening the grant's token.
    async fn grant_version(
        &mut self,
        blueprint: &str,
        server: &str,
    ) -> Result<Option<GrantVersion>, StoreError>;
    /// The grants on file, without opening their tokens.
    async fn list_grants(
        &mut self,
        blueprint: Option<&str>,
    ) -> Result<Vec<GrantOverview>, StoreError>;
    async fn remove_blueprint_grants(&mut self, blueprint: &str) -> Result<(), StoreError>;
    /// Whether the session is active, as [`Session::require_active`] defines
    /// it, without loading the aggregate: session state is read on every
    /// `submilli:session` call. False for a session that never existed.
    async fn session_is_active(&mut self, id: &str) -> Result<bool, StoreError>;
    async fn session_state_get(
        &mut self,
        session_id: &str,
        key: &[u16],
    ) -> Result<Option<Vec<u16>>, StoreError>;
    async fn session_state_has(
        &mut self,
        session_id: &str,
        key: &[u16],
    ) -> Result<bool, StoreError>;
    async fn session_state_usage(
        &mut self,
        session_id: &str,
        key: &[u16],
    ) -> Result<SessionStateUsage, StoreError>;
    /// Write an entry the caller has checked against the session's limits.
    async fn session_state_put(
        &mut self,
        session_id: &str,
        key: &[u16],
        payload: &[u16],
        size: SessionStateSize,
    ) -> Result<(), StoreError>;
    async fn session_state_remove(
        &mut self,
        session_id: &str,
        key: &[u16],
    ) -> Result<bool, StoreError>;
    /// Up to `max_keys` keys after `after`, in UTF-16 code-unit order.
    async fn session_state_keys(
        &mut self,
        session_id: &str,
        after: Option<&[u16]>,
        max_keys: usize,
    ) -> Result<SessionStateKeys, StoreError>;
    /// Record a session's last run over the one before. Returns the spill
    /// file the one before was kept in, to release once this unit commits.
    async fn put_last_run(
        &mut self,
        session_id: &str,
        run_id: &str,
        completed_at: SystemTime,
        staged: &StagedLastRun,
    ) -> Result<Option<String>, StoreError>;
    /// Forget a session's last run. Returns the spill file it was kept in, to
    /// release once this unit commits.
    async fn remove_last_run(&mut self, session_id: &str) -> Result<Option<String>, StoreError>;
    async fn commit(self: Box<Self>) -> Result<(), StoreError>;
}

/// What a session's state holds when a write is checked against its limits.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) struct SessionStateUsage {
    pub entries: u64,
    /// Retained bytes of every entry.
    pub bytes: u64,
    /// Retained bytes of the entry under the key being written, if any.
    pub replaced: Option<u64>,
}

/// An entry's sizes as the session limits count them, recorded with it so a
/// listing and a usage check need not open values.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct SessionStateSize {
    pub value_bytes: u64,
    pub retained_bytes: u64,
}

/// A run of a session's keys in code-unit order, with each value's size.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct SessionStateKeys {
    pub keys: Vec<(Vec<u16>, u64)>,
    /// Whether keys remain after the last one returned.
    pub more: bool,
}
