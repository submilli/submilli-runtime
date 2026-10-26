//! Atomic, disposable download staging for ordinary execution.
use super::*;
use crate::runtime::filesystem::{Download, DownloadOptions};

pub(super) async fn begin(
    vfs: Vfs,
    path: String,
    options: DownloadOptions,
) -> Result<Box<dyn Download>, FilesystemError> {
    blocking(move || {
        let destination = fs::resolve_content(&vfs, vfs.cwd(), &path)?;
        destination.writable()?;
        if destination.is_root() {
            return Err(FilesystemError::Rejected(
                "the VFS root or a mount point is a directory, not a file".into(),
            ));
        }
        if !options.overwrite && destination.try_exists()? {
            return Err(FilesystemError::Rejected(
                "file exists (pass { overwrite: true } to clobber)".into(),
            ));
        }
        destination.check_rename_end()?;
        let temporary = destination.temp_sibling();
        let file = temporary.create_new().map_err(|error| match error {
            ContainError::Io(error) if error.kind() == std::io::ErrorKind::NotFound => {
                FilesystemError::Rejected("parent directory does not exist".into())
            }
            other => other.into(),
        })?;
        let quota = QuotaCharge::new(
            destination.placement().quota().cloned(),
            destination.regular_file(),
        );
        Ok(Box::new(StagedDownload {
            destination,
            temporary,
            file,
            quota,
            maximum: options.maximum_bytes,
            written: 0,
            error: None,
        }) as Box<dyn Download>)
    })
    .await
}

struct StagedDownload {
    destination: ContentPath,
    temporary: ContentPath,
    file: cap_std::fs::File,
    quota: QuotaCharge,
    maximum: u64,
    written: u64,
    error: Option<FilesystemError>,
}

impl Write for StagedDownload {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        let result = self.write_chunk(bytes);
        match result {
            Ok(count) => Ok(count),
            Err(error) => {
                let message = error.to_string();
                self.error = Some(error);
                Err(std::io::Error::other(message))
            }
        }
    }
    fn flush(&mut self) -> std::io::Result<()> {
        self.file.flush()
    }
}

impl StagedDownload {
    fn write_chunk(&mut self, bytes: &[u8]) -> Result<usize, FilesystemError> {
        if bytes.len() as u64 > self.maximum.saturating_sub(self.written) {
            return Err(FilesystemError::Execution(
                crate::runtime::host::range_error("http.download: maximum download size exceeded"),
            ));
        }
        self.quota.reserve(bytes.len() as u64)?;
        let result = self.file.write(bytes);
        let written = result.as_ref().copied().unwrap_or(0) as u64;
        self.quota.unreserve(bytes.len() as u64 - written);
        self.written += written;
        result.map_err(Into::into)
    }
}

#[async_trait::async_trait]
impl Download for StagedDownload {
    fn bytes_written(&self) -> u64 {
        self.written
    }
    fn take_error(&mut self) -> Option<FilesystemError> {
        self.error.take()
    }
    async fn publish(mut self: Box<Self>, _work: Arc<WorkBudget>) -> Result<(), FilesystemError> {
        blocking(move || {
            self.file.sync_all()?;
            self.quota.cover(self.destination.regular_file())?;
            self.temporary.rename_to(&self.destination)?;
            std::mem::replace(&mut self.quota, QuotaCharge::new(None, None)).commit();
            Ok(())
        })
        .await
    }
}

impl Drop for StagedDownload {
    fn drop(&mut self) {
        let _ = self.temporary.remove_file();
    }
}
