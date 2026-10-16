use super::{FileIdentity, RecoveryError, paths};
use serde::{Deserialize, Serialize};

/// An immutable object-store reference, never a host path or mutable inode.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Content {
    pub id: String,
    pub bytes: u64,
    pub sha256: [u8; 32],
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WorkspaceIdentity {
    pub id: String,
    pub volumes: Vec<VolumeIdentity>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct VolumeIdentity {
    pub mount: String,
    pub root: FileIdentity,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub enum Mutation {
    Write {
        path: String,
        content: Content,
    },
    Append {
        path: String,
        content: Content,
    },
    Move {
        source: String,
        destination: String,
    },
    Remove {
        path: String,
    },
    CreateDirectory {
        path: String,
    },
    /// Trusted publication after the caller admits the Git repository capability.
    GitWrite {
        repository: String,
        path: String,
        content: Content,
        executable: bool,
    },
    GitRemove {
        repository: String,
        path: String,
    },
    GitDirectory {
        repository: String,
        path: String,
    },
}

impl Mutation {
    pub fn validate(&self) -> Result<(), RecoveryError> {
        let (path, other, content) = match self {
            Self::Write { path, content }
            | Self::Append { path, content }
            | Self::GitWrite { path, content, .. } => (path, None, Some(content)),
            Self::Move {
                source,
                destination,
            } => (source, Some(destination), None),
            Self::Remove { path }
            | Self::CreateDirectory { path }
            | Self::GitRemove { path, .. }
            | Self::GitDirectory { path, .. } => (path, None, None),
        };
        paths::validate_guest_path(path)?;
        if let Some(repository) = self.repository() {
            paths::validate_guest_path(repository)?;
            if !repository.starts_with('/') || !path.starts_with('/') {
                return Err(RecoveryError::InvalidRecord);
            }
            let repository = interpreter::runtime::fs::guest_normalize("/", repository)
                .map_err(interpreter::runtime::fs::ContainError::from)?;
            let path = interpreter::runtime::fs::guest_normalize("/", path)
                .map_err(interpreter::runtime::fs::ContainError::from)?;
            let prefix = format!("{}/", repository.trim_end_matches('/'));
            if path == repository
                || !path.starts_with(&prefix)
                || path
                    .split('/')
                    .any(|part| part.to_ascii_lowercase().starts_with(".git-submilli-"))
            {
                return Err(RecoveryError::InvalidRecord);
            }
        }
        if let Some(other) = other {
            paths::validate_guest_path(other)?;
        }
        if let Some(content) = content {
            paths::validate_id(&content.id)?;
            if content.bytes > i64::MAX as u64 {
                return Err(RecoveryError::Limit);
            }
        }
        Ok(())
    }
    pub fn repository(&self) -> Option<&str> {
        match self {
            Self::GitWrite { repository, .. }
            | Self::GitRemove { repository, .. }
            | Self::GitDirectory { repository, .. } => Some(repository),
            _ => None,
        }
    }
}

/// Versioned persistence input. Its fields are private so only validated
/// preparation/decoding creates an intent; adapters seal its encoding in SQL.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Intent {
    pub(super) version: u8,
    pub(super) workspace: WorkspaceIdentity,
    pub(super) effect: String,
    pub(super) mutation: Mutation,
    pub(super) source: Entry,
    pub(super) destination: Option<Entry>,
    pub(super) original: Option<FileIdentity>,
    pub(super) destination_original: Option<FileIdentity>,
    pub(super) candidate: Option<FileIdentity>,
    pub(super) original_bytes: u64,
    pub(super) original_sha256: Option<[u8; 32]>,
    pub(super) original_directory: bool,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Entry {
    pub mount: String,
    pub parent: String,
    pub parent_identity: FileIdentity,
    pub name: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Outcome {
    pub bytes: u64,
}

impl Intent {
    pub fn encode(&self) -> Result<Vec<u8>, RecoveryError> {
        self.validate()?;
        let bytes = serde_json::to_vec(self).map_err(|_| RecoveryError::InvalidRecord)?;
        if bytes.len() > 64 * 1024 {
            return Err(RecoveryError::Limit);
        }
        Ok(bytes)
    }

    pub fn decode(bytes: &[u8]) -> Result<Self, RecoveryError> {
        if bytes.len() > 64 * 1024 {
            return Err(RecoveryError::Limit);
        }
        let intent: Self =
            serde_json::from_slice(bytes).map_err(|_| RecoveryError::InvalidRecord)?;
        intent.validate()?;
        Ok(intent)
    }

    pub fn mutation(&self) -> &Mutation {
        &self.mutation
    }
    pub fn effect(&self) -> &str {
        &self.effect
    }

    pub(super) fn validate(&self) -> Result<(), RecoveryError> {
        if self.version != 1 {
            return Err(RecoveryError::InvalidRecord);
        }
        paths::validate_id(&self.effect)?;
        paths::validate_id(&self.workspace.id)?;
        self.mutation.validate()?;
        self.source.validate(self.mutation.repository().is_some())?;
        if let Some(destination) = &self.destination {
            destination.validate(false)?;
        }
        if self.workspace.volumes.is_empty() || self.workspace.volumes.len() > 65 {
            return Err(RecoveryError::InvalidRecord);
        }
        let mut mounts = std::collections::BTreeSet::new();
        for volume in &self.workspace.volumes {
            paths::validate_guest_path(&volume.mount)?;
            if !mounts.insert(&volume.mount) {
                return Err(RecoveryError::InvalidRecord);
            }
        }
        if !mounts.contains(&self.source.mount)
            || self
                .destination
                .as_ref()
                .is_some_and(|entry| !mounts.contains(&entry.mount))
            || self.original_bytes > i64::MAX as u64
            || (self.original_directory
                && !matches!(
                    self.mutation,
                    Mutation::Remove { .. }
                        | Mutation::CreateDirectory { .. }
                        | Mutation::GitRemove { .. }
                        | Mutation::GitDirectory { .. }
                ))
        {
            return Err(RecoveryError::InvalidRecord);
        }
        let valid = match self.mutation {
            Mutation::Write { .. } | Mutation::GitWrite { .. } => {
                self.candidate.is_some() && self.destination.is_none()
            }
            Mutation::Append { .. } => {
                self.destination.is_none() && (self.original.is_some() || self.candidate.is_some())
            }
            Mutation::Move { .. } => self.original.is_some() && self.destination.is_some(),
            Mutation::Remove { .. } | Mutation::GitRemove { .. } => {
                self.original.is_some() && self.destination.is_none()
            }
            Mutation::CreateDirectory { .. } | Mutation::GitDirectory { .. } => {
                self.destination.is_none() && self.candidate.is_none()
            }
        };
        if !valid {
            return Err(RecoveryError::InvalidRecord);
        }
        Ok(())
    }
}

impl Entry {
    fn validate(&self, git: bool) -> Result<(), RecoveryError> {
        paths::validate_relative(&self.parent, true)?;
        paths::validate_relative(&self.name, false)?;
        let parent = std::path::Path::new(&self.parent);
        let name = std::path::Path::new(&self.name);
        if self.name.contains('/')
            || paths::private_recovery_path(parent)
            || paths::private_recovery_path(name)
            || (!git
                && (interpreter::runtime::fs::protected_metadata(parent)
                    || interpreter::runtime::fs::protected_metadata(name)))
        {
            return Err(RecoveryError::InvalidRecord);
        }
        paths::validate_guest_path(&self.mount)
    }
}
