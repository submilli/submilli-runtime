//! Cross-package compilation driver: compiles a manifest's packages in
//! dependency order and installs the resulting artifacts into a package store.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use interpreter::{
    Diagnostic, ModulePath, PackageDeclaration, PackageSourceModule, Sources,
    compile_package_with_transitive, diagnostics,
};

use crate::artifact::{ArtifactReadBudget, ArtifactReadLimits};

use crate::{
    ArtifactDependency, ArtifactError, ArtifactMetadata, ArtifactSource,
    CANONICAL_SOURCE_EXTENSION, CapabilitySchema, DependencyKind, LEGACY_SOURCE_EXTENSION,
    PackageManifest, PackageName, PackageSource, PackageStore, PackageStoreError, PackageVersion,
    ProjectManifest, derive_capability_schema, write_package_artifact_with_docs_and_sources,
};

#[derive(Clone, Debug)]
pub struct BuiltPackage {
    pub name: PackageName,
    pub version: PackageVersion,
    pub description: String,
    pub keywords: Vec<String>,
    pub documentation: String,
    pub dependencies: Vec<ArtifactDependency>,
    pub wasm: Vec<u8>,
    pub type_info: interpreter::TypeInfoTable,
    pub capabilities: CapabilitySchema,
    pub authority_map: interpreter::AuthorityMap,
    pub declaration: PackageDeclaration,
    pub sources: Vec<ArtifactSource>,
    /// Rendered warning blocks (message + `--> path:line:col` + source context),
    /// ready to print verbatim.
    pub warnings: Vec<String>,
    /// Source provenance to stamp into the installed artifact's metadata. `None`
    /// for a plain `build_packages` run (local compile); `submilli install` sets
    /// it to the resolved GitHub repo + SHA before calling `install_packages`.
    pub source: Option<PackageSource>,
}

/// Compile every package in the manifest (or the sibling-dependency closure of
/// `only`) in dependency order. Sibling dependencies resolve to declarations
/// compiled earlier in the same run; external dependencies load from
/// `externals`. Reads sources from disk and writes nothing.
pub fn build_packages(
    manifest: &ProjectManifest,
    manifest_dir: &Path,
    externals: &PackageStore,
    only: Option<&PackageName>,
) -> Result<Vec<BuiltPackage>, DriverError> {
    let scoped = scope_packages(manifest, only)?;
    let order = topo_order(&scoped)?;

    let mut built: BTreeMap<PackageName, PackageDeclaration> = BTreeMap::new();
    let mut external_cache: BTreeMap<PackageName, ExternalArtifact> = BTreeMap::new();
    let mut external_budget = ArtifactReadBudget::new(ArtifactReadLimits::default());
    let mut results = Vec::with_capacity(order.len());
    // Schema derivation resolves a binding path through these as the
    // compiler does.
    let stdlib_declarations = interpreter::runtime::stdlib_package_declarations();
    let (prelude_declarations, host_declarations, _) =
        interpreter::runtime::prelude::cached_runtime_package_declarations();
    for index in order {
        let package = scoped[index];
        load_external_dependencies(
            package,
            externals,
            &mut external_cache,
            &mut external_budget,
        )?;
        let dependency_refs: Vec<&PackageDeclaration> = package
            .dependencies
            .iter()
            .map(|dep| match dep.kind {
                DependencyKind::Sibling => &built[&dep.name],
                DependencyKind::External | DependencyKind::Github => {
                    &external_cache[&dep.name].declaration
                }
            })
            .collect();

        let src_dir = manifest_dir.join(package.path.as_path()).join("src");
        let documentation = read_package_docs(manifest_dir, package)?;
        let modules = discover_modules(&src_dir, manifest_dir)?;
        let module_refs: Vec<PackageSourceModule<'_>> = modules
            .iter()
            .map(|module| PackageSourceModule {
                path: module.path.clone(),
                source: &module.text,
            })
            .collect();
        let transitive_refs =
            transitive_dependency_decls(package, &scoped, &built, &external_cache);
        let compiled = compile_package_with_transitive(
            package.name.as_str(),
            ModulePath::from("lib"),
            &module_refs,
            &dependency_refs,
            &transitive_refs,
        )
        .map_err(|diags| DriverError::Compile {
            package: package.name.clone(),
            rendered: render_diagnostics(&modules, &diags),
        })?;

        let schema_dependencies: Vec<&PackageDeclaration> = dependency_refs
            .iter()
            .chain(&transitive_refs)
            .copied()
            .chain(&stdlib_declarations)
            .chain(prelude_declarations)
            .chain(host_declarations)
            .collect();
        let capabilities = derive_capability_schema(
            &compiled.declaration,
            &schema_dependencies,
            &compiled.required_capabilities,
        );
        built.insert(package.name.clone(), compiled.declaration.clone());
        results.push(BuiltPackage {
            name: package.name.clone(),
            version: package.version.clone(),
            description: package.description.clone(),
            keywords: package.keywords.clone(),
            documentation,
            dependencies: package
                .dependencies
                .iter()
                .map(|dep| {
                    let version = match &dep.version {
                        Some(version) => version.as_str().to_string(),
                        None => external_cache[&dep.name].version.clone(),
                    };
                    ArtifactDependency::new(dep.name.as_str(), version)
                })
                .collect(),
            wasm: compiled.wasm,
            capabilities,
            authority_map: compiled.authority_map,
            declaration: compiled.declaration,
            sources: modules
                .iter()
                .map(|module| ArtifactSource {
                    path: module.path.clone(),
                    text: module.text.clone(),
                })
                .collect(),
            warnings: render_diagnostics_list(&modules, &compiled.warnings).map_err(|source| {
                DriverError::Diagnostic {
                    package: package.name.clone(),
                    source,
                }
            })?,
            type_info: compiled.type_info,
            source: None,
        });
    }
    Ok(results)
}

/// The inputs of one manifest package as [`build_packages`] would read them now: its
/// source modules, in the form an installed artifact embeds them, and its
/// `docs/readme.md`. Comparing them with [`crate::read_installed_sources`] says whether
/// the installed copy was built from what is on disk.
pub fn package_sources(
    manifest_dir: &Path,
    package: &PackageManifest,
) -> Result<(Vec<ArtifactSource>, String), DriverError> {
    let src_dir = manifest_dir.join(package.path.as_path()).join("src");
    let modules = discover_modules(&src_dir, manifest_dir)?;
    let documentation = read_package_docs(manifest_dir, package)?;
    let sources = modules
        .into_iter()
        .map(|module| ArtifactSource {
            path: module.path,
            text: module.text,
        })
        .collect();
    Ok((sources, documentation))
}

/// Write each built package into the store layout (`<root>/@scope/name/`).
/// Returns the installed directories in build order.
pub fn install_packages(
    store: &PackageStore,
    packages: &[BuiltPackage],
) -> Result<Vec<PathBuf>, DriverError> {
    let mut dirs = Vec::with_capacity(packages.len());
    for package in packages {
        let dir =
            store
                .package_dir(package.name.as_str())
                .map_err(|source| DriverError::Store {
                    package: package.name.clone(),
                    source: Box::new(source),
                })?;
        let mut metadata = ArtifactMetadata::with_description(
            package.name.as_str(),
            package.version.as_str(),
            package.description.clone(),
            package.keywords.clone(),
            package.dependencies.clone(),
        );
        metadata.source = package.source.clone();
        write_package_artifact_with_docs_and_sources(
            &dir,
            &package.wasm,
            &package.type_info,
            &package.capabilities,
            &package.declaration,
            &metadata,
            &package.documentation,
            &package.sources,
        )
        .map_err(|source| DriverError::Install {
            package: package.name.clone(),
            source: Box::new(source),
        })?;
        dirs.push(dir);
    }
    Ok(dirs)
}

#[derive(Debug)]
pub enum DriverError {
    Diagnostic {
        package: PackageName,
        source: interpreter::rendering::RenderError,
    },
    DependencyDepth {
        package: PackageName,
    },
    MissingSibling {
        package: PackageName,
        dependency: PackageName,
    },
    DependencyCycle {
        cycle: Vec<PackageName>,
    },
    UnknownPackage {
        requested: PackageName,
        available: Vec<PackageName>,
    },
    MissingExternal {
        package: PackageName,
        source: Box<PackageStoreError>,
    },
    ExternalVersionMismatch {
        package: PackageName,
        required: PackageVersion,
        found: String,
    },
    SourceRead {
        path: PathBuf,
        source: io::Error,
    },
    DocumentationRead {
        package: PackageName,
        path: PathBuf,
        source: io::Error,
    },
    NonUtf8ModulePath {
        path: PathBuf,
    },
    ModuleOutsideSource {
        source_root: PathBuf,
        path: PathBuf,
    },
    SourceDirectoryCycle {
        path: PathBuf,
    },
    AmbiguousModulePath {
        module: String,
        first: PathBuf,
        second: PathBuf,
    },
    Compile {
        package: PackageName,
        rendered: String,
    },
    Install {
        package: PackageName,
        source: Box<ArtifactError>,
    },
    Store {
        package: PackageName,
        source: Box<PackageStoreError>,
    },
}

impl fmt::Display for DriverError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            DriverError::Diagnostic { package, source } => write!(
                f,
                "package `{}` diagnostic failed: {source}",
                package.as_str()
            ),
            DriverError::DependencyDepth { package } => write!(
                f,
                "package dependency traversal exceeds {} levels at `{}`; shorten the dependency chain",
                crate::MAX_DEPENDENCY_DEPTH,
                package.as_str()
            ),
            DriverError::MissingSibling {
                package,
                dependency,
            } => write!(
                f,
                "package `{}` depends on missing sibling package `{}`; add its [[package]] declaration or remove the dependency",
                package.as_str(),
                dependency.as_str(),
            ),
            DriverError::DependencyCycle { cycle } => {
                let names = cycle
                    .iter()
                    .map(PackageName::as_str)
                    .collect::<Vec<_>>()
                    .join(" -> ");
                write!(
                    f,
                    "circular package dependency: {names}; break the cycle by removing one of the dependencies or extracting shared code into a third package"
                )
            }
            DriverError::UnknownPackage {
                requested,
                available,
            } => {
                write!(
                    f,
                    "package `{}` is not declared in submilli.toml",
                    requested.as_str()
                )?;
                if available.is_empty() {
                    Ok(())
                } else {
                    let names = available
                        .iter()
                        .map(|name| format!("`{}`", name.as_str()))
                        .collect::<Vec<_>>()
                        .join(", ");
                    write!(f, "; declared packages: {names}")
                }
            }
            DriverError::MissingExternal { package, source } => write!(
                f,
                "failed to load external dependency `{}`: {source}",
                package.as_str()
            ),
            DriverError::ExternalVersionMismatch {
                package,
                required,
                found,
            } => write!(
                f,
                "external dependency `{name}` has version {found} in the store, but submilli.toml requires {required}; rebuild `{name}` at {required} or change submilli.toml to {found}",
                name = package.as_str(),
                required = required.as_str(),
            ),
            DriverError::SourceRead { path, source } => {
                write!(f, "failed to read {}: {source}", path.display())
            }
            DriverError::DocumentationRead {
                package,
                path,
                source,
            } => write!(
                f,
                "package `{}` is missing documentation at {}; create docs/readme.md: {source}",
                package.as_str(),
                path.display()
            ),
            DriverError::NonUtf8ModulePath { path } => write!(
                f,
                "package source path {} is not valid UTF-8; rename the file",
                path.display()
            ),
            DriverError::ModuleOutsideSource { source_root, path } => write!(
                f,
                "package source path {} is outside source directory {}",
                path.display(),
                source_root.display()
            ),
            DriverError::SourceDirectoryCycle { path } => write!(
                f,
                "package source directory {} forms a link cycle; remove the cyclic link",
                path.display()
            ),
            DriverError::AmbiguousModulePath {
                module,
                first,
                second,
            } => write!(
                f,
                "delete or rename one source file for module `{module}`; both {} and {} exist",
                first.display(),
                second.display()
            ),
            DriverError::Compile { package, rendered } => {
                write!(
                    f,
                    "package `{}` failed to compile:\n{rendered}",
                    package.as_str()
                )
            }
            DriverError::Install { package, source } => write!(
                f,
                "failed to install package `{}`: {source}",
                package.as_str()
            ),
            DriverError::Store { package, source } => write!(
                f,
                "failed to install package `{}`: {source}",
                package.as_str()
            ),
        }
    }
}

impl std::error::Error for DriverError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            DriverError::MissingExternal { source, .. } | DriverError::Store { source, .. } => {
                Some(source)
            }
            DriverError::SourceRead { source, .. }
            | DriverError::DocumentationRead { source, .. } => Some(source),
            DriverError::Install { source, .. } => Some(source),
            DriverError::Diagnostic { source, .. } => Some(source),
            _ => None,
        }
    }
}

fn read_package_docs(
    manifest_dir: &Path,
    package: &PackageManifest,
) -> Result<String, DriverError> {
    let path = manifest_dir
        .join(package.path.as_path())
        .join("docs")
        .join("readme.md");
    fs::read_to_string(&path).map_err(|source| DriverError::DocumentationRead {
        package: package.name.clone(),
        path,
        source,
    })
}

fn scope_packages<'a>(
    manifest: &'a ProjectManifest,
    only: Option<&PackageName>,
) -> Result<Vec<&'a PackageManifest>, DriverError> {
    let Some(only) = only else {
        return Ok(manifest.packages.iter().collect());
    };
    if !manifest.packages.iter().any(|p| p.name == *only) {
        return Err(DriverError::UnknownPackage {
            requested: only.clone(),
            available: manifest.packages.iter().map(|p| p.name.clone()).collect(),
        });
    }

    let by_name: BTreeMap<&PackageName, &PackageManifest> =
        manifest.packages.iter().map(|p| (&p.name, p)).collect();
    let mut closure = BTreeSet::new();
    let mut pending = vec![only.clone()];
    while let Some(name) = pending.pop() {
        if !closure.insert(name.clone()) {
            continue;
        }
        let Some(package) = by_name.get(&name) else {
            continue;
        };
        for dep in &package.dependencies {
            if dep.kind == DependencyKind::Sibling {
                pending.push(dep.name.clone());
            }
        }
    }
    Ok(manifest
        .packages
        .iter()
        .filter(|p| closure.contains(&p.name))
        .collect())
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Mark {
    Visiting,
    Done,
}

fn topo_order(packages: &[&PackageManifest]) -> Result<Vec<usize>, DriverError> {
    let index_by_name: BTreeMap<&PackageName, usize> = packages
        .iter()
        .enumerate()
        .map(|(index, p)| (&p.name, index))
        .collect();
    let mut marks: BTreeMap<usize, Mark> = BTreeMap::new();
    let mut stack = Vec::new();
    let mut order = Vec::with_capacity(packages.len());
    for index in 0..packages.len() {
        visit(
            index,
            packages,
            &index_by_name,
            &mut marks,
            &mut stack,
            &mut order,
        )?;
    }
    Ok(order)
}

fn visit(
    index: usize,
    packages: &[&PackageManifest],
    index_by_name: &BTreeMap<&PackageName, usize>,
    marks: &mut BTreeMap<usize, Mark>,
    stack: &mut Vec<usize>,
    order: &mut Vec<usize>,
) -> Result<(), DriverError> {
    match marks.get(&index) {
        Some(Mark::Done) => return Ok(()),
        Some(Mark::Visiting) => {
            let start = stack.iter().position(|i| *i == index).unwrap_or(0);
            let mut cycle: Vec<PackageName> = stack[start..]
                .iter()
                .map(|i| packages[*i].name.clone())
                .collect();
            cycle.push(packages[index].name.clone());
            return Err(DriverError::DependencyCycle { cycle });
        }
        None => {}
    }
    if stack.len() >= crate::MAX_DEPENDENCY_DEPTH {
        return Err(DriverError::DependencyDepth {
            package: packages[index].name.clone(),
        });
    }
    marks.insert(index, Mark::Visiting);
    stack.push(index);
    for dep in &packages[index].dependencies {
        if dep.kind != DependencyKind::Sibling {
            continue;
        }
        let Some(dep_index) = index_by_name.get(&dep.name) else {
            return Err(DriverError::MissingSibling {
                package: packages[index].name.clone(),
                dependency: dep.name.clone(),
            });
        };
        visit(*dep_index, packages, index_by_name, marks, stack, order)?;
    }
    stack.pop();
    marks.insert(index, Mark::Done);
    order.push(index);
    Ok(())
}

/// A dependency loaded from the store: its declaration (for compilation) plus
/// its resolved version (used for the dependent's recorded dependency edge —
/// GitHub deps declare no version, so it is adopted from the installed artifact).
struct ExternalArtifact {
    declaration: PackageDeclaration,
    version: String,
    /// The artifact's own dependencies, for walking the closure. An installed
    /// artifact records them in its metadata; they are all in the store too,
    /// since `install` loads a closure.
    dependencies: Vec<PackageName>,
}

/// Loads `package`'s store-backed dependencies **and their own**, transitively.
/// A direct dependency's public surface can name a type from one of its
/// dependencies, so the compiler needs the whole closure to resolve it.
fn load_external_dependencies(
    package: &PackageManifest,
    externals: &PackageStore,
    cache: &mut BTreeMap<PackageName, ExternalArtifact>,
    budget: &mut ArtifactReadBudget,
) -> Result<(), DriverError> {
    let mut queue: Vec<(PackageName, Option<PackageVersion>)> = package
        .dependencies
        .iter()
        .filter(|dep| matches!(dep.kind, DependencyKind::External | DependencyKind::Github))
        .map(|dep| (dep.name.clone(), dep.version.clone()))
        .collect();
    while let Some((name, required_version)) = queue.pop() {
        if cache.contains_key(&name) {
            continue;
        }
        let artifact = externals
            .load_with_budget(name.as_str(), budget)
            .map_err(|source| DriverError::MissingExternal {
                package: name.clone(),
                source: Box::new(source),
            })?;
        // GitHub deps pin by SHA and adopt the fetched version; only a
        // version-declared external must match the store exactly.
        if let Some(required) = &required_version
            && artifact.metadata.package_version != required.as_str()
        {
            return Err(DriverError::ExternalVersionMismatch {
                package: name.clone(),
                required: required.clone(),
                found: artifact.metadata.package_version,
            });
        }
        let dependencies: Vec<PackageName> = artifact
            .metadata
            .dependencies
            .iter()
            .map(|d| PackageName::new(d.name.clone()))
            .collect();
        // Enqueued without a required version: the edge constraint belongs to
        // the *dependent's* manifest, which this package doesn't own.
        queue.extend(dependencies.iter().map(|name| (name.clone(), None)));
        cache.insert(
            name,
            ExternalArtifact {
                declaration: artifact.package_declaration,
                version: artifact.metadata.package_version,
                dependencies,
            },
        );
    }
    Ok(())
}

/// The rest of `package`'s dependency closure: everything its dependencies
/// depend on, minus what it declares itself. These are not importable — they
/// only have to be *resolvable*, because a direct dependency's public surface
/// can name a class or interface from one of them.
fn transitive_dependency_decls<'a>(
    package: &PackageManifest,
    scoped: &[&PackageManifest],
    built: &'a BTreeMap<PackageName, PackageDeclaration>,
    external_cache: &'a BTreeMap<PackageName, ExternalArtifact>,
) -> Vec<&'a PackageDeclaration> {
    let direct: BTreeSet<&PackageName> = package.dependencies.iter().map(|d| &d.name).collect();
    let deps_of = |name: &PackageName| -> Vec<PackageName> {
        if let Some(manifest) = scoped.iter().find(|p| &p.name == name) {
            return manifest
                .dependencies
                .iter()
                .map(|d| d.name.clone())
                .collect();
        }
        external_cache
            .get(name)
            .map(|a| a.dependencies.clone())
            .unwrap_or_default()
    };
    let mut queue: Vec<PackageName> = direct.iter().flat_map(|name| deps_of(name)).collect();
    let mut seen: BTreeSet<PackageName> = BTreeSet::new();
    let mut decls = Vec::new();
    while let Some(name) = queue.pop() {
        if direct.contains(&name) || !seen.insert(name.clone()) {
            continue;
        }
        queue.extend(deps_of(&name));
        // A sibling is compiled before its dependents (topological order), so
        // `built` holds it; anything else came from the store.
        if let Some(decl) = built
            .get(&name)
            .or_else(|| external_cache.get(&name).map(|a| &a.declaration))
        {
            decls.push(decl);
        }
    }
    decls
}

/// A package source file: its import-facing module path (`lib`,
/// `internal/math`), the manifest-relative on-disk path diagnostics point at
/// (`app/src/lib.ts`), and its text.
#[derive(Debug)]
struct DiscoveredModule {
    path: ModulePath,
    display_path: String,
    text: String,
}

fn discover_modules(
    src_dir: &Path,
    manifest_dir: &Path,
) -> Result<Vec<DiscoveredModule>, DriverError> {
    let mut files = Vec::new();
    collect_source_files(src_dir, &mut files)?;
    files.sort();
    let mut seen: BTreeMap<String, PathBuf> = BTreeMap::new();
    let mut modules = Vec::with_capacity(files.len());
    for file in files {
        let module_path = module_path(src_dir, &file)?;
        if let Some(first) = seen.insert(module_path.clone(), file.clone()) {
            return Err(DriverError::AmbiguousModulePath {
                module: module_path,
                first,
                second: file,
            });
        }
        let display_path = file
            .strip_prefix(manifest_dir)
            .unwrap_or(&file)
            .to_string_lossy()
            .replace('\\', "/");
        let text = fs::read_to_string(&file).map_err(|source| DriverError::SourceRead {
            path: file.clone(),
            source,
        })?;
        modules.push(DiscoveredModule {
            path: ModulePath::from(module_path),
            display_path,
            text,
        });
    }
    Ok(modules)
}

fn module_path(src_dir: &Path, file: &Path) -> Result<String, DriverError> {
    let relative = file
        .strip_prefix(src_dir)
        .map_err(|_| DriverError::ModuleOutsideSource {
            source_root: src_dir.to_path_buf(),
            path: file.to_path_buf(),
        })?;
    let without_ext = relative.with_extension("");
    let module_path = without_ext
        .to_str()
        .ok_or_else(|| DriverError::NonUtf8ModulePath {
            path: file.to_path_buf(),
        })?
        .replace('\\', "/");
    Ok(module_path)
}

fn collect_source_files(dir: &Path, out: &mut Vec<PathBuf>) -> Result<(), DriverError> {
    struct Directory {
        path: PathBuf,
        canonical: PathBuf,
        entries: fs::ReadDir,
    }

    let canonical = fs::canonicalize(dir).map_err(|source| DriverError::SourceRead {
        path: dir.to_path_buf(),
        source,
    })?;
    let entries = fs::read_dir(dir).map_err(|source| DriverError::SourceRead {
        path: dir.to_path_buf(),
        source,
    })?;
    let mut ancestors = BTreeSet::from([canonical.clone()]);
    let mut directories = vec![Directory {
        path: dir.to_path_buf(),
        canonical,
        entries,
    }];
    while let Some(directory) = directories.last_mut() {
        let Some(entry) = directory.entries.next() else {
            ancestors.remove(&directory.canonical);
            directories.pop();
            continue;
        };
        let child = entry
            .map_err(|source| DriverError::SourceRead {
                path: directory.path.clone(),
                source,
            })?
            .path();
        let metadata = fs::metadata(&child).map_err(|source| DriverError::SourceRead {
            path: child.clone(),
            source,
        })?;
        if metadata.is_dir() {
            let canonical = fs::canonicalize(&child).map_err(|source| DriverError::SourceRead {
                path: child.clone(),
                source,
            })?;
            if ancestors.contains(&canonical) {
                return Err(DriverError::SourceDirectoryCycle { path: child });
            }
            let entries = fs::read_dir(&child).map_err(|source| DriverError::SourceRead {
                path: child.clone(),
                source,
            })?;
            ancestors.insert(canonical.clone());
            directories.push(Directory {
                path: child,
                canonical,
                entries,
            });
        } else if is_source_file(&child) {
            out.push(child);
        }
    }
    Ok(())
}

fn is_source_file(path: &Path) -> bool {
    matches!(
        path.extension().and_then(|ext| ext.to_str()),
        Some(CANONICAL_SOURCE_EXTENSION | LEGACY_SOURCE_EXTENSION)
    )
}

// compile_package registers its modules in slice order, so rebuilding a
// `Sources` from the same slice reproduces its FileId assignment and the
// rendered diagnostics point at the right files — here under their on-disk
// display paths instead of the import-facing module paths.
fn display_sources(
    modules: &[DiscoveredModule],
) -> Result<Sources, interpreter::source::SourceError> {
    let mut sources = Sources::new();
    for module in modules {
        sources.add(module.display_path.clone(), module.text.clone())?;
    }
    Ok(sources)
}

fn render_diagnostics(modules: &[DiscoveredModule], diags: &[Diagnostic]) -> String {
    let result = display_sources(modules)
        .map_err(interpreter::rendering::RenderError::from)
        .and_then(|sources| diagnostics::render_collection(diags, &sources));
    result.map_or_else(
        |error| {
            interpreter::rendering::failure_text(
                diags
                    .first()
                    .map_or("package compilation failed", |diagnostic| {
                        diagnostic.message.as_str()
                    }),
                &error,
            )
        },
        |rendered| rendered.text,
    )
}

fn render_diagnostics_list(
    modules: &[DiscoveredModule],
    diags: &[Diagnostic],
) -> Result<Vec<String>, interpreter::rendering::RenderError> {
    diagnostics::render_list(diags, &display_sources(modules)?)
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::path::Path;

    use tempfile::tempdir;

    use super::*;
    use crate::{PackageEntrypoint, PackagePath, ResolvedDependency, write_package_artifact};

    #[test]
    fn module_path_rejects_file_outside_source_directory() {
        let error = module_path(Path::new("package/src"), Path::new("other/lib.ts"))
            .expect_err("the file is outside the source directory");
        assert!(matches!(error, DriverError::ModuleOutsideSource { .. }));
    }

    #[test]
    fn external_dependency_cache_shares_one_package_budget() {
        let root = tempdir().expect("store root");
        write_external_artifact(root.path(), "@external/a");
        write_external_artifact(root.path(), "@external/b");
        let store = PackageStore::new(root.path());
        let package = PackageManifest {
            name: PackageName::new("@app/main"),
            version: PackageVersion::new("1.0.0"),
            description: String::new(),
            keywords: Vec::new(),
            path: PackagePath::new("."),
            entrypoint: PackageEntrypoint::new("src/lib.ts"),
            dependencies: vec![
                ResolvedDependency {
                    name: PackageName::new("@external/a"),
                    version: Some(PackageVersion::new("1.0.0")),
                    kind: DependencyKind::External,
                },
                ResolvedDependency {
                    name: PackageName::new("@external/b"),
                    version: Some(PackageVersion::new("1.0.0")),
                    kind: DependencyKind::External,
                },
            ],
        };
        let limits = ArtifactReadLimits {
            max_file_bytes: 1 << 20,
            max_total_bytes: 1 << 20,
            max_packages: 1,
        };
        let mut budget = ArtifactReadBudget::new(limits);
        let mut cache = BTreeMap::new();

        let error = load_external_dependencies(&package, &store, &mut cache, &mut budget)
            .expect_err("the second retained external must exceed the shared budget");

        assert!(matches!(
            error,
            DriverError::MissingExternal { source, .. }
                if matches!(
                    source.as_ref(),
                    PackageStoreError::Artifact {
                        source: ArtifactError::PackageLimitExceeded { limit: 1, .. },
                        ..
                    }
                )
        ));
        let mut budget = ArtifactReadBudget::new(ArtifactReadLimits::default());
        let mut cache = BTreeMap::new();
        load_external_dependencies(&package, &store, &mut cache, &mut budget)
            .expect("ordinary budget recovers");
        assert_eq!(cache.len(), 2);
    }

    fn write_external_artifact(root: &Path, name: &str) {
        let mut declaration = PackageDeclaration::with_package(name);
        declaration.refresh_shapes();
        let type_info = interpreter::TypeInfoTable {
            package_name: name.to_string(),
            types: Vec::new(),
        };
        let capabilities = derive_capability_schema(&declaration, &[], &[]);
        let (scope, package) = name.split_once('/').expect("scoped package");
        write_package_artifact(
            root.join(scope).join(package),
            b"\0asm\x01\0\0\0",
            &type_info,
            &capabilities,
            &declaration,
            &ArtifactMetadata::new(name, "1.0.0", Vec::new()),
        )
        .expect("write external artifact");
    }

    #[cfg(unix)]
    #[test]
    fn source_discovery_rejects_directory_link_cycle() {
        let root = tempfile::tempdir().expect("source root");
        std::os::unix::fs::symlink(root.path(), root.path().join("loop"))
            .expect("create directory link");
        let mut files = Vec::new();
        let error = collect_source_files(root.path(), &mut files)
            .expect_err("a cyclic source directory must fail");
        assert!(matches!(error, DriverError::SourceDirectoryCycle { .. }));
    }

    #[cfg(unix)]
    #[test]
    fn source_discovery_reports_self_referential_link() {
        let root = tempfile::tempdir().expect("source root");
        std::os::unix::fs::symlink("loop", root.path().join("loop"))
            .expect("create self-referential link");
        let mut files = Vec::new();
        let error = collect_source_files(root.path(), &mut files)
            .expect_err("an unreadable source entry must fail");
        assert!(matches!(error, DriverError::SourceRead { .. }));
    }

    fn write_module(src_dir: &Path, relative: &str, text: &str) {
        let path = src_dir.join(relative);
        fs::create_dir_all(path.parent().expect("module parent")).expect("create module dir");
        fs::write(path, text).expect("write module");
    }

    #[test]
    fn discover_modules_maps_nested_paths_and_skips_other_files() {
        let tmp = tempdir().expect("tempdir");
        let src = tmp.path().join("src");
        write_module(
            &src,
            "lib.ts",
            "export function api(): number { return 1; }",
        );
        write_module(&src, "internal/math.subm", "export const x = 1;");
        write_module(&src, "notes.txt", "not a module");

        let modules = discover_modules(&src, tmp.path()).expect("discover modules");

        let paths: Vec<&str> = modules.iter().map(|m| m.path.as_str()).collect();
        assert_eq!(paths, vec!["internal/math", "lib"]);
        let display_paths: Vec<&str> = modules.iter().map(|m| m.display_path.as_str()).collect();
        assert_eq!(display_paths, vec!["src/internal/math.subm", "src/lib.ts"]);
    }

    #[test]
    fn discover_modules_sorts_for_deterministic_file_ids() {
        let tmp = tempdir().expect("tempdir");
        let src = tmp.path().join("src");
        write_module(&src, "zeta.ts", "export const z = 1;");
        write_module(&src, "alpha.subm", "export const a = 1;");
        write_module(&src, "lib.ts", "export const l = 1;");

        let modules = discover_modules(&src, tmp.path()).expect("discover modules");

        let paths: Vec<&str> = modules.iter().map(|m| m.path.as_str()).collect();
        assert_eq!(paths, vec!["alpha", "lib", "zeta"]);
    }

    #[test]
    fn discover_modules_rejects_same_module_with_two_extensions() {
        let tmp = tempdir().expect("tempdir");
        let src = tmp.path().join("src");
        write_module(&src, "wrap.ts", "export const x = 1;");
        write_module(&src, "wrap.subm", "export const x = 2;");

        let err = discover_modules(&src, tmp.path()).expect_err("ambiguous module");

        let DriverError::AmbiguousModulePath {
            module,
            first,
            second,
        } = err
        else {
            panic!("expected ambiguous module error, got {err}");
        };
        assert_eq!(module, "wrap");
        let first = first.display().to_string();
        let second = second.display().to_string();
        assert!(first.ends_with("wrap.subm") || first.ends_with("wrap.ts"));
        assert!(second.ends_with("wrap.subm") || second.ends_with("wrap.ts"));
    }
}
