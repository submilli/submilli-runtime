use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::io::Read;
use std::path::{Component, Path, PathBuf};

use anyhow::{Context, bail};
use serde::Serialize;
use sha2::{Digest, Sha256};
use submilli_build::{DependencyKind, ProjectManifest, find_manifest_upwards, parse_manifest};

const MAX_BYTES: usize = 512 * 1024;
const MAX_FILES: usize = 1024;
const MAX_ENTRIES: usize = 20_000;

#[derive(Serialize)]
pub(super) struct Snapshot {
    pub packages: Vec<String>,
    pub configuration: BTreeMap<String, BTreeMap<String, String>>,
    pub authority: Option<super::authority::Evidence>,
    pub files: BTreeMap<String, Source>,
    pub coverage_gaps: Vec<String>,
}

#[derive(Serialize)]
pub(super) struct Source {
    pub sha256: String,
    pub content: String,
}

impl Snapshot {
    pub fn source_hash(&self) -> anyhow::Result<String> {
        let hashes: BTreeMap<_, _> = self
            .files
            .iter()
            .map(|(path, source)| (path, &source.sha256))
            .collect();
        Ok(format!(
            "{:x}",
            Sha256::digest(serde_json::to_vec(&(
                env!("CARGO_PKG_VERSION"),
                &self.packages,
                hashes
            ))?)
        ))
    }
}

pub(super) fn collect(only_package: Option<&str>) -> anyhow::Result<(Snapshot, ProjectManifest)> {
    let cwd = std::env::current_dir()?;
    collect_from(&cwd, only_package)
}

pub(super) fn collect_from(
    directory: &Path,
    only_package: Option<&str>,
) -> anyhow::Result<(Snapshot, ProjectManifest)> {
    collect_from_with_store(
        directory,
        only_package,
        &submilli_build::PackageStore::default(),
    )
}

pub(super) fn collect_from_with_store(
    directory: &Path,
    only_package: Option<&str>,
    store: &submilli_build::PackageStore,
) -> anyhow::Result<(Snapshot, ProjectManifest)> {
    let manifest_path = find_manifest_upwards(directory)
        .context("no submilli.toml found; run security-review from a package project")?;
    let root = manifest_path
        .parent()
        .context("manifest has no parent")?
        .canonicalize()?;
    let mut snapshot = Snapshot {
        packages: Vec::new(),
        configuration: BTreeMap::new(),
        authority: None,
        files: BTreeMap::new(),
        coverage_gaps: Vec::new(),
    };
    let mut remaining = MAX_BYTES;
    add_file(
        &root,
        &root.join("submilli.toml"),
        &mut snapshot,
        &mut remaining,
    )?;
    let manifest_text = &snapshot
        .files
        .get("submilli.toml")
        .context("manifest missing from snapshot")?
        .content;
    let manifest = parse_manifest(manifest_text, &root).map_err(|diagnostics| {
        anyhow::anyhow!(
            "invalid package manifest: {}",
            diagnostics
                .iter()
                .map(|d| d.message.as_str())
                .collect::<Vec<_>>()
                .join("; ")
        )
    })?;
    let packages: BTreeMap<_, _> = manifest
        .packages
        .iter()
        .map(|p| (p.name.as_str(), p))
        .collect();
    let mut pending: Vec<_> = match only_package {
        Some(name) if packages.contains_key(name) => vec![name],
        Some(name) => bail!("unknown package {name}; select a package declared in submilli.toml"),
        None => packages.keys().copied().collect(),
    };
    let mut seen = BTreeSet::new();
    let mut visited_entries = 0usize;
    while let Some(name) = pending.pop() {
        if !seen.insert(name) {
            continue;
        }
        let package = packages
            .get(name)
            .context("local dependency is absent from manifest")?;
        snapshot.packages.push(name.to_owned());
        collect_package(
            &root,
            package.path.as_path(),
            &mut snapshot,
            &mut remaining,
            &mut visited_entries,
        )?;
        if only_package.is_some() && only_package != Some(name) {
            let config_path: PathBuf = package
                .path
                .as_path()
                .join("src/config.ts")
                .components()
                .filter(|component| !matches!(component, Component::CurDir))
                .collect();
            let legacy_path = config_path.with_extension("subm");
            if snapshot
                .files
                .keys()
                .any(|path| Path::new(path) == config_path || Path::new(path) == legacy_path)
            {
                match store.load(name) {
                    Ok(artifact) => {
                        let overrides = artifact.metadata.config.overrides;
                        let size = serde_json::to_vec(&overrides)?.len();
                        remaining = remaining
                            .checked_sub(size)
                            .context("captured configuration exceeds review size limit")?;
                        snapshot.configuration.insert(name.to_owned(), overrides);
                    }
                    Err(submilli_build::PackageStoreError::MissingPackage { .. }) => {}
                    Err(error) => return Err(error.into()),
                }
            }
        }
        for dependency in &package.dependencies {
            match dependency.kind {
                DependencyKind::Sibling => pending.push(dependency.name.as_str()),
                DependencyKind::External | DependencyKind::Github => snapshot.coverage_gaps.push(format!(
                    "{} depends on {}; external dependency source is not included. Review it in its source project.", name, dependency.name.as_str()
                )),
            }
        }
    }
    snapshot.packages.sort();
    Ok((snapshot, manifest))
}

fn collect_package(
    root: &Path,
    package: &Path,
    snapshot: &mut Snapshot,
    remaining: &mut usize,
    visited: &mut usize,
) -> anyhow::Result<()> {
    let package_dir = root.join(package);
    ensure_contained(root, &package_dir)?;
    let source_dir = package_dir.join("src");
    let mut pending = vec![(package_dir, 0usize)];
    while let Some((directory, depth)) = pending.pop() {
        if depth > 64 {
            bail!("package directory nesting exceeds 64 levels");
        }
        for entry in
            fs::read_dir(&directory).with_context(|| format!("read {}", directory.display()))?
        {
            let entry = entry?;
            *visited = visited
                .checked_add(1)
                .context("too many directory entries")?;
            if *visited > MAX_ENTRIES {
                bail!(
                    "review exceeds {MAX_ENTRIES} directory entries; select a smaller package with -p"
                );
            }
            let name = entry.file_name();
            let name = name.to_str().context("review paths must be UTF-8")?;
            // The compiler includes every source beneath src, including hidden
            // files and directories whose names resemble generated artifacts.
            if !directory.starts_with(&source_dir)
                && (name.starts_with('.')
                    || matches!(name, "node_modules" | "target" | "dist" | "graphify-out"))
            {
                continue;
            }
            let path = entry.path();
            let kind = entry.file_type()?;
            if kind.is_symlink() {
                bail!(
                    "review refuses symlink {}; use regular source files",
                    path.display()
                );
            }
            if kind.is_dir() {
                pending.push((path, depth.saturating_add(1)));
            } else if is_review_file(&path, name) {
                add_file(root, &path, snapshot, remaining)?;
            }
        }
    }
    Ok(())
}

fn is_review_file(path: &Path, name: &str) -> bool {
    matches!(
        path.extension().and_then(|s| s.to_str()),
        Some("ts" | "subm")
    ) || matches!(
        name,
        "readme.md" | "README.md" | "capabilities.yaml" | "submilli.lock"
    )
}

fn ensure_contained(root: &Path, path: &Path) -> anyhow::Result<()> {
    let relative = path
        .strip_prefix(root)
        .context("source path is outside the project")?;
    let mut ancestor = root.to_path_buf();
    for component in relative.components() {
        match component {
            Component::Normal(name) => ancestor.push(name),
            Component::CurDir => continue,
            _ => bail!(
                "source path must stay inside the project: {}",
                path.display()
            ),
        }
        if fs::symlink_metadata(&ancestor)?.file_type().is_symlink() {
            bail!("review refuses symlink {}", ancestor.display());
        }
    }
    Ok(())
}

fn add_file(
    root: &Path,
    path: &Path,
    snapshot: &mut Snapshot,
    remaining: &mut usize,
) -> anyhow::Result<()> {
    ensure_contained(root, path)?;
    let relative: PathBuf = path
        .strip_prefix(root)?
        .components()
        .filter(|c| !matches!(c, Component::CurDir))
        .collect();
    let name = relative
        .to_str()
        .context("review paths must be UTF-8")?
        .replace(std::path::MAIN_SEPARATOR, "/");
    if snapshot.files.contains_key(&name) {
        return Ok(());
    }
    if snapshot.files.len() >= MAX_FILES {
        bail!("review exceeds {MAX_FILES} files; select a smaller package with -p");
    }
    let mut options = fs::OpenOptions::new();
    options.read(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK);
    }
    let file = options
        .open(path)
        .with_context(|| format!("read {}", path.display()))?;
    if !file.metadata()?.is_file() {
        bail!("review requires a regular file: {}", path.display());
    }
    let mut content = String::new();
    file.take(u64::try_from(*remaining)?.saturating_add(1))
        .read_to_string(&mut content)?;
    *remaining = remaining
        .checked_sub(content.len())
        .context("review source exceeds 512 KiB; select a smaller package with -p")?;
    let sha256 = format!("{:x}", Sha256::digest(content.as_bytes()));
    snapshot.files.insert(name, Source { sha256, content });
    Ok(())
}

#[cfg(test)]
mod configuration_tests {
    use super::*;

    #[test]
    fn configuration_is_captured_for_siblings_before_isolated_review() {
        let project = tempfile::tempdir().unwrap();
        let root = project.path();
        fs::write(root.join("submilli.toml"), "[[package]]\nname = '@pied-piper/app'\nversion = '1.0.0'\ndescription = 'App.'\npath = 'app'\ndependencies = ['@pied-piper/service']\n[[package]]\nname = '@pied-piper/service'\nversion = '1.0.0'\ndescription = 'Service.'\npath = 'service'\n").unwrap();
        for name in ["app", "service"] {
            fs::create_dir_all(root.join(name).join("src")).unwrap();
            fs::create_dir_all(root.join(name).join("docs")).unwrap();
            fs::write(root.join(name).join("docs/readme.md"), "# Package\n").unwrap();
        }
        fs::write(
            root.join("service/src/config.ts"),
            "export declare const baseUrl: string;",
        )
        .unwrap();
        fs::write(root.join("service/src/lib.ts"), "import { baseUrl } from './config';\n/** Endpoint.\n * @returns Endpoint.\n */\nexport function endpoint(): string { return baseUrl; }").unwrap();
        fs::write(root.join("app/src/lib.ts"), "import { endpoint } from '@pied-piper/service';\n/** Read.\n * @returns Endpoint.\n */\nexport function read(): string { return endpoint(); }").unwrap();
        let manifest = submilli_build::load_manifest(&root.join("submilli.toml")).unwrap();
        let home = tempfile::tempdir().unwrap();
        let store = submilli_build::PackageStore::new(home.path());
        let selected = submilli_build::PackageName::new("@pied-piper/service");
        let inputs = submilli_build::ConfigInputs::parse(
            &["baseUrl=https://captured.test".into()],
            &[],
            true,
        )
        .unwrap();
        let built = submilli_build::build_packages_with_inputs(
            &manifest,
            root,
            &store,
            Some(&selected),
            &inputs,
        )
        .unwrap();
        submilli_build::install_packages(&store, &built).unwrap();
        let (snapshot, manifest) =
            collect_from_with_store(root, Some("@pied-piper/app"), &store).unwrap();
        assert_eq!(
            snapshot.configuration["@pied-piper/service"]["baseUrl"],
            "https://captured.test"
        );
        assert!(!snapshot.configuration.contains_key("@pied-piper/app"));
        fs::remove_dir_all(home.path().join("@pied-piper/service")).unwrap();
        let evidence = super::super::authority::collect(
            &snapshot,
            &manifest,
            Some("@pied-piper/app"),
            &submilli_build::ConfigInputs::default(),
        )
        .unwrap();
        let evidence = serde_json::to_value(evidence).unwrap();
        let service = evidence["packages"]
            .as_array()
            .unwrap()
            .iter()
            .find(|package| package["name"] == "@pied-piper/service")
            .unwrap();
        assert_eq!(
            service["config"]["values"]["baseUrl"],
            "https://captured.test"
        );
    }
}
