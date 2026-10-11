//! Local prebuilt package artifact reader/writer.

use std::fmt;
use std::fs::{self, File};
use std::io::{self, Read};
use std::path::{Path, PathBuf};

use interpreter::{ModulePath, PackageDeclaration, TypeInfoTable};
use serde::{Deserialize, Serialize};

use crate::CapabilitySchema;

// v3: class nominal identity — vtable parent-link slot, per-class vtable-global
// exports/imports; older pkg.wasm artifacts are ABI-incompatible.
// v4: caller identity moved to the module's `name` section. A v3 artifact carries no module
// name and still imports the removed `caller_stack_push`/`pop`, so it would fail to link —
// and if it did link, every gated call from it would be refused for an unnameable principal.
// The version gate turns both into "rebuild this package" instead of an opaque runtime error.
// v5: generic classes. A class's `extends` clause carries type arguments
// (`ClassExtends { parent, args }`), so a v4 declaration's bare-string parent no
// longer deserializes; and class-method vtable slots now erase class types and
// type variables uniformly, so a v4 module's method signatures no longer match
// what a consumer reconstructs. Both the declaration and the wasm are
// incompatible, hence the gate rather than a migration.
// v6: every class-method parameter uses a nullable boxed slot to support
// contravariant overrides. Older method signatures cannot link to new consumers.
// v7: optional field names carry per-instance presence bits. Older artifacts
// conflate absent fields with explicit null and cannot share the new markers.
// v8: replaceable object-shape arrays and explicitly marked accessor payload names.
// v9: structural objects and interface declarations carry string index signatures.
// v10: `submilli:fs` `Info` drops `pathLimit`, so its intrinsic layout loses a field.
pub const ARTIFACT_SCHEMA_VERSION: u32 = 11;

const WASM_FILE: &str = "pkg.wasm";
const CAPABILITIES_FILE: &str = "capabilities.yaml";
const PACKAGE_DECLARATION_FILE: &str = "package-declaration.json";
const TYPE_INFO_FILE: &str = "type-info.json";
const METADATA_FILE: &str = "metadata.json";
const DOCS_README_FILE: &str = "docs/readme.md";
const SOURCES_FILE: &str = "sources.json";

// Keep one Wasm file compatible with the engine's default 256 MiB module cap,
// while bounding auxiliary files and the whole retained package set too.
const MAX_ARTIFACT_FILE_BYTES: usize = 256 << 20;
const MAX_ARTIFACT_LOAD_BYTES: usize = 512 << 20;
// Byte limits alone allow pathological numbers of nearly empty packages and
// their map/vector bookkeeping.
const MAX_ARTIFACT_PACKAGES: usize = 4_096;
const READ_BUFFER_BYTES: usize = 8 << 10;

#[derive(Clone, Copy)]
pub(crate) struct ArtifactReadLimits {
    pub(crate) max_file_bytes: usize,
    pub(crate) max_total_bytes: usize,
    pub(crate) max_packages: usize,
}

impl Default for ArtifactReadLimits {
    fn default() -> Self {
        Self {
            max_file_bytes: MAX_ARTIFACT_FILE_BYTES,
            max_total_bytes: MAX_ARTIFACT_LOAD_BYTES,
            max_packages: MAX_ARTIFACT_PACKAGES,
        }
    }
}

pub(crate) struct ArtifactReadBudget {
    limits: ArtifactReadLimits,
    remaining: usize,
    packages: usize,
}

impl ArtifactReadBudget {
    pub(crate) fn new(limits: ArtifactReadLimits) -> Self {
        Self {
            limits,
            remaining: limits.max_total_bytes,
            packages: 0,
        }
    }

    fn begin_package(&mut self, path: &Path) -> Result<(), ArtifactError> {
        self.packages =
            self.packages
                .checked_add(1)
                .ok_or_else(|| ArtifactError::PackageLimitExceeded {
                    path: path.to_path_buf(),
                    limit: self.limits.max_packages,
                })?;
        if self.packages > self.limits.max_packages {
            return Err(ArtifactError::PackageLimitExceeded {
                path: path.to_path_buf(),
                limit: self.limits.max_packages,
            });
        }
        Ok(())
    }

    fn ensure(&self, path: &Path, bytes: u64) -> Result<(), ArtifactError> {
        if bytes > self.limits.max_file_bytes as u64 {
            return Err(ArtifactError::FileTooLarge {
                path: path.to_path_buf(),
                bytes,
                limit: self.limits.max_file_bytes,
            });
        }
        if bytes > self.remaining as u64 {
            return Err(ArtifactError::LoadBudgetExceeded {
                path: path.to_path_buf(),
                bytes,
                remaining: self.remaining,
                limit: self.limits.max_total_bytes,
            });
        }
        Ok(())
    }

    fn commit(&mut self, path: &Path, bytes: usize) -> Result<(), ArtifactError> {
        self.remaining =
            self.remaining
                .checked_sub(bytes)
                .ok_or_else(|| ArtifactError::LoadBudgetExceeded {
                    path: path.to_path_buf(),
                    bytes: bytes as u64,
                    remaining: self.remaining,
                    limit: self.limits.max_total_bytes,
                })?;
        Ok(())
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ArtifactMetadata {
    pub schema_version: u32,
    pub package_name: String,
    pub package_version: String,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub keywords: Vec<String>,
    pub dependencies: Vec<ArtifactDependency>,
    /// Where this package came from, when it was installed from a source rather
    /// than built locally. `None` for `build publish-local` artifacts; `Some`
    /// records the GitHub repo + pinned commit a `submilli install` resolved to,
    /// so re-install/`--upgrade` and the server can compare pins. Optional and
    /// additive — older artifacts (no `source`) read back as `None`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source: Option<PackageSource>,
}

/// Provenance for a package installed from an external source.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PackageSource {
    Github(GithubSource),
}

/// A GitHub repo pinned to a concrete commit SHA.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct GithubSource {
    pub org: String,
    pub repo: String,
    pub sha: String,
    /// sha256 of the downloaded source tarball — the integrity anchor recorded
    /// in the lockfile. Additive and optional: artifacts installed before this
    /// field existed read back as `None`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_hash: Option<String>,
}

impl ArtifactMetadata {
    pub fn new(
        package_name: impl Into<String>,
        package_version: impl Into<String>,
        dependencies: Vec<ArtifactDependency>,
    ) -> Self {
        let mut metadata = Self {
            schema_version: ARTIFACT_SCHEMA_VERSION,
            package_name: package_name.into(),
            package_version: package_version.into(),
            description: String::new(),
            keywords: Vec::new(),
            dependencies,
            source: None,
        };
        metadata.sort_dependencies();
        metadata
    }

    pub fn with_description(
        package_name: impl Into<String>,
        package_version: impl Into<String>,
        description: impl Into<String>,
        keywords: Vec<String>,
        dependencies: Vec<ArtifactDependency>,
    ) -> Self {
        let mut metadata = Self {
            schema_version: ARTIFACT_SCHEMA_VERSION,
            package_name: package_name.into(),
            package_version: package_version.into(),
            description: description.into(),
            keywords,
            dependencies,
            source: None,
        };
        metadata.sort_dependencies();
        metadata
    }

    /// Attach source provenance (builder style) — used by `submilli install` to
    /// stamp the GitHub repo + pinned SHA onto the artifact before writing it.
    pub fn with_source(mut self, source: PackageSource) -> Self {
        self.source = Some(source);
        self
    }

    fn sort_dependencies(&mut self) {
        self.dependencies
            .sort_by(|a, b| a.name.cmp(&b.name).then_with(|| a.version.cmp(&b.version)));
        self.keywords.sort();
        self.keywords.dedup();
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ArtifactDependency {
    pub name: String,
    pub version: String,
}

impl ArtifactDependency {
    pub fn new(name: impl Into<String>, version: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            version: version.into(),
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Artifact {
    pub wasm: Vec<u8>,
    pub type_info: TypeInfoTable,
    pub capabilities: CapabilitySchema,
    pub package_declaration: PackageDeclaration,
    pub metadata: ArtifactMetadata,
    pub documentation: String,
    pub sources: Vec<ArtifactSource>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ArtifactSource {
    pub path: ModulePath,
    pub text: String,
}

#[derive(Debug)]
pub enum ArtifactError {
    Io {
        path: PathBuf,
        source: io::Error,
    },
    Json {
        path: PathBuf,
        source: serde_json::Error,
    },
    Yaml {
        path: PathBuf,
        source: serde_yml::Error,
    },
    UnsupportedSchema {
        path: PathBuf,
        found: u32,
    },
    FileTooLarge {
        path: PathBuf,
        bytes: u64,
        limit: usize,
    },
    LoadBudgetExceeded {
        path: PathBuf,
        bytes: u64,
        remaining: usize,
        limit: usize,
    },
    Allocation {
        path: PathBuf,
    },
    PackageLimitExceeded {
        path: PathBuf,
        limit: usize,
    },
}

impl fmt::Display for ArtifactError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ArtifactError::Io { path, source } => {
                write!(f, "failed to read or write {}: {source}", path.display())
            }
            ArtifactError::Json { path, source } => {
                write!(
                    f,
                    "failed to decode JSON artifact file {}: {source}",
                    path.display()
                )
            }
            ArtifactError::Yaml { path, source } => {
                write!(
                    f,
                    "failed to decode YAML artifact file {}: {source}",
                    path.display()
                )
            }
            ArtifactError::UnsupportedSchema { path, found } => write!(
                f,
                "unsupported artifact schema version {found} in {}; expected {ARTIFACT_SCHEMA_VERSION}",
                path.display()
            ),
            ArtifactError::FileTooLarge { path, bytes, limit } => write!(
                f,
                "artifact file {} is {bytes} bytes; limit is {limit} bytes",
                path.display()
            ),
            ArtifactError::LoadBudgetExceeded {
                path,
                bytes,
                remaining,
                limit,
            } => write!(
                f,
                "artifact file {} is {bytes} bytes with {remaining} bytes left in the {limit}-byte package-load budget",
                path.display()
            ),
            ArtifactError::Allocation { path } => {
                write!(f, "could not allocate artifact file {}", path.display())
            }
            ArtifactError::PackageLimitExceeded { path, limit } => write!(
                f,
                "artifact package count exceeds limit {limit} while loading {}",
                path.display()
            ),
        }
    }
}

impl std::error::Error for ArtifactError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            ArtifactError::Io { source, .. } => Some(source),
            ArtifactError::Json { source, .. } => Some(source),
            ArtifactError::Yaml { source, .. } => Some(source),
            ArtifactError::UnsupportedSchema { .. }
            | ArtifactError::FileTooLarge { .. }
            | ArtifactError::LoadBudgetExceeded { .. }
            | ArtifactError::Allocation { .. }
            | ArtifactError::PackageLimitExceeded { .. } => None,
        }
    }
}

pub fn write_package_artifact(
    dir: impl AsRef<Path>,
    wasm: &[u8],
    type_info: &TypeInfoTable,
    capabilities: &CapabilitySchema,
    package_declaration: &PackageDeclaration,
    metadata: &ArtifactMetadata,
) -> Result<(), ArtifactError> {
    write_package_artifact_with_docs(
        dir,
        wasm,
        type_info,
        capabilities,
        package_declaration,
        metadata,
        "",
    )
}

pub fn write_package_artifact_with_docs(
    dir: impl AsRef<Path>,
    wasm: &[u8],
    type_info: &TypeInfoTable,
    capabilities: &CapabilitySchema,
    package_declaration: &PackageDeclaration,
    metadata: &ArtifactMetadata,
    documentation: &str,
) -> Result<(), ArtifactError> {
    write_package_artifact_with_docs_and_sources(
        dir,
        wasm,
        type_info,
        capabilities,
        package_declaration,
        metadata,
        documentation,
        &[],
    )
}

#[allow(clippy::too_many_arguments)]
pub fn write_package_artifact_with_docs_and_sources(
    dir: impl AsRef<Path>,
    wasm: &[u8],
    type_info: &TypeInfoTable,
    capabilities: &CapabilitySchema,
    package_declaration: &PackageDeclaration,
    metadata: &ArtifactMetadata,
    documentation: &str,
    sources: &[ArtifactSource],
) -> Result<(), ArtifactError> {
    let dir = dir.as_ref();
    create_dir_all(dir)?;

    let mut metadata = metadata.clone();
    metadata.sort_dependencies();
    write_bytes(dir.join(WASM_FILE), wasm)?;
    write_json(dir.join(TYPE_INFO_FILE), type_info)?;
    write_yaml(dir.join(CAPABILITIES_FILE), capabilities)?;
    write_json(dir.join(PACKAGE_DECLARATION_FILE), package_declaration)?;
    write_json(dir.join(METADATA_FILE), &metadata)?;
    write_bytes(dir.join(DOCS_README_FILE), documentation.as_bytes())?;
    write_json(dir.join(SOURCES_FILE), sources)?;
    Ok(())
}

/// Write just the derived `capabilities.yaml` into `dir`, without the full
/// install artifact set. Used by `build check`/`build test` to surface a
/// package's required (and provided) capabilities next to its source.
pub fn write_capabilities_file(
    dir: impl AsRef<Path>,
    capabilities: &CapabilitySchema,
) -> Result<(), ArtifactError> {
    let dir = dir.as_ref();
    create_dir_all(dir)?;
    write_yaml(dir.join(CAPABILITIES_FILE), capabilities)
}

pub fn read_package_artifact(dir: impl AsRef<Path>) -> Result<Artifact, ArtifactError> {
    let mut budget = ArtifactReadBudget::new(ArtifactReadLimits::default());
    read_package_artifact_with_budget(dir, &mut budget)
}

/// What an installed package was built from: its metadata, documentation, and
/// source modules. Reads only those files, not the wasm, so a caller can compare an
/// installed package with its source tree cheaply.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct InstalledSources {
    pub metadata: ArtifactMetadata,
    pub documentation: String,
    pub sources: Vec<ArtifactSource>,
}

/// Read an installed package's [`InstalledSources`] from its directory.
pub fn read_installed_sources(dir: impl AsRef<Path>) -> Result<InstalledSources, ArtifactError> {
    let dir = dir.as_ref();
    let mut budget = ArtifactReadBudget::new(ArtifactReadLimits::default());
    budget.begin_package(dir)?;
    let metadata_path = dir.join(METADATA_FILE);
    let metadata: ArtifactMetadata = read_json(&metadata_path, &mut budget)?;
    if metadata.schema_version != ARTIFACT_SCHEMA_VERSION {
        return Err(ArtifactError::UnsupportedSchema {
            path: metadata_path,
            found: metadata.schema_version,
        });
    }
    let documentation = read_optional_text(dir.join(DOCS_README_FILE), &mut budget)?;
    let sources = read_optional_json(dir.join(SOURCES_FILE), &mut budget)?.unwrap_or_default();
    Ok(InstalledSources {
        metadata,
        documentation,
        sources,
    })
}

pub(crate) fn read_package_artifact_with_budget(
    dir: impl AsRef<Path>,
    budget: &mut ArtifactReadBudget,
) -> Result<Artifact, ArtifactError> {
    let dir = dir.as_ref();
    budget.begin_package(dir)?;
    let metadata_path = dir.join(METADATA_FILE);
    // The version gate comes first: a schema bump can change the shape of the
    // other files, and decoding them against the current types would surface a
    // serde error about some interior field instead of "rebuild this package".
    let metadata: ArtifactMetadata = read_json(&metadata_path, budget)?;
    if metadata.schema_version != ARTIFACT_SCHEMA_VERSION {
        return Err(ArtifactError::UnsupportedSchema {
            path: metadata_path,
            found: metadata.schema_version,
        });
    }
    let wasm = read_bytes(dir.join(WASM_FILE), budget)?;
    let type_info = read_json(dir.join(TYPE_INFO_FILE), budget)?;
    let capabilities = read_yaml(dir.join(CAPABILITIES_FILE), budget)?;
    let package_declaration = read_json(dir.join(PACKAGE_DECLARATION_FILE), budget)?;
    let documentation = read_optional_text(dir.join(DOCS_README_FILE), budget)?;
    let sources = read_optional_json(dir.join(SOURCES_FILE), budget)?.unwrap_or_default();

    Ok(Artifact {
        wasm,
        type_info,
        capabilities,
        package_declaration,
        metadata,
        documentation,
        sources,
    })
}

fn create_dir_all(path: &Path) -> Result<(), ArtifactError> {
    fs::create_dir_all(path).map_err(|source| ArtifactError::Io {
        path: path.to_path_buf(),
        source,
    })
}

fn read_bytes(path: PathBuf, budget: &mut ArtifactReadBudget) -> Result<Vec<u8>, ArtifactError> {
    let file = File::open(&path).map_err(|source| ArtifactError::Io {
        path: path.clone(),
        source,
    })?;
    read_open_file(file, path, budget)
}

fn read_optional_text(
    path: PathBuf,
    budget: &mut ArtifactReadBudget,
) -> Result<String, ArtifactError> {
    let Some(bytes) = read_optional_file(path.clone(), budget)? else {
        return Ok(String::new());
    };
    String::from_utf8(bytes).map_err(|source| ArtifactError::Io {
        path,
        source: io::Error::new(io::ErrorKind::InvalidData, source),
    })
}

fn read_optional_json<T>(
    path: PathBuf,
    budget: &mut ArtifactReadBudget,
) -> Result<Option<T>, ArtifactError>
where
    T: for<'de> Deserialize<'de>,
{
    let Some(bytes) = read_optional_file(path.clone(), budget)? else {
        return Ok(None);
    };
    serde_json::from_slice(&bytes)
        .map(Some)
        .map_err(|source| ArtifactError::Json { path, source })
}

fn read_optional_file(
    path: PathBuf,
    budget: &mut ArtifactReadBudget,
) -> Result<Option<Vec<u8>>, ArtifactError> {
    let file = match File::open(&path) {
        Ok(file) => file,
        Err(source) if source.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(source) => return Err(ArtifactError::Io { path, source }),
    };
    read_open_file(file, path, budget).map(Some)
}

fn read_open_file(
    mut file: File,
    path: PathBuf,
    budget: &mut ArtifactReadBudget,
) -> Result<Vec<u8>, ArtifactError> {
    let declared_size = file
        .metadata()
        .map_err(|source| ArtifactError::Io {
            path: path.clone(),
            source,
        })?
        .len();
    budget.ensure(&path, declared_size)?;
    let declared_size =
        usize::try_from(declared_size).map_err(|_| ArtifactError::FileTooLarge {
            path: path.clone(),
            bytes: declared_size,
            limit: budget.limits.max_file_bytes,
        })?;
    let mut bytes = Vec::new();
    bytes
        .try_reserve_exact(declared_size)
        .map_err(|_| ArtifactError::Allocation { path: path.clone() })?;
    budget.ensure(&path, bytes.capacity() as u64)?;
    let mut buffer = [0_u8; READ_BUFFER_BYTES];
    loop {
        let read = file.read(&mut buffer).map_err(|source| ArtifactError::Io {
            path: path.clone(),
            source,
        })?;
        if read == 0 {
            break;
        }
        let total = bytes
            .len()
            .checked_add(read)
            .ok_or_else(|| ArtifactError::FileTooLarge {
                path: path.clone(),
                bytes: u64::MAX,
                limit: budget.limits.max_file_bytes,
            })?;
        budget.ensure(&path, total as u64)?;
        reserve_for_growth(&mut bytes, total, &path, budget)?;
        bytes.extend_from_slice(&buffer[..read]);
    }
    // Vec capacity, rather than logical length, is the allocation retained by
    // Wasm/text artifacts. This also keeps a truncated file or an allocator's
    // rounded capacity from escaping the aggregate budget.
    budget.commit(&path, bytes.capacity())?;
    Ok(bytes)
}

fn reserve_for_growth(
    bytes: &mut Vec<u8>,
    required: usize,
    path: &Path,
    budget: &ArtifactReadBudget,
) -> Result<(), ArtifactError> {
    if required <= bytes.capacity() {
        return Ok(());
    }
    let ceiling = budget.limits.max_file_bytes.min(budget.remaining);
    let target = bytes
        .capacity()
        .max(READ_BUFFER_BYTES)
        .saturating_mul(2)
        .max(required)
        .min(ceiling);
    bytes
        .try_reserve_exact(target - bytes.len())
        .map_err(|_| ArtifactError::Allocation {
            path: path.to_path_buf(),
        })?;
    budget.ensure(path, bytes.capacity() as u64)
}

fn write_bytes(path: PathBuf, bytes: &[u8]) -> Result<(), ArtifactError> {
    if let Some(parent) = path.parent() {
        create_dir_all(parent)?;
    }
    fs::write(&path, bytes).map_err(|source| ArtifactError::Io { path, source })
}

fn read_json<T>(path: impl AsRef<Path>, budget: &mut ArtifactReadBudget) -> Result<T, ArtifactError>
where
    T: for<'de> Deserialize<'de>,
{
    let path = path.as_ref();
    let bytes = read_bytes(path.to_path_buf(), budget)?;
    serde_json::from_slice(&bytes).map_err(|source| ArtifactError::Json {
        path: path.to_path_buf(),
        source,
    })
}

fn write_json<T>(path: PathBuf, value: &T) -> Result<(), ArtifactError>
where
    T: Serialize + ?Sized,
{
    let bytes = serde_json::to_vec_pretty(value).map_err(|source| ArtifactError::Json {
        path: path.clone(),
        source,
    })?;
    fs::write(&path, bytes).map_err(|source| ArtifactError::Io { path, source })
}

fn read_yaml<T>(path: PathBuf, budget: &mut ArtifactReadBudget) -> Result<T, ArtifactError>
where
    T: for<'de> Deserialize<'de>,
{
    let bytes = read_bytes(path.clone(), budget)?;
    serde_yml::from_slice(&bytes).map_err(|source| ArtifactError::Yaml { path, source })
}

fn write_yaml<T>(path: PathBuf, value: &T) -> Result<(), ArtifactError>
where
    T: Serialize,
{
    let text = serde_yml::to_string(value).map_err(|source| ArtifactError::Yaml {
        path: path.clone(),
        source,
    })?;
    fs::write(&path, text).map_err(|source| ArtifactError::Io { path, source })
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use tempfile::tempdir;

    use super::*;
    use interpreter::{
        DefaultValue, Dispatch, FileId, MethodSig, NamespaceSymbol, ObjectField, Param,
        PropertySig, Span, Type, TypeKind, TypePredicate, TypeSymbol, ValueKind, ValueSymbol,
        compile_script, doc, mangle,
    };

    fn sample_declaration() -> PackageDeclaration {
        let file = FileId(7);
        let mut defs = PackageDeclaration::with_package("@acme/util");
        defs.values.insert(
            "greet".to_string(),
            ValueSymbol {
                name: "greet".to_string(),
                mangled_name: mangle::package_symbol("@acme/util", "greet"),
                declaration_span: Span::new(file, 0, 12).unwrap(),
                kind: ValueKind::Function {
                    generics: vec!["T".to_string()],
                    params: vec![
                        Param::with_default(
                            "name",
                            Type::String,
                            DefaultValue::String("world".to_string()),
                        ),
                        Param::rest("tags", Type::Array(Box::new(Type::String))),
                    ],
                    ret: Type::String,
                    type_predicate: Some(TypePredicate {
                        parameter_index: 0,
                        asserted_type: Type::String,
                    }),
                    doc: doc(file, "/** Greet someone. @param name who */"),
                },
            },
        );

        let mut properties = BTreeMap::new();
        properties.insert(
            "label".to_string(),
            PropertySig {
                ty: Type::String,
                readonly: true,
                intrinsic: false,
                optional: false,
                doc: doc(file, "/** Label. */"),
            },
        );
        let mut methods = BTreeMap::new();
        methods.insert(
            "rename".to_string(),
            MethodSig {
                optional: false,
                generics: Vec::new(),
                params: vec![Param::new("name", Type::String)],
                ret: Type::Void,
                predicate: None,
                doc: doc(file, "/** Rename. */"),
            },
        );
        defs.types.insert(
            "Thing".to_string(),
            TypeSymbol {
                name: "Thing".to_string(),
                mangled_name: mangle::package_symbol("@acme/util", "Thing"),
                declaration_span: Span::new(file, 20, 40).unwrap(),
                kind: TypeKind::Interface {
                    index: None,
                    generics: Vec::new(),
                    methods,
                    properties,
                    dispatch: Dispatch::Direct,
                    doc: doc(file, "/** Thing docs. */"),
                },
            },
        );

        let mut ns_values = BTreeMap::new();
        ns_values.insert(
            "version".to_string(),
            ValueSymbol {
                name: "version".to_string(),
                mangled_name: mangle::package_symbol("@acme/util", "Meta#version"),
                declaration_span: Span::new(file, 45, 60).unwrap(),
                kind: ValueKind::Const {
                    ty: Type::StringLiteral("1.0.0".to_string()),
                    doc: doc(file, "/** Current version. */"),
                },
            },
        );
        defs.namespaces.insert(
            "Meta".to_string(),
            NamespaceSymbol {
                name: "Meta".to_string(),
                mangled_prefix: mangle::package_symbol("@acme/util", "Meta"),
                declaration_span: Span::new(file, 40, 44).unwrap(),
                values: ns_values,
                types: BTreeMap::new(),
                namespaces: BTreeMap::new(),
                doc: doc(file, "/** Metadata. */"),
            },
        );

        let mut fields = BTreeMap::new();
        fields.insert("label".to_string(), ObjectField::required(Type::String));
        fields.insert("count".to_string(), ObjectField::optional(Type::Number));
        defs.shapes.push(interpreter::Shape::Object {
            index: None,
            fields,
        });
        defs
    }

    fn sample_capabilities() -> CapabilitySchema {
        CapabilitySchema {
            namespace: "acme".to_string(),
            provides: Vec::new(),
            requires: Vec::new(),
        }
    }

    fn sample_type_info() -> TypeInfoTable {
        TypeInfoTable {
            package_name: "@acme/util".to_string(),
            types: Vec::new(),
        }
    }

    #[test]
    fn package_declaration_json_round_trips() {
        let defs = sample_declaration();
        let bytes = serde_json::to_vec(&defs).expect("serialize package declaration");
        let reloaded: PackageDeclaration =
            serde_json::from_slice(&bytes).expect("deserialize package declaration");
        assert_eq!(reloaded, defs);
    }

    #[test]
    fn artifact_round_trips_and_sorts_dependencies() {
        let dir = tempdir().expect("tempdir");
        let defs = sample_declaration();
        let metadata = ArtifactMetadata::with_description(
            "@acme/util",
            "1.2.3",
            "Utility helpers.",
            vec!["zeta".to_string(), "alpha".to_string(), "alpha".to_string()],
            vec![
                ArtifactDependency::new("@z/last", "2.0.0"),
                ArtifactDependency::new("@a/first", "1.0.0"),
            ],
        );
        let wasm = b"\0asm\x01\0\0\0".to_vec();

        let capabilities = sample_capabilities();
        let type_info = sample_type_info();
        let sources = vec![ArtifactSource {
            path: ModulePath::from("lib"),
            text: "export const value = 1;".to_string(),
        }];
        write_package_artifact_with_docs_and_sources(
            dir.path(),
            &wasm,
            &type_info,
            &capabilities,
            &defs,
            &metadata,
            "# Utility\n",
            &sources,
        )
        .expect("write artifact");
        let artifact = read_package_artifact(dir.path()).expect("read artifact");

        assert_eq!(artifact.wasm, wasm);
        assert_eq!(artifact.type_info, type_info);
        assert_eq!(artifact.capabilities, capabilities);
        assert_eq!(artifact.package_declaration, defs);
        assert_eq!(artifact.metadata.description, "Utility helpers.");
        assert_eq!(artifact.metadata.keywords, vec!["alpha", "zeta"]);
        assert_eq!(artifact.documentation, "# Utility\n");
        assert_eq!(artifact.sources, sources);
        assert_eq!(
            artifact.metadata.dependencies,
            vec![
                ArtifactDependency::new("@a/first", "1.0.0"),
                ArtifactDependency::new("@z/last", "2.0.0"),
            ]
        );
    }

    #[test]
    fn artifact_reports_missing_files() {
        let dir = tempdir().expect("tempdir");
        // Metadata is read first so the schema gate can run before anything is
        // decoded against the current types.
        let err = read_package_artifact(dir.path()).expect_err("missing metadata fails");
        assert!(matches!(err, ArtifactError::Io { .. }));
        assert!(err.to_string().contains("metadata.json"));
    }

    #[test]
    fn artifact_reports_missing_wasm() {
        let dir = tempdir().expect("tempdir");
        write_json(
            dir.path().join(METADATA_FILE),
            &ArtifactMetadata::new("@t/p", "0.1.0", Vec::new()),
        )
        .expect("write metadata");

        let err = read_package_artifact(dir.path()).expect_err("missing wasm fails");

        assert!(matches!(err, ArtifactError::Io { .. }));
        assert!(err.to_string().contains("pkg.wasm"));
    }

    #[test]
    fn artifact_rejects_an_oversized_file_before_decoding_it() {
        let dir = tempdir().expect("tempdir");
        let defs = sample_declaration();
        write_package_artifact(
            dir.path(),
            b"\0asm\x01\0\0\0",
            &sample_type_info(),
            &sample_capabilities(),
            &defs,
            &ArtifactMetadata::new("@acme/util", "1.0.0", Vec::new()),
        )
        .expect("write artifact");
        fs::write(dir.path().join(WASM_FILE), vec![0_u8; 1_025]).expect("replace wasm");
        let limits = ArtifactReadLimits {
            max_file_bytes: 1_024,
            max_total_bytes: 16 << 10,
            max_packages: 1,
        };
        let mut budget = ArtifactReadBudget::new(limits);

        let error = read_package_artifact_with_budget(dir.path(), &mut budget)
            .expect_err("oversized wasm must fail");

        assert!(matches!(
            error,
            ArtifactError::FileTooLarge {
                bytes: 1_025,
                limit: 1_024,
                ..
            }
        ));
        assert!(error.to_string().contains(WASM_FILE));
    }

    #[test]
    fn bounded_reader_accepts_the_exact_file_and_total_limit() {
        let dir = tempdir().expect("tempdir");
        let path = dir.path().join("exact.bin");
        fs::write(&path, b"1234").expect("write fixture");
        let mut budget = ArtifactReadBudget::new(ArtifactReadLimits {
            max_file_bytes: 4,
            max_total_bytes: 4,
            max_packages: 1,
        });

        assert_eq!(read_bytes(path, &mut budget).unwrap(), b"1234");
        assert_eq!(budget.remaining, 0);
    }

    /// A stale artifact reports its version rather than a serde error about
    /// whatever field the newer schema reshaped.
    #[test]
    fn artifact_rejects_old_schema_before_decoding_the_declaration() {
        let dir = tempdir().expect("tempdir");
        let mut metadata = ArtifactMetadata::new("@t/p", "0.1.0", Vec::new());
        metadata.schema_version = ARTIFACT_SCHEMA_VERSION - 1;
        write_json(dir.path().join(METADATA_FILE), &metadata).expect("write metadata");
        // Deliberately leave every other file absent: the version gate must
        // fire before any of them is opened.
        let err = read_package_artifact(dir.path()).expect_err("stale schema fails");

        assert!(
            matches!(err, ArtifactError::UnsupportedSchema { .. }),
            "{err}"
        );
        assert!(
            err.to_string()
                .contains("unsupported artifact schema version")
        );
    }

    #[test]
    fn artifact_rejects_unsupported_schema() {
        let dir = tempdir().expect("tempdir");
        let defs = sample_declaration();
        let mut metadata = ArtifactMetadata::new("@acme/util", "1.2.3", Vec::new());
        metadata.schema_version = ARTIFACT_SCHEMA_VERSION + 1;

        let capabilities = sample_capabilities();
        write_package_artifact(
            dir.path(),
            b"\0asm\x01\0\0\0",
            &sample_type_info(),
            &capabilities,
            &defs,
            &metadata,
        )
        .expect("write artifact");
        let err = read_package_artifact(dir.path()).expect_err("unsupported schema fails");
        assert!(matches!(err, ArtifactError::UnsupportedSchema { .. }));
    }

    #[test]
    fn reloaded_declaration_is_accepted_as_external_package() {
        let defs = sample_declaration();
        let bytes = serde_json::to_vec(&defs).expect("serialize package declaration");
        let reloaded: PackageDeclaration =
            serde_json::from_slice(&bytes).expect("deserialize package declaration");
        let source = r#"
            import { greet } from "@acme/util";

            function main(): string {
                return greet<string>("Ada");
            }
        "#;

        compile_script(source, "main.subm", FileId(1), &[&reloaded], &[])
            .expect("consumer compiles against reloaded package declaration");
    }
}
