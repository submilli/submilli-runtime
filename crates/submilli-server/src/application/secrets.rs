//! Operator secrets: stored, listed by name and removed through the management
//! API, revealed only to the programs whose blueprints declare them.
pub(crate) mod import;
pub(crate) mod list;
pub(crate) mod remove;
pub(crate) mod reveal;
pub(crate) mod store;

use crate::application::error::StoreError;
use crate::domain::secret::SecretError;

#[derive(Debug, thiserror::Error)]
pub(crate) enum SecretFailure {
    #[error(transparent)]
    Rule(#[from] SecretError),
    #[error(transparent)]
    Store(#[from] StoreError),
}
