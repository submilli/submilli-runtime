//! Externref payloads for `submilli:fs` open resources.
//!
//! Each payload owns a backend resource plus a clone of
//! `TenantLimits.host_attached_bytes` (via [`ByteCharge`]). It charges its buffer
//! footprint at creation and refunds it when the handle closes or is reclaimed.
//!
//! Readers close eagerly. Writers publish only through explicit backend close;
//! dropping a guest handle releases resources without publishing output.

use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use crate::runtime::durable::EffectId;
use crate::runtime::filesystem::{Filesystem, FilesystemError};
use crate::runtime::fuel::WorkBudget;
use crate::runtime::limits::{MemoryCapExceeded, TenantLimits};

pub use crate::runtime::default_filesystem::kind_of;

/// Footprint charged per reader/writer, matching the default `BufReader`/`BufWriter`
/// capacity. Byte readers add their `chunk_size` working buffer on top.
const HANDLE_BUF_BYTES: u64 = 8 * 1024;

/// Bytes charged against the store's host-attached counter, refunded on `release`/`drop`.
pub(super) struct ByteCharge {
    bytes: u64,
    counter: Arc<AtomicU64>,
}

impl ByteCharge {
    pub(super) fn new(limits: &TenantLimits, bytes: u64) -> Result<Self, MemoryCapExceeded> {
        limits.charge_host_bytes(bytes)?;
        Ok(Self {
            bytes,
            counter: limits.host_attached_counter(),
        })
    }

    pub(super) fn grow(
        &mut self,
        limits: &TenantLimits,
        bytes: u64,
    ) -> Result<(), MemoryCapExceeded> {
        limits.charge_host_bytes(bytes)?;
        self.bytes = self.bytes.saturating_add(bytes);
        Ok(())
    }

    /// Refunds the charged bytes once; subsequent calls are no-ops.
    fn release(&mut self) {
        let n = std::mem::take(&mut self.bytes);
        if n == 0 {
            return;
        }
        let _ = self
            .counter
            .fetch_update(Ordering::Relaxed, Ordering::Relaxed, |current| {
                Some(current.saturating_sub(n))
            });
    }
}

impl Drop for ByteCharge {
    fn drop(&mut self) {
        self.release();
    }
}

/// Explicit guest close is effect-aware. Drop only releases local resources.
#[async_trait::async_trait]
pub trait Closable: Send + Sync {
    /// Release process-local accounting after a successful explicit close,
    /// including when execution history skipped the backend call.
    fn release_buffer(&mut self);
    fn id(&self) -> EffectId;
    async fn close(
        &mut self,
        filesystem: &dyn Filesystem,
        effect_id: &EffectId,
        work: Option<Arc<WorkBudget>>,
        limits: &TenantLimits,
    ) -> Result<(), FilesystemError>;
}

pub struct BackendReader {
    pub reader: crate::runtime::filesystem::FileReader,
    charge: ByteCharge,
}
impl BackendReader {
    pub fn new(
        reader: crate::runtime::filesystem::FileReader,
        limits: &TenantLimits,
    ) -> Result<Self, MemoryCapExceeded> {
        Ok(Self {
            reader,
            charge: ByteCharge::new(limits, HANDLE_BUF_BYTES)?,
        })
    }
    pub fn id(&self) -> EffectId {
        self.reader.id.clone()
    }
}
#[async_trait::async_trait]
impl Closable for BackendReader {
    fn release_buffer(&mut self) {
        self.charge.release();
    }
    fn id(&self) -> EffectId {
        self.reader.id.clone()
    }
    async fn close(
        &mut self,
        filesystem: &dyn Filesystem,
        effect_id: &EffectId,
        work: Option<Arc<WorkBudget>>,
        _limits: &TenantLimits,
    ) -> Result<(), FilesystemError> {
        filesystem
            .close_reader(effect_id, work, &mut self.reader)
            .await?;
        self.charge.release();
        Ok(())
    }
}

pub struct ChargedDirIter {
    pub walk: crate::runtime::filesystem::DirectoryReader,
    charge: ByteCharge,
}
impl ChargedDirIter {
    pub fn new(
        walk: crate::runtime::filesystem::DirectoryReader,
        limits: &TenantLimits,
    ) -> Result<Self, MemoryCapExceeded> {
        Ok(Self {
            charge: ByteCharge::new(limits, HANDLE_BUF_BYTES)?,
            walk,
        })
    }
    pub fn id(&self) -> EffectId {
        self.walk.id.clone()
    }
}
#[async_trait::async_trait]
impl Closable for ChargedDirIter {
    fn release_buffer(&mut self) {
        self.charge.release();
    }
    fn id(&self) -> EffectId {
        self.walk.id.clone()
    }
    async fn close(
        &mut self,
        filesystem: &dyn Filesystem,
        effect_id: &EffectId,
        work: Option<Arc<WorkBudget>>,
        _limits: &TenantLimits,
    ) -> Result<(), FilesystemError> {
        filesystem
            .close_directory(effect_id, work, &mut self.walk, _limits)
            .await?;
        self.charge.release();
        Ok(())
    }
}

/// A guest writer owns its resource state and accounts for its bounded buffer.
pub struct BackendFileWriter {
    writer: crate::runtime::filesystem::FileWriter,
    charge: ByteCharge,
}
impl BackendFileWriter {
    pub fn release_buffer(&mut self) {
        self.charge.release();
    }
    pub fn id(&self) -> EffectId {
        self.writer.id.clone()
    }
    pub fn new(
        writer: crate::runtime::filesystem::FileWriter,
        limits: &TenantLimits,
    ) -> Result<Self, MemoryCapExceeded> {
        Ok(Self {
            writer,
            charge: ByteCharge::new(limits, HANDLE_BUF_BYTES)?,
        })
    }
    pub async fn write(
        &mut self,
        filesystem: &dyn Filesystem,
        effect_id: &EffectId,
        work: Option<Arc<WorkBudget>>,
        bytes: Vec<u8>,
    ) -> Result<(), FilesystemError> {
        filesystem
            .write_chunk(effect_id, work, &mut self.writer, bytes)
            .await
    }
    pub async fn close(
        &mut self,
        filesystem: &dyn Filesystem,
        effect_id: &EffectId,
        work: Option<Arc<WorkBudget>>,
    ) -> Result<(), FilesystemError> {
        let result = filesystem
            .close_writer(effect_id, work, &mut self.writer)
            .await;
        if self.writer.closed {
            self.charge.release();
        }
        result
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::runtime::default_filesystem::DefaultFilesystem;

    #[tokio::test]
    async fn failed_native_backend_close_refunds_a_discarded_buffer() {
        use crate::runtime::default_filesystem::DefaultFilesystem;
        use crate::runtime::filesystem::Filesystem;
        let root = tempfile::tempdir().unwrap();
        let filesystem = DefaultFilesystem::new(
            crate::runtime::Vfs::external(root.path().to_path_buf()).unwrap(),
            "run".into(),
        );
        let effect_id = EffectId::stub().unwrap();
        let limits = TenantLimits::new(64 * 1024);
        let writer = filesystem
            .open_writer(&effect_id, None, "/target")
            .await
            .unwrap();
        let mut handle = BackendFileWriter::new(writer, &limits).unwrap();
        handle
            .write(&filesystem, &effect_id, None, b"data".to_vec())
            .await
            .unwrap();
        std::fs::create_dir(root.path().join("target")).unwrap();
        assert!(handle.close(&filesystem, &effect_id, None).await.is_err());
        assert_eq!(limits.host_attached_bytes(), 0);
        assert!(handle.close(&filesystem, &effect_id, None).await.is_ok());
    }

    async fn writer_over(vfs: &crate::runtime::Vfs, limits: &TenantLimits) -> BackendFileWriter {
        use crate::runtime::default_filesystem::DefaultFilesystem;
        use crate::runtime::filesystem::Filesystem;
        let writer = DefaultFilesystem::new(vfs.clone(), "run".into())
            .open_writer(&EffectId::stub().unwrap(), None, "/out.txt")
            .await
            .unwrap();
        BackendFileWriter::new(writer, limits).unwrap()
    }

    fn temp_leftovers(root: &std::path::Path) -> Vec<std::ffi::OsString> {
        std::fs::read_dir(root)
            .unwrap()
            .map(|e| e.unwrap().file_name())
            .filter(|n| n.to_string_lossy().starts_with("out.txt."))
            .collect()
    }

    #[tokio::test]
    async fn unclosed_writer_drop_removes_temp_and_skips_rename() {
        let limits = TenantLimits::new(10 * 1024 * 1024);
        let dir = tempfile::tempdir().unwrap();
        let vfs = crate::runtime::Vfs::external(dir.path().to_path_buf()).unwrap();
        let mut w = writer_over(&vfs, &limits).await;
        w.write(
            &DefaultFilesystem::new(vfs.clone(), "run".into()),
            &EffectId::stub().unwrap(),
            None,
            b"hello\n".to_vec(),
        )
        .await
        .unwrap();
        assert!(limits.host_attached_bytes() > 0);
        drop(w);
        assert!(
            temp_leftovers(dir.path()).is_empty(),
            "temp file removed on drop"
        );
        assert!(
            !dir.path().join("out.txt").exists(),
            "no rename without explicit close"
        );
        assert_eq!(limits.host_attached_bytes(), 0);
    }

    #[tokio::test]
    async fn writer_close_renames_and_refunds() {
        let limits = TenantLimits::new(10 * 1024 * 1024);
        let dir = tempfile::tempdir().unwrap();
        let vfs = crate::runtime::Vfs::external(dir.path().to_path_buf()).unwrap();
        let final_path = dir.path().join("out.txt");
        let mut w = writer_over(&vfs, &limits).await;
        w.write(
            &DefaultFilesystem::new(vfs.clone(), "run".into()),
            &EffectId::stub().unwrap(),
            None,
            b"hello\n".to_vec(),
        )
        .await
        .unwrap();
        w.close(
            &DefaultFilesystem::new(vfs.clone(), "run".into()),
            &EffectId::stub().unwrap(),
            None,
        )
        .await
        .unwrap();
        assert!(final_path.exists());
        assert!(temp_leftovers(dir.path()).is_empty());
        assert_eq!(limits.host_attached_bytes(), 0);
        assert_eq!(std::fs::read_to_string(&final_path).unwrap(), "hello\n");
    }
}
