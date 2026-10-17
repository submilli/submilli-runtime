use super::*;
use interpreter::runtime::{DiskQuota, fs::FileIdentity};
use std::collections::HashMap;
use std::sync::Weak;

pub(crate) type SharedQuotas = HashMap<FileIdentity, Weak<DiskQuota>>;

// Reopening a logical workspace in the same process must keep the counter used
// by its existing VFS owners. Weak entries do not retain completed workspaces.
pub(super) async fn share(
    context: &FilesystemContext,
    mut vfs: Vfs,
) -> Result<Vfs, FilesystemError> {
    let mut shared = context.filesystem_quotas.lock().await;
    shared.retain(|_, quota| quota.strong_count() != 0);
    if let (Some(root), Some(quota)) = (vfs.dir(), vfs.quota()) {
        let identity = FileIdentity::of(&root.dir_metadata().map_err(RecoveryError::from)?)
            .map_err(RecoveryError::from)?;
        let quota = reuse(&mut shared, identity, quota)?;
        vfs = vfs.with_shared_quota(quota);
    }
    let mounts = vfs.mounts().to_vec();
    for mount in mounts {
        if let Some(quota) = mount.quota() {
            let path = interpreter::runtime::fs::resolve_content(&vfs, "/", mount.guest_path())
                .map_err(RecoveryError::from)?;
            let identity = FileIdentity::of(&path.metadata().map_err(RecoveryError::from)?)
                .map_err(RecoveryError::from)?;
            let quota = reuse(&mut shared, identity, quota)?;
            vfs = vfs.with_mount_quota(mount.guest_path(), quota);
        }
    }
    Ok(vfs)
}

fn reuse(
    shared: &mut SharedQuotas,
    id: FileIdentity,
    quota: &Arc<DiskQuota>,
) -> Result<Arc<DiskQuota>, FilesystemError> {
    if let Some(existing) = shared.get(&id).and_then(Weak::upgrade) {
        if existing.limit() != quota.limit() {
            return Err(RecoveryError::Conflict.into());
        }
        return Ok(existing);
    }
    shared.insert(id, Arc::downgrade(quota));
    Ok(quota.clone())
}

pub(super) async fn live(context: &FilesystemContext) -> HashMap<FileIdentity, Arc<DiskQuota>> {
    context
        .filesystem_quotas
        .lock()
        .await
        .iter()
        .filter_map(|(id, quota)| quota.upgrade().map(|quota| (*id, quota)))
        .collect()
}
