//! Shared plumbing for the server-free local commands (`submilli secret …`,
//! `submilli mcp …`, and `submilli run`'s blueprint path): the local plaintext
//! secret store and a current-thread `block_on` for their async surface.

use std::future::Future;
use std::io::{IsTerminal, Read};
use std::sync::Arc;

use anyhow::{Context, Result, bail};
use submilli_shared::secret_store::{PlaintextFileSecretStore, SecretStore};

/// Read a secret value for `key` without it ever appearing in an argument list.
/// At a terminal, prompt with echo off so a pasted value doesn't land in the
/// scrollback; otherwise take the whole of stdin, minus one trailing newline so
/// `echo secret | …` stores `secret`.
pub fn read_secret_value(key: &str) -> Result<String> {
    let stdin = std::io::stdin();
    if stdin.is_terminal() {
        let value = dialoguer::Password::new()
            .with_prompt(format!("Value for '{key}'"))
            .interact()
            .context("reading secret value")?;
        if value.is_empty() {
            bail!("no value entered for '{key}'");
        }
        return Ok(value);
    }
    let mut value = String::new();
    stdin
        .lock()
        .read_to_string(&mut value)
        .context("reading secret value from stdin")?;
    Ok(value.strip_suffix('\n').unwrap_or(&value).to_string())
}

/// Open the per-user local secret store at `$SUBMILLI_HOME/secrets` (default
/// `~/.submilli/secrets`): what `submilli run --blueprint` resolves `store:`
/// secrets from and where `mcp authenticate` keeps credentials. Plaintext
/// `0600` files: an encryption key kept on the same machine protects nothing, so
/// the store is honest about being readable by its owner.
///
/// The server's encrypted store lives apart, under `$SUBMILLI_HOME/server/`.
/// The two share a file-name scheme, so sharing a directory would let each
/// overwrite the other's entries; `submilli server secret put` is how a value
/// reaches a running server.
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
