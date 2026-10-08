//! Factories create independent application-scoped units of work.
mod sqlite;

pub(crate) use sqlite::SqliteUnitOfWorkFactory;

pub(crate) fn for_sessions(
    database: std::sync::Arc<crate::database::ServerDatabase>,
    session_root: std::path::PathBuf,
    cipher: Option<std::sync::Arc<submilli_shared::secret_store::SecretCipher>>,
) -> std::sync::Arc<dyn crate::application::unit_of_work::UnitOfWorkFactory> {
    let cipher = cipher.or_else(|| database.ephemeral_cipher());
    std::sync::Arc::new(SqliteUnitOfWorkFactory {
        database,
        session_root,
        cipher,
    })
}

#[cfg(test)]
mod tests;
