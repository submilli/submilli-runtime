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
use std::sync::{Arc, RwLock};

use interpreter::runtime::{DiskQuota, measure_host_dir};
use tokio::sync::OnceCell;

use crate::config::{
    Access, SizeLimit, VolumeKind, VolumeSpec, VolumeTable, default_managed_volume_root,
    is_managed_name,
};
use crate::session_manager::SessionError;

pub struct VolumeRegistry {
    /// Fixed at startup, except that the operator-trusted local apply path may add
    /// a `managed-local` volume ([`Self::declare_managed`]), and withdraw one it
    /// just added when the blueprint naming it was not stored
    /// ([`Self::withdraw_managed`]); nothing changes a declaration. Poison means a
    /// panic interrupted such an addition or withdrawal; AGENTS.md permits the
    /// poisoned-lock panic rather than reading a table that may be half-updated.
    table: RwLock<VolumeTable>,
    managed_root: PathBuf,
    /// One cell per volume with a byte limit, so concurrent first users
    /// measure it once and share the result. Grows with the table, under the
    /// same poisoning rule.
    quotas: RwLock<BTreeMap<String, Arc<OnceCell<Arc<DiskQuota>>>>>,
    /// For a volume that is a partial copy: what the source holds that the copy does not,
    /// added to the copy's own measure. `None` is a source that could not be measured.
    /// Empty for every volume of a normal run.
    usage_offsets: BTreeMap<String, Option<u64>>,
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

/// Why a volume could not be declared at runtime.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DeclareError {
    /// The name cannot be one directory name under the managed root.
    BadName { volume: String },
    /// The name differs only in letter case from a declared volume or a directory
    /// under the managed root: on a case-insensitive filesystem both would be
    /// stored in one directory.
    CaseConflict { volume: String, existing: String },
}

impl std::fmt::Display for DeclareError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            DeclareError::BadName { volume } => write!(
                f,
                "volume '{volume}' cannot be stored as a managed volume: a name is up to 64 \
                 letters, digits, `.`, `_`, or `-`, starting with a letter or digit"
            ),
            DeclareError::CaseConflict { volume, existing } => write!(
                f,
                "volume '{volume}' differs only in letter case from '{existing}', which this \
                 server already stores; on a case-insensitive filesystem they would share one \
                 directory, so use '{existing}' or a name that differs in more than case"
            ),
        }
    }
}

impl std::error::Error for DeclareError {}

/// The checks on a volume `table` does not declare yet, against the table alone.
fn check_new_managed(table: &VolumeTable, volume: &str) -> Result<(), DeclareError> {
    if !is_managed_name(volume) {
        return Err(DeclareError::BadName {
            volume: volume.to_owned(),
        });
    }
    match table.keys().find(|name| name.eq_ignore_ascii_case(volume)) {
        Some(existing) => Err(DeclareError::CaseConflict {
            volume: volume.to_owned(),
            existing: existing.clone(),
        }),
        None => Ok(()),
    }
}

impl VolumeRegistry {
    pub fn new(table: VolumeTable, managed_root: PathBuf) -> Self {
        let quotas = table
            .iter()
            .filter(|(_, spec)| matches!(spec.size_limit, SizeLimit::Bytes(_)))
            .map(|(name, _)| (name.clone(), Arc::new(OnceCell::new())))
            .collect();
        Self {
            table: RwLock::new(table),
            managed_root,
            quotas: RwLock::new(quotas),
            usage_offsets: BTreeMap::new(),
        }
    }

    /// Whether `volume` could be declared as a `managed-local` volume: its name is
    /// one directory name, and differs in more than letter case from every declared
    /// volume and every directory under the managed root. A volume already declared
    /// passes. The operator-trusted local apply path checks this before it stores a
    /// blueprint, so the declaration that follows the store cannot be refused.
    pub(crate) fn check_managed(&self, volume: &str) -> Result<(), DeclareError> {
        let table = self.table.read().expect("volume table poisoned");
        if table.contains_key(volume) {
            return Ok(());
        }
        check_new_managed(&table, volume)?;
        // A directory a volume of another case left behind, from an earlier run.
        // No managed root yet holds no directory to collide with; any other failure
        // to list it surfaces when the volume is first used.
        let Ok(entries) = std::fs::read_dir(&self.managed_root) else {
            return Ok(());
        };
        for entry in entries.flatten() {
            let name = entry.file_name();
            if let Some(name) = name.to_str()
                && name != volume
                && name.eq_ignore_ascii_case(volume)
            {
                return Err(DeclareError::CaseConflict {
                    volume: volume.to_owned(),
                    existing: name.to_owned(),
                });
            }
        }
        Ok(())
    }

    /// Declares `volume` as a `managed-local` volume, read-write, stored at
    /// `<managed root>/<volume>`, unless a volume of that name is already declared.
    /// Returns whether it was added. Only the operator-trusted local apply path
    /// calls this, after [`Self::check_managed`]: a deployed server's table stays
    /// what its operator declared.
    pub(crate) fn declare_managed(
        &self,
        volume: &str,
        size_limit: SizeLimit,
    ) -> Result<bool, DeclareError> {
        // Lock order: the table, then the quotas, as `quota` never holds both.
        let mut table = self.table.write().expect("volume table poisoned");
        if table.contains_key(volume) {
            return Ok(false);
        }
        check_new_managed(&table, volume)?;
        if matches!(size_limit, SizeLimit::Bytes(_)) {
            self.quotas
                .write()
                .expect("volume quotas poisoned")
                .insert(volume.to_owned(), Arc::new(OnceCell::new()));
        }
        table.insert(volume.to_owned(), VolumeSpec::managed(size_limit));
        Ok(true)
    }

    /// Removes a `managed-local` declaration [`Self::declare_managed`] added. Only the
    /// local apply path calls this, for a volume it declared under the blueprint tag
    /// lock it still holds, when the blueprint that names the volume was not
    /// stored: no stored blueprint references it, so no run resolves it.
    pub(crate) fn withdraw_managed(&self, volume: &str) {
        // Lock order: the table, then the quotas, as in `declare_managed`.
        let mut table = self.table.write().expect("volume table poisoned");
        if table
            .get(volume)
            .is_some_and(|spec| matches!(spec.kind, VolumeKind::ManagedLocal))
        {
            table.remove(volume);
            self.quotas
                .write()
                .expect("volume quotas poisoned")
                .remove(volume);
        }
    }

    fn spec(&self, volume: &str) -> Option<VolumeSpec> {
        self.table
            .read()
            .expect("volume table poisoned")
            .get(volume)
            .cloned()
    }

    /// Counts `offsets` against each named volume's size limit on top of what its
    /// directory holds, for a volume that holds only part of its source.
    pub(crate) fn with_usage_offsets(mut self, offsets: BTreeMap<String, Option<u64>>) -> Self {
        self.usage_offsets = offsets;
        self
    }

    /// The declarations, by name, as they are now.
    pub fn table(&self) -> VolumeTable {
        self.table.read().expect("volume table poisoned").clone()
    }

    /// Whether a blueprint may name `volume` with `requested` access: the
    /// volume must be declared, and the blueprint may not ask for more than the
    /// server allows.
    pub fn check_reference(
        &self,
        volume: &str,
        requested: Option<Access>,
    ) -> Result<(), ReferenceError> {
        check_reference(
            &self.table.read().expect("volume table poisoned"),
            volume,
            requested,
        )
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
        let Some(spec) = self.spec(volume) else {
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
        let Some(spec) = self.spec(volume) else {
            return Err(SessionError::UnknownVolume(volume.to_string()));
        };
        let SizeLimit::Bytes(limit) = spec.size_limit else {
            return Ok(None);
        };
        let cell = self
            .quotas
            .read()
            .expect("volume quotas poisoned")
            .get(volume)
            .cloned();
        let Some(cell) = cell else {
            return Err(SessionError::UnknownVolume(volume.to_string()));
        };
        let host = self.host_of(volume, &spec.kind)?;
        let offset = self.usage_offsets.get(volume).copied();
        let measured = cell
            .get_or_try_init(|| async move {
                let mut used = tokio::task::spawn_blocking(move || measure_host_dir(&host))
                    .await
                    .map_err(|err| std::io::Error::other(err.to_string()))??;
                match offset {
                    None => {}
                    Some(Some(offset)) => used = used.saturating_add(offset),
                    Some(None) => {
                        return Err(std::io::Error::other("the source volume was not measured"));
                    }
                }
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
        for (name, spec) in &self.table() {
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

    #[test]
    fn a_withdrawn_declaration_is_gone_and_can_be_made_again() {
        let root = tempfile::tempdir().unwrap();
        let registry = registry(root.path(), VolumeTable::new());
        assert_eq!(
            registry.declare_managed("notes", SizeLimit::Bytes(10)),
            Ok(true)
        );
        registry.withdraw_managed("notes");
        assert!(!registry.table().contains_key("notes"));
        assert!(
            !registry
                .quotas
                .read()
                .expect("volume quotas poisoned")
                .contains_key("notes")
        );
        assert!(registry.check_reference("notes", None).is_err());
        assert_eq!(
            registry.declare_managed("notes", SizeLimit::Unlimited),
            Ok(true)
        );
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
