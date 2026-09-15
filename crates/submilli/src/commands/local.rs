//! Shared plumbing for the server-free local commands (`submilli secret …`,
//! `submilli mcp …`, and `submilli run`'s blueprint path): the local plaintext
//! secret store and a current-thread `block_on` for their async surface.

use std::future::Future;
use std::sync::Arc;

use anyhow::{Context, Result};
use submilli_shared::secret_store::{PlaintextFileSecretStore, SecretStore};

/// Open the per-user local secret store at `$SUBMILLI_HOME/secrets` (default
/// `~/.submilli/secrets`) — the same default directory and key scheme the server
/// uses, so `store:` secrets and MCP credentials resolve identically. Plaintext
/// `0600` files: an encryption key kept on the same machine protects nothing, so
/// the store is honest about being readable by its owner.
pub fn open_secret_store() -> Result<Arc<dyn SecretStore>> {
    let dir = submilli_build::default_data_root().join("secrets");
    let store = PlaintextFileSecretStore::open(dir).context("opening the local secret store")?;
    Ok(Arc::new(store))
}

/// Drive a future to completion on a private current-thread runtime. The local
/// commands are otherwise synchronous; the store and OAuth surfaces are async.
pub fn block_on<F: Future>(fut: F) -> Result<F::Output> {
    Ok(tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .context("building tokio runtime")?
        .block_on(fut))
}
