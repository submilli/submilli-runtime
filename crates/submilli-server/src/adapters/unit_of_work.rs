//! Factories create independent application-scoped units of work.
mod sqlite;
mod stores;

pub(crate) use sqlite::SqliteUnitOfWorkFactory;
pub(crate) use stores::StoreUnitOfWorkFactory;

pub(crate) fn for_sessions(
    sessions: std::sync::Arc<dyn crate::session_store::DurableSessionStore>,
    session_root: std::path::PathBuf,
    cipher: Option<std::sync::Arc<submilli_shared::secret_store::SecretCipher>>,
) -> std::sync::Arc<dyn crate::application::unit_of_work::UnitOfWorkFactory> {
    match sessions.database() {
        Some(database) => std::sync::Arc::new(SqliteUnitOfWorkFactory {
            database,
            session_root,
            cipher,
        }),
        None => std::sync::Arc::new(StoreUnitOfWorkFactory {
            blueprints: std::sync::Arc::new(crate::blueprint::InMemoryBlueprintStore::default()),
            sessions,
            session_root,
            cipher,
        }),
    }
}

#[cfg(test)]
mod tests;
