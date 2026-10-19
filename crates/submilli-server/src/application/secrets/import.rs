//! Bring secrets and MCP OAuth grants kept before the database into it, once.
use std::time::SystemTime;

use crate::application::error::StoreError;
use crate::application::unit_of_work::{UnitOfWork, UnitOfWorkFactory};
use crate::domain::mcp_grant::{GrantCredential, McpGrant};
use crate::domain::secret::Secret;

/// One entry of a secret store kept before the database.
pub(crate) enum PreviousEntry {
    Secret(Secret),
    Grant {
        blueprint: String,
        server: String,
        credential: GrantCredential,
        scopes: Vec<String>,
    },
}

/// An entry left out of the import, and why. It stays in the archive.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct SkippedEntry {
    pub entry: String,
    pub reason: String,
}

/// What a previous secret store holds: the entries the import can take, and
/// those it must leave behind.
#[derive(Default)]
pub(crate) struct PreviousSecrets {
    pub entries: Vec<PreviousEntry>,
    pub skipped: Vec<SkippedEntry>,
}

/// Where the secrets kept before the database are, and where they go once
/// imported.
#[async_trait::async_trait]
pub(crate) trait PreviousSecretStore: Send + Sync {
    /// Every entry, all opened before anything is imported, so an entry that
    /// cannot be read stops the import with nothing changed.
    async fn read(&self) -> Result<PreviousSecrets, StoreError>;
    /// Set the store aside once its entries are in the database.
    async fn archive(&self) -> Result<(), StoreError>;
}

#[derive(Debug, Default, PartialEq, Eq)]
pub(crate) struct Imported {
    pub secrets: usize,
    pub grants: usize,
    pub skipped: Vec<SkippedEntry>,
}

#[derive(Debug, thiserror::Error)]
pub enum ImportFailure {
    #[error(transparent)]
    Store(#[from] StoreError),
    #[error(
        "{0} is already in the database with a different value; nothing was imported. Remove \
         it from the secret-store directory to keep the database's value, then restart"
    )]
    Conflict(String),
}

pub(crate) struct ImportSecrets<'a> {
    units: &'a dyn UnitOfWorkFactory,
    previous: &'a dyn PreviousSecretStore,
}

impl<'a> ImportSecrets<'a> {
    pub fn new(units: &'a dyn UnitOfWorkFactory, previous: &'a dyn PreviousSecretStore) -> Self {
        Self { units, previous }
    }

    /// Import every entry in one unit, then archive the previous store. An
    /// entry already in the database must match; a grant whose blueprint is
    /// gone, or that breaks a grant rule, is skipped.
    pub async fn execute(&self) -> Result<Imported, ImportFailure> {
        let previous = self.previous.read().await?;
        if previous.entries.is_empty() && previous.skipped.is_empty() {
            return Ok(Imported::default());
        }
        let mut imported = Imported {
            skipped: previous.skipped,
            ..Imported::default()
        };
        let now = SystemTime::now();
        let mut unit = self.units.begin().await?;
        for entry in previous.entries {
            match entry {
                PreviousEntry::Secret(secret) => {
                    if import_secret(unit.as_mut(), secret, now).await? {
                        imported.secrets += 1;
                    }
                }
                PreviousEntry::Grant {
                    blueprint,
                    server,
                    credential,
                    scopes,
                } => match import_grant(unit.as_mut(), blueprint, server, credential, scopes, now)
                    .await?
                {
                    GrantImport::Imported => imported.grants += 1,
                    GrantImport::AlreadyThere => {}
                    GrantImport::Skipped(skipped) => imported.skipped.push(skipped),
                },
            }
        }
        unit.commit().await?;
        self.previous.archive().await?;
        Ok(imported)
    }
}

/// Whether `secret` was imported; a matching one already in the database is
/// left as it is.
async fn import_secret(
    unit: &mut dyn UnitOfWork,
    secret: Secret,
    now: SystemTime,
) -> Result<bool, ImportFailure> {
    match unit.get_secret(secret.name()).await? {
        Some(existing) if existing == secret => Ok(false),
        Some(_) => Err(ImportFailure::Conflict(format!(
            "secret '{}'",
            secret.name().as_str()
        ))),
        None => {
            unit.save_secret(secret, now).await?;
            Ok(true)
        }
    }
}

enum GrantImport {
    Imported,
    AlreadyThere,
    Skipped(SkippedEntry),
}

async fn import_grant(
    unit: &mut dyn UnitOfWork,
    blueprint: String,
    server: String,
    credential: GrantCredential,
    scopes: Vec<String>,
    now: SystemTime,
) -> Result<GrantImport, ImportFailure> {
    let entry = format!("the MCP grant for '{server}' in blueprint '{blueprint}'");
    if !unit.blueprint_exists(&blueprint).await? {
        return Ok(GrantImport::Skipped(SkippedEntry {
            entry,
            reason: "its blueprint no longer exists".into(),
        }));
    }
    // Checked before the comparison, so a grant accepted before grants had
    // rules is skipped even where the database holds one; it stays in the
    // archive.
    let grant = match McpGrant::deposit(
        blueprint.clone(),
        server.clone(),
        uuid::Uuid::new_v4().to_string(),
        credential,
        scopes,
        now,
    ) {
        Ok(grant) => grant,
        Err(error) => {
            return Ok(GrantImport::Skipped(SkippedEntry {
                entry,
                reason: error.to_string(),
            }));
        }
    };
    match unit.get_grant(&blueprint, &server).await? {
        Some(existing)
            if existing.credential() == grant.credential()
                && existing.scopes() == grant.scopes() =>
        {
            Ok(GrantImport::AlreadyThere)
        }
        Some(_) => Err(ImportFailure::Conflict(entry)),
        None => {
            unit.save_grant(grant).await?;
            Ok(GrantImport::Imported)
        }
    }
}
