//! Which project and which blueprint a playground serves.
//!
//! The project is the nearest directory, walking up from where the command runs,
//! that holds a `submilli/` folder with a `submilli.toml`, or a root
//! `submilli.toml`. The blueprint is the one `--blueprint` names, otherwise the
//! single file under `submilli/blueprints/`, or the project's root
//! `blueprint.yaml`; a playground serves one blueprint. Either way the blueprint's
//! path is resolved through symlinks, so the watcher watches the file saves land in,
//! and a blueprint inside the playground's state directory is refused: the serving
//! process never opens its own state through a path the developer chose.
//!
//! The project's volumes are the directories under `submilli/volumes/`, each a
//! read-only volume of its name. A symlinked one, or one that holds the state
//! directory, is skipped with a warning, so a program cannot read the playground's
//! tokens or its lock file through a volume.

use std::fmt;
use std::path::{Path, PathBuf};

use submilli_server::config::{Access, VolumeSpec, VolumeTable, is_managed_name};

use super::log::warn;
use super::state::StateDir;

pub(crate) struct Project {
    /// The directory that holds the project; the playground's state lives under it.
    pub(crate) root: PathBuf,
    /// The directory holding the project's `submilli.toml`: `<root>/submilli` or the
    /// root itself.
    pub(crate) package_dir: PathBuf,
    pub(crate) blueprint: PathBuf,
}

#[derive(Debug)]
pub(crate) enum DiscoveryError {
    NoProject {
        from: PathBuf,
    },
    NoBlueprint {
        searched: Vec<PathBuf>,
    },
    SeveralBlueprints {
        found: Vec<PathBuf>,
    },
    InStateDir {
        blueprint: PathBuf,
        state_dir: PathBuf,
    },
    Io {
        path: PathBuf,
        error: std::io::Error,
    },
}

impl fmt::Display for DiscoveryError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NoProject { from } => write!(
                f,
                "no Submilli project at or above {}: looked for `submilli/submilli.toml` and \
                 `submilli.toml`. Create one with `submilli playground init`.",
                from.display()
            ),
            Self::NoBlueprint { searched } => {
                write!(f, "the project has no blueprint; looked in")?;
                for path in searched {
                    write!(f, " {}", path.display())?;
                }
                write!(f, ". Add one, or pass `--blueprint <path>`.")
            }
            Self::SeveralBlueprints { found } => {
                write!(
                    f,
                    "a playground serves one blueprint, and the project has {}:",
                    found.len()
                )?;
                for path in found {
                    write!(f, "\n  {}", path.display())?;
                }
                write!(f, "\nChoose one with `--blueprint <path>`.")
            }
            Self::InStateDir {
                blueprint,
                state_dir,
            } => write!(
                f,
                "{} is inside the playground's state directory {}, which holds only what the \
                 playground writes; keep the blueprint elsewhere in the project and pass that \
                 path to `--blueprint`.",
                blueprint.display(),
                state_dir.display()
            ),
            Self::Io { path, error } => write!(f, "reading {}: {error}", path.display()),
        }
    }
}

impl std::error::Error for DiscoveryError {}

/// The project enclosing `from`, and its blueprint.
pub(crate) fn discover(from: &Path, blueprint: Option<&Path>) -> Result<Project, DiscoveryError> {
    let from = canonical(from)?;
    let (root, package_dir) = find_root(&from).ok_or(DiscoveryError::NoProject { from })?;
    let blueprint = match blueprint {
        Some(path) => canonical(path)?,
        None => canonical(&only_blueprint(&root, &package_dir)?)?,
    };
    if let Some(state_dir) =
        holding_state_dir(&root, &blueprint, |state, path| path.starts_with(state))
    {
        return Err(DiscoveryError::InStateDir {
            blueprint,
            state_dir,
        });
    }
    Ok(Project {
        root,
        package_dir,
        blueprint,
    })
}

/// The project enclosing `from`, without choosing a blueprint.
pub(crate) fn find_project_root(from: &Path) -> Option<PathBuf> {
    let from = from.canonicalize().ok()?;
    find_root(&from).map(|(root, _)| root)
}

/// The project root and the directory holding its `submilli.toml`.
fn find_root(from: &Path) -> Option<(PathBuf, PathBuf)> {
    from.ancestors().find_map(|dir| {
        let nested = dir.join("submilli");
        if nested.join("submilli.toml").is_file() {
            return Some((dir.to_path_buf(), nested));
        }
        if !dir.join("submilli.toml").is_file() {
            return None;
        }
        // Inside a nested project's `submilli/` folder, the project is its parent.
        match dir.parent() {
            Some(parent) if dir.file_name() == Some("submilli".as_ref()) => {
                Some((parent.to_path_buf(), dir.to_path_buf()))
            }
            _ => Some((dir.to_path_buf(), dir.to_path_buf())),
        }
    })
}

fn only_blueprint(root: &Path, package_dir: &Path) -> Result<PathBuf, DiscoveryError> {
    let directory = package_dir.join("blueprints");
    let root_file = root.join("blueprint.yaml");
    let mut found = Vec::new();
    if directory.is_dir() {
        let entries = std::fs::read_dir(&directory).map_err(|error| DiscoveryError::Io {
            path: directory.clone(),
            error,
        })?;
        for entry in entries {
            let path = entry
                .map_err(|error| DiscoveryError::Io {
                    path: directory.clone(),
                    error,
                })?
                .path();
            let yaml = path
                .extension()
                .is_some_and(|extension| extension == "yaml" || extension == "yml");
            if yaml && path.is_file() {
                found.push(path);
            }
        }
    }
    if root_file.is_file() {
        found.push(root_file.clone());
    }
    found.sort();
    match found.len() {
        0 => Err(DiscoveryError::NoBlueprint {
            searched: vec![directory, root_file],
        }),
        1 => Ok(found.remove(0)),
        _ => Err(DiscoveryError::SeveralBlueprints { found }),
    }
}

/// The project's volumes: each directory under `submilli/volumes/` is a read-only
/// volume of that name, so a fixture there is read through a recorded file read. A
/// blueprint naming any other volume gets a managed one under the state directory.
/// A symlinked directory, or one that holds the state directory, is skipped.
pub(crate) fn volumes(project: &Project) -> VolumeTable {
    let mut volumes = VolumeTable::new();
    let Ok(entries) = std::fs::read_dir(project.package_dir.join("volumes")) else {
        return volumes;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        let Some(name) = entry.file_name().to_str().map(str::to_owned) else {
            continue;
        };
        let Ok(file_type) = entry.file_type() else {
            continue;
        };
        if file_type.is_symlink() && path.is_dir() {
            warn(&format!(
                "{} is a symlink, so it is not served as volume `{name}`; copy the files \
                 into a directory there instead",
                path.display()
            ));
            continue;
        }
        if !file_type.is_dir() || !is_managed_name(&name) {
            continue;
        }
        let Ok(resolved) = path.canonicalize() else {
            continue;
        };
        // `volumes/` itself may be a link: a volume that holds the state directory
        // would serve the playground's tokens and lock file to programs.
        if holding_state_dir(&project.root, &resolved, |state, path| {
            state.starts_with(path) || path.starts_with(state)
        })
        .is_some()
        {
            warn(&format!(
                "{} holds the playground's state directory, so it is not served as volume \
                 `{name}`",
                path.display()
            ));
            continue;
        }
        volumes.insert(
            name,
            VolumeSpec::local_path(resolved).with_access(Access::ReadOnly),
        );
    }
    volumes
}

/// The playground's state directory under `root`, as written and as resolved, when
/// `overlaps` holds for it and the canonical `path`.
fn holding_state_dir(
    root: &Path,
    path: &Path,
    overlaps: impl Fn(&Path, &Path) -> bool,
) -> Option<PathBuf> {
    let state_dir = StateDir::for_project(root).root().to_path_buf();
    let resolved = state_dir.canonicalize().ok();
    let overlapping = [Some(&state_dir), resolved.as_ref()]
        .into_iter()
        .flatten()
        .any(|state| overlaps(state, path));
    overlapping.then_some(state_dir)
}

fn canonical(path: &Path) -> Result<PathBuf, DiscoveryError> {
    path.canonicalize().map_err(|error| DiscoveryError::Io {
        path: path.to_path_buf(),
        error,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn write(path: &Path, text: &str) {
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, text).unwrap();
    }

    #[test]
    fn a_nested_project_is_found_from_a_subdirectory() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().canonicalize().unwrap();
        write(&root.join("submilli/submilli.toml"), "");
        write(&root.join("submilli/blueprints/demo.yaml"), "name: demo\n");
        std::fs::create_dir_all(root.join("src/deep")).unwrap();
        let project = discover(&root.join("src/deep"), None).unwrap();
        assert_eq!(project.root, root);
        assert_eq!(
            project.blueprint,
            root.join("submilli/blueprints/demo.yaml")
        );
    }

    #[test]
    fn a_root_project_is_found_from_a_subdirectory() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().canonicalize().unwrap();
        write(&root.join("submilli.toml"), "");
        write(&root.join("blueprint.yaml"), "name: demo\n");
        std::fs::create_dir_all(root.join("app/handlers")).unwrap();
        let project = discover(&root.join("app/handlers"), None).unwrap();
        assert_eq!(project.root, root);
        assert_eq!(project.package_dir, root);
        assert_eq!(project.blueprint, root.join("blueprint.yaml"));
        assert_eq!(find_project_root(&root.join("app")), Some(root));
    }

    #[test]
    fn the_nested_layout_names_its_package_folder() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().canonicalize().unwrap();
        write(&root.join("submilli/submilli.toml"), "");
        write(&root.join("submilli/blueprints/demo.yaml"), "name: demo\n");
        let project = discover(&root.join("submilli/blueprints"), None).unwrap();
        assert_eq!(project.root, root);
        assert_eq!(project.package_dir, root.join("submilli"));
    }

    #[cfg(unix)]
    #[test]
    fn a_symlinked_blueprint_resolves_to_its_target() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().canonicalize().unwrap();
        write(&root.join("submilli/submilli.toml"), "");
        write(&root.join("shared/demo.yaml"), "name: demo\n");
        std::fs::create_dir_all(root.join("submilli/blueprints")).unwrap();
        std::os::unix::fs::symlink(
            root.join("shared/demo.yaml"),
            root.join("submilli/blueprints/demo.yaml"),
        )
        .unwrap();
        let implicit = discover(&root, None).unwrap();
        assert_eq!(implicit.blueprint, root.join("shared/demo.yaml"));
        let explicit = discover(&root, Some(&root.join("submilli/blueprints/demo.yaml"))).unwrap();
        assert_eq!(explicit.blueprint, implicit.blueprint);
    }

    #[test]
    fn a_blueprint_inside_the_state_directory_is_refused() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().canonicalize().unwrap();
        write(&root.join("submilli/submilli.toml"), "");
        write(&root.join("submilli/blueprints/demo.yaml"), "name: demo\n");
        let inside = root.join(".submilli/playground/blueprints/demo.yaml");
        write(&inside, "name: demo\n");
        let error = discover(&root, Some(&inside)).err().unwrap();
        assert!(
            matches!(error, DiscoveryError::InStateDir { .. }),
            "{error}"
        );
        assert!(error.to_string().contains("--blueprint"), "{error}");
        // The implicit blueprint is refused when a link leads there too.
        #[cfg(unix)]
        {
            std::fs::remove_file(root.join("submilli/blueprints/demo.yaml")).unwrap();
            std::os::unix::fs::symlink(&inside, root.join("submilli/blueprints/demo.yaml"))
                .unwrap();
            let error = discover(&root, None).err().unwrap();
            assert!(
                matches!(error, DiscoveryError::InStateDir { .. }),
                "{error}"
            );
        }
    }

    #[cfg(unix)]
    #[test]
    fn a_symlinked_volume_or_one_holding_the_state_directory_is_not_served() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().canonicalize().unwrap();
        write(&root.join("submilli/submilli.toml"), "");
        write(&root.join("submilli/blueprints/demo.yaml"), "name: demo\n");
        write(&root.join("submilli/volumes/billing/charges.json"), "[]");
        write(&root.join(".submilli/playground/tokens/admin"), "secret");
        std::os::unix::fs::symlink(&root, root.join("submilli/volumes/everything")).unwrap();
        write(&root.join("shared/notes.txt"), "hi");
        std::os::unix::fs::symlink(root.join("shared"), root.join("submilli/volumes/shared"))
            .unwrap();
        let project = discover(&root, None).unwrap();
        let served = volumes(&project);
        assert_eq!(served.keys().collect::<Vec<_>>(), ["billing"]);
        assert_eq!(served["billing"].access, Access::ReadOnly);

        // `volumes/` itself a link: a folder there that is the state directory is
        // not served, and its neighbours are.
        std::fs::remove_dir_all(root.join("submilli/volumes")).unwrap();
        std::fs::create_dir_all(root.join(".submilli/data")).unwrap();
        std::os::unix::fs::symlink(root.join(".submilli"), root.join("submilli/volumes")).unwrap();
        let served = volumes(&project);
        assert_eq!(served.keys().collect::<Vec<_>>(), ["data"]);
    }

    #[test]
    fn several_blueprints_are_listed() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        write(&root.join("submilli.toml"), "");
        write(&root.join("blueprint.yaml"), "name: a\n");
        write(&root.join("blueprints/b.yml"), "name: b\n");
        let error = discover(root, None).err().unwrap();
        assert!(
            matches!(&error, DiscoveryError::SeveralBlueprints { found } if found.len() == 2),
            "{error}"
        );
        assert!(error.to_string().contains("--blueprint"));
    }

    #[test]
    fn no_project_names_init() {
        let dir = tempfile::tempdir().unwrap();
        let error = discover(dir.path(), None).err().unwrap();
        assert!(matches!(error, DiscoveryError::NoProject { .. }));
        assert!(error.to_string().contains("submilli playground init"));
    }
}
