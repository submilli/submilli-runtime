//! Project scaffolding behind `submilli build init` / `submilli build new`:
//! creates `submilli.toml`, package folders, and entrypoint stubs.

use std::fmt;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use interpreter::PackageDeclaration;
use interpreter::packages::{render_lib_submilli_d_ts, render_packages_d_ts, render_stdlib_d_ts};

use crate::package_store::split_scoped_name;
use crate::{
    BuildDiagnostic, CANONICAL_SOURCE_EXTENSION, ProjectManifest, RawManifest,
    normalize_relative_package_path, parse_manifest,
};

pub const DEFAULT_PACKAGE_VERSION: &str = "0.1.0";

#[derive(Clone, Debug)]
pub struct ScaffoldedPackage {
    pub manifest_path: PathBuf,
    pub package_dir: PathBuf,
    pub entrypoint: PathBuf,
    pub docs_readme: PathBuf,
    pub readme: PathBuf,
    pub test_file: PathBuf,
}

/// True when `name` is a store-installable scoped name (`@scope/name`) that
/// can also be embedded verbatim in a generated TOML basic string.
pub fn is_valid_package_name(name: &str) -> bool {
    split_scoped_name(name).is_some()
        && !name
            .chars()
            .any(|c| c == '"' || c == '\\' || c.is_control())
}

/// Create `submilli.toml` in `dir` with a first package, scaffolding the
/// package's folders and a `src/lib.ts` stub.
pub fn init_project(
    dir: &Path,
    name: &str,
    package_path: &Path,
) -> Result<ScaffoldedPackage, ScaffoldError> {
    let manifest_path = dir.join("submilli.toml");
    if manifest_path.exists() {
        return Err(ScaffoldError::ManifestExists {
            path: manifest_path,
        });
    }
    let package_path = validated_package_path(name, package_path)?;
    let scaffolded = create_package_skeleton(dir, &manifest_path, &package_path, name)?;
    let manifest_text = package_block(name, &package_path);
    validate_manifest(&manifest_path, dir, &manifest_text)?;
    write_text(&manifest_path, &manifest_text)?;
    let manifest =
        parse_manifest(&manifest_text, dir).map_err(|diagnostics| ScaffoldError::Parse {
            path: manifest_path.clone(),
            text: manifest_text.clone(),
            diagnostics,
        })?;
    scaffold_editor_files(dir, &manifest, true)?;
    Ok(scaffolded)
}

/// Add a `[[package]]` entry to an existing `submilli.toml`, scaffolding the
/// package's folders and a `src/lib.ts` stub. When the manifest holds a
/// single package without an explicit `path` (legal only outside a monorepo),
/// `path = "."` is made explicit so the grown manifest stays valid.
pub fn add_package(
    manifest_path: &Path,
    name: &str,
    package_path: &Path,
) -> Result<ScaffoldedPackage, ScaffoldError> {
    let manifest_dir = manifest_dir(manifest_path);
    let text = read_manifest_text(manifest_path)?;
    let manifest =
        parse_manifest(&text, manifest_dir).map_err(|diagnostics| ScaffoldError::Parse {
            path: manifest_path.to_path_buf(),
            text: text.clone(),
            diagnostics,
        })?;

    let package_path = validated_package_path(name, package_path)?;
    if manifest.packages.iter().any(|p| p.name.as_str() == name) {
        return Err(ScaffoldError::DuplicatePackageName {
            name: name.to_string(),
        });
    }
    if let Some(existing) = manifest
        .packages
        .iter()
        .find(|p| p.path.as_path() == package_path)
    {
        return Err(ScaffoldError::DuplicatePackagePath {
            path: package_path,
            existing: existing.name.as_str().to_string(),
        });
    }

    let mut updated = ensure_explicit_paths(&text);
    if !updated.ends_with('\n') && !updated.is_empty() {
        updated.push('\n');
    }
    updated.push('\n');
    updated.push_str(&package_block(name, &package_path));

    let scaffolded = create_package_skeleton(manifest_dir, manifest_path, &package_path, name)?;
    validate_manifest(manifest_path, manifest_dir, &updated)?;
    write_text(manifest_path, &updated)?;
    let manifest =
        parse_manifest(&updated, manifest_dir).map_err(|diagnostics| ScaffoldError::Parse {
            path: manifest_path.to_path_buf(),
            text: updated.clone(),
            diagnostics,
        })?;
    scaffold_editor_files(manifest_dir, &manifest, false)?;
    Ok(scaffolded)
}

#[derive(Debug)]
pub enum ScaffoldError {
    ManifestExists {
        path: PathBuf,
    },
    ManifestMissing {
        path: PathBuf,
    },
    InvalidPackageName {
        name: String,
    },
    InvalidPackagePath {
        message: String,
    },
    DuplicatePackageName {
        name: String,
    },
    DuplicatePackagePath {
        path: PathBuf,
        existing: String,
    },
    Io {
        path: PathBuf,
        source: io::Error,
    },
    Parse {
        path: PathBuf,
        text: String,
        diagnostics: Vec<BuildDiagnostic>,
    },
}

impl fmt::Display for ScaffoldError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ScaffoldError::ManifestExists { path } => write!(
                f,
                "{} already exists; use `submilli build new <@scope/name> <path>` to add a package",
                path.display()
            ),
            ScaffoldError::ManifestMissing { path } => write!(
                f,
                "no submilli.toml at {}; run `submilli build init` first",
                path.display()
            ),
            ScaffoldError::InvalidPackageName { name } => {
                write!(f, "invalid package name `{name}`; expected `@scope/name`")
            }
            ScaffoldError::InvalidPackagePath { message } => f.write_str(message),
            ScaffoldError::DuplicatePackageName { name } => write!(
                f,
                "package `{name}` is already declared in submilli.toml; pick a different name"
            ),
            ScaffoldError::DuplicatePackagePath { path, existing } => write!(
                f,
                "path {} is already used by package `{existing}`; pick a different path",
                path.display()
            ),
            ScaffoldError::Io { path, source } => {
                write!(f, "failed to read or write {}: {source}", path.display())
            }
            ScaffoldError::Parse { path, .. } => write!(
                f,
                "{} has errors; fix them before scaffolding",
                path.display()
            ),
        }
    }
}

impl std::error::Error for ScaffoldError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            ScaffoldError::Io { source, .. } => Some(source),
            _ => None,
        }
    }
}

fn manifest_dir(manifest_path: &Path) -> &Path {
    manifest_path
        .parent()
        .filter(|dir| !dir.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."))
}

fn read_manifest_text(manifest_path: &Path) -> Result<String, ScaffoldError> {
    match fs::read_to_string(manifest_path) {
        Ok(text) => Ok(text),
        Err(source) if source.kind() == io::ErrorKind::NotFound => {
            Err(ScaffoldError::ManifestMissing {
                path: manifest_path.to_path_buf(),
            })
        }
        Err(source) => Err(ScaffoldError::Io {
            path: manifest_path.to_path_buf(),
            source,
        }),
    }
}

fn validated_package_path(name: &str, package_path: &Path) -> Result<PathBuf, ScaffoldError> {
    if !is_valid_package_name(name) {
        return Err(ScaffoldError::InvalidPackageName {
            name: name.to_string(),
        });
    }
    let raw = package_path
        .to_str()
        .ok_or_else(|| ScaffoldError::InvalidPackagePath {
            message: format!(
                "replace path {} with a UTF-8 path relative to submilli.toml",
                package_path.display()
            ),
        })?;
    normalize_relative_package_path(raw)
        .map_err(|message| ScaffoldError::InvalidPackagePath { message })
}

fn create_package_skeleton(
    manifest_dir: &Path,
    manifest_path: &Path,
    package_path: &Path,
    name: &str,
) -> Result<ScaffoldedPackage, ScaffoldError> {
    let package_dir = if package_path == Path::new(".") {
        manifest_dir.to_path_buf()
    } else {
        manifest_dir.join(package_path)
    };
    let src_dir = package_dir.join("src");
    fs::create_dir_all(&src_dir).map_err(|source| ScaffoldError::Io {
        path: src_dir.clone(),
        source,
    })?;
    let entrypoint = src_dir.join(format!("lib.{CANONICAL_SOURCE_EXTENSION}"));
    if !entrypoint.exists() {
        write_text(&entrypoint, &entrypoint_stub(name))?;
    }

    let docs_dir = package_dir.join("docs");
    fs::create_dir_all(&docs_dir).map_err(|source| ScaffoldError::Io {
        path: docs_dir.clone(),
        source,
    })?;
    let docs_readme = docs_dir.join("readme.md");
    if !docs_readme.exists() {
        write_text(&docs_readme, &docs_readme_stub(name))?;
    }

    let readme = package_dir.join("README.md");
    if !readme.exists() {
        write_text(&readme, &readme_stub(name))?;
    }

    let tests_dir = package_dir.join("tests");
    fs::create_dir_all(&tests_dir).map_err(|source| ScaffoldError::Io {
        path: tests_dir.clone(),
        source,
    })?;
    let test_file = tests_dir.join(format!("lib.test.{CANONICAL_SOURCE_EXTENSION}"));
    if !test_file.exists() {
        write_text(&test_file, &test_stub(name))?;
    }

    Ok(ScaffoldedPackage {
        manifest_path: manifest_path.to_path_buf(),
        package_dir,
        entrypoint,
        docs_readme,
        readme,
        test_file,
    })
}

fn entrypoint_stub(name: &str) -> String {
    format!("export function hello(): string {{\n    return \"hello from {name}\";\n}}\n")
}

fn docs_readme_stub(name: &str) -> String {
    format!(
        "# {name}\n\nDescribe what this package does, when to use it, and any important behavior an agent should know before importing it.\n"
    )
}

/// The readme for the person who installs and grants the package, next to
/// `docs/readme.md`, which the model reads.
fn readme_stub(name: &str) -> String {
    format!(
        "# {name}\n\nDescribe what this package is for, the credential to bind and how the service issues it, and which operations to grant with their filter fields.\n"
    )
}

fn scaffold_editor_files(
    manifest_dir: &Path,
    manifest: &ProjectManifest,
    scaffold_user_files: bool,
) -> Result<(), ScaffoldError> {
    refresh_editor_files(manifest_dir, manifest)?;

    if scaffold_user_files {
        write_once(
            &manifest_dir.join("tsconfig.json"),
            "{\n  \"extends\": \"./.submilli/tsconfig.submilli.json\"\n}\n",
        )?;
        let vscode_dir = manifest_dir.join(".vscode");
        fs::create_dir_all(&vscode_dir).map_err(|source| ScaffoldError::Io {
            path: vscode_dir.clone(),
            source,
        })?;
        write_once(&vscode_dir.join("tasks.json"), &tasks_json_text())?;
        append_gitignore_entry(manifest_dir, ".submilli/")?;
    }

    Ok(())
}

pub fn refresh_editor_files(
    manifest_dir: &Path,
    manifest: &ProjectManifest,
) -> Result<(), ScaffoldError> {
    let submilli_dir = manifest_dir.join(".submilli");
    fs::create_dir_all(&submilli_dir).map_err(|source| ScaffoldError::Io {
        path: submilli_dir.clone(),
        source,
    })?;
    let generated_tsconfig = submilli_dir.join("tsconfig.submilli.json");
    write_text(&generated_tsconfig, &generated_tsconfig_text(manifest))?;

    let types_dir = ensure_types_dir(manifest_dir)?;
    write_text(
        &types_dir.join("lib.submilli.d.ts"),
        &format!("{}\n", render_lib_submilli_d_ts()),
    )?;
    write_text(
        &types_dir.join("stdlib.d.ts"),
        &format!("{}\n", render_stdlib_d_ts()),
    )?;

    Ok(())
}

/// Write `.submilli/types/packages.d.ts` from the given dependency
/// declarations. Sibling packages need none: the generated tsconfig maps them
/// to their sources. `project_packages` are store copies of those packages,
/// which the dependencies may borrow types from.
pub fn refresh_dependency_types(
    manifest_dir: &Path,
    dependencies: &[&PackageDeclaration],
    project_packages: &[&PackageDeclaration],
) -> Result<(), ScaffoldError> {
    let types_dir = ensure_types_dir(manifest_dir)?;
    write_text(
        &types_dir.join("packages.d.ts"),
        &format!("{}\n", render_packages_d_ts(dependencies, project_packages)),
    )
}

fn ensure_types_dir(manifest_dir: &Path) -> Result<PathBuf, ScaffoldError> {
    let types_dir = manifest_dir.join(".submilli").join("types");
    fs::create_dir_all(&types_dir).map_err(|source| ScaffoldError::Io {
        path: types_dir.clone(),
        source,
    })?;
    Ok(types_dir)
}

fn generated_tsconfig_text(manifest: &ProjectManifest) -> String {
    let mut paths = serde_json::Map::new();
    let mut include = Vec::new();
    for package in &manifest.packages {
        paths.insert(
            package.name.as_str().to_string(),
            serde_json::json!([path_relative_to_submilli(package.entrypoint.as_path())]),
        );
        include.push(path_relative_to_submilli(
            &package.path.as_path().join("src").join("**").join("*"),
        ));
        include.push(path_relative_to_submilli(
            &package.path.as_path().join("tests").join("**").join("*"),
        ));
    }
    include.push("./types/**/*.d.ts".to_string());

    let config = serde_json::json!({
        "compilerOptions": {
            "noEmit": true,
            "strict": true,
            // submilli reads optional fields as `T | null`, TS as `T | undefined` —
            // irreconcilable without noise. tsserver is the navigation/completion
            // layer; `submilli build check` is the checker, via the tasks.json
            // problem matcher.
            "strictNullChecks": false,
            // `strict` would type a `catch (e)` binding as `unknown`; submilli
            // binds it as `Error`, so `e.message` must not be flagged.
            "useUnknownInCatchVariables": false,
            "noLib": true,
            "module": "preserve",
            "target": "es2022",
            "moduleResolution": "bundler",
            "paths": paths,
        },
        "include": include,
    });
    // This closed JSON Value tree has string keys and no custom serializers;
    // serializing it to an in-memory string cannot return a data or I/O error.
    format!(
        "{}\n",
        serde_json::to_string_pretty(&config).expect("generated tsconfig is serializable")
    )
}

fn tasks_json_text() -> String {
    let tasks = serde_json::json!({
        "version": "2.0.0",
        "tasks": [
            {
                "label": "submilli build check",
                "type": "shell",
                "command": "submilli build check",
                "group": {
                    "kind": "build",
                    "isDefault": true,
                },
                "problemMatcher": {
                    "owner": "submilli",
                    "fileLocation": ["relative", "${workspaceFolder}"],
                    "pattern": [
                        {
                            "regexp": "^\\s*(error|warning):\\s+(.*)$",
                            "severity": 1,
                            "message": 2,
                        },
                        {
                            "regexp": "^\\s*-->\\s+(.+):(\\d+):(\\d+)$",
                            "file": 1,
                            "line": 2,
                            "column": 3,
                        },
                    ],
                },
            },
        ],
    });
    // This fixed JSON Value contains only strings, numbers, booleans and
    // containers; its in-memory serializer has no data or I/O failure path.
    format!(
        "{}\n",
        serde_json::to_string_pretty(&tasks).expect("generated tasks.json is serializable")
    )
}

fn path_relative_to_submilli(path: &Path) -> String {
    let mut normalized = PathBuf::from("..");
    for component in path.components() {
        match component {
            std::path::Component::CurDir => {}
            std::path::Component::Normal(segment) => normalized.push(segment),
            std::path::Component::ParentDir => normalized.push(".."),
            std::path::Component::RootDir | std::path::Component::Prefix(_) => {}
        }
    }
    normalized.display().to_string().replace('\\', "/")
}

fn write_once(path: &Path, text: &str) -> Result<(), ScaffoldError> {
    if path.exists() {
        return Ok(());
    }
    write_text(path, text)
}

fn append_gitignore_entry(manifest_dir: &Path, entry: &str) -> Result<(), ScaffoldError> {
    let path = manifest_dir.join(".gitignore");
    let existing = match fs::read_to_string(&path) {
        Ok(text) => text,
        Err(source) if source.kind() == io::ErrorKind::NotFound => String::new(),
        Err(source) => {
            return Err(ScaffoldError::Io {
                path: path.clone(),
                source,
            });
        }
    };
    if existing.lines().any(|line| line.trim() == entry) {
        return Ok(());
    }
    let mut updated = existing;
    if !updated.is_empty() && !updated.ends_with('\n') {
        updated.push('\n');
    }
    updated.push_str(entry);
    updated.push('\n');
    write_text(&path, &updated)
}

/// A runnable sample test exercising the entrypoint stub. `label` names the
/// reported segment; `assert` is a language built-in. Runs green under
/// `submilli build test` against a freshly scaffolded package.
fn test_stub(name: &str) -> String {
    format!(
        "import {{ label }} from \"submilli:test\";\n\
         import {{ hello }} from \"{name}\";\n\
         \n\
         function main(): void {{\n\
         \x20   label(\"hello greets\");\n\
         \x20   assert(hello() === \"hello from {name}\", \"greeting should match the stub\");\n\
         }}\n"
    )
}

fn package_block(name: &str, package_path: &Path) -> String {
    let path = package_path
        .to_str()
        .expect("validated package path is UTF-8")
        .replace('\\', "/");
    format!(
        "[[package]]\nname = \"{name}\"\nversion = \"{DEFAULT_PACKAGE_VERSION}\"\ndescription = \"Package {name}.\"\nkeywords = []\npath = \"{path}\"\n"
    )
}

// A single-package manifest may omit `path` (defaults to `.`); once a second
// package joins, every entry needs an explicit path. The spans from the raw
// parse locate the `version` line so `path = "."` lands inside the right table.
fn ensure_explicit_paths(text: &str) -> String {
    let Ok(raw) = toml::from_str::<RawManifest>(text) else {
        return text.to_string();
    };
    let Some(packages) = raw.package else {
        return text.to_string();
    };
    let mut insertions: Vec<usize> = packages
        .iter()
        .filter(|p| p.get_ref().path.is_none())
        .map(|p| line_end(text, p.get_ref().version.span().end))
        .collect();
    insertions.sort_unstable();
    let mut out = text.to_string();
    for pos in insertions.into_iter().rev() {
        out.insert_str(pos, "\npath = \".\"");
    }
    out
}

fn line_end(text: &str, from: usize) -> usize {
    text[from..].find('\n').map_or(text.len(), |i| from + i)
}

fn validate_manifest(
    manifest_path: &Path,
    manifest_dir: &Path,
    text: &str,
) -> Result<(), ScaffoldError> {
    parse_manifest(text, manifest_dir).map_err(|diagnostics| ScaffoldError::Parse {
        path: manifest_path.to_path_buf(),
        text: text.to_string(),
        diagnostics,
    })?;
    Ok(())
}

fn write_text(path: &Path, text: &str) -> Result<(), ScaffoldError> {
    fs::write(path, text).map_err(|source| ScaffoldError::Io {
        path: path.to_path_buf(),
        source,
    })
}
