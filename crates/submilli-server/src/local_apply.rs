//! Applying a blueprint from an operator-trusted local file, as the playground does
//! when the developer saves it.
//!
//! The blueprint is validated exactly as `PUT /v1/blueprints/{name}` validates it
//! (parse, volume references, secrets, packages and the filter-field check against
//! capability metadata), then registered with an opaque version tag that runs read
//! in the same lookup as the blueprint and record. Unlike registration over HTTP, a
//! volume the blueprint names that the server does not declare is declared on the
//! spot as `managed-local` under the managed volume root, so a new volume needs no
//! restart. That is the only path that adds to the volume table: it trusts the file
//! the way an operator's config is trusted.

use serde::Serialize;
use submilli_blueprint::{Blueprint, YamlPath};

use crate::app::AppState;
use crate::blueprint::StoredBlueprint;
use crate::config::{SizeLimit, is_managed_name};
use crate::handlers::blueprint::{
    ErrorResponse, check_volume_references, parse_blueprint, permissions_last_preserving_comments,
    verify_packages, verify_secrets,
};

/// A blueprint registered from a local file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LocalApplied {
    pub name: String,
    /// `true` when no blueprint of that name was registered before.
    pub created: bool,
    /// Volumes this apply declared as `managed-local`, by name.
    pub declared_volumes: Vec<String>,
}

/// Why a local blueprint was refused, in the terms registration refuses it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct LocalApplyError {
    /// The code `PUT /v1/blueprints/{name}` answers with, such as `parse_error`,
    /// `invalid_filter`, or `undeclared_volume`; `store_failed` when the
    /// blueprint store could not be written.
    pub code: &'static str,
    pub message: String,
    pub diagnostics: Vec<LocalDiagnostic>,
}

/// Where in the file a refusal points.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct LocalDiagnostic {
    pub path: Option<YamlPath>,
    /// 1-based, when the parser reported one.
    pub line: Option<usize>,
    pub col: Option<usize>,
    pub message: String,
}

impl std::fmt::Display for LocalApplyError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.message)
    }
}

impl std::error::Error for LocalApplyError {}

impl From<ErrorResponse> for LocalApplyError {
    fn from(response: ErrorResponse) -> Self {
        Self {
            code: response.error,
            message: response.message,
            diagnostics: response
                .diagnostics
                .unwrap_or_default()
                .into_iter()
                .map(|diagnostic| LocalDiagnostic {
                    path: diagnostic.path,
                    line: diagnostic.line,
                    col: diagnostic.col,
                    message: diagnostic.message,
                })
                .collect(),
        }
    }
}

fn refused<T>(
    result: Result<T, (axum::http::StatusCode, axum::Json<ErrorResponse>)>,
) -> Result<T, LocalApplyError> {
    result.map_err(|(_, axum::Json(response))| response.into())
}

impl AppState {
    /// What [`Self::apply_local_blueprint`] would refuse, without changing anything:
    /// the YAML parsed and checked as registration checks it, except that a volume
    /// the server does not declare passes when its name can be a managed volume.
    pub async fn check_local_blueprint(&self, yaml: &str) -> Result<Blueprint, LocalApplyError> {
        let blueprint = refused(parse_blueprint(yaml))?;
        self.check_local_volumes(&blueprint)?;
        refused(verify_secrets(self, &blueprint).await)?;
        refused(verify_packages(self, &blueprint))?;
        Ok(blueprint)
    }

    /// Registers `yaml` from an operator-trusted local file, create-or-replace,
    /// with `version_tag` stored beside it in the same write: every run decided
    /// under this blueprint records that tag. Validation is registration's; a
    /// newly named volume is declared `managed-local` (no size limit) under the
    /// managed volume root first. Afterwards the blueprint's MCP catalog and
    /// prepared packages are evicted, so the next run discovers and prepares them
    /// afresh.
    pub async fn apply_local_blueprint(
        &self,
        yaml: &str,
        version_tag: &str,
    ) -> Result<LocalApplied, LocalApplyError> {
        let blueprint = self.check_local_blueprint(yaml).await?;
        let name = blueprint.name.clone();
        let references = volume_references(&blueprint);
        let stored = StoredBlueprint::new(blueprint, permissions_last_preserving_comments(yaml));
        let (created, declared_volumes) = {
            // Held across the declarations and the store write: no run reads the
            // blueprint before its volumes are declared, and no other local apply,
            // nor a registration (which checks volume references again under this
            // lock), relies on a declaration this one may withdraw.
            let mut tags = self.blueprint_tags_for_write().await;
            self.check_new_volumes(&references)?;
            // Before the store write, so a refused declaration leaves the version in
            // force, under its own tag.
            let declared = self.declare_local_volumes(&references)?;
            let created = match self.blueprints().upsert_yaml(stored).await {
                Ok(created) => created,
                Err(error) => {
                    // No stored blueprint names them, so a refused write declares
                    // nothing.
                    self.withdraw_local_volumes(&declared);
                    return Err(LocalApplyError {
                        code: "store_failed",
                        message: format!("the blueprint could not be stored: {error}"),
                        diagnostics: Vec::new(),
                    });
                }
            };
            tags.insert(name.clone(), version_tag.to_owned());
            (created, declared)
        };
        self.evict_mcp_catalog(&name);
        self.evict_prepared_packages(&name);
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
        let mut table = self.session_manager().volumes();
        for reference in blueprint.vfs.named_references() {
            if !table.contains_key(reference.volume) && is_managed_name(reference.volume) {
                // Stands in for the declaration an apply would make.
                table.insert(
                    reference.volume.to_owned(),
                    crate::config::VolumeSpec::managed(SizeLimit::Unlimited),
                );
            }
        }
        check_volume_references(blueprint, &table).map_err(|problem| LocalApplyError {
            code: problem.code,
            message: problem.message.clone(),
            diagnostics: vec![LocalDiagnostic {
                path: Some(problem.path),
                line: None,
                col: None,
                message: problem.message,
            }],
        })
    }

    /// A volume the server does not declare whose name could be a managed volume's
    /// but differs only in letter case from one it stores is refused; other names
    /// are left to the volume-reference check.
    fn check_new_volumes(&self, references: &[(String, YamlPath)]) -> Result<(), LocalApplyError> {
        let registry = self.session_manager().volume_registry();
        for (volume, path) in references {
            if !is_managed_name(volume) {
                continue;
            }
            if let Err(error) = registry.check_managed(volume) {
                return Err(volume_refusal("volume_name_conflict", &error, path));
            }
        }
        Ok(())
    }

    /// Declares each named volume the server lacks. Runs after
    /// [`Self::check_new_volumes`] under the same tag lock, so a refusal here
    /// means only a name registration's check let through; the volumes declared
    /// before it are withdrawn.
    fn declare_local_volumes(
        &self,
        references: &[(String, YamlPath)],
    ) -> Result<Vec<String>, LocalApplyError> {
        let registry = self.session_manager().volume_registry();
        let mut declared = Vec::new();
        for (volume, path) in references {
            match registry.declare_managed(volume, SizeLimit::Unlimited) {
                Ok(true) => declared.push(volume.clone()),
                Ok(false) => {}
                Err(error) => {
                    self.withdraw_local_volumes(&declared);
                    return Err(volume_refusal("undeclared_volume", &error, path));
                }
            }
        }
        Ok(declared)
    }

    /// Withdraws volumes [`Self::declare_local_volumes`] just declared for a
    /// blueprint that was not stored. Called under the tag lock that covered the
    /// declaration, so no stored blueprint references them.
    fn withdraw_local_volumes(&self, declared: &[String]) {
        let registry = self.session_manager().volume_registry();
        for volume in declared {
            registry.withdraw_managed(volume);
        }
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

fn volume_refusal(
    code: &'static str,
    error: &crate::volumes::DeclareError,
    path: &YamlPath,
) -> LocalApplyError {
    LocalApplyError {
        code,
        message: error.to_string(),
        diagnostics: vec![LocalDiagnostic {
            path: Some(path.clone()),
            line: None,
            col: None,
            message: error.to_string(),
        }],
    }
}
