use crate::application::error::StoreError;
use submilli_shared::secret_store::KEY_ID_LEN;

/// Prepares a run's last-run JSON to be recorded with the run's end, and owns
/// the spill files around it: which one a session's last run is kept in, and
/// deleting those no record names any more.
#[async_trait::async_trait]
pub(crate) trait LastRunStaging: Send + Sync {
    /// Seal `json` for `session_id`. A large one is written to its own spill
    /// file, durably, before this returns; one over the retained limit keeps
    /// only its size.
    async fn stage(&self, session_id: &str, json: Vec<u8>) -> Result<StagedLastRun, StoreError>;
    /// The spill file the session's last run is kept in now, if any.
    async fn spill_file(&self, session_id: &str) -> Result<Option<String>, StoreError>;
    /// Delete those of `files` no record names any more. Best effort: startup
    /// deletes what this leaves.
    async fn release(&self, session_id: &str, files: Vec<String>);
}

/// A run's last-run JSON as staging prepared it. The application carries it
/// from staging to the unit that records the run's end, unopened.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct StagedLastRun {
    /// The length of the plaintext JSON.
    pub encoded_bytes: u64,
    pub storage: LastRunStorage,
}

impl StagedLastRun {
    /// The spill file it is kept in, if it is kept in one.
    pub fn spill_file(&self) -> Option<&str> {
        match &self.storage {
            LastRunStorage::SpillFile { name, .. } => Some(name),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum LastRunStorage {
    /// Kept in its row. `key_id` names the key that sealed `payload`; `None`
    /// means plaintext.
    Inline {
        payload: Vec<u8>,
        key_id: Option<[u8; KEY_ID_LEN]>,
    },
    /// Kept in the spill file `name` in the session's directory.
    SpillFile {
        name: String,
        key_id: Option<[u8; KEY_ID_LEN]>,
    },
    /// Over the retained `limit`: only its size is kept.
    TooLarge { limit: u64 },
}
