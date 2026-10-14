//! Transactional queries and writes for application use cases.
use crate::application::error::StoreError;
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
    async fn get_request(
        &mut self,
        session_id: &str,
        key: &str,
    ) -> Result<Option<crate::domain::idempotent_request::IdempotentRequest>, StoreError>;
    async fn save_request(
        &mut self,
        request: crate::domain::idempotent_request::IdempotentRequest,
    ) -> Result<(), StoreError>;
    async fn remove_request(&mut self, session_id: &str, key: &str) -> Result<(), StoreError>;
    async fn remove_session_requests(&mut self, session_id: &str) -> Result<(), StoreError>;
    async fn unfinished_requests(
        &mut self,
    ) -> Result<Vec<crate::domain::idempotent_request::IdempotentRequest>, StoreError>;
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
    /// Drop the bodies of `keys`, leaving tombstones; a request that is no longer
    /// completed is left alone. Returns how many expired. A bulk transition in
    /// storage, deliberately: loading the requests would read the very bodies
    /// this drops.
    async fn expire_responses(
        &mut self,
        session_id: &str,
        keys: Vec<String>,
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
    async fn commit(self: Box<Self>) -> Result<(), StoreError>;
}
