//! Local prebuilt package artifact reader/writer.

use std::fmt;
use std::fs;
use std::io;
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
pub const ARTIFACT_SCHEMA_VERSION: u32 = 5;

const WASM_FILE: &str = "pkg.wasm";
const CAPABILITIES_FILE: &str = "capabilities.yaml";
const PACKAGE_DECLARATION_FILE: &str = "package-declaration.json";
const TYPE_INFO_FILE: &str = "type-info.json";
const METADATA_FILE: &str = "metadata.json";
const DOCS_README_FILE: &str = "docs/readme.md";
const SOURCES_FILE: &str = "sources.json";

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
        }
    }
}

impl std::error::Error for ArtifactError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            ArtifactError::Io { source, .. } => Some(source),
            ArtifactError::Json { source, .. } => Some(source),
            ArtifactError::Yaml { source, .. } => Some(source),
            ArtifactError::UnsupportedSchema { .. } => None,
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
    let dir = dir.as_ref();
    let metadata_path = dir.join(METADATA_FILE);
    // The version gate comes first: a schema bump can change the shape of the
    // other files, and decoding them against the current types would surface a
    // serde error about some interior field instead of "rebuild this package".
    let metadata: ArtifactMetadata = read_json(&metadata_path)?;
    if metadata.schema_version != ARTIFACT_SCHEMA_VERSION {
        return Err(ArtifactError::UnsupportedSchema {
            path: metadata_path,
            found: metadata.schema_version,
        });
    }
    let wasm = read_bytes(dir.join(WASM_FILE))?;
    let type_info = read_json(dir.join(TYPE_INFO_FILE))?;
    let capabilities = read_yaml(dir.join(CAPABILITIES_FILE))?;
    let package_declaration = read_json(dir.join(PACKAGE_DECLARATION_FILE))?;
    let documentation = read_optional_text(dir.join(DOCS_README_FILE))?;
    let sources = read_optional_json(dir.join(SOURCES_FILE))?.unwrap_or_default();

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

fn read_bytes(path: PathBuf) -> Result<Vec<u8>, ArtifactError> {
    fs::read(&path).map_err(|source| ArtifactError::Io { path, source })
}

fn read_optional_text(path: PathBuf) -> Result<String, ArtifactError> {
    match fs::read_to_string(&path) {
        Ok(text) => Ok(text),
        Err(source) if source.kind() == io::ErrorKind::NotFound => Ok(String::new()),
        Err(source) => Err(ArtifactError::Io { path, source }),
    }
}

fn read_optional_json<T>(path: PathBuf) -> Result<Option<T>, ArtifactError>
where
    T: for<'de> Deserialize<'de>,
{
    match fs::read(&path) {
        Ok(bytes) => serde_json::from_slice(&bytes)
            .map(Some)
            .map_err(|source| ArtifactError::Json { path, source }),
        Err(source) if source.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(source) => Err(ArtifactError::Io { path, source }),
    }
}

fn write_bytes(path: PathBuf, bytes: &[u8]) -> Result<(), ArtifactError> {
    if let Some(parent) = path.parent() {
        create_dir_all(parent)?;
    }
    fs::write(&path, bytes).map_err(|source| ArtifactError::Io { path, source })
}

fn read_json<T>(path: impl AsRef<Path>) -> Result<T, ArtifactError>
where
    T: for<'de> Deserialize<'de>,
{
    let path = path.as_ref();
    let bytes = fs::read(path).map_err(|source| ArtifactError::Io {
        path: path.to_path_buf(),
        source,
    })?;
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

fn read_yaml<T>(path: PathBuf) -> Result<T, ArtifactError>
where
    T: for<'de> Deserialize<'de>,
{
    let bytes = fs::read(&path).map_err(|source| ArtifactError::Io {
        path: path.clone(),
        source,
    })?;
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
                declaration_span: Span::new(file, 0, 12),
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
                declaration_span: Span::new(file, 20, 40),
                kind: TypeKind::Interface {
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
                declaration_span: Span::new(file, 45, 60),
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
                declaration_span: Span::new(file, 40, 44),
                values: ns_values,
                types: BTreeMap::new(),
                namespaces: BTreeMap::new(),
                doc: doc(file, "/** Metadata. */"),
            },
        );

        let mut fields = BTreeMap::new();
        fields.insert("label".to_string(), ObjectField::required(Type::String));
        fields.insert("count".to_string(), ObjectField::optional(Type::Number));
        defs.shapes.push(interpreter::Shape::Object { fields });
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
