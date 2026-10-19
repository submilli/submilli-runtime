//! Each run's file system: the session's own directory, ephemeral scratch, and
//! the project's volumes with their size limits.
//!
//! Copied from `submilli-server`'s `volumes.rs` (`VolumeRegistry`), its
//! `session_manager.rs` (`build_vfs`, `attach_limits`, `EphemeralRoot`), and
//! `config.rs` (`create_owned_dir`). The volume table is the project's, fixed
//! when the playground starts. Managed volumes stay under
//! `<state>/volumes/managed-local/`, where the embedded server kept them; scratch
//! lives under `<state>/host/`. Adding a `managed-local` volume a blueprint apply
//! names follows `volumes.rs` (`check_managed`, `declare_managed`,
//! `withdraw_managed`).

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, RwLock};

use interpreter::runtime::{
    Access as RtAccess, DiskQuota, MountError, MountSpec, Vfs, VfsMode as RtVfsMode,
    measure_host_dir,
};
use submilli_blueprint::{Blueprint, VarBindings, VfsConfig};
use submilli_server::config::{
    Access, SizeLimit, VolumeKind, VolumeSpec, VolumeTable, is_managed_name,
};
use tokio::sync::OnceCell;

use super::super::log::warn;
use super::super::state::StateDir;
use super::blueprints::{LocalVolumes, VolumeProblem};

/// Why a run's file system could not be built. The messages are the server's,
/// and name a volume, never its directory.
#[derive(Debug)]
pub(crate) enum VolumeError {
    Io(String),
    InvalidVfs(String),
    /// The blueprint names a volume the project does not have.
    UnknownVolume(String),
    /// A volume's directory is gone, is not a directory, or is unreadable.
    VolumeUnavailable(String),
    /// A volume could not be grafted at its mount path in the root.
    MountFailed {
        volume: String,
        reason: String,
    },
}

impl std::fmt::Display for VolumeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidVfs(msg) => write!(f, "invalid session filesystem: {msg}"),
            Self::Io(msg) => write!(f, "session vfs io: {msg}"),
            Self::UnknownVolume(name) => write!(
                f,
                "volume '{name}' is not declared on this server; ask the operator to declare it \
                 under `volumes:` in the server config"
            ),
            Self::VolumeUnavailable(name) => write!(
                f,
                "volume '{name}' is declared but unavailable; the server log has the details"
            ),
            Self::MountFailed { volume, reason } => {
                write!(f, "volume '{volume}' could not be mounted: {reason}")
            }
        }
    }
}

impl std::error::Error for VolumeError {}

/// The project's volumes, resolved by name for every file system that uses one.
/// A volume's size limit belongs to the volume: every file system that opens it
/// shares one [`DiskQuota`], measured once, the first time a run that may write
/// opens it.
pub(crate) struct Volumes {
    /// Fixed at start, except that a blueprint apply may add a `managed-local`
    /// volume, and withdraw one it just added. Poison means a panic interrupted
    /// such a change; AGENTS.md permits the poisoned-lock panic rather than
    /// reading a table that may be half-updated.
    table: RwLock<VolumeTable>,
    managed_root: PathBuf,
    /// Grows with the table, under the same poisoning rule.
    quotas: RwLock<BTreeMap<String, Arc<OnceCell<Arc<DiskQuota>>>>>,
    ephemeral: EphemeralRoot,
}

impl Volumes {
    /// The volumes of `table`, managed ones under `state`'s managed volume
    /// directory. Scratch an earlier process left behind is swept now.
    pub(crate) fn boot(state: &StateDir, table: VolumeTable) -> Self {
        let quotas = table
            .iter()
            .filter(|(_, spec)| matches!(spec.size_limit, SizeLimit::Bytes(_)))
            .map(|(name, _)| (name.clone(), Arc::new(OnceCell::new())))
            .collect();
        let volumes = Self {
            table: RwLock::new(table),
            managed_root: state.managed_volumes_dir(),
            quotas: RwLock::new(quotas),
            ephemeral: EphemeralRoot {
                path: state.host_ephemeral_dir(),
            },
        };
        if let Err(error) = volumes.ephemeral.sweep() {
            warn(&format!(
                "removing leftover ephemeral scratch failed: {error}; it is retried at the next start"
            ));
        }
        volumes.prepare();
        volumes
    }

    /// Where `volume` lives and the access a run gets: what the blueprint asked
    /// for, never more than the volume allows.
    fn resolve(
        &self,
        volume: &str,
        requested: Option<Access>,
    ) -> Result<(PathBuf, Access), VolumeError> {
        let Some(spec) = self.spec(volume) else {
            return Err(VolumeError::UnknownVolume(volume.to_string()));
        };
        let access = match requested {
            Some(access) if access > spec.access => spec.access,
            Some(access) => access,
            None => spec.access,
        };
        let host = self.host_of(volume, &spec)?;
        if spec.kind == VolumeKind::ManagedLocal {
            create_owned_dir(&host).map_err(|err| {
                warn(&format!(
                    "preparing managed volume '{volume}' at {} failed: {err}",
                    host.display()
                ));
                VolumeError::VolumeUnavailable(volume.to_string())
            })?;
        }
        Ok((host, access))
    }

    fn spec(&self, volume: &str) -> Option<VolumeSpec> {
        self.table
            .read()
            .expect("volume table poisoned")
            .get(volume)
            .cloned()
    }

    /// Where `volume`'s files live. A managed name becomes a path component, so it
    /// is checked again here.
    fn host_of(&self, volume: &str, spec: &VolumeSpec) -> Result<PathBuf, VolumeError> {
        match &spec.kind {
            VolumeKind::LocalPath { path } => Ok(path.clone()),
            VolumeKind::ManagedLocal if is_managed_name(volume) => {
                Ok(self.managed_root.join(volume))
            }
            VolumeKind::ManagedLocal => Err(VolumeError::VolumeUnavailable(volume.to_string())),
        }
    }

    /// The size limit every user of `volume` shares, or `None` when it has none.
    /// A failed measurement is not remembered; meanwhile the volume counts as
    /// full.
    async fn quota(&self, volume: &str) -> Result<Option<Arc<DiskQuota>>, VolumeError> {
        let Some(spec) = self.spec(volume) else {
            return Err(VolumeError::UnknownVolume(volume.to_string()));
        };
        let SizeLimit::Bytes(limit) = spec.size_limit else {
            return Ok(None);
        };
        let Some(cell) = self
            .quotas
            .read()
            .expect("volume quotas poisoned")
            .get(volume)
            .cloned()
        else {
            return Err(VolumeError::UnknownVolume(volume.to_string()));
        };
        let host = self.host_of(volume, &spec)?;
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
                warn(&format!(
                    "volume '{volume}' could not be measured against its size limit ({err}); \
                     treating it as full"
                ));
                Ok(Some(Arc::new(DiskQuota::unmeasured(limit))))
            }
        }
    }

    /// Create every managed volume's directory now, so a problem shows at start
    /// rather than on a first run. A failure is retried when the volume is used.
    fn prepare(&self) {
        if let Err(err) = create_owned_dir(&self.managed_root) {
            warn(&format!(
                "creating {} failed: {err}",
                self.managed_root.display()
            ));
            return;
        }
        for (name, spec) in self.table.read().expect("volume table poisoned").iter() {
            if spec.kind != VolumeKind::ManagedLocal {
                continue;
            }
            if let Ok(dir) = self.host_of(name, spec)
                && let Err(err) = create_owned_dir(&dir)
            {
                warn(&format!(
                    "creating managed volume '{name}' at {} failed: {err}; it is retried on use",
                    dir.display()
                ));
            }
        }
    }

    /// Build the file system for one run. Every named volume, a `named` root or
    /// a mount, resolves here. Size limits are attached by [`Self::attach_limits`],
    /// since only a run that may write needs them.
    pub(crate) fn build_vfs(
        &self,
        blueprint: &Blueprint,
        variables: &VarBindings,
        session_root: Option<&Path>,
    ) -> Result<Vfs, VolumeError> {
        let io = |mode: &'static str| {
            move |err: std::io::Error| {
                warn(&format!("building the {mode} workspace failed: {err}"));
                VolumeError::Io(format!(
                    "the {mode} workspace could not be opened; the server log has the details"
                ))
            }
        };
        let config = blueprint
            .vfs
            .resolve(variables)
            .map_err(|e| VolumeError::InvalidVfs(e.to_string()))?;
        let root = match &config {
            VfsConfig::None => return Ok(Vfs::none()),
            VfsConfig::Ephemeral { .. } => {
                Vfs::tempdir_in(&self.ephemeral.path).map_err(io("ephemeral"))?
            }
            VfsConfig::PerSession { .. } => {
                let root = session_root
                    .ok_or_else(|| VolumeError::Io("per_session vfs root missing".into()))?;
                Vfs::external_with_mode(root.to_path_buf(), RtVfsMode::PerSession)
                    .map_err(io("per_session"))?
            }
            VfsConfig::Named {
                volume,
                access,
                sub_path,
                ..
            } => self.mount_root(volume, *access, sub_path.as_deref())?,
        };
        let vfs = config
            .mounts()
            .iter()
            .try_fold(root, |vfs, (path, mount)| {
                self.graft(
                    vfs,
                    path,
                    &mount.volume,
                    mount.access,
                    mount.sub_path.as_deref(),
                )
            })?;
        vfs.with_cwd(config.cwd()).map_err(|_| {
            VolumeError::InvalidVfs(format!(
                "cwd {} must name an existing directory or a directory in writable storage",
                config.cwd()
            ))
        })
    }

    /// Enforce the size limits on `vfs`: the blueprint's `size_limit` on an
    /// ephemeral or per-session root, and each named volume's own shared limit.
    pub(crate) async fn attach_limits(
        &self,
        vfs: Vfs,
        blueprint: &Blueprint,
    ) -> Result<Vfs, VolumeError> {
        let mut vfs = attach_size_limit(vfs, blueprint).await?;
        if let Some(volume) = vfs.volume().map(str::to_string)
            && let Some(quota) = self.quota(&volume).await?
        {
            vfs = vfs.with_shared_quota(quota);
        }
        let mounts: Vec<(String, String)> = vfs
            .mounts()
            .iter()
            .map(|mount| (mount.guest_path().to_string(), mount.volume().to_string()))
            .collect();
        for (path, volume) in mounts {
            if let Some(quota) = self.quota(&volume).await? {
                vfs = vfs.with_mount_quota(&path, quota);
            }
        }
        Ok(vfs)
    }

    fn mount_root(
        &self,
        volume: &str,
        access: Option<Access>,
        sub_path: Option<&str>,
    ) -> Result<Vfs, VolumeError> {
        let (host, access) = self.resolve(volume, access)?;
        let vfs = Vfs::external_with_mode(host.clone(), RtVfsMode::Named).map_err(|err| {
            warn(&format!(
                "mounting volume '{volume}' at {} failed: {err}",
                host.display()
            ));
            VolumeError::VolumeUnavailable(volume.to_string())
        })?;
        vfs.with_access(runtime_access(access))
            .with_volume_name(volume)
            .with_subpath(sub_path)
            .map_err(|_| VolumeError::MountFailed {
                volume: volume.into(),
                reason:
                    "subPath must name a directory; missing directories require read_write access"
                        .into(),
            })
    }

    fn graft(
        &self,
        vfs: Vfs,
        path: &str,
        volume: &str,
        access: Option<Access>,
        sub_path: Option<&str>,
    ) -> Result<Vfs, VolumeError> {
        let (host, access) = self.resolve(volume, access)?;
        vfs.with_mount_subpath(
            MountSpec {
                guest_path: path.to_string(),
                host: host.clone(),
                volume: volume.to_string(),
                access: runtime_access(access),
                // Attached by `attach_limits`, and only for a run that may write.
                quota: None,
            },
            sub_path,
        )
        .map_err(|err| match err {
            MountError::Io(err) => {
                warn(&format!(
                    "mounting volume '{volume}' at {} failed: {err}",
                    host.display()
                ));
                VolumeError::VolumeUnavailable(volume.to_string())
            }
            MountError::RootIo(err) => {
                warn(&format!("preparing the mount point {path} failed: {err}"));
                VolumeError::MountFailed {
                    volume: volume.to_string(),
                    reason: format!(
                        "the mount point {path} could not be prepared; the server log has the details"
                    ),
                }
            }
            other => VolumeError::MountFailed {
                volume: volume.to_string(),
                reason: other.to_string(),
            },
        })
    }
}

impl LocalVolumes for Volumes {
    fn is_declared(&self, volume: &str) -> bool {
        self.spec(volume).is_some()
    }

    fn check_reference(
        &self,
        volume: &str,
        requested: Option<Access>,
    ) -> Result<(), VolumeProblem> {
        let table = self.table.read().expect("volume table poisoned");
        let Some(spec) = table.get(volume) else {
            let declared: Vec<String> = table.keys().cloned().collect();
            return Err(VolumeProblem::Undeclared(if declared.is_empty() {
                format!(
                    "volume '{volume}' is not declared on this server, which declares no \
                     volumes; ask the operator to declare it under `volumes:` in the server config"
                )
            } else {
                format!(
                    "volume '{volume}' is not declared on this server; declared volumes: {}",
                    declared.join(", ")
                )
            }));
        };
        if requested.is_some_and(|access| access > spec.access) {
            return Err(VolumeProblem::AccessExceeds(format!(
                "volume '{volume}' is read_only on this server; drop `access: read_write` (or \
                 write `access: read_only`), or ask the operator to declare it read_write"
            )));
        }
        Ok(())
    }

    fn check_managed(&self, volume: &str) -> Result<(), String> {
        let table = self.table.read().expect("volume table poisoned");
        if table.contains_key(volume) {
            return Ok(());
        }
        check_new_managed(&table, volume)?;
        // A directory a volume of another case left behind, from an earlier run.
        // A root that cannot be listed fails the volume when it is used.
        let Ok(entries) = std::fs::read_dir(&self.managed_root) else {
            return Ok(());
        };
        for entry in entries.flatten() {
            let name = entry.file_name();
            if let Some(name) = name.to_str()
                && name != volume
                && name.eq_ignore_ascii_case(volume)
            {
                return Err(case_conflict(volume, name));
            }
        }
        Ok(())
    }

    fn declare_managed(&self, volume: &str) -> Result<bool, String> {
        let mut table = self.table.write().expect("volume table poisoned");
        if table.contains_key(volume) {
            return Ok(false);
        }
        check_new_managed(&table, volume)?;
        table.insert(volume.to_owned(), VolumeSpec::managed(SizeLimit::Unlimited));
        Ok(true)
    }

    fn withdraw_managed(&self, volume: &str) {
        // Lock order: the table, then the quotas, as `quota` never holds both.
        let mut table = self.table.write().expect("volume table poisoned");
        if table
            .get(volume)
            .is_some_and(|spec| spec.kind == VolumeKind::ManagedLocal)
        {
            table.remove(volume);
            self.quotas
                .write()
                .expect("volume quotas poisoned")
                .remove(volume);
        }
    }
}

/// The checks on a volume `table` does not declare yet, against the table alone.
fn check_new_managed(table: &VolumeTable, volume: &str) -> Result<(), String> {
    if !is_managed_name(volume) {
        return Err(format!(
            "volume '{volume}' cannot be stored as a managed volume: a name is up to 64 \
             letters, digits, `.`, `_`, or `-`, starting with a letter or digit"
        ));
    }
    match table.keys().find(|name| name.eq_ignore_ascii_case(volume)) {
        Some(existing) => Err(case_conflict(volume, existing)),
        None => Ok(()),
    }
}

fn case_conflict(volume: &str, existing: &str) -> String {
    format!(
        "volume '{volume}' differs only in letter case from '{existing}', which this \
         server already stores; on a case-insensitive filesystem they would share one \
         directory, so use '{existing}' or a name that differs in more than case"
    )
}

/// Enforce the blueprint's own `size_limit` on an ephemeral or per-session root.
async fn attach_size_limit(vfs: Vfs, blueprint: &Blueprint) -> Result<Vfs, VolumeError> {
    let Some(limit) = blueprint.vfs.size_limit() else {
        return Ok(vfs);
    };
    let mode = blueprint.vfs.mode_str();
    let (vfs, unmeasurable) = tokio::task::spawn_blocking(move || {
        let measured = vfs.measure_usage();
        let used = measured.as_ref().ok().copied();
        (vfs.with_measured_limit(limit, used), measured.err())
    })
    .await
    .map_err(|err| VolumeError::Io(format!("measuring the {mode} workspace failed: {err}")))?;
    if let Some(err) = unmeasurable {
        // The run still starts, treated as full; the program is told so when it
        // tries to write.
        warn(&format!(
            "the {mode} workspace could not be measured against its size limit: {err}"
        ));
    }
    Ok(vfs)
}

fn runtime_access(access: Access) -> RtAccess {
    match access {
        Access::ReadOnly => RtAccess::ReadOnly,
        Access::ReadWrite => RtAccess::ReadWrite,
    }
}

/// Where ephemeral scratch lives, one directory per run.
struct EphemeralRoot {
    path: PathBuf,
}

impl EphemeralRoot {
    /// Remove scratch an earlier process left behind: scratch is removed when
    /// its run ends, so at start every one is a leftover. Entries without the
    /// scratch prefix are left alone, and a link is removed without following it.
    fn sweep(&self) -> std::io::Result<()> {
        for entry in std::fs::read_dir(&self.path)? {
            let Ok(entry) = entry else { break };
            let is_scratch = entry
                .file_name()
                .to_str()
                .is_some_and(|name| name.starts_with(interpreter::runtime::vfs::TEMPDIR_PREFIX));
            if !is_scratch {
                continue;
            }
            let path = entry.path();
            let removed = entry.file_type().and_then(|file_type| {
                if file_type.is_dir() {
                    std::fs::remove_dir_all(&path)
                } else {
                    std::fs::remove_file(&path)
                }
            });
            if let Err(error) = removed {
                warn(&format!(
                    "removing leftover scratch {} failed: {error}; it is retried at the next start",
                    path.display()
                ));
            }
        }
        Ok(())
    }
}

/// Create `dir`, owner-only, unless a plain directory is already there. The
/// parent must exist.
pub(crate) fn create_owned_dir(dir: &Path) -> std::io::Result<()> {
    use std::os::unix::fs::DirBuilderExt;
    match std::fs::symlink_metadata(dir) {
        Ok(meta) => require_plain_dir(&meta),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => {
            match std::fs::DirBuilder::new().mode(0o700).create(dir) {
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
        "the path exists and is not a plain directory",
    ))
}

#[cfg(test)]
mod tests;
