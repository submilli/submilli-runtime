use super::{FileWriter, FilesystemError, Invocation};
use crate::runtime::fs::{ContainError, ContentPath, FileIdentity};
use crate::runtime::{DiskQuota, Holder, OpenFileGuard, QuotaCharge, QuotaExceeded};
use std::{
    fs::File,
    io::{BufWriter, Write},
    sync::Arc,
};
/// Buffered writer to a temp sibling file. Explicit [`close`](Self::close) fsyncs and
/// atomically renames it onto `final_path`; `Drop` (the safety net for an un-`close`d
/// writer) discards the temp file without renaming, removing it only while its name
/// still refers to the writer's file.
pub struct NativeWriter {
    /// `None` once the writer is closed.
    open: Option<OpenWrite>,
    /// Both sides stay contained paths for the whole lifetime of the writer. The commit
    /// happens through the same handle the open resolved against, so a link swapped over
    /// the parent between `writer()` and `close()` cannot redirect it — and the drop path
    /// cannot unlink something under the server's working directory.
    temp: TempFile,
    final_path: ContentPath,
}

/// The temp file a writer streams into. The program can reach it by name while the
/// writer is open, so before committing or discarding it, the writer checks that the
/// name still refers to the file it created.
pub struct TempFile {
    path: ContentPath,
    identity: FileIdentity,
}

impl TempFile {
    /// The temp file just created at `path` and opened as `file`.
    pub fn created(path: ContentPath, file: &cap_std::fs::File) -> std::io::Result<Self> {
        Ok(Self {
            path,
            identity: FileIdentity::of(&file.metadata()?)?,
        })
    }

    fn is_intact(&self) -> Result<bool, ContainError> {
        self.path.refers_to(self.identity)
    }
}

/// An open writer's buffer and its claim on the VFS's size limit. Program code runs
/// between writes, so the claim replaces nothing up front: every byte written counts
/// until `close` covers the file it replaces.
struct OpenWrite {
    writer: BufWriter<File>,
    disk_charge: QuotaCharge,
    /// Registers the temp file as the writer's own, so a removal of it by name
    /// leaves its bytes for the writer to settle.
    held: Option<OpenFileGuard>,
}

/// Why a write through a [`NativeWriter`] failed.
#[derive(Debug)]
pub enum WriteError {
    Io(std::io::Error),
    Contain(ContainError),
    Full(QuotaExceeded),
}

impl std::fmt::Display for WriteError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            WriteError::Io(e) => e.fmt(f),
            WriteError::Contain(e) => e.fmt(f),
            WriteError::Full(e) => e.fmt(f),
        }
    }
}

impl NativeWriter {
    pub fn new(
        file: File,
        temp: TempFile,
        final_path: ContentPath,
        quota: Option<Arc<DiskQuota>>,
    ) -> Self {
        let held = quota
            .as_ref()
            .map(|quota| quota.hold(temp.identity, Holder::Writer));
        Self {
            open: Some(OpenWrite {
                writer: BufWriter::new(file),
                disk_charge: QuotaCharge::new(quota, None),
                held,
            }),
            temp,
            final_path,
        }
    }

    pub fn write_line(&mut self, line: &str) -> Result<(), WriteError> {
        let open = self.reserve(line.len() as u64 + 1)?;
        writeln!(open.writer, "{line}").map_err(WriteError::Io)
    }

    pub fn write_bytes(&mut self, bytes: &[u8]) -> Result<(), WriteError> {
        let open = self.reserve(bytes.len() as u64)?;
        open.writer.write_all(bytes).map_err(WriteError::Io)
    }

    fn reserve(&mut self, bytes: u64) -> Result<&mut OpenWrite, WriteError> {
        let open = self
            .open
            .as_mut()
            .ok_or_else(|| WriteError::Io(std::io::Error::other("writer already closed")))?;
        open.disk_charge.reserve(bytes).map_err(WriteError::Full)?;
        Ok(open)
    }

    /// Flush, fsync, and atomically rename temp→final. Idempotent: a second call after a
    /// successful close is a no-op.
    pub fn close(&mut self) -> Result<(), WriteError> {
        let Some(open) = self.open.take() else {
            return Ok(());
        };
        finalize(open, &self.temp, &self.final_path)
    }
}

impl Drop for NativeWriter {
    fn drop(&mut self) {
        if let Some(open) = self.open.take() {
            // Discard the temp file rather than rename it: a GC-timed rename would be
            // non-deterministic.
            let unlinked = open
                .writer
                .into_inner()
                .ok()
                .is_some_and(|file| is_unlinked(&file));
            discard_temp(&self.temp, open.disk_charge, unlinked);
            drop(open.held);
        }
    }
}

/// Whether `file` has no name left, so its data goes once every handle on it
/// closes.
fn is_unlinked(file: &File) -> bool {
    cap_std::fs::Metadata::from_file(file)
        .is_ok_and(|meta| crate::runtime::fs::link_count(&meta) == Some(0))
}

/// Remove an uncommitted temp file and settle its disk charge. The program can reach
/// the temp file by name while the writer is open. If the name no longer refers to
/// the writer's file, it was moved or replaced: nothing is removed, and its bytes
/// stay counted while they may still be on disk under another name. Once the file
/// has no name at all, whether removed here or by the program, its bytes go when
/// the last handle on it closes.
fn discard_temp(temp: &TempFile, disk_charge: QuotaCharge, unlinked: bool) {
    let removed = temp.is_intact().unwrap_or(false) && temp.path.remove_file().is_ok();
    if removed || unlinked {
        disk_charge.release_when_closed(temp.identity);
    } else {
        disk_charge.keep();
    }
}

/// Commit the temp file over the final one. Any failure discards the temp file.
fn finalize(open: OpenWrite, temp: &TempFile, final_path: &ContentPath) -> Result<(), WriteError> {
    let OpenWrite {
        writer,
        mut disk_charge,
        held,
    } = open;
    let committed = match flush_and_sync(writer) {
        Ok(unlinked) => {
            let renamed = rename_into_place(temp, final_path, &mut disk_charge);
            match renamed {
                Ok(()) => disk_charge.commit(),
                Err(_) => discard_temp(temp, disk_charge, unlinked),
            }
            renamed
        }
        Err(err) => {
            discard_temp(temp, disk_charge, false);
            Err(err)
        }
    };
    drop(held);
    committed
}

/// Flush and sync the writer's file and close it, reporting whether the file has no
/// name left. Closing before the rename lets Windows handles release; unix `rename`
/// over an open file works either way.
fn flush_and_sync(writer: BufWriter<File>) -> Result<bool, WriteError> {
    let mut file = writer
        .into_inner()
        .map_err(|e| WriteError::Io(e.into_error()))?;
    file.flush().map_err(WriteError::Io)?;
    file.sync_all().map_err(WriteError::Io)?;
    Ok(is_unlinked(&file))
}

/// Rename the temp file over the final one, first covering what it replaces.
fn rename_into_place(
    temp: &TempFile,
    final_path: &ContentPath,
    disk_charge: &mut QuotaCharge,
) -> Result<(), WriteError> {
    if !temp.is_intact().map_err(WriteError::Contain)? {
        return Err(WriteError::Io(std::io::Error::other(
            "the writer's temporary file was moved or replaced",
        )));
    }
    disk_charge
        .cover(final_path.regular_file())
        .map_err(WriteError::Full)?;
    temp.path.rename_to(final_path).map_err(WriteError::Contain)
}

#[async_trait::async_trait]
impl FileWriter for NativeWriter {
    fn is_closed(&self) -> bool {
        self.open.is_none()
    }
    async fn write(&mut self, _: &Invocation, bytes: Vec<u8>) -> Result<(), FilesystemError> {
        self.write_bytes(&bytes).map_err(Into::into)
    }
    async fn close(&mut self, _: &Invocation) -> Result<(), FilesystemError> {
        NativeWriter::close(self).map_err(Into::into)
    }
}
impl From<WriteError> for FilesystemError {
    fn from(error: WriteError) -> Self {
        match error {
            WriteError::Io(e) => Self::Io(e),
            WriteError::Contain(e) => Self::Containment(e),
            WriteError::Full(e) => Self::Quota(e),
        }
    }
}
