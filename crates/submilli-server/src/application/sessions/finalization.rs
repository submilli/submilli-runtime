//! Termination attempts every registration independently; only crash recovery redelivers.
use crate::application::error::StoreError;
use crate::domain::finalization::{FinalizerFailure, FinalizerInvocation};

#[async_trait::async_trait]
pub(crate) trait Finalizations: Send + Sync {
    async fn claim_next(&self, session: &str) -> Result<Option<FinalizerInvocation>, StoreError>;
    async fn complete(
        &self,
        id: &str,
        outcome: Result<(), FinalizerFailure>,
    ) -> Result<(), StoreError>;
}
#[async_trait::async_trait]
pub(crate) trait FinalizerExecutor: Send + Sync {
    async fn execute(&self, invocation: &FinalizerInvocation) -> Result<(), FinalizerFailure>;
}

pub(crate) async fn finalize_session(
    registrations: &dyn Finalizations,
    executor: &dyn FinalizerExecutor,
    session: &str,
) -> Result<(), StoreError> {
    while let Some(invocation) = registrations.claim_next(session).await? {
        let outcome = match &invocation.value {
            Err(failure) => Err(*failure),
            Ok(_) => executor.execute(&invocation).await,
        };
        registrations.complete(&invocation.id, outcome).await?;
    }
    Ok(())
}
