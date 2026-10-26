//! Download admission is durable; streaming and publication have no SQL boundary.
//! Missing effect history always downloads again, even after an earlier rename.
use super::*;
use crate::io::{Entry, existing, sync};
use cap_std::fs::{Dir, File, OpenOptions};
use interpreter::runtime::filesystem::{Download, DownloadOptions};
use std::io::Write;

const CHECKED: &[u8] = br#"{"Checked":{}}"#;

pub(super) async fn begin(
    filesystem: DurableFilesystem,
    effect: EffectId,
    path: String,
    options: DownloadOptions,
    work: Arc<WorkBudget>,
) -> Result<Box<dyn Download>, PortError> {
    validate_id(&effect).map_err(backend)?;
    paths::validate_guest_path(&path)
        .map_err(FilesystemError::from)
        .map_err(backend)?;
    let gate = Arc::new(filesystem.filesystem_gate.clone().lock_owned().await);
    let mut request = filesystem.filesystem_config_fingerprint.to_vec();
    request.extend_from_slice(&Sha256::digest(
        serialize(&(&path, &options)).map_err(backend)?,
    ));
    let admission = JournalEntry {
        id: effect.clone(),
        run_id: filesystem.run_id.clone(),
        operation: "download-admission".into(),
        version: 4,
        request,
        state: Some(CHECKED.to_vec()),
    };
    let mut unit = (filesystem.units)()
        .await
        .map_err(FilesystemError::from)
        .map_err(backend)?;
    let prior = unit
        .get_journal_entry(&filesystem.run_id, &effect)
        .await
        .map_err(FilesystemError::from)
        .map_err(backend)?;
    if prior.as_ref().is_some_and(|prior| prior != &admission) {
        return Err(backend(RecoveryError::Conflict.into()));
    }
    let checked = prior.is_some();
    if !checked {
        require_idle(unit.as_mut(), &filesystem.run_id)
            .await
            .map_err(backend)?;
        require_capacity(unit.as_mut(), &filesystem.run_id)
            .await
            .map_err(backend)?;
        require_unused_operation(unit.as_mut(), &filesystem.run_id, &effect)
            .await
            .map_err(backend)?;
    }
    let files = filesystem.files.clone();
    let target = path.clone();
    let overwrite = options.overwrite || checked;
    let entry = blocking(gate.clone(), move || {
        files.admit_download(&target, overwrite, &work)
    })
    .await
    .map_err(download_error)?;
    if !checked {
        // Own the gate through SQL completion and an authoritative reread after
        // acknowledgement loss, even if the awaiting download is cancelled.
        let retained = gate.clone();
        let retry_filesystem = filesystem.clone();
        finish_owned(async move {
            let _gate = retained;
            unit.save_journal_entry(admission.clone()).await?;
            if let Err(error) = unit.commit().await {
                let mut authoritative = (retry_filesystem.units)().await?;
                if authoritative
                    .get_journal_entry(&admission.run_id, &admission.id)
                    .await?
                    != Some(admission)
                {
                    return Err(error.into());
                }
            }
            test_checkpoint("download-checked");
            Ok(())
        })
        .await
        .map_err(backend)?;
    }
    let files = filesystem.files.clone();
    let retained = gate.clone();
    blocking(gate, move || {
        let directory = files.volume(&entry.mount)?.recovery.try_clone()?;
        let name = format!("{effect}.download");
        // A crash can preserve both rename names. Unlink our name before
        // creating a fresh inode; never truncate a possibly published file.
        match directory.remove_file(&name) {
            Ok(()) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => return Err(error.into()),
        }
        let file = directory.open_with(&name, OpenOptions::new().write(true).create_new(true))?;
        Ok(Box::new(StagedDownload {
            files,
            entry,
            path,
            directory,
            name,
            file,
            gate: retained,
            maximum: options.maximum_bytes,
            written: 0,
            error: None,
        }) as Box<dyn Download>)
    })
    .await
    .map_err(backend)
}

impl Files {
    fn admit_download(
        &self,
        path: &str,
        overwrite: bool,
        work: &WorkBudget,
    ) -> Result<Entry, RecoveryError> {
        // Content resolution preserves existing download symlink containment.
        let resolved = paths::resolve_content(self.vfs(), self.vfs().cwd(), path)?;
        if resolved.is_root() {
            return Err(RecoveryError::Unsupported(
                "the VFS root or a mount point is a directory, not a file",
            ));
        }
        let entry = self.entry(path, false)?;
        let parent = self.parent(&entry)?;
        if !overwrite && resolved.try_exists()? {
            return Err(std::io::Error::new(
                std::io::ErrorKind::AlreadyExists,
                "file exists (pass { overwrite: true } to clobber)",
            )
            .into());
        }
        if let Some(metadata) = existing(&parent, &entry.name)? {
            if metadata.file_type().is_symlink() {
                // Follow final links for containment and protected metadata checks,
                // while publication still replaces the link itself.
                resolved.check_rename_end()?;
            } else {
                self.require_regular(&entry, &metadata, Some(work))?;
            }
        }
        Ok(entry)
    }
}

struct StagedDownload {
    files: Arc<Files>,
    entry: Entry,
    path: String,
    directory: Dir,
    name: String,
    file: File,
    gate: Arc<tokio::sync::OwnedMutexGuard<()>>,
    maximum: u64,
    written: u64,
    error: Option<PortError>,
}

impl Write for StagedDownload {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        if bytes.len() as u64 > self.maximum.saturating_sub(self.written) {
            self.error = Some(PortError::Execution(
                interpreter::runtime::host::range_error(
                    "http.download: maximum download size exceeded",
                ),
            ));
            return Err(std::io::Error::other("maximum download size exceeded"));
        }
        // Only one bounded disposable stage exists for this serialized attempt.
        // Physical capacity errors propagate; private bytes do not use guest quota.
        let written = self.file.write(bytes)?;
        self.written += written as u64;
        test_checkpoint("download-streamed");
        Ok(written)
    }
    fn flush(&mut self) -> std::io::Result<()> {
        self.file.flush()
    }
}

#[async_trait::async_trait]
impl Download for StagedDownload {
    fn bytes_written(&self) -> u64 {
        self.written
    }
    fn take_error(&mut self) -> Option<PortError> {
        self.error.take()
    }
    async fn publish(self: Box<Self>, work: Arc<WorkBudget>) -> Result<(), PortError> {
        tokio::task::spawn_blocking(move || self.publish_file(&work))
            .await
            .map_err(|error| PortError::Backend(Box::new(error)))?
    }
}

impl StagedDownload {
    fn publish_file(&self, work: &WorkBudget) -> Result<(), PortError> {
        let prepared = (|| -> Result<_, RecoveryError> {
            let entry = self.files.admit_download(&self.path, true, work)?;
            if entry != self.entry {
                return Err(RecoveryError::Conflict);
            }
            let parent = self.files.parent(&entry)?;
            let previous = existing(&parent, &entry.name)?
                .filter(cap_std::fs::Metadata::is_file)
                .map_or(0, |metadata| metadata.len());
            let volume = self.files.volume(&entry.mount)?;
            volume.check_growth_before_mutation(self.written.saturating_sub(previous))?;
            self.file.sync_all()?;
            test_checkpoint("download-file-synced");
            Ok((parent, previous))
        })()
        .map_err(|error| download_error(error.into()))?;
        let (parent, previous) = prepared;
        // The owned gate survives cancellation until publication and sync finish.
        let _gate = &self.gate;
        let published = (|| -> Result<(), RecoveryError> {
            self.directory
                .rename(&self.name, &parent, &self.entry.name)?;
            self.files
                .volume(&self.entry.mount)?
                .record_size_change(previous, self.written);
            test_checkpoint("download-published");
            sync(&parent)?;
            sync(&self.directory)?;
            test_checkpoint("download-synced");
            Ok(())
        })();
        published.map_err(|error| backend(error.into()))
    }
}

impl Drop for StagedDownload {
    fn drop(&mut self) {
        let _ = self.directory.remove_file(&self.name);
    }
}

fn download_error(error: FilesystemError) -> PortError {
    match error {
        FilesystemError::Recovery(RecoveryError::QuotaRefused(error)) => PortError::Quota(error),
        FilesystemError::Recovery(error) if preflight_refusal(&error) => {
            PortError::Rejected(error.to_string())
        }
        other => backend(other),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn download_resolves_lost_admission_acknowledgement() {
        let root = tempfile::tempdir().unwrap();
        let (units, state) = crate::unit_of_work::tests::filesystem_units();
        let run = uuid::Uuid::new_v4().to_string().into();
        let filesystem =
            DurableFilesystem::open(Vfs::external(root.path().to_owned()).unwrap(), &run, units)
                .await
                .unwrap();
        state.lock().unwrap().lose_checked_ack = true;
        let effect = uuid::Uuid::new_v4().to_string().into();
        let mut download = filesystem
            .begin_download(
                &effect,
                "/file",
                DownloadOptions {
                    overwrite: false,
                    maximum_bytes: 100,
                    request: [1; 32],
                },
                Arc::new(interpreter::runtime::fuel::WorkBudget::new(u64::MAX)),
            )
            .await
            .unwrap();
        download.write_all(b"complete").unwrap();
        download
            .publish(Arc::new(interpreter::runtime::fuel::WorkBudget::new(
                u64::MAX,
            )))
            .await
            .unwrap();
        assert_eq!(
            std::fs::read(root.path().join("file")).unwrap(),
            b"complete"
        );
    }
}
