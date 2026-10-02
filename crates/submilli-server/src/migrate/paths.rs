//! Refuse relocation when a boot dependency still names a moving directory.

use std::fs;
use std::io;
use std::path::{Component, Path, PathBuf};

use anyhow::{Context, Result, bail};
use submilli_server::config::{ServerDirectories, VolumeTable};

use super::{BLUEPRINTS, LegacyLayout, SERVER_DIR, SESSIONS, STAGING_DIR, VFS_SESSIONS};

pub(crate) fn validate_dependencies(
    layout: &LegacyLayout,
    directories: &ServerDirectories,
    volumes: &VolumeTable,
) -> Result<()> {
    let sources = moving_directories(layout);
    if sources.is_empty() {
        return Ok(());
    }
    let resolved = sources
        .iter()
        .map(|path| {
            fs::canonicalize(path).with_context(|| format!("resolving `{}`", path.display()))
        })
        .collect::<Result<Vec<_>>>()?;
    for (name, path) in dependency_paths(directories).into_iter().chain(
        volumes
            .iter()
            .map(|(name, path)| (name.as_str(), path.as_path())),
    ) {
        if let Some(index) = traversed_source(path, &resolved, 0)? {
            let pin_advice = if sources[index].starts_with(layout.root.join(STAGING_DIR)) {
                ""
            } else {
                "; alternatively, explicitly configure the legacy source directory to pin it before migration"
            };
            bail!(
                "migration would relocate {name} `{}` because it depends on `{}`; \
                 relocate this dependency outside the migration source and update its configuration{pin_advice}",
                path.display(),
                sources[index].display()
            );
        }
    }
    Ok(())
}

fn moving_directories(layout: &LegacyLayout) -> Vec<PathBuf> {
    let server = layout.root.join(SERVER_DIR);
    let staging = layout.root.join(STAGING_DIR);
    if !server.exists() {
        let mut sources: Vec<_> = super::legacy_directories(layout)
            .into_iter()
            .map(|relative| layout.root.join(relative))
            .collect();
        if staging.exists() {
            sources.push(staging);
        }
        return sources;
    }
    [BLUEPRINTS, SESSIONS, VFS_SESSIONS]
        .into_iter()
        .filter(|relative| {
            let source = staging.join(relative);
            let present = if *relative == VFS_SESSIONS {
                source.is_dir()
            } else {
                source.exists()
            };
            present && !server.join(relative).exists()
        })
        .map(|relative| staging.join(relative))
        .collect()
}

fn dependency_paths(directories: &ServerDirectories) -> Vec<(&'static str, &Path)> {
    let ServerDirectories {
        blueprint_dir,
        blueprint_seed_dir,
        package_store_root,
        package_fallback_root,
        secret_store_dir,
        secret_store_key_file,
        api_token_files,
        github_token_file,
        session_storage_root,
        session_store_dir,
        ephemeral_storage_root,
        config_file,
    } = directories;
    [
        ("blueprint store", blueprint_dir),
        ("blueprint seed directory", blueprint_seed_dir),
        ("package store", package_store_root),
        ("fallback package store", package_fallback_root),
        ("secret store", secret_store_dir),
        ("secret-store key file", secret_store_key_file),
        ("GitHub token file", github_token_file),
        ("per-session VFS root", session_storage_root),
        ("durable session store", session_store_dir),
        ("ephemeral storage root", ephemeral_storage_root),
        ("server config file", config_file),
    ]
    .into_iter()
    .filter_map(|(name, path)| path.as_deref().map(|path| (name, path)))
    .chain(
        api_token_files
            .iter()
            .map(|path| ("API token file", path.as_path())),
    )
    .collect()
}

/// Check every traversed prefix, including the location of a symlink itself.
/// Canonicalizing only the final target misses links inside a moving directory
/// that point outside it. Walking also resolves aliases to not-yet-created paths.
fn traversed_source(path: &Path, sources: &[PathBuf], links: usize) -> Result<Option<usize>> {
    if links > 40 {
        bail!(
            "too many symlinks while checking migration dependency `{}`",
            path.display()
        );
    }
    let absolute = std::env::current_dir()?.join(path);
    let mut current = PathBuf::new();
    let mut components = absolute.components();
    while let Some(component) = components.next() {
        if component == Component::ParentDir {
            current.pop();
        } else {
            current.push(component);
        }
        // Existing prefixes may have another spelling on the filesystem
        // (for example `SESSIONS` on a case-insensitive volume).
        let canonical = fs::canonicalize(&current).ok();
        if let Some(index) = sources.iter().position(|source| {
            current.starts_with(source)
                || canonical
                    .as_ref()
                    .is_some_and(|path| path.starts_with(source))
        }) {
            return Ok(Some(index));
        }
        match fs::symlink_metadata(&current) {
            Ok(metadata) if metadata.is_symlink() => {
                let target = fs::read_link(&current)
                    .with_context(|| format!("reading symlink `{}`", current.display()))?;
                current.pop();
                let remaining = current.join(target).join(components.as_path());
                return traversed_source(&remaining, sources, links + 1);
            }
            Ok(_) => {}
            Err(error) if error.kind() == io::ErrorKind::NotFound => {}
            Err(error) => {
                return Err(error).with_context(|| {
                    format!("checking migration dependency `{}`", current.display())
                });
            }
        }
    }
    Ok(None)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn layout(root: &Path) -> LegacyLayout {
        LegacyLayout {
            root: root.to_path_buf(),
            blueprints: true,
            sessions: true,
            vfs_sessions: true,
            secrets: None,
            key_configured: false,
        }
    }

    fn check_seed(layout: &LegacyLayout, path: PathBuf) -> Result<()> {
        validate_dependencies(
            layout,
            &ServerDirectories {
                blueprint_seed_dir: Some(path),
                ..Default::default()
            },
            &VolumeTable::new(),
        )
    }

    #[test]
    fn configured_missing_descendant_is_refused_but_server_default_is_allowed() {
        let home = tempfile::tempdir().unwrap();
        fs::create_dir(home.path().join(SESSIONS)).unwrap();
        let layout = layout(home.path());
        let result = check_seed(&layout, home.path().join("sessions/missing/seed"));
        assert!(result.unwrap_err().to_string().contains("blueprint seed"));
        check_seed(&layout, home.path().join("server/blueprints")).unwrap();
        check_seed(&layout, home.path().join("unrelated/seed")).unwrap();
        assert!(!home.path().join(STAGING_DIR).exists());
    }

    #[test]
    fn explicitly_pinned_source_is_not_a_candidate() {
        let home = tempfile::tempdir().unwrap();
        fs::create_dir(home.path().join(SESSIONS)).unwrap();
        let mut layout = layout(home.path());
        layout.sessions = false;
        check_seed(&layout, home.path().join("sessions/seed")).unwrap();
    }

    #[test]
    fn staging_dependencies_are_refused_only_when_their_directory_moves() {
        let home = tempfile::tempdir().unwrap();
        let staged = home.path().join("server.migrating/sessions");
        fs::create_dir_all(&staged).unwrap();
        let layout = layout(home.path());
        let error = check_seed(&layout, staged.join("seed"))
            .unwrap_err()
            .to_string();
        assert!(error.contains("relocate this dependency"));
        assert!(!error.contains("pin it"));
        fs::create_dir(home.path().join(SERVER_DIR)).unwrap();
        assert!(check_seed(&layout, staged.join("seed")).is_err());
        fs::create_dir(home.path().join("server/sessions")).unwrap();
        check_seed(&layout, staged.join("seed")).unwrap();
    }

    #[test]
    fn volume_roots_are_dependencies() {
        let home = tempfile::tempdir().unwrap();
        fs::create_dir(home.path().join(SESSIONS)).unwrap();
        let volumes = VolumeTable::from([("data".into(), home.path().join("sessions/data"))]);
        assert!(
            validate_dependencies(
                &layout(home.path()),
                &ServerDirectories::default(),
                &volumes
            )
            .is_err()
        );
    }

    #[cfg(unix)]
    #[test]
    fn symlink_aliases_and_links_inside_sources_are_dependencies() {
        use std::os::unix::fs::symlink;
        let home = tempfile::tempdir().unwrap();
        let sessions = home.path().join(SESSIONS);
        fs::create_dir(&sessions).unwrap();
        let outside = home.path().join("outside");
        fs::create_dir(&outside).unwrap();
        symlink(&sessions, home.path().join("alias")).unwrap();
        symlink(sessions.join("missing"), home.path().join("dangling")).unwrap();
        symlink(&outside, sessions.join("link")).unwrap();
        let layout = layout(home.path());
        for path in ["alias/seed", "dangling/seed", "sessions/link/seed"] {
            assert!(
                check_seed(&layout, home.path().join(path)).is_err(),
                "{path}"
            );
        }
    }

    #[test]
    fn filesystem_case_aliases_are_dependencies() {
        let home = tempfile::tempdir().unwrap();
        let sessions = home.path().join(SESSIONS);
        fs::create_dir(&sessions).unwrap();
        let alias = home.path().join("SESSIONS");
        if !alias.is_dir() {
            return; // This filesystem distinguishes the two spellings.
        }

        assert!(check_seed(&layout(home.path()), alias.join("missing/seed")).is_err());
        assert!(sessions.is_dir());
    }

    #[test]
    fn relative_dependency_paths_are_resolved_from_the_working_directory() {
        let cwd = std::env::current_dir().unwrap();
        let home = tempfile::tempdir_in(&cwd).unwrap();
        fs::create_dir(home.path().join(SESSIONS)).unwrap();
        let path = home
            .path()
            .strip_prefix(&cwd)
            .unwrap()
            .join("sessions/seed");
        assert!(check_seed(&layout(home.path()), path).is_err());
    }
}
