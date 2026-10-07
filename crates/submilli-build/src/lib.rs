//! Build-side project and package orchestration.
//!
//! This crate owns Submilli package-project structure. The first slice is the
//! v1 `submilli.toml` manifest parser; future build, curate, and package graph
//! steps live here rather than in the interpreter.

pub mod artifact;
pub mod blueprint_validation;
pub mod capabilities;
pub mod doc_examples;
pub mod driver;
pub mod install;
pub mod lockfile;
pub mod package_store;
pub mod resolve;
pub mod scaffold;
mod warning_policy;

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::ops::Range;
use std::path::{Component, Path, PathBuf};

use serde::Deserialize;
use toml::Spanned;

pub use artifact::{
    ARTIFACT_SCHEMA_VERSION, Artifact, ArtifactDependency, ArtifactError, ArtifactMetadata,
    ArtifactSource, GithubSource, InstalledSources, PackageSource, read_installed_sources,
    read_package_artifact, write_capabilities_file, write_package_artifact,
    write_package_artifact_with_docs, write_package_artifact_with_docs_and_sources,
};
pub use capabilities::{
    CapabilitySchema, ProvidedCapability, ProvidedField, RequiredCapability,
    derive_capability_schema,
};
pub use doc_examples::{
    DocExample, compile_check_doc_example, compile_doc_example_warnings, extract_doc_examples,
};
pub use driver::{BuiltPackage, DriverError, build_packages, install_packages, package_sources};
pub use install::{
    InstallConflict, InstallError, InstallPreparation, InstallReport, InstalledPackage,
    install_from_dir, install_plan,
};
pub use lockfile::{LOCKFILE_NAME, LockedPackage, Lockfile, LockfileError};
pub use package_store::{
    LocatedPackage, PackageStore, PackageStoreError, default_data_root, default_package_store_dir,
};
pub use resolve::{
    FetchError, FetchErrorKind, FetchedRepo, GithubClosure, PlannedInstall, RepoFetcher,
    ResolveError, resolve_github_closure,
};
pub use scaffold::{
    ScaffoldError, ScaffoldedPackage, add_package, init_project, is_valid_package_name,
    refresh_dependency_types, refresh_editor_files,
};
pub use warning_policy::{deny_warnings_from_env, warning_denial_message};

// Bound recursive graph traversal on request threads before following an edge.
pub(crate) const MAX_DEPENDENCY_DEPTH: usize = 128;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ProjectManifest {
    pub dependencies: BTreeMap<PackageName, DependencySource>,
    pub packages: Vec<PackageManifest>,
}

/// How a top-level `[dependencies]` entry is sourced.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum DependencySource {
    /// Declared by version string; resolved from the local package store.
    Version(PackageVersion),
    /// Pinned to a GitHub repo + commit SHA; fetched into the store on build.
    Github { url: String, sha: String },
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PackageManifest {
    pub name: PackageName,
    pub version: PackageVersion,
    pub description: String,
    pub keywords: Vec<String>,
    pub path: PackagePath,
    pub entrypoint: PackageEntrypoint,
    pub dependencies: Vec<ResolvedDependency>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ResolvedDependency {
    pub name: PackageName,
    /// Declared version. `None` for a GitHub dependency — a GitHub pin
    /// identifies by commit SHA, and the version is adopted from the fetched
    /// package's own manifest once it is installed in the store.
    pub version: Option<PackageVersion>,
    pub kind: DependencyKind,
}

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct PackageName(String);

impl PackageName {
    pub fn new(name: impl Into<String>) -> Self {
        Self(name.into())
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct PackageVersion(String);

impl PackageVersion {
    pub fn new(version: impl Into<String>) -> Self {
        Self(version.into())
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PackagePath(PathBuf);

impl PackagePath {
    pub fn new(path: impl Into<PathBuf>) -> Self {
        Self(path.into())
    }

    pub fn as_path(&self) -> &Path {
        &self.0
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PackageEntrypoint(PathBuf);

impl PackageEntrypoint {
    pub fn new(entrypoint: impl Into<PathBuf>) -> Self {
        Self(entrypoint.into())
    }

    pub fn as_path(&self) -> &Path {
        &self.0
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DependencyKind {
    /// Another `[[package]]` in the same manifest.
    Sibling,
    /// A versioned dependency resolved from the local package store.
    External,
    /// A dependency fetched from a pinned GitHub commit (source in the
    /// top-level `[dependencies]`); loaded from the store once installed.
    Github,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BuildDiagnostic {
    pub severity: BuildSeverity,
    pub span: Option<SourceSpan>,
    pub message: String,
    pub help: Vec<String>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BuildSeverity {
    Error,
    Warning,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SourceSpan {
    pub start: usize,
    pub end: usize,
}

pub const CANONICAL_SOURCE_EXTENSION: &str = "ts";
pub const LEGACY_SOURCE_EXTENSION: &str = "subm";

#[derive(Deserialize)]
struct RawManifest {
    #[serde(default)]
    dependencies: BTreeMap<String, Spanned<RawDependency>>,
    package: Option<Vec<Spanned<RawPackage>>>,
}

/// A top-level `[dependencies]` value: either a bare version string
/// (`"@org/leaf" = "1.0"`) or a GitHub source table
/// (`"@org/leaf" = { github = "github.com/org/repo", rev = "<sha>" }`).
#[derive(Deserialize)]
#[serde(untagged)]
enum RawDependency {
    Version(String),
    Github { github: String, rev: String },
}

#[derive(Deserialize)]
struct RawPackage {
    name: Spanned<String>,
    version: Spanned<String>,
    description: Option<Spanned<String>>,
    #[serde(default)]
    keywords: Vec<Spanned<String>>,
    path: Option<Spanned<String>>,
    #[serde(default)]
    dependencies: Vec<Spanned<String>>,
}

pub fn parse_manifest(
    source: &str,
    manifest_dir: &Path,
) -> Result<ProjectManifest, Vec<BuildDiagnostic>> {
    let raw = match toml::from_str::<RawManifest>(source) {
        Ok(raw) => raw,
        Err(err) => return Err(vec![toml_error(err)]),
    };

    lower_manifest(raw, manifest_dir)
}

/// Locate the `submilli.toml` governing `start`, walking up parent
/// directories the way cargo locates `Cargo.toml`.
pub fn find_manifest_upwards(start: &Path) -> Option<PathBuf> {
    let mut dir = start;
    loop {
        let candidate = dir.join("submilli.toml");
        if candidate.is_file() {
            return Some(candidate);
        }
        dir = dir.parent()?;
    }
}

pub fn load_manifest(manifest_path: &Path) -> Result<ProjectManifest, Vec<BuildDiagnostic>> {
    let source = fs::read_to_string(manifest_path).map_err(|err| {
        vec![error(
            None,
            format!(
                "read {} before parsing submilli.toml: {err}",
                manifest_path.display()
            ),
        )]
    })?;
    let manifest_dir = manifest_path.parent().unwrap_or_else(|| Path::new("."));
    parse_manifest(&source, manifest_dir)
}

fn lower_manifest(
    raw: RawManifest,
    manifest_dir: &Path,
) -> Result<ProjectManifest, Vec<BuildDiagnostic>> {
    let Some(raw_packages) = raw.package else {
        return Err(vec![error(
            None,
            "add a [[package]] table to submilli.toml",
        )]);
    };
    if raw_packages.is_empty() {
        return Err(vec![error(
            None,
            "add a [[package]] table to submilli.toml",
        )]);
    }

    let mut diagnostics = Vec::new();
    let sibling_versions = collect_sibling_versions(&raw_packages, &mut diagnostics);
    let external_dependencies = lower_dependencies(raw.dependencies, &mut diagnostics);

    let is_monorepo = raw_packages.len() >= 2;
    let mut packages = Vec::with_capacity(raw_packages.len());
    for raw_package in raw_packages {
        let package_span = span(raw_package.span());
        let raw_package = raw_package.into_inner();
        let name_span = span(raw_package.name.span());
        let name = raw_package.name.get_ref().clone();

        if let Err(message) = submilli_blueprint::validate_scoped_name(&name) {
            diagnostics.push(error(name_span, message));
            continue;
        }

        let package_path = match package_path(&raw_package, &name, is_monorepo, package_span) {
            Ok(package_path) => package_path,
            Err(diagnostic) => {
                diagnostics.push(diagnostic);
                continue;
            }
        };
        let package_path = match normalize_relative_package_path(&package_path) {
            Ok(package_path) => package_path,
            Err(message) => {
                diagnostics.push(error(name_span.or(package_span), message));
                continue;
            }
        };

        let entrypoint = match resolve_entrypoint(manifest_dir, &package_path) {
            Ok(entrypoint) => entrypoint,
            Err(message) => {
                diagnostics.push(error(name_span.or(package_span), message));
                continue;
            }
        };

        let version = raw_package.version.into_inner();
        let description = match raw_package.description {
            Some(description) if !description.get_ref().trim().is_empty() => {
                description.into_inner()
            }
            Some(description) => {
                diagnostics.push(error(
                    span(description.span()),
                    "replace empty package description with a one-line summary",
                ));
                continue;
            }
            None => {
                diagnostics.push(error(
                    package_span,
                    format!("add description = \"...\" for package {name:?}"),
                ));
                continue;
            }
        };
        let keywords = raw_package
            .keywords
            .into_iter()
            .filter_map(|keyword| {
                if keyword.get_ref().trim().is_empty() {
                    diagnostics.push(error(
                        span(keyword.span()),
                        "remove empty package keyword or replace it with a search term",
                    ));
                    None
                } else {
                    Some(keyword.into_inner())
                }
            })
            .collect();
        let dependencies = resolve_dependencies(
            &raw_package.dependencies,
            &sibling_versions,
            &external_dependencies,
            &mut diagnostics,
        );

        packages.push(PackageManifest {
            name: PackageName::new(name),
            version: PackageVersion::new(version),
            description,
            keywords,
            path: PackagePath::new(package_path),
            entrypoint: PackageEntrypoint::new(entrypoint),
            dependencies,
        });
    }

    if diagnostics.is_empty() {
        Ok(ProjectManifest {
            dependencies: external_dependencies,
            packages,
        })
    } else {
        Err(diagnostics)
    }
}

fn collect_sibling_versions(
    raw_packages: &[Spanned<RawPackage>],
    diagnostics: &mut Vec<BuildDiagnostic>,
) -> BTreeMap<PackageName, PackageVersion> {
    let mut versions = BTreeMap::new();
    let mut seen = BTreeSet::new();
    for raw_package in raw_packages {
        let package = raw_package.get_ref();
        let name = package.name.get_ref();
        let package_name = PackageName::new(name.clone());
        if !seen.insert(package_name.clone()) {
            diagnostics.push(error(
                span(package.name.span()),
                format!("rename duplicate package {name:?}; package names must be unique"),
            ));
            continue;
        }
        versions.insert(
            package_name,
            PackageVersion::new(package.version.get_ref().clone()),
        );
    }
    versions
}

fn package_path(
    raw_package: &RawPackage,
    name: &str,
    is_monorepo: bool,
    package_span: Option<SourceSpan>,
) -> Result<String, BuildDiagnostic> {
    match &raw_package.path {
        Some(path) => Ok(path.get_ref().clone()),
        None if is_monorepo => Err(error(
            package_span,
            format!("add path = \"...\" for package {name:?}; path is required in a monorepo"),
        )),
        None => Ok(".".to_string()),
    }
}

fn normalize_relative_package_path(package_path: &str) -> Result<PathBuf, String> {
    let path = Path::new(package_path);
    if path.is_absolute() {
        return Err(format!(
            "replace path {package_path:?} with a path relative to submilli.toml"
        ));
    }

    let mut normalized = PathBuf::new();
    for component in path.components() {
        match component {
            Component::CurDir => {}
            Component::Normal(segment) => normalized.push(segment),
            Component::ParentDir => {
                if !normalized.pop() {
                    return Err(format!(
                        "replace path {package_path:?}; package paths may not escape the manifest directory"
                    ));
                }
            }
            Component::RootDir | Component::Prefix(_) => {
                return Err(format!(
                    "replace path {package_path:?} with a path relative to submilli.toml"
                ));
            }
        }
    }

    if normalized.as_os_str().is_empty() {
        Ok(PathBuf::from("."))
    } else {
        Ok(normalized)
    }
}

fn resolve_entrypoint(manifest_dir: &Path, package_path: &Path) -> Result<PathBuf, String> {
    let canonical = package_path
        .join("src")
        .join(format!("lib.{CANONICAL_SOURCE_EXTENSION}"));
    let legacy = package_path
        .join("src")
        .join(format!("lib.{LEGACY_SOURCE_EXTENSION}"));
    let canonical_exists = manifest_dir.join(&canonical).is_file();
    let legacy_exists = manifest_dir.join(&legacy).is_file();
    match (canonical_exists, legacy_exists) {
        (true, false) => Ok(canonical),
        (false, true) => Ok(legacy),
        (true, true) => Err(format!(
            "delete or rename one package entrypoint; both {} and {} exist",
            canonical.display(),
            legacy.display()
        )),
        (false, false) => Err(format!(
            "create {}; package entrypoints are conventional",
            canonical.display()
        )),
    }
}

/// Lower and validate the top-level `[dependencies]` table into a
/// name → source map. GitHub entries must name a GitHub host and pin a full
/// 40-hex commit SHA (a GitHub pin is the unit of reproducibility).
fn lower_dependencies(
    raw_dependencies: BTreeMap<String, Spanned<RawDependency>>,
    diagnostics: &mut Vec<BuildDiagnostic>,
) -> BTreeMap<PackageName, DependencySource> {
    let mut resolved = BTreeMap::new();
    for (name, raw) in raw_dependencies {
        let dep_span = span(raw.span());
        let source = match raw.into_inner() {
            RawDependency::Version(version) => {
                DependencySource::Version(PackageVersion::new(version))
            }
            RawDependency::Github { github, rev } => {
                if let Err(message) = validate_github_dependency(&github, &rev) {
                    diagnostics.push(error(dep_span, message));
                    continue;
                }
                DependencySource::Github {
                    url: github,
                    sha: rev,
                }
            }
        };
        resolved.insert(PackageName::new(name), source);
    }
    resolved
}

fn validate_github_dependency(github: &str, rev: &str) -> Result<(), String> {
    if !is_commit_sha(rev) {
        return Err(format!(
            "pin rev {rev:?} to a full 40-character commit SHA; branches and tags are not reproducible"
        ));
    }
    if !is_github_url(github) {
        return Err(format!(
            "point github {github:?} at a github.com repository (e.g. \"github.com/org/repo\")"
        ));
    }
    Ok(())
}

fn is_commit_sha(rev: &str) -> bool {
    rev.len() == 40 && rev.chars().all(|c| c.is_ascii_hexdigit())
}

fn is_github_url(url: &str) -> bool {
    let host = url
        .trim_start_matches("https://")
        .trim_start_matches("http://");
    host.starts_with("github.com/")
}

fn resolve_dependencies(
    raw_dependencies: &[Spanned<String>],
    sibling_versions: &BTreeMap<PackageName, PackageVersion>,
    external_dependencies: &BTreeMap<PackageName, DependencySource>,
    diagnostics: &mut Vec<BuildDiagnostic>,
) -> Vec<ResolvedDependency> {
    let mut resolved = Vec::with_capacity(raw_dependencies.len());
    for raw_dependency in raw_dependencies {
        let name = raw_dependency.get_ref();
        let package_name = PackageName::new(name.clone());
        if let Some(version) = sibling_versions.get(&package_name) {
            resolved.push(ResolvedDependency {
                name: package_name,
                version: Some(version.clone()),
                kind: DependencyKind::Sibling,
            });
        } else if let Some(source) = external_dependencies.get(&package_name) {
            let (version, kind) = match source {
                DependencySource::Version(version) => {
                    (Some(version.clone()), DependencyKind::External)
                }
                DependencySource::Github { .. } => (None, DependencyKind::Github),
            };
            resolved.push(ResolvedDependency {
                name: package_name,
                version,
                kind,
            });
        } else {
            diagnostics.push(error(
                span(raw_dependency.span()),
                format!(
                    "declare dependency {name:?} as a sibling [[package]] or in top-level [dependencies]"
                ),
            ));
        }
    }
    resolved
}

fn toml_error(err: toml::de::Error) -> BuildDiagnostic {
    error(err.span().map(span_from_range), err.message().to_string())
}

fn error(span: Option<SourceSpan>, message: impl Into<String>) -> BuildDiagnostic {
    BuildDiagnostic {
        severity: BuildSeverity::Error,
        span,
        message: message.into(),
        help: Vec::new(),
    }
}

fn span(range: Range<usize>) -> Option<SourceSpan> {
    Some(span_from_range(range))
}

fn span_from_range(range: Range<usize>) -> SourceSpan {
    SourceSpan {
        start: range.start,
        end: range.end,
    }
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::path::Path;

    use tempfile::TempDir;

    use super::*;

    fn project() -> TempDir {
        tempfile::tempdir().expect("tempdir")
    }

    fn write_entrypoint(package_path: &Path) {
        write_entrypoint_with_extension(package_path, LEGACY_SOURCE_EXTENSION);
    }

    fn write_ts_entrypoint(package_path: &Path) {
        write_entrypoint_with_extension(package_path, CANONICAL_SOURCE_EXTENSION);
    }

    fn write_entrypoint_with_extension(package_path: &Path, extension: &str) {
        let src = package_path.join("src");
        fs::create_dir_all(&src).expect("create src dir");
        fs::write(
            src.join(format!("lib.{extension}")),
            "export function api(): number { return 1; }",
        )
        .expect("write lib");
    }

    fn parse(source: &str, dir: &Path) -> Result<ProjectManifest, Vec<BuildDiagnostic>> {
        parse_manifest(source, dir)
    }

    fn messages(diags: &[BuildDiagnostic]) -> Vec<&str> {
        diags.iter().map(|d| d.message.as_str()).collect()
    }

    #[test]
    fn single_package_defaults_path_to_current_dir() {
        let tmp = project();
        write_entrypoint(tmp.path());

        let manifest = parse(
            r#"
[[package]]
name = "@acme/stripe"
version = "0.1.0"
description = "Stripe helpers."
keywords = ["payments", "stripe"]
"#,
            tmp.path(),
        )
        .expect("manifest parses");

        assert_eq!(manifest.packages.len(), 1);
        let package = &manifest.packages[0];
        assert_eq!(package.name.as_str(), "@acme/stripe");
        assert_eq!(package.version.as_str(), "0.1.0");
        assert_eq!(package.description, "Stripe helpers.");
        assert_eq!(package.keywords, vec!["payments", "stripe"]);
        assert_eq!(package.path.as_path(), Path::new("."));
        assert_eq!(package.entrypoint.as_path(), Path::new("./src/lib.subm"));
        assert!(package.dependencies.is_empty());
    }

    #[test]
    fn package_description_is_required() {
        let tmp = project();
        write_entrypoint(tmp.path());

        let diags = parse(
            r#"
[[package]]
name = "@acme/stripe"
version = "0.1.0"
"#,
            tmp.path(),
        )
        .expect_err("diagnostic");

        assert_eq!(
            messages(&diags),
            vec!["add description = \"...\" for package \"@acme/stripe\""]
        );
    }

    #[test]
    fn package_entrypoint_prefers_canonical_ts() {
        let tmp = project();
        write_ts_entrypoint(tmp.path());

        let manifest = parse(
            r#"
[[package]]
name = "@acme/stripe"
version = "0.1.0"
description = "Stripe helpers."
"#,
            tmp.path(),
        )
        .expect("manifest parses");

        assert_eq!(
            manifest.packages[0].entrypoint.as_path(),
            Path::new("./src/lib.ts")
        );
    }

    #[test]
    fn monorepo_resolves_sibling_before_top_level_dependency() {
        let tmp = project();
        write_entrypoint(&tmp.path().join("packages/json"));
        write_entrypoint(&tmp.path().join("packages/http"));

        let manifest = parse(
            r#"
[dependencies]
"@submilli/json" = "9.9.9"

[[package]]
name = "@submilli/json"
version = "1.0.0"
description = "JSON helpers."
path = "packages/json"

[[package]]
name = "@submilli/http"
version = "1.0.0"
description = "HTTP helpers."
path = "packages/http"
dependencies = ["@submilli/json"]
"#,
            tmp.path(),
        )
        .expect("manifest parses");

        let http = manifest
            .packages
            .iter()
            .find(|p| p.name.as_str() == "@submilli/http")
            .expect("http package");
        assert_eq!(
            http.dependencies,
            vec![ResolvedDependency {
                name: PackageName::new("@submilli/json"),
                version: Some(PackageVersion::new("1.0.0")),
                kind: DependencyKind::Sibling,
            }]
        );
    }

    #[test]
    fn resolves_external_dependency_from_top_level_dependencies() {
        let tmp = project();
        write_entrypoint(tmp.path());

        let manifest = parse(
            r#"
[dependencies]
"@submilli/http" = "1.0.0"

[[package]]
name = "@acme/stripe"
version = "0.1.0"
description = "Stripe helpers."
dependencies = ["@submilli/http"]
"#,
            tmp.path(),
        )
        .expect("manifest parses");

        assert_eq!(
            manifest.packages[0].dependencies,
            vec![ResolvedDependency {
                name: PackageName::new("@submilli/http"),
                version: Some(PackageVersion::new("1.0.0")),
                kind: DependencyKind::External,
            }]
        );
    }

    const SAMPLE_SHA: &str = "0123456789abcdef0123456789abcdef01234567";

    #[test]
    fn resolves_github_dependency_from_top_level_dependencies() {
        let tmp = project();
        write_entrypoint(tmp.path());

        let manifest = parse(
            &format!(
                r#"
[dependencies]
"@acme/slack" = {{ github = "github.com/acme/stripe", rev = "{SAMPLE_SHA}" }}

[[package]]
name = "@me/app"
version = "0.1.0"
description = "App."
dependencies = ["@acme/slack"]
"#
            ),
            tmp.path(),
        )
        .expect("manifest parses");

        assert_eq!(
            manifest.dependencies.get(&PackageName::new("@acme/slack")),
            Some(&DependencySource::Github {
                url: "github.com/acme/stripe".to_string(),
                sha: SAMPLE_SHA.to_string(),
            })
        );
        assert_eq!(
            manifest.packages[0].dependencies,
            vec![ResolvedDependency {
                name: PackageName::new("@acme/slack"),
                version: None,
                kind: DependencyKind::Github,
            }]
        );
    }

    #[test]
    fn github_dependency_rejects_non_sha_rev() {
        let tmp = project();
        write_entrypoint(tmp.path());

        let diags = parse(
            r#"
[dependencies]
"@acme/slack" = { github = "github.com/acme/stripe", rev = "main" }

[[package]]
name = "@me/app"
version = "0.1.0"
description = "App."
dependencies = ["@acme/slack"]
"#,
            tmp.path(),
        )
        .expect_err("diagnostic");

        assert!(
            messages(&diags)
                .iter()
                .any(|m| m.contains("full 40-character commit SHA")),
            "unexpected diagnostics: {:?}",
            messages(&diags)
        );
    }

    #[test]
    fn github_dependency_rejects_non_github_host() {
        let tmp = project();
        write_entrypoint(tmp.path());

        let diags = parse(
            &format!(
                r#"
[dependencies]
"@acme/slack" = {{ github = "gitlab.com/acme/stripe", rev = "{SAMPLE_SHA}" }}

[[package]]
name = "@me/app"
version = "0.1.0"
description = "App."
dependencies = ["@acme/slack"]
"#
            ),
            tmp.path(),
        )
        .expect_err("diagnostic");

        assert!(
            messages(&diags).iter().any(|m| m.contains("github.com")),
            "unexpected diagnostics: {:?}",
            messages(&diags)
        );
    }

    #[test]
    fn missing_package_table_is_an_error() {
        let diags = parse("[dependencies]\n", Path::new(".")).expect_err("diagnostic");
        assert_eq!(
            messages(&diags),
            vec!["add a [[package]] table to submilli.toml"]
        );
    }

    #[test]
    fn monorepo_requires_path() {
        let tmp = project();
        write_entrypoint(tmp.path());
        write_entrypoint(&tmp.path().join("packages/http"));

        let diags = parse(
            r#"
[[package]]
name = "@submilli/json"
version = "1.0.0"
description = "JSON helpers."

[[package]]
name = "@submilli/http"
version = "1.0.0"
description = "HTTP helpers."
path = "packages/http"
"#,
            tmp.path(),
        )
        .expect_err("diagnostic");

        assert!(messages(&diags)[0].contains("path is required in a monorepo"));
    }

    #[test]
    fn undeclared_dependency_is_an_error() {
        let tmp = project();
        write_entrypoint(tmp.path());

        let diags = parse(
            r#"
[[package]]
name = "@acme/stripe"
version = "0.1.0"
description = "Stripe helpers."
dependencies = ["@submilli/json"]
"#,
            tmp.path(),
        )
        .expect_err("diagnostic");

        assert_eq!(
            messages(&diags),
            vec![
                "declare dependency \"@submilli/json\" as a sibling [[package]] or in top-level [dependencies]"
            ]
        );
    }

    #[test]
    fn missing_entrypoint_is_an_error() {
        let tmp = project();

        let diags = parse(
            r#"
[[package]]
name = "@acme/stripe"
version = "0.1.0"
description = "Stripe helpers."
"#,
            tmp.path(),
        )
        .expect_err("diagnostic");

        assert_eq!(
            messages(&diags),
            vec!["create ./src/lib.ts; package entrypoints are conventional"]
        );
    }

    #[test]
    fn ambiguous_entrypoint_is_an_error() {
        let tmp = project();
        write_entrypoint(tmp.path());
        write_ts_entrypoint(tmp.path());

        let diags = parse(
            r#"
[[package]]
name = "@acme/stripe"
version = "0.1.0"
description = "Stripe helpers."
"#,
            tmp.path(),
        )
        .expect_err("diagnostic");

        let message = messages(&diags)[0];
        assert!(message.contains("delete or rename one package entrypoint"));
        assert!(message.contains("./src/lib.ts"));
        assert!(message.contains("./src/lib.subm"));
    }

    #[test]
    fn duplicate_package_names_are_an_error() {
        let tmp = project();
        write_entrypoint(&tmp.path().join("a"));
        write_entrypoint(&tmp.path().join("b"));

        let diags = parse(
            r#"
[[package]]
name = "@submilli/json"
version = "1.0.0"
description = "JSON helpers."
path = "a"

[[package]]
name = "@submilli/json"
version = "1.0.1"
description = "JSON helpers."
path = "b"
"#,
            tmp.path(),
        )
        .expect_err("diagnostic");

        assert_eq!(
            messages(&diags),
            vec!["rename duplicate package \"@submilli/json\"; package names must be unique"]
        );
    }

    #[test]
    fn path_may_not_escape_manifest_directory() {
        let tmp = project();

        let diags = parse(
            r#"
[[package]]
name = "@acme/stripe"
version = "0.1.0"
description = "Stripe helpers."
path = "../stripe"
"#,
            tmp.path(),
        )
        .expect_err("diagnostic");

        assert_eq!(
            messages(&diags),
            vec!["replace path \"../stripe\"; package paths may not escape the manifest directory"]
        );
    }

    #[test]
    fn toml_syntax_errors_return_spanned_diagnostics() {
        let diags = parse("[[package]\n", Path::new(".")).expect_err("diagnostic");

        assert_eq!(diags.len(), 1);
        assert_eq!(diags[0].severity, BuildSeverity::Error);
        assert!(diags[0].span.is_some());
    }
}
