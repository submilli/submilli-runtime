use super::operation::FilesystemOperation;
use super::{
    FilesystemError, blocking, deserialize, fingerprint, preflight_refusal, serialize,
    test_checkpoint, validate_journal_configuration,
};
use crate::io::{self as paths, Files, RecoveryError, existing, sync, validate_guest_path};
use crate::unit_of_work::{JournalEntry, Outcome};
use cap_std::fs::OpenOptions;
use interpreter::runtime::durable::EffectId;
use serde::{Deserialize, Serialize};
use std::io::{Seek, SeekFrom, Write};
use std::sync::Arc;

/// Saved progress for this operation. Every unfinished stage is safe to repeat.
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
enum AppendState {
    #[serde(skip)]
    Initial,
    Prepared {
        offset: u64,
    },
    Completed(Result<Outcome, String>),
}

impl FilesystemOperation {
    pub async fn append_bytes(
        &self,
        effect: &EffectId,
        path: &str,
        bytes: Vec<u8>,
    ) -> Result<Outcome, FilesystemError> {
        validate_guest_path(path)?;
        self.charge(interpreter::runtime::fuel::HASH, bytes.len() as u64)?;
        let input_fingerprint = fingerprint(effect, &bytes)?;
        let bytes = Arc::new(bytes);
        let gate = Arc::new(self.filesystem.filesystem_gate.clone().lock_owned().await);
        let mut unit = (self.filesystem.units)().await?;
        let mut journal_entry = self
            .create_initial_or_get_last_journal_entry(
                unit.as_mut(),
                effect,
                "append",
                &(path, &input_fingerprint),
            )
            .await?;
        unit.commit().await?;

        let mut state = decode_state(
            &journal_entry,
            self.filesystem.filesystem_config_fingerprint,
        )?;
        if !matches!(state, AppendState::Completed(_)) {
            self.charge(interpreter::runtime::fuel::IO, input_fingerprint.bytes)?;
            test_checkpoint("effect-request");
        }
        loop {
            state = match state {
                AppendState::Initial => {
                    let prepared = {
                        let appended_bytes = input_fingerprint.bytes;
                        let files = self.filesystem.files.clone();
                        let work = self.work.clone();
                        let path = path.to_owned();
                        blocking(gate.clone(), move || {
                            let offset = files.size(&path, work.as_deref())?;
                            offset
                                .checked_add(appended_bytes)
                                .filter(|end| *end <= i64::MAX as u64)
                                .ok_or(RecoveryError::Limit)?;
                            files.check_growth(&path, appended_bytes)?;
                            paths::test_checkpoint("prepared")?;
                            Ok(offset)
                        })
                        .await
                    };
                    match prepared {
                        Ok(offset) => AppendState::Prepared { offset },
                        Err(FilesystemError::Recovery(error)) if preflight_refusal(&error) => {
                            AppendState::Completed(Err(error.to_string()))
                        }
                        Err(error) => return Err(error),
                    }
                }
                AppendState::Prepared { offset } => {
                    let written_bytes = input_fingerprint.bytes;
                    if let Some(work) = &self.work {
                        work.before_effect();
                    }
                    let files = self.filesystem.files.clone();
                    let bytes = bytes.clone();
                    let path = path.to_owned();
                    blocking(gate.clone(), move || {
                        files.append_at_saved_offset(&path, offset, &bytes)
                    })
                    .await?;
                    test_checkpoint("effect-applied");
                    AppendState::Completed(Ok(Outcome {
                        bytes: written_bytes,
                    }))
                }
                AppendState::Completed(result) => return result.map_err(FilesystemError::Rejected),
            };
            encode_state(&mut journal_entry, &state)?;
            self.save_journal_entry(&journal_entry).await?;
            match &state {
                AppendState::Prepared { .. } => test_checkpoint("effect-intent"),
                AppendState::Completed(Ok(_)) => test_checkpoint("effect-receipt"),
                _ => {}
            }
        }
    }
}

fn decode_state(
    journal_entry: &JournalEntry,
    config: [u8; 32],
) -> Result<AppendState, FilesystemError> {
    validate_journal_configuration(journal_entry, config)?;
    match &journal_entry.state {
        Some(bytes) => deserialize(bytes),
        None => Ok(AppendState::Initial),
    }
}

fn encode_state(
    journal_entry: &mut JournalEntry,
    state: &AppendState,
) -> Result<(), FilesystemError> {
    journal_entry.state = match state {
        AppendState::Initial => None,
        _ => Some(serialize(state)?),
    };
    Ok(())
}

impl Files {
    fn append_at_saved_offset(
        &self,
        path: &str,
        offset: u64,
        bytes: &[u8],
    ) -> Result<(), RecoveryError> {
        let end = offset
            .checked_add(bytes.len() as u64)
            .filter(|end| *end <= i64::MAX as u64)
            .ok_or(RecoveryError::InvalidRecord)?;
        let entry = self.entry(path, true)?;
        let parent = self.parent(&entry)?;
        let mut length = existing(&parent, &entry.name)?.map_or(0, |metadata| metadata.len());
        if length < offset || length > end {
            return Err(RecoveryError::Conflict);
        }
        let volume = self.volume(&entry.mount)?;
        volume.check_growth(end - length)?;
        // Do not use O_APPEND: a partial retry overwrites the original range.
        let mut file = parent.open_with(
            &entry.name,
            OpenOptions::new().read(true).write(true).create(true),
        )?;
        file.seek(SeekFrom::Start(offset))?;
        let mut position = offset;
        for chunk in bytes.chunks(64 * 1024) {
            let mut remaining = chunk;
            while !remaining.is_empty() {
                let written = match file.write(remaining) {
                    Ok(0) => return Err(std::io::Error::from(std::io::ErrorKind::WriteZero).into()),
                    Ok(written) => written,
                    Err(error) if error.kind() == std::io::ErrorKind::Interrupted => continue,
                    Err(error) => return Err(error.into()),
                };
                // File::write returns at most remaining.len(); the checked end
                // above bounds position. Count partial public writes as well.
                position += written as u64;
                let next_length = length.max(position);
                crate::test_checkpoint("append-written-unaccounted");
                volume.record_size_change(length, next_length);
                length = next_length;
                remaining = &remaining[written..];
            }
            paths::test_checkpoint("write-chunk")?;
        }
        file.set_len(end)?;
        paths::test_checkpoint("append-written")?;
        file.sync_all()?;
        paths::test_checkpoint("append-sync")?;
        sync(&parent)?;
        paths::test_checkpoint("before-receipt")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_missing_state_decodes_as_initial() {
        let mut entry = JournalEntry {
            run_id: "run".into(),
            id: "effect".into(),
            operation: "append".into(),
            version: 4,
            request: vec![0; 64],
            state: None,
        };
        assert!(matches!(
            decode_state(&entry, [0; 32]).unwrap(),
            AppendState::Initial
        ));
        for bytes in [
            b"broken".as_slice(),
            br#""Unknown""#,
            br#""Initial""#,
            br#"{"Prepared":null}"#,
            br#"{"Prepared":{}}"#,
            br#"{"Prepared":{"offset":-1}}"#,
            br#"{"Prepared":{"offset":0,"entry":{}}}"#,
        ] {
            entry.state = Some(bytes.to_vec());
            assert!(decode_state(&entry, [0; 32]).is_err());
        }
        encode_state(&mut entry, &AppendState::Prepared { offset: 6 }).unwrap();
        assert!(matches!(
            decode_state(&entry, [0; 32]).unwrap(),
            AppendState::Prepared { offset: 6 }
        ));
        encode_state(
            &mut entry,
            &AppendState::Completed(Ok(Outcome { bytes: 3 })),
        )
        .unwrap();
        assert!(matches!(
            decode_state(&entry, [0; 32]).unwrap(),
            AppendState::Completed(Ok(Outcome { bytes: 3 }))
        ));
        encode_state(&mut entry, &AppendState::Completed(Err("refused".into()))).unwrap();
        assert!(
            matches!(decode_state(&entry, [0; 32]).unwrap(), AppendState::Completed(Err(reason)) if reason == "refused")
        );
    }
}
