//! The named volumes this server declares, resolved by name for every VFS that
//! uses one.
//!
//! A volume's size limit belongs to the volume, not to a session: every VFS
//! that opens it — any session, any blueprint, any mount path — is handed the
//! same [`DiskQuota`], so the limit holds however many places write at once.
//! The count starts from one walk of the directory, the first time a program
//! that may write opens the volume, and is kept for the life of the process.
//!
//! Nothing here deletes a volume's files. A `managed-local` volume's directory
//! is created on first use under the managed root and left alone after; a
//! `local-path` directory belongs to the operator outright.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use interpreter::runtime::{DiskQuota, measure_host_dir};
use tokio::sync::OnceCell;

use crate::config::{
    Access, SizeLimit, VolumeKind, VolumeTable, default_managed_volume_root, is_managed_name,
};
use crate::session_manager::SessionError;

pub struct VolumeRegistry {
    table: VolumeTable,
    managed_root: PathBuf,
    /// One cell per volume with a byte limit, so concurrent first users
    /// measure it once and share the result.
    quotas: BTreeMap<String, OnceCell<Arc<DiskQuota>>>,
}

impl Default for VolumeRegistry {
    fn default() -> Self {
        Self::new(VolumeTable::new(), default_managed_volume_root())
    }
}

/// A volume resolved for one VFS: where it lives and what the program may do.
/// Never shown to a client — the host path is the operator's business.
#[derive(Debug)]
pub(crate) struct ResolvedVolume {
    pub host: PathBuf,
    pub access: Access,
}

/// Why a blueprint's reference to a volume cannot be registered.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ReferenceError {
    /// No volume of that name is declared; `declared` lists those that are.
    Undeclared {
        volume: String,
        declared: Vec<String>,
    },
    /// The blueprint asks for more access than the server allows.
    AccessExceeds { volume: String },
}

impl std::fmt::Display for ReferenceError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ReferenceError::Undeclared { volume, declared } if declared.is_empty() => write!(
                f,
                "volume '{volume}' is not declared on this server, which declares no volumes; \
                 ask the operator to declare it under `volumes:` in the server config"
            ),
            ReferenceError::Undeclared { volume, declared } => write!(
                f,
                "volume '{volume}' is not declared on this server; declared volumes: {}",
                declared.join(", ")
            ),
            ReferenceError::AccessExceeds { volume } => write!(
                f,
                "volume '{volume}' is read_only on this server; drop `access: read_write` (or \
                 write `access: read_only`), or ask the operator to declare it read_write"
            ),
        }
    }
}

impl std::error::Error for ReferenceError {}

impl VolumeRegistry {
    pub fn new(table: VolumeTable, managed_root: PathBuf) -> Self {
        let quotas = table
            .iter()
            .filter(|(_, spec)| matches!(spec.size_limit, SizeLimit::Bytes(_)))
            .map(|(name, _)| (name.clone(), OnceCell::new()))
            .collect();
        Self {
            table,
            managed_root,
            quotas,
        }
    }

    /// The declarations, by name.
    pub fn table(&self) -> &VolumeTable {
        &self.table
    }

    /// Whether a blueprint may name `volume` with `requested` access: the
    /// volume must be declared, and the blueprint may not ask for more than the
    /// server allows.
    pub fn check_reference(
        &self,
        volume: &str,
        requested: Option<Access>,
    ) -> Result<(), ReferenceError> {
        check_reference(&self.table, volume, requested)
    }

    /// Where `volume` lives and the access a program gets: what the blueprint
    /// asked for, never more than the server allows. A blueprint registered
    /// before the operator narrowed a volume is narrowed with it rather than
    /// refused, and its writes then fail as writes to a read-only volume.
    pub(crate) fn resolve(
        &self,
        volume: &str,
        requested: Option<Access>,
    ) -> Result<ResolvedVolume, SessionError> {
        let Some(spec) = self.table.get(volume) else {
            tracing::warn!(
                volume,
                "blueprint names a volume this server does not declare"
            );
            return Err(SessionError::UnknownVolume(volume.to_string()));
        };
        let access = match requested {
            Some(access) if access > spec.access => {
                tracing::warn!(
                    volume,
                    "blueprint asks for more access than the volume allows; narrowing it"
                );
                spec.access
            }
            Some(access) => access,
            None => spec.access,
        };
        let host = self.host_of(volume, &spec.kind)?;
        if spec.kind == VolumeKind::ManagedLocal {
            ensure_managed_dir(&self.managed_root, &host).map_err(|err| {
                tracing::error!(
                    volume,
                    path = %host.display(),
                    %err,
                    "preparing the managed volume failed"
                );
                SessionError::VolumeUnavailable(volume.to_string())
            })?;
        }
        Ok(ResolvedVolume { host, access })
    }

    /// Where `volume`'s files live. A managed name is checked again here, since a
    /// table built in code rather than read from the config file skips boot
    /// validation, and the name becomes a path component.
    fn host_of(&self, volume: &str, kind: &VolumeKind) -> Result<PathBuf, SessionError> {
        match kind {
            VolumeKind::LocalPath { path } => Ok(path.clone()),
            VolumeKind::ManagedLocal if is_managed_name(volume) => {
                Ok(self.managed_root.join(volume))
            }
            VolumeKind::ManagedLocal => {
                tracing::error!(volume, "managed volume name is not a plain directory name");
                Err(SessionError::VolumeUnavailable(volume.to_string()))
            }
        }
    }

    /// The size limit every user of `volume` shares, or `None` when the volume
    /// is declared `unlimited`. The first call measures the directory; one that
    /// fails is not remembered, so the next call measures again, and this call
    /// gets a limit that refuses every write that grows the volume.
    pub(crate) async fn quota(&self, volume: &str) -> Result<Option<Arc<DiskQuota>>, SessionError> {
        let Some(spec) = self.table.get(volume) else {
            return Err(SessionError::UnknownVolume(volume.to_string()));
        };
        let SizeLimit::Bytes(limit) = spec.size_limit else {
            return Ok(None);
        };
        let Some(cell) = self.quotas.get(volume) else {
            return Err(SessionError::UnknownVolume(volume.to_string()));
        };
        let host = self.host_of(volume, &spec.kind)?;
        let measured = cell
            .get_or_try_init(|| async move {
                let used = tokio::task::spawn_blocking(move || measure_host_dir(&host))
                    .await
                    .map_err(|err| std::io::Error::other(err.to_string()))??;
                Ok::<_, std::io::Error>(Arc::new(DiskQuota::new(limit, used)))
            })
            .await;
        match measured {
            Ok(quota) => Ok(Some(Arc::clone(quota))),
            Err(err) => {
                tracing::warn!(
                    volume,
                    %err,
                    "the volume could not be measured against its size limit; treating it as full"
                );
                Ok(Some(Arc::new(DiskQuota::unmeasured(limit))))
            }
        }
    }

    /// Create every managed volume's directory now, so a misconfigured root
    /// shows up in the boot log rather than on a first execute. A failure is
    /// only logged: the volume is retried when it is used.
    pub fn prepare(&self) {
        for (name, spec) in &self.table {
            if spec.kind != VolumeKind::ManagedLocal {
                continue;
            }
            let Ok(dir) = self.host_of(name, &spec.kind) else {
                continue;
            };
            if let Err(err) = ensure_managed_dir(&self.managed_root, &dir) {
                tracing::warn!(
                    volume = name.as_str(),
                    path = %dir.display(),
                    %err,
                    "the managed volume's directory could not be created; it is retried on use"
                );
            }
        }
    }
}

/// Whether a blueprint may name `volume` with `requested` access in `table`;
/// see [`VolumeRegistry::check_reference`].
pub fn check_reference(
    table: &VolumeTable,
    volume: &str,
    requested: Option<Access>,
) -> Result<(), ReferenceError> {
    let Some(spec) = table.get(volume) else {
        return Err(ReferenceError::Undeclared {
            volume: volume.to_string(),
            declared: table.keys().cloned().collect(),
        });
    };
    if requested.is_some_and(|access| access > spec.access) {
        return Err(ReferenceError::AccessExceeds {
            volume: volume.to_string(),
        });
    }
    Ok(())
}

/// Make sure a managed volume's directory exists as a plain directory, creating
/// it (owner-only on Unix) when it does not. A link there is refused: the
/// directory is opened with the server's full authority, so a link swapped in
/// would aim the volume anywhere.
fn ensure_managed_dir(root: &Path, dir: &Path) -> std::io::Result<()> {
    std::fs::create_dir_all(root)?;
    match std::fs::symlink_metadata(dir) {
        Ok(meta) => require_plain_dir(&meta),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => {
            let mut builder = std::fs::DirBuilder::new();
            #[cfg(unix)]
            std::os::unix::fs::DirBuilderExt::mode(&mut builder, 0o700);
            match builder.create(dir) {
                Ok(()) => Ok(()),
                // Another first user created it in between; check what it made.
                Err(err) if err.kind() == std::io::ErrorKind::AlreadyExists => {
                    require_plain_dir(&std::fs::symlink_metadata(dir)?)
                }
                Err(err) => Err(err),
            }
        }
        Err(err) => Err(err),
    }
}

fn require_plain_dir(meta: &std::fs::Metadata) -> std::io::Result<()> {
    if meta.is_dir() {
        return Ok(());
    }
    Err(std::io::Error::other(
        "the managed volume's path exists and is not a plain directory",
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::VolumeSpec;

    fn registry(root: &Path, table: VolumeTable) -> VolumeRegistry {
        VolumeRegistry::new(table, root.join("volumes"))
    }

    #[test]
    fn references_must_be_declared_and_within_the_access_ceiling() {
        let root = tempfile::tempdir().unwrap();
        let registry = registry(
            root.path(),
            VolumeTable::from([(
                "handbook".to_string(),
                VolumeSpec::local_path(root.path().join("handbook")).with_access(Access::ReadOnly),
            )]),
        );
        assert_eq!(registry.check_reference("handbook", None), Ok(()));
        assert_eq!(
            registry.check_reference("handbook", Some(Access::ReadOnly)),
            Ok(())
        );
        assert_eq!(
            registry.check_reference("handbook", Some(Access::ReadWrite)),
            Err(ReferenceError::AccessExceeds {
                volume: "handbook".into()
            })
        );
        let undeclared = registry.check_reference("other", None).unwrap_err();
        assert!(
            undeclared
                .to_string()
                .contains("declared volumes: handbook")
        );
    }

    #[test]
    fn resolution_narrows_access_to_the_server_ceiling() {
        let root = tempfile::tempdir().unwrap();
        let registry = registry(
            root.path(),
            VolumeTable::from([(
                "handbook".to_string(),
                VolumeSpec::local_path(root.path().join("handbook")).with_access(Access::ReadOnly),
            )]),
        );
        let resolved = registry
            .resolve("handbook", Some(Access::ReadWrite))
            .unwrap();
        assert_eq!(resolved.access, Access::ReadOnly);
        assert!(matches!(
            registry.resolve("other", None),
            Err(SessionError::UnknownVolume(_))
        ));
    }

    #[test]
    fn managed_volumes_are_created_under_the_root_and_links_are_refused() {
        let root = tempfile::tempdir().unwrap();
        let registry = registry(
            root.path(),
            VolumeTable::from([
                (
                    "memory".to_string(),
                    VolumeSpec::managed(SizeLimit::Bytes(100)),
                ),
                (
                    "linked".to_string(),
                    VolumeSpec::managed(SizeLimit::Unlimited),
                ),
            ]),
        );
        let resolved = registry.resolve("memory", None).unwrap();
        assert_eq!(resolved.host, root.path().join("volumes/memory"));
        assert!(resolved.host.is_dir());
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = std::fs::metadata(&resolved.host)
                .unwrap()
                .permissions()
                .mode();
            assert_eq!(mode & 0o777, 0o700);
            std::os::unix::fs::symlink(root.path(), root.path().join("volumes/linked")).unwrap();
            let err = registry.resolve("linked", None).unwrap_err();
            assert!(matches!(err, SessionError::VolumeUnavailable(_)));
            assert!(
                !err.to_string().contains(&root.path().display().to_string()),
                "the host path stays in the log: {err}"
            );
        }
    }

    #[tokio::test]
    async fn every_user_of_a_volume_shares_one_measured_limit() {
        let root = tempfile::tempdir().unwrap();
        let dir = root.path().join("data");
        std::fs::create_dir(&dir).unwrap();
        std::fs::write(dir.join("a"), [0u8; 30]).unwrap();
        let registry = Arc::new(registry(
            root.path(),
            VolumeTable::from([
                (
                    "data".to_string(),
                    VolumeSpec {
                        size_limit: SizeLimit::Bytes(100),
                        ..VolumeSpec::local_path(&dir)
                    },
                ),
                ("open".to_string(), VolumeSpec::local_path(&dir)),
            ]),
        ));
        let mut handles = Vec::new();
        for _ in 0..8 {
            let registry = Arc::clone(&registry);
            handles.push(tokio::spawn(async move {
                registry.quota("data").await.unwrap().unwrap()
            }));
        }
        let mut quotas = Vec::new();
        for handle in handles {
            quotas.push(handle.await.unwrap());
        }
        assert!(quotas.iter().all(|quota| Arc::ptr_eq(quota, &quotas[0])));
        assert_eq!(quotas[0].used(), 30);
        assert_eq!(quotas[0].limit(), 100);
        assert!(registry.quota("open").await.unwrap().is_none());
    }

    #[tokio::test]
    async fn a_failed_measurement_is_retried_and_opens_full_meanwhile() {
        let root = tempfile::tempdir().unwrap();
        let dir = root.path().join("later");
        let registry = registry(
            root.path(),
            VolumeTable::from([(
                "later".to_string(),
                VolumeSpec {
                    size_limit: SizeLimit::Bytes(100),
                    ..VolumeSpec::local_path(&dir)
                },
            )]),
        );
        let full = registry.quota("later").await.unwrap().unwrap();
        assert!(full.is_unmeasured());
        std::fs::create_dir(&dir).unwrap();
        let measured = registry.quota("later").await.unwrap().unwrap();
        assert!(!measured.is_unmeasured());
        assert_eq!(measured.used(), 0);
    }
}
