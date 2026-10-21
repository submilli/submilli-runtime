//! The blueprint the host serves, applied from the project's file each time the
//! developer saves it.
//!
//! Copied from `submilli-server`'s `local_apply.rs`, with the validators it uses
//! from `handlers/blueprint.rs` (parse, volume references, secrets, packages and
//! the filter-field check against capability metadata). The blueprint is stored
//! with an opaque version tag, and a run reads both in one lookup. A volume the
//! blueprint names that the host does not declare is declared on the spot as
//! `managed-local`, so a new volume needs no restart. The host serves one
//! blueprint, by the name it had at start, so a file that renames it is refused.
//!
//! Nothing is kept on disk: the version tag lives with the registration, and the
//! file is applied again at every start.

use std::sync::Arc;

use serde::Serialize;
use submilli_blueprint::{Access, Blueprint, BlueprintError, YamlPath, yaml_path};
use submilli_build::PackageStore;
use submilli_shared::BlueprintSecretResolver;
use submilli_shared::host::SecretStore;
use tokio::sync::RwLock;

#[cfg(test)]
mod tests;

/// The volume declarations a blueprint's references are checked against, and the
/// `managed-local` declarations an apply may add.
pub(crate) trait LocalVolumes: Send + Sync {
    /// Whether a volume of that name is declared.
    fn is_declared(&self, volume: &str) -> bool;
    /// Whether a blueprint may name `volume` with `requested` access.
    fn check_reference(&self, volume: &str, requested: Option<Access>)
    -> Result<(), VolumeProblem>;
    /// Whether `volume` could be declared `managed-local`: a volume already
    /// declared passes; a name differing only in letter case from a declared
    /// volume or a directory under the managed root does not.
    fn check_managed(&self, volume: &str) -> Result<(), String>;
    /// Declares `volume` `managed-local`, read-write, with no size limit, unless
    /// it is declared already. Returns whether it was added.
    fn declare_managed(&self, volume: &str) -> Result<bool, String>;
    /// Removes a `managed-local` declaration [`Self::declare_managed`] just added.
    fn withdraw_managed(&self, volume: &str);
}

/// Why a blueprint's reference to a volume cannot be served.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum VolumeProblem {
    /// No volume of that name is declared.
    Undeclared(String),
    /// The blueprint asks for more access than the declaration allows.
    AccessExceeds(String),
}

/// A blueprint as runs read it: parsed, and the tag of the version it was applied
/// as.
#[derive(Debug, Clone)]
pub(crate) struct AppliedBlueprint {
    pub(crate) blueprint: Arc<Blueprint>,
    pub(crate) version_tag: String,
}

/// A blueprint applied from the file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct LocalApplied {
    pub(crate) name: String,
    /// `true` when no blueprint was applied before.
    pub(crate) created: bool,
    /// Volumes this apply declared as `managed-local`, by name.
    pub(crate) declared_volumes: Vec<String>,
}

/// Why a blueprint was refused, in the terms the server's registration refuses it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub(crate) struct LocalApplyError {
    /// Such as `parse_error`, `invalid_filter`, `undeclared_volume`, or
    /// `name_changed`.
    pub(crate) code: &'static str,
    pub(crate) message: String,
    pub(crate) diagnostics: Vec<LocalDiagnostic>,
}

/// Where in the file a refusal points.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub(crate) struct LocalDiagnostic {
    pub(crate) path: Option<YamlPath>,
    /// 1-based, when the parser reported one.
    pub(crate) line: Option<usize>,
    pub(crate) col: Option<usize>,
    pub(crate) message: String,
}

impl std::fmt::Display for LocalApplyError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.message)
    }
}

impl std::error::Error for LocalApplyError {}

impl LocalApplyError {
    fn at(code: &'static str, message: String, path: Option<YamlPath>) -> Self {
        Self {
            code,
            diagnostics: vec![LocalDiagnostic {
                path,
                line: None,
                col: None,
                message: message.clone(),
            }],
            message,
        }
    }
}

/// The blueprint the host serves, and what applying one checks against.
pub(crate) struct Blueprints {
    /// The name the blueprint had at start, which every apply must keep.
    name: String,
    volumes: Arc<dyn LocalVolumes>,
    secret_store: Option<Arc<dyn SecretStore>>,
    package_store: PackageStore,
    /// Held for writing across the volume declarations and the store, so no run
    /// reads the blueprint before its volumes are declared.
    current: RwLock<Option<AppliedBlueprint>>,
}

impl Blueprints {
    pub(crate) fn new(
        name: String,
        volumes: Arc<dyn LocalVolumes>,
        secret_store: Option<Arc<dyn SecretStore>>,
        package_store: PackageStore,
    ) -> Self {
        Self {
            name,
            volumes,
            secret_store,
            package_store,
            current: RwLock::new(None),
        }
    }

    /// The blueprint in force and its version tag, in one lookup; `None` before
    /// the first apply. A session reads it again for every run, so its next run
    /// gets the blueprint the last apply stored.
    pub(crate) async fn for_run(&self) -> Option<AppliedBlueprint> {
        self.current.read().await.clone()
    }

    /// What [`Self::apply`] would refuse, without changing anything: the YAML
    /// parsed and checked as registration checks it, except that a volume the
    /// host does not declare passes when its name can be a managed volume.
    pub(crate) async fn check(&self, yaml: &str) -> Result<Blueprint, LocalApplyError> {
        let blueprint = parse_blueprint(yaml)?;
        if blueprint.name != self.name {
            let message = format!(
                "the blueprint's `name:` changed from `{}` to `{}`; the playground serves `{}` \
                 until it restarts, so put the name back or restart it",
                self.name, blueprint.name, self.name
            );
            return Err(LocalApplyError::at(
                "name_changed",
                message,
                Some(yaml_path!["name"]),
            ));
        }
        self.check_local_volumes(&blueprint)?;
        verify_secrets(self.secret_store.clone(), &blueprint).await?;
        verify_packages(&self.package_store, &blueprint)?;
        Ok(blueprint)
    }

    /// Applies `yaml`, replacing the blueprint in force, with `version_tag` stored
    /// beside it in the same write: every run under this blueprint records that
    /// tag. A newly named volume is declared `managed-local` first.
    pub(crate) async fn apply(
        &self,
        yaml: &str,
        version_tag: &str,
    ) -> Result<LocalApplied, LocalApplyError> {
        let blueprint = self.check(yaml).await?;
        let name = blueprint.name.clone();
        let references = volume_references(&blueprint);
        let mut current = self.current.write().await;
        self.check_new_volumes(&references)?;
        let declared_volumes = self.declare_local_volumes(&references)?;
        let created = current.is_none();
        *current = Some(AppliedBlueprint {
            blueprint: Arc::new(blueprint),
            version_tag: version_tag.to_owned(),
        });
        Ok(LocalApplied {
            name,
            created,
            declared_volumes,
        })
    }

    /// Volume references as registration checks them, except that an undeclared
    /// volume passes when its name can be a managed volume's directory.
    fn check_local_volumes(&self, blueprint: &Blueprint) -> Result<(), LocalApplyError> {
        let references = volume_references(blueprint);
        self.check_new_volumes(&references)?;
        for reference in blueprint.vfs.named_references() {
            // A managed volume is read-write, so it takes any access.
            if !self.volumes.is_declared(reference.volume) && is_managed_name(reference.volume) {
                continue;
            }
            if let Err(problem) = self
                .volumes
                .check_reference(reference.volume, reference.access)
            {
                let (code, field, message) = match problem {
                    VolumeProblem::Undeclared(message) => ("undeclared_volume", "volume", message),
                    VolumeProblem::AccessExceeds(message) => {
                        ("volume_access_exceeded", "access", message)
                    }
                };
                return Err(LocalApplyError::at(
                    code,
                    message,
                    Some(reference.yaml_path(field)),
                ));
            }
        }
        Ok(())
    }

    /// A volume the host does not declare whose name could be a managed volume's
    /// but differs only in letter case from one it stores is refused; other names
    /// are left to the volume-reference check.
    fn check_new_volumes(&self, references: &[(String, YamlPath)]) -> Result<(), LocalApplyError> {
        for (volume, path) in references {
            if !is_managed_name(volume) {
                continue;
            }
            if let Err(message) = self.volumes.check_managed(volume) {
                return Err(LocalApplyError::at(
                    "volume_name_conflict",
                    message,
                    Some(path.clone()),
                ));
            }
        }
        Ok(())
    }

    /// Declares each named volume the host lacks; on a refusal, the volumes
    /// declared before it are withdrawn.
    fn declare_local_volumes(
        &self,
        references: &[(String, YamlPath)],
    ) -> Result<Vec<String>, LocalApplyError> {
        let mut declared = Vec::new();
        for (volume, path) in references {
            match self.volumes.declare_managed(volume) {
                Ok(true) => declared.push(volume.clone()),
                Ok(false) => {}
                Err(message) => {
                    for volume in &declared {
                        self.volumes.withdraw_managed(volume);
                    }
                    return Err(LocalApplyError::at(
                        "undeclared_volume",
                        message,
                        Some(path.clone()),
                    ));
                }
            }
        }
        Ok(declared)
    }
}

/// Each volume the blueprint names, with where it names it.
fn volume_references(blueprint: &Blueprint) -> Vec<(String, YamlPath)> {
    blueprint
        .vfs
        .named_references()
        .into_iter()
        .map(|reference| (reference.volume.to_owned(), reference.yaml_path("volume")))
        .collect()
}

/// Whether `name` can be a managed volume's directory name on every platform.
pub(crate) fn is_managed_name(name: &str) -> bool {
    let mut chars = name.chars();
    chars.next().is_some_and(|c| c.is_ascii_alphanumeric())
        && name.len() <= 64
        && chars.all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-'))
}

/// Parse YAML into a `Blueprint`, naming each failure as registration does.
fn parse_blueprint(yaml: &str) -> Result<Blueprint, LocalApplyError> {
    submilli_blueprint::parse(yaml).map_err(|err| {
        let code = match err {
            BlueprintError::Empty | BlueprintError::Parse(_) => "parse_error",
            BlueprintError::InvalidKind(_) => "invalid_kind",
            BlueprintError::InvalidName(_) => "invalid_name",
            BlueprintError::InvalidVfs(_) => "invalid_vfs",
            BlueprintError::InvalidPackages(_) => "invalid_packages",
            BlueprintError::InvalidSecrets(_) => "invalid_secrets",
            BlueprintError::InvalidVariables(_) => "invalid_variables",
            BlueprintError::InvalidAuthProxy(_) => "invalid_auth_proxy",
            BlueprintError::InvalidPermissions(_) => "invalid_permissions",
            BlueprintError::InvalidGit(_) => "invalid_git",
            BlueprintError::InvalidMcp(_) => "invalid_mcp",
            BlueprintError::InvalidLlm(_) => "invalid_llm",
            BlueprintError::InvalidEmbedding(_) => "invalid_embedding",
        };
        let diagnostics = err
            .fault()
            .map(|fault| LocalDiagnostic {
                path: fault.path.clone(),
                line: fault.location.map(|(line, _)| line),
                col: fault.location.map(|(_, col)| col),
                message: fault.message.clone(),
            })
            .into_iter()
            .collect();
        LocalApplyError {
            code,
            message: err.to_string(),
            diagnostics,
        }
    })
}

/// Verify that every declared secret resolves now, from the secret store.
/// Point-in-time only — see `submilli_blueprint::verify_secrets`.
async fn verify_secrets(
    store: Option<Arc<dyn SecretStore>>,
    blueprint: &Blueprint,
) -> Result<(), LocalApplyError> {
    let resolver = BlueprintSecretResolver::new(store);
    submilli_blueprint::verify_secrets(blueprint, &resolver)
        .await
        .map_err(|err| {
            let path = match &err {
                submilli_blueprint::AuthError::UndeclaredSecret(name)
                | submilli_blueprint::AuthError::MissingSecret(name) => {
                    Some(yaml_path!["secrets", name])
                }
                submilli_blueprint::AuthError::Other(_) => None,
            };
            LocalApplyError::at(
                "unresolved_secret",
                format!("secret check failed: {err}"),
                path,
            )
        })
}

/// The packages' closure is installed, each package's required rules are present,
/// and every filter names fields its capability reports.
fn verify_packages(store: &PackageStore, blueprint: &Blueprint) -> Result<(), LocalApplyError> {
    use submilli_build::PackageStoreError;
    use submilli_shared::blueprint_validation::{self, PackageValidationError};

    blueprint_validation::validate_packages(blueprint, store).map_err(|error| {
        if let PackageValidationError::InvalidFilter(problem) = &error {
            return LocalApplyError::at(
                "invalid_filter",
                problem.message.clone(),
                problem.path.clone(),
            );
        }
        let (code, detail) = match &error {
            PackageValidationError::Store(
                PackageStoreError::MissingPackage { name, .. }
                | PackageStoreError::MissingDependency { name, .. },
            ) => (
                "package_missing",
                format!(
                    "package `{name}` is not installed; install it with `submilli server \
                     packages install <org/repo> {name}`"
                ),
            ),
            _ => ("invalid_packages", error.to_string()),
        };
        LocalApplyError::at(
            code,
            format!("package check failed: {detail}"),
            Some(yaml_path!["packages"]),
        )
    })
}
