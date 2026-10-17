use super::*;
use interpreter::runtime::fs::FileIdentity;
use tokio::sync::Mutex;

/// Shared same-process coordination for access to the same backing roots.
/// The caller retains this context across filesystem handles; it is not a lease.
#[derive(Clone, Default)]
pub struct FilesystemContext {
    pub(super) filesystem_gate: Arc<Mutex<()>>,
    pub(super) filesystem_quotas: Arc<Mutex<quotas::SharedQuotas>>,
}
impl FilesystemContext {
    #[cfg(feature = "test-support")]
    pub fn test_gate(&self) -> &Arc<Mutex<()>> {
        &self.filesystem_gate
    }
}

// A retry must resolve the same guest paths against the same configured roots.
// Keep only a fingerprint in each record, not a second persisted mount catalog.
pub(super) fn binding(vfs: &Vfs) -> Result<[u8; 32], FilesystemError> {
    let root = vfs
        .dir()
        .ok_or(RecoveryError::Unsupported("filesystem disabled"))?;
    let root_id = FileIdentity::of(&root.dir_metadata().map_err(RecoveryError::from)?)
        .map_err(RecoveryError::from)?;
    let mounts = vfs
        .mounts()
        .iter()
        .map(|mount| {
            let id = FileIdentity::of(&mount.dir().dir_metadata()?)?;
            Ok((
                mount.guest_path(),
                mount.access(),
                id,
                mount.quota().map(|q| q.limit()),
            ))
        })
        .collect::<Result<Vec<_>, RecoveryError>>()?;
    let bytes = serde_json::to_vec(&(
        vfs.cwd(),
        vfs.access(),
        root_id,
        vfs.quota().map(|q| q.limit()),
        mounts,
    ))
    .map_err(|e| StoreError::Io(e.to_string()))?;
    Ok(Sha256::digest(bytes).into())
}
