//! `submilli.lock` — the resolved GitHub-dependency closure recorded beside
//! `submilli.toml`. Author-committed and shipped in the repo tarball, it pins
//! every transitive GitHub dependency to a commit SHA + source hash so a build
//! reproduces the exact same closure without re-resolving.

use std::fmt;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

pub const LOCKFILE_NAME: &str = "submilli.lock";

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Lockfile {
    #[serde(default, rename = "package")]
    pub packages: Vec<LockedPackage>,
}

/// One resolved package in the closure: its identity plus the GitHub source it
/// was fetched from and the integrity hash of that source.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct LockedPackage {
    pub name: String,
    pub version: String,
    pub github: String,
    pub sha: String,
    pub source_hash: String,
}

impl Lockfile {
    pub fn new(mut packages: Vec<LockedPackage>) -> Self {
        packages.sort_by(|a, b| a.name.cmp(&b.name));
        Self { packages }
    }

    /// Read `submilli.lock` from `manifest_dir`. Absent file → `Ok(None)`.
    pub fn read(manifest_dir: &Path) -> Result<Option<Lockfile>, LockfileError> {
        let path = manifest_dir.join(LOCKFILE_NAME);
        match fs::read_to_string(&path) {
            Ok(text) => toml::from_str(&text)
                .map(Some)
                .map_err(|source| LockfileError::Parse { path, source }),
            Err(source) if source.kind() == io::ErrorKind::NotFound => Ok(None),
            Err(source) => Err(LockfileError::Io { path, source }),
        }
    }

    /// Write `submilli.lock` into `manifest_dir` (packages sorted by name for a
    /// deterministic file).
    pub fn write(&self, manifest_dir: &Path) -> Result<(), LockfileError> {
        let path = manifest_dir.join(LOCKFILE_NAME);
        let sorted = Lockfile::new(self.packages.clone());
        let text = toml::to_string_pretty(&sorted).map_err(|source| LockfileError::Serialize {
            path: path.clone(),
            source,
        })?;
        fs::write(&path, text).map_err(|source| LockfileError::Io { path, source })
    }

    /// Remove `submilli.lock` from `manifest_dir` if present (used when a project
    /// no longer declares any GitHub dependency).
    pub fn remove(manifest_dir: &Path) -> Result<(), LockfileError> {
        let path = manifest_dir.join(LOCKFILE_NAME);
        match fs::remove_file(&path) {
            Ok(()) => Ok(()),
            Err(source) if source.kind() == io::ErrorKind::NotFound => Ok(()),
            Err(source) => Err(LockfileError::Io { path, source }),
        }
    }
}

#[derive(Debug)]
pub enum LockfileError {
    Io {
        path: PathBuf,
        source: io::Error,
    },
    Parse {
        path: PathBuf,
        source: toml::de::Error,
    },
    Serialize {
        path: PathBuf,
        source: toml::ser::Error,
    },
}

impl fmt::Display for LockfileError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            LockfileError::Io { path, source } => {
                write!(f, "read or write {}: {source}", path.display())
            }
            LockfileError::Parse { path, source } => {
                write!(f, "parse {}: {source}", path.display())
            }
            LockfileError::Serialize { path, source } => {
                write!(f, "serialize {}: {source}", path.display())
            }
        }
    }
}

impl std::error::Error for LockfileError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            LockfileError::Io { source, .. } => Some(source),
            LockfileError::Parse { source, .. } => Some(source),
            LockfileError::Serialize { source, .. } => Some(source),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    fn sample() -> Vec<LockedPackage> {
        vec![
            LockedPackage {
                name: "@acme/http".to_string(),
                version: "1.0.0".to_string(),
                github: "github.com/acme/http".to_string(),
                sha: "1111111111111111111111111111111111111111".to_string(),
                source_hash: "sha256:bbbb".to_string(),
            },
            LockedPackage {
                name: "@acme/slack".to_string(),
                version: "0.3.1".to_string(),
                // SSH dependencies are locked by their URL verbatim.
                github: "git@github.com:acme/slack.git".to_string(),
                sha: "0000000000000000000000000000000000000000".to_string(),
                source_hash: "sha256:aaaa".to_string(),
            },
        ]
    }

    #[test]
    fn round_trips_and_sorts_by_name() {
        let dir = tempdir().expect("tempdir");
        // Deliberately unsorted input; write() sorts.
        let mut packages = sample();
        packages.reverse();
        Lockfile { packages }.write(dir.path()).expect("write");

        let read = Lockfile::read(dir.path()).expect("read").expect("present");
        assert_eq!(read.packages, sample());
    }

    #[test]
    fn absent_lockfile_reads_as_none() {
        let dir = tempdir().expect("tempdir");
        assert_eq!(Lockfile::read(dir.path()).expect("read"), None);
    }

    #[test]
    fn remove_is_idempotent() {
        let dir = tempdir().expect("tempdir");
        Lockfile::remove(dir.path()).expect("remove absent");
        Lockfile::new(sample()).write(dir.path()).expect("write");
        Lockfile::remove(dir.path()).expect("remove present");
        assert_eq!(Lockfile::read(dir.path()).expect("read"), None);
    }
}
