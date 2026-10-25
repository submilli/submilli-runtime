//! A failed or interrupted root creation must repeat its directory barriers.
use super::{HistoryObjects, files, invalid, new_id, require_supported_platform, sql};
use crate::config::VolumeRoot;
use crate::database::ServerDatabase;
use interpreter::runtime::blocking::BlockingWork;
use std::path::PathBuf;
use submilli_durable::unit_of_work::StoreError;

impl HistoryObjects {
    pub(crate) async fn prepare_root(
        database: &ServerDatabase,
        root: &VolumeRoot,
    ) -> Result<(), StoreError> {
        require_supported_platform()?;
        let root = root.clone();
        let probe = root.clone();
        let mut workers = BlockingWork::default();
        let parents = workers
            .spawn(move || probe.creation_parents())
            .await
            .map_err(invalid)?
            .map_err(invalid)?;
        let parents = serde_json::to_string(&parents).map_err(invalid)?;
        let identity = new_id()?;
        // Commit before mkdir: an interrupted initializer must not mistake its own
        // unsynced directories for pre-existing, durable parents on the next boot.
        let pending = database
            .transaction(move |connection| {
                Box::pin(sql::begin_initialization(connection, identity, parents))
            })
            .await
            .map_err(|e| StoreError::Database(Box::new(e)))?;
        let parents: Vec<PathBuf> = serde_json::from_str(&pending).map_err(invalid)?;
        workers
            .spawn(move || {
                root.prepare().map_err(invalid)?;
                super::checkpoint("root-created");
                for parent in parents {
                    files::sync_directory(&parent).map_err(invalid)?;
                }
                Ok::<_, StoreError>(())
            })
            .await
            .map_err(invalid)??;
        database
            .transaction(|connection| Box::pin(sql::finish_initialization(connection)))
            .await
            .map_err(|e| StoreError::Database(Box::new(e)))
    }
}

#[cfg(all(test, windows))]
mod tests {
    use super::*;

    #[tokio::test]
    async fn windows_rejects_history_objects_before_creating_the_volume_root() {
        let temporary = tempfile::tempdir().unwrap();
        let database = ServerDatabase::open(&temporary.path().join("db"))
            .await
            .unwrap();
        let root = VolumeRoot::default_root(temporary.path().join("volumes"));
        let error = HistoryObjects::prepare_root(&database, &root)
            .await
            .unwrap_err();
        assert!(
            error
                .to_string()
                .contains("native Windows is not supported")
        );
        assert!(!root.path().exists());
    }
}
