use crate::application::sessions::error::SessionError;
use crate::application::sessions::ports::SessionWorkspaces;
use crate::domain::session::RootVfs;
use crate::session_manager::build_vfs;
use crate::volumes::VolumeRegistry;
use std::path::{Path, PathBuf};
use submilli_blueprint::{Blueprint, VarBindings, VfsConfig};

pub(crate) struct LocalSessionWorkspaces<'a> {
    pub session_root: &'a Path,
    pub ephemeral_root: Option<&'a Path>,
    pub volumes: &'a VolumeRegistry,
}

impl SessionWorkspaces for LocalSessionWorkspaces<'_> {
    fn resolve_root(
        &self,
        id: &str,
        blueprint: &Blueprint,
        variables: &VarBindings,
    ) -> Result<RootVfs, SessionError> {
        let config = blueprint
            .vfs
            .resolve(variables)
            .map_err(|error| SessionError::InvalidVfs(error.to_string()))?;
        Ok(match config {
            VfsConfig::None => RootVfs::None,
            VfsConfig::Ephemeral { .. } => RootVfs::Ephemeral,
            VfsConfig::PerSession { .. } => RootVfs::PerSession {
                path: self.session_root.join(id),
            },
            VfsConfig::Named {
                volume,
                access,
                sub_path,
                ..
            } => {
                let resolved = self.volumes.resolve(&volume, access)?;
                let path = match sub_path {
                    Some(path) => resolved.host.join(path),
                    None => resolved.host,
                };
                RootVfs::Named { path }
            }
        })
    }

    fn provision(
        &self,
        id: &str,
        blueprint: &Blueprint,
        variables: &VarBindings,
        existed: bool,
    ) -> Result<(), SessionError> {
        let owned_root = matches!(blueprint.vfs, VfsConfig::PerSession { .. })
            .then(|| self.session_root.join(id));
        if let Some(root) = &owned_root {
            std::fs::create_dir_all(root).map_err(|e| SessionError::Io(e.to_string()))?;
        }
        let vfs = match build_vfs(
            blueprint,
            variables,
            owned_root.as_deref(),
            self.ephemeral_root,
            self.volumes,
        ) {
            Ok(vfs) => vfs,
            Err(error) => {
                if !existed
                    && let Some(root) = &owned_root
                    && let Err(cleanup) = std::fs::remove_dir(root)
                {
                    tracing::warn!(%cleanup, "failed binding workspace cleanup");
                }
                return Err(error);
            }
        };
        drop(vfs);
        Ok(())
    }
    fn list(&self) -> Result<Vec<(String, PathBuf)>, SessionError> {
        let entries = match std::fs::read_dir(self.session_root) {
            Ok(entries) => entries,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
            Err(error) => return Err(SessionError::Io(error.to_string())),
        };
        let mut folders = Vec::new();
        for entry in entries {
            let entry = entry.map_err(|error| SessionError::Io(error.to_string()))?;
            if entry
                .file_type()
                .map_err(|error| SessionError::Io(error.to_string()))?
                .is_dir()
                && let Some(id) = entry.file_name().to_str()
            {
                folders.push((id.to_owned(), entry.path()));
            }
        }
        Ok(folders)
    }
}
