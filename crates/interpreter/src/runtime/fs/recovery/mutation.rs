use super::paths::{checkpoint, current, existing, identity, sync};
use super::plan::Entry;
use super::{
    CapturedContent, Content, FileIdentity, Intent, Mutation, Outcome, RecoveryError, Workspace,
};
use cap_std::fs::{Dir, File, OpenOptions};
use sha2::{Digest, Sha256};
use std::io::{Read, Seek, SeekFrom, Write};

impl Workspace {
    pub(super) fn prepare_mutation(
        &self,
        effect: &str,
        mutation: Mutation,
        content: &mut dyn CapturedContent,
    ) -> Result<Intent, RecoveryError> {
        let (path, follows) = match &mutation {
            Mutation::Write { path, .. }
            | Mutation::Remove { path }
            | Mutation::CreateDirectory { path }
            | Mutation::GitWrite { path, .. }
            | Mutation::GitRemove { path, .. }
            | Mutation::GitDirectory { path, .. } => (path, false),
            Mutation::Append { path, .. } => (path, true),
            Mutation::Move { source, .. } => (source, false),
        };
        let (source, _) = self.entry_access(path, follows, mutation.repository().is_some())?;
        let parent = self.parent(&source)?;
        let metadata = existing(&parent, &source.name)?;
        let original_directory = metadata
            .as_ref()
            .is_some_and(|m| m.is_dir() && !m.file_type().is_symlink());
        if let Some(metadata) = &metadata {
            if original_directory
                && matches!(
                    mutation,
                    Mutation::Remove { .. } | Mutation::GitRemove { .. }
                )
            {
                require_empty(&parent.open_dir(&source.name)?)?;
            } else if !original_directory
                || !matches!(
                    mutation,
                    Mutation::CreateDirectory { .. } | Mutation::GitDirectory { .. }
                )
            {
                self.require_regular(&source, metadata)?;
            }
        }
        let artifacts = self.artifacts(&source, effect, true)?;
        let mut intent = Intent {
            version: 1,
            workspace: self.identity(),
            effect: effect.into(),
            mutation: mutation.clone(),
            source,
            destination: None,
            original: metadata.as_ref().map(identity).transpose()?,
            original_bytes: metadata
                .as_ref()
                .filter(|m| m.is_file())
                .map_or(0, cap_std::fs::Metadata::len),
            original_sha256: None,
            original_directory,
            destination_original: None,
            candidate: None,
        };
        if intent.original_directory {
            sync(&parent.open_dir(&intent.source.name)?)?;
            sync(&parent)?;
        } else if let Some(original) = intent.original {
            let mut file = parent.open(&intent.source.name)?;
            file.sync_all()?;
            sync(&parent)?;
            intent.original_sha256 = Some(hash(&mut file, intent.original_bytes)?);
            pin(
                &parent,
                &intent.source.name,
                &artifacts,
                "original",
                original,
            )?;
        }
        match mutation {
            Mutation::Write {
                content: payload, ..
            }
            | Mutation::GitWrite {
                content: payload, ..
            } => {
                intent.candidate =
                    Some(self.prepare_candidate(&intent.source, &artifacts, &payload, content)?);
            }
            Mutation::Append {
                content: payload, ..
            } => {
                intent
                    .original_bytes
                    .checked_add(payload.bytes)
                    .filter(|bytes| *bytes <= i64::MAX as u64)
                    .ok_or(RecoveryError::Limit)?;
                if intent.original.is_none() {
                    intent.candidate = Some(self.prepare_candidate(
                        &intent.source,
                        &artifacts,
                        &payload,
                        content,
                    )?);
                } else {
                    self.volume(&intent.source.mount)?
                        .check_growth(payload.bytes)?;
                }
            }
            Mutation::Move { destination, .. } => {
                self.prepare_destination(&mut intent, &artifacts, &destination)?;
            }
            Mutation::Remove { .. } | Mutation::GitRemove { .. } => {
                if intent.original.is_none() {
                    return Err(std::io::Error::from(std::io::ErrorKind::NotFound).into());
                }
            }
            Mutation::CreateDirectory { .. } | Mutation::GitDirectory { .. } => {
                if intent.original.is_some() && !intent.original_directory {
                    return Err(std::io::Error::from(std::io::ErrorKind::AlreadyExists).into());
                }
            }
        }
        sync(&artifacts)?;
        checkpoint("prepared")?;
        intent.encode()?;
        Ok(intent)
    }

    fn prepare_candidate(
        &self,
        entry: &Entry,
        artifacts: &Dir,
        payload: &Content,
        content: &mut dyn CapturedContent,
    ) -> Result<FileIdentity, RecoveryError> {
        let present = existing(artifacts, "candidate")?;
        if present
            .as_ref()
            .is_some_and(|m| !m.is_file() || crate::runtime::fs::link_count(m) != Some(1))
        {
            return Err(RecoveryError::Conflict);
        }
        self.volume(&entry.mount)?
            .check_growth(payload.bytes.saturating_sub(present.map_or(0, |m| m.len())))?;
        let mut file = artifacts.open_with(
            "candidate",
            OpenOptions::new()
                .read(true)
                .write(true)
                .create(true)
                .truncate(true),
        )?;
        copy_exact(content.open(payload)?, &mut file, payload)?;
        file.sync_all()?;
        checkpoint("candidate-sync")?;
        sync(artifacts)?;
        identity(&file.metadata()?)
    }

    fn prepare_destination(
        &self,
        intent: &mut Intent,
        artifacts: &Dir,
        guest: &str,
    ) -> Result<(), RecoveryError> {
        if intent.original.is_none() {
            return Err(std::io::Error::from(std::io::ErrorKind::NotFound).into());
        }
        let (destination, _) = self.entry(guest, false)?;
        if destination.mount != intent.source.mount {
            return Err(RecoveryError::Unsupported(
                "cross-volume move requires a compound plan",
            ));
        }
        if destination == intent.source {
            return Err(RecoveryError::Unsupported("move source equals destination"));
        }
        let parent = self.parent(&destination)?;
        if let Some(metadata) = existing(&parent, &destination.name)? {
            self.require_regular(&destination, &metadata)?;
            let original = identity(&metadata)?;
            if Some(original) == intent.original {
                return Err(RecoveryError::Unsupported("aliased move destination"));
            }
            parent.open(&destination.name)?.sync_all()?;
            sync(&parent)?;
            pin(
                &parent,
                &destination.name,
                artifacts,
                "destination",
                original,
            )?;
            intent.destination_original = Some(original);
        }
        intent.destination = Some(destination);
        Ok(())
    }

    pub(super) fn apply_mutation(
        &self,
        intent: &Intent,
        content: &mut dyn CapturedContent,
    ) -> Result<Outcome, RecoveryError> {
        let artifacts = self.artifacts(&intent.source, &intent.effect, false)?;
        match &intent.mutation {
            Mutation::Write {
                content: payload, ..
            }
            | Mutation::GitWrite {
                content: payload, ..
            } => self.publish_candidate(intent, &artifacts, payload, content)?,
            Mutation::Append {
                content: payload, ..
            } if intent.original.is_none() => {
                self.publish_candidate(intent, &artifacts, payload, content)?;
            }
            Mutation::Append {
                content: payload, ..
            } => self.append_range(intent, &artifacts, payload, content)?,
            Mutation::Move { .. } => self.move_entry(intent, &artifacts)?,
            Mutation::Remove { .. } | Mutation::GitRemove { .. } if intent.original_directory => {
                self.remove_directory(intent)?;
            }
            Mutation::Remove { .. } | Mutation::GitRemove { .. } => {
                self.remove_entry(intent, &artifacts)?;
            }
            Mutation::CreateDirectory { .. } | Mutation::GitDirectory { .. } => {
                self.create_directory(intent)?;
            }
        }
        checkpoint("before-receipt")?;
        Ok(Outcome {
            bytes: match &intent.mutation {
                Mutation::Write { content, .. }
                | Mutation::Append { content, .. }
                | Mutation::GitWrite { content, .. } => content.bytes,
                _ => 0,
            },
        })
    }

    fn publish_candidate(
        &self,
        intent: &Intent,
        artifacts: &Dir,
        payload: &Content,
        content: &mut dyn CapturedContent,
    ) -> Result<(), RecoveryError> {
        let candidate = intent.candidate.ok_or(RecoveryError::InvalidRecord)?;
        if current(artifacts, "candidate")? != Some(candidate) {
            return Err(RecoveryError::Conflict);
        }
        let parent = self.parent(&intent.source)?;
        let target = current(&parent, &intent.source.name)?;
        if target != intent.original && target != Some(candidate) {
            return Err(RecoveryError::Conflict);
        }
        if let Some(original) = intent.original {
            require_pin(artifacts, "original", original)?;
        }
        // An uncommitted candidate may have suffered dirty writeback. Repair the
        // same inode, including when the rename reached disk before the receipt.
        let mut file =
            artifacts.open_with("candidate", OpenOptions::new().read(true).write(true))?;
        let length = file.metadata()?.len();
        if length != payload.bytes || hash(&mut file, length)? != payload.sha256 {
            self.volume(&intent.source.mount)?
                .check_growth(payload.bytes.saturating_sub(length))?;
            file.seek(SeekFrom::Start(0))?;
            copy_exact(content.open(payload)?, &mut file, payload)?;
            file.set_len(payload.bytes)?;
        }
        if let Mutation::GitWrite { executable, .. } = intent.mutation {
            #[cfg(unix)]
            {
                use cap_std::fs::PermissionsExt;
                file.set_permissions(cap_std::fs::Permissions::from_mode(if executable {
                    0o755
                } else {
                    0o644
                }))?;
            }
            #[cfg(not(unix))]
            if executable {
                return Err(RecoveryError::Unsupported("Git executable mode"));
            }
        }
        file.sync_all()?;
        checkpoint("candidate-sync")?;
        if target != Some(candidate) {
            pin(artifacts, "candidate", artifacts, "publish", candidate)?;
            artifacts.rename("publish", &parent, &intent.source.name)?;
            checkpoint("published")?;
        }
        sync(&parent)?;
        sync(artifacts)?;
        Ok(())
    }

    fn append_range(
        &self,
        intent: &Intent,
        artifacts: &Dir,
        payload: &Content,
        content: &mut dyn CapturedContent,
    ) -> Result<(), RecoveryError> {
        let original = intent.original.ok_or(RecoveryError::InvalidRecord)?;
        require_pin(artifacts, "original", original)?;
        let parent = self.parent(&intent.source)?;
        if current(&parent, &intent.source.name)? != Some(original) {
            return Err(RecoveryError::Conflict);
        }
        let final_length = intent
            .original_bytes
            .checked_add(payload.bytes)
            .filter(|bytes| *bytes <= i64::MAX as u64)
            .ok_or(RecoveryError::Limit)?;
        // Deliberately omit O_APPEND: every retry repairs the original interval.
        let mut file =
            artifacts.open_with("original", OpenOptions::new().read(true).write(true))?;
        let length = file.metadata()?.len();
        if length < intent.original_bytes
            || length > final_length
            || Some(hash(&mut file, intent.original_bytes)?) != intent.original_sha256
        {
            return Err(RecoveryError::Conflict);
        }
        self.volume(&intent.source.mount)?
            .check_growth(final_length - length)?;
        file.seek(SeekFrom::Start(intent.original_bytes))?;
        copy_exact(content.open(payload)?, &mut file, payload)?;
        file.set_len(final_length)?;
        checkpoint("append-written")?;
        file.sync_all()?;
        checkpoint("append-sync")?;
        sync(&parent)?;
        Ok(())
    }

    fn move_entry(&self, intent: &Intent, artifacts: &Dir) -> Result<(), RecoveryError> {
        let original = intent.original.ok_or(RecoveryError::InvalidRecord)?;
        require_pin(artifacts, "original", original)?;
        let destination = intent
            .destination
            .as_ref()
            .ok_or(RecoveryError::InvalidRecord)?;
        let source_parent = self.parent(&intent.source)?;
        let destination_parent = self.parent(destination)?;
        let source = current(&source_parent, &intent.source.name)?;
        let target = current(&destination_parent, &destination.name)?;
        if ![None, Some(original)].contains(&source)
            || ![intent.destination_original, Some(original)].contains(&target)
        {
            return Err(RecoveryError::Conflict);
        }
        if target != Some(original) {
            pin(artifacts, "original", artifacts, "publish", original)?;
            artifacts.rename("publish", &destination_parent, &destination.name)?;
            checkpoint("destination-published")?;
        }
        // A destination found on retry may be visible but not yet durable.
        // Persist it before unlinking the source on every attempt.
        sync(&destination_parent)?;
        if source.is_some() {
            source_parent.remove_file(&intent.source.name)?;
            checkpoint("source-removed")?;
        }
        sync(&source_parent)?;
        sync(&destination_parent)?;
        sync(artifacts)?;
        Ok(())
    }

    fn remove_entry(&self, intent: &Intent, artifacts: &Dir) -> Result<(), RecoveryError> {
        let original = intent.original.ok_or(RecoveryError::InvalidRecord)?;
        require_pin(artifacts, "original", original)?;
        let parent = self.parent(&intent.source)?;
        match current(&parent, &intent.source.name)? {
            None => {}
            Some(current) if current == original => {
                parent.remove_file(&intent.source.name)?;
                checkpoint("source-removed")?;
            }
            Some(_) => return Err(RecoveryError::Conflict),
        }
        sync(&parent)
    }

    fn create_directory(&self, intent: &Intent) -> Result<(), RecoveryError> {
        let parent = self.parent(&intent.source)?;
        match existing(&parent, &intent.source.name)? {
            None if intent.original.is_none() => {
                parent.create_dir(&intent.source.name)?;
                checkpoint("directory-created")?;
            }
            Some(metadata) if metadata.is_dir() && !metadata.file_type().is_symlink() => {
                if let Some(original) = intent.original {
                    if identity(&metadata)? != original {
                        return Err(RecoveryError::Conflict);
                    }
                } else {
                    // No later mutation is admitted before this receipt. A new
                    // pending mkdir may only have produced an empty directory.
                    require_empty(&parent.open_dir(&intent.source.name)?)?;
                }
            }
            _ => return Err(RecoveryError::Conflict),
        }
        sync(&parent.open_dir(&intent.source.name)?)?;
        sync(&parent)
    }

    fn remove_directory(&self, intent: &Intent) -> Result<(), RecoveryError> {
        let parent = self.parent(&intent.source)?;
        if let Some(metadata) = existing(&parent, &intent.source.name)? {
            if Some(identity(&metadata)?) != intent.original
                || !metadata.is_dir()
                || metadata.file_type().is_symlink()
            {
                return Err(RecoveryError::Conflict);
            }
            require_empty(&parent.open_dir(&intent.source.name)?)?;
            parent.remove_dir(&intent.source.name)?;
            checkpoint("directory-removed")?;
        }
        sync(&parent)
    }
}

fn require_empty(dir: &Dir) -> Result<(), RecoveryError> {
    if let Some(entry) = dir.entries()?.next() {
        entry?;
        return Err(std::io::Error::from(std::io::ErrorKind::DirectoryNotEmpty).into());
    }
    Ok(())
}

pub(super) fn pin(
    source: &Dir,
    name: &str,
    artifacts: &Dir,
    retained: &str,
    expected: FileIdentity,
) -> Result<(), RecoveryError> {
    if current(source, name)? != Some(expected) {
        return Err(RecoveryError::Conflict);
    }
    match current(artifacts, retained)? {
        Some(actual) if actual != expected => return Err(RecoveryError::Conflict),
        Some(_) => {}
        None => source.hard_link(name, artifacts, retained)?,
    }
    checkpoint("pin-created")?;
    sync(artifacts)?;
    Ok(())
}

fn require_pin(artifacts: &Dir, name: &str, expected: FileIdentity) -> Result<(), RecoveryError> {
    if current(artifacts, name)? != Some(expected) {
        return Err(RecoveryError::Conflict);
    }
    Ok(())
}

fn buffer() -> Result<Vec<u8>, RecoveryError> {
    let mut buffer = Vec::new();
    buffer
        .try_reserve_exact(64 * 1024)
        .map_err(|_| RecoveryError::Limit)?;
    buffer.resize(64 * 1024, 0);
    Ok(buffer)
}

fn hash(file: &mut File, bytes: u64) -> Result<[u8; 32], RecoveryError> {
    file.seek(SeekFrom::Start(0))?;
    let mut remaining = bytes;
    let mut digest = Sha256::new();
    let mut buffer = buffer()?;
    while remaining != 0 {
        let count = remaining.min(buffer.len() as u64) as usize;
        file.read_exact(&mut buffer[..count])?;
        digest.update(&buffer[..count]);
        remaining -= count as u64;
    }
    Ok(digest.finalize().into())
}

fn copy_exact(
    mut source: Box<dyn Read + Send>,
    file: &mut File,
    payload: &Content,
) -> Result<(), RecoveryError> {
    let mut remaining = payload.bytes;
    let mut digest = Sha256::new();
    let mut buffer = buffer()?;
    while remaining != 0 {
        let count = remaining.min(buffer.len() as u64) as usize;
        source.read_exact(&mut buffer[..count])?;
        digest.update(&buffer[..count]);
        // Fault tests can stop between chunks, exercising partial append.
        file.write_all(&buffer[..count])?;
        remaining -= count as u64;
        checkpoint("write-chunk")?;
    }
    let mut trailing = [0];
    if source.read(&mut trailing)? != 0 || <[u8; 32]>::from(digest.finalize()) != payload.sha256 {
        return Err(RecoveryError::Conflict);
    }
    Ok(())
}
