use axum::{
    Json,
    extract::{Path, State},
    http::StatusCode,
};
use serde::{Deserialize, Serialize};
use submilli_blueprint::{self, Blueprint, BlueprintError, SecretSource, YamlPath, yaml_path};
use submilli_shared::BlueprintSecretResolver;

use crate::app::AppState;
use crate::blueprint::{StoreError, StoredBlueprint};
use crate::config::VolumeTable;
use crate::volumes::ReferenceError;

#[derive(Debug, Deserialize)]
pub struct AddRequest {
    pub yaml: String,
}

#[derive(Debug, Serialize)]
pub struct AddResponse {
    pub name: String,
}

/// One editor-anchorable diagnostic: the YAML path (segments — string keys and
/// numeric indexes) and/or the 1-based line/column, always with the leaf
/// message. Validation is fail-fast today, so responses carry 0 or 1 of these;
/// the array shape leaves room for collect-all linting.
#[derive(Debug, Serialize)]
pub struct Diagnostic {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub path: Option<YamlPath>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub line: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub col: Option<usize>,
    pub message: String,
}

#[derive(Debug, Serialize)]
pub struct ErrorResponse {
    pub error: &'static str,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub diagnostics: Option<Vec<Diagnostic>>,
}

impl ErrorResponse {
    fn new(error: &'static str, message: String) -> Self {
        ErrorResponse {
            error,
            message,
            name: None,
            diagnostics: None,
        }
    }

    fn named(error: &'static str, message: String, name: String) -> Self {
        ErrorResponse {
            name: Some(name),
            ..ErrorResponse::new(error, message)
        }
    }

    fn diagnostic(mut self, path: Option<YamlPath>, message: String) -> Self {
        self.diagnostics = Some(vec![Diagnostic {
            path,
            line: None,
            col: None,
            message,
        }]);
        self
    }
}

/// Parse YAML into a `Blueprint`, mapping each failure to its HTTP response.
fn parse_blueprint(yaml: &str) -> Result<Blueprint, (StatusCode, Json<ErrorResponse>)> {
    submilli_blueprint::parse(yaml).map_err(|err| {
        let error = match err {
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
        };
        let diagnostics = err.fault().map(|fault| {
            vec![Diagnostic {
                path: fault.path.clone(),
                line: fault.location.map(|(line, _)| line),
                col: fault.location.map(|(_, col)| col),
                message: fault.message.clone(),
            }]
        });
        (
            StatusCode::BAD_REQUEST,
            Json(ErrorResponse {
                error,
                message: err.to_string(),
                name: None,
                diagnostics,
            }),
        )
    })
}

/// Verify, at apply/add time, that every declared secret currently resolves on
/// this server (present in the secret store).
/// Point-in-time only — see `submilli_blueprint::verify_secrets`.
async fn verify_secrets(
    state: &AppState,
    blueprint: &Blueprint,
) -> Result<(), (StatusCode, Json<ErrorResponse>)> {
    let resolver = BlueprintSecretResolver::new(state.secret_store().cloned());
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
            let message = format!("secret check failed: {err}");
            (
                StatusCode::BAD_REQUEST,
                Json(
                    ErrorResponse::named(
                        "unresolved_secret",
                        message.clone(),
                        blueprint.name.clone(),
                    )
                    .diagnostic(path, message),
                ),
            )
        })
}

/// A blueprint's reference to a named volume that no session could mount, with
/// where in the blueprint it sits.
#[derive(Debug)]
pub(crate) struct VolumeReferenceProblem {
    pub code: &'static str,
    pub message: String,
    pub path: submilli_blueprint::YamlPath,
}

/// Reject a blueprint naming a volume the operator has not declared, or asking
/// for more access than the declaration allows, so the store never holds one no
/// session could mount as written. Checks the `named` root and every mount.
pub(crate) fn check_volume_references(
    blueprint: &Blueprint,
    volumes: &VolumeTable,
) -> Result<(), VolumeReferenceProblem> {
    for reference in blueprint.vfs.named_references() {
        let Err(err) = crate::volumes::check_reference(volumes, reference.volume, reference.access)
        else {
            continue;
        };
        let (code, field) = match err {
            ReferenceError::Undeclared { .. } => ("undeclared_volume", "volume"),
            ReferenceError::AccessExceeds { .. } => ("volume_access_exceeded", "access"),
        };
        return Err(VolumeReferenceProblem {
            code,
            message: err.to_string(),
            path: reference.yaml_path(field),
        });
    }
    Ok(())
}

fn reject_unusable_volume_reference(
    blueprint: &Blueprint,
    volumes: &VolumeTable,
) -> Result<(), (StatusCode, Json<ErrorResponse>)> {
    check_volume_references(blueprint, volumes).map_err(|problem| {
        (
            StatusCode::BAD_REQUEST,
            Json(
                ErrorResponse::named(
                    problem.code,
                    problem.message.clone(),
                    blueprint.name.clone(),
                )
                .diagnostic(Some(problem.path), problem.message),
            ),
        )
    })
}

fn verify_packages(
    state: &AppState,
    blueprint: &Blueprint,
) -> Result<(), (StatusCode, Json<ErrorResponse>)> {
    use submilli_build::{
        PackageStoreError,
        blueprint_validation::{self, PackageValidationError},
    };

    blueprint_validation::validate_packages(blueprint, state.package_store()).map_err(|error| {
        if let PackageValidationError::InvalidFilter(problem) = &error {
            return (StatusCode::BAD_REQUEST, Json(
                ErrorResponse::named("invalid_filter", problem.message.clone(), blueprint.name.clone())
                    .diagnostic(problem.path.clone(), problem.message.clone()),
            ));
        }
        let (code, detail) = match &error {
            PackageValidationError::Store(
                PackageStoreError::MissingPackage { name, .. }
                | PackageStoreError::MissingDependency { name, .. },
            ) => (
                "package_missing",
                format!("package `{name}` is not installed; install it with `submilli server packages install <org/repo> {name}`"),
            ),
            _ => ("invalid_packages", error.to_string()),
        };
        let message = format!("package check failed: {detail}");
        (StatusCode::BAD_REQUEST, Json(
            ErrorResponse::named(code, message.clone(), blueprint.name.clone())
                .diagnostic(Some(yaml_path!["packages"]), message),
        ))
    })
}

pub async fn add(
    State(state): State<AppState>,
    Json(req): Json<AddRequest>,
) -> Result<(StatusCode, Json<AddResponse>), (StatusCode, Json<ErrorResponse>)> {
    let blueprint = parse_blueprint(&req.yaml)?;
    reject_unusable_volume_reference(&blueprint, state.session_manager().volumes())?;
    verify_secrets(&state, &blueprint).await?;
    verify_packages(&state, &blueprint)?;
    let name = blueprint.name.clone();
    crate::audit::annotate(serde_json::json!({"name": name,
        "new_hash": crate::audit::blueprint_hash(&blueprint)}));
    let owner = state.clone();
    let created_name = name.clone();
    let stored = StoredBlueprint::new(blueprint, permissions_last_preserving_comments(&req.yaml));
    owned_mutation(&state, async move {
        owner
            .blueprints()
            .add_yaml(stored)
            .await
            .map_err(|error| match error {
                StoreError::AlreadyExists => (
                    StatusCode::CONFLICT,
                    Json(ErrorResponse::named(
                        "already_exists",
                        format!("blueprint '{created_name}' is already registered"),
                        created_name,
                    )),
                ),
                error => store_error(error),
            })
    })
    .await?;
    Ok((StatusCode::OK, Json(AddResponse { name })))
}

#[derive(Debug, Serialize)]
pub struct ApplyResponse {
    pub name: String,
    /// `true` if the blueprint was newly registered, `false` if it replaced one.
    pub created: bool,
}

/// `PUT /v1/blueprints/{name}` — create-or-replace. The name in the path must
/// match the `name:` in the YAML body; a mismatch is rejected so the URL and
/// the file can't silently disagree about which blueprint is being written.
pub async fn apply(
    State(state): State<AppState>,
    Path(name): Path<String>,
    Json(req): Json<AddRequest>,
) -> Result<(StatusCode, Json<ApplyResponse>), (StatusCode, Json<ErrorResponse>)> {
    let blueprint = parse_blueprint(&req.yaml)?;
    crate::audit::annotate(
        serde_json::json!({"new_hash": crate::audit::blueprint_hash(&blueprint)}),
    );
    reject_unusable_volume_reference(&blueprint, state.session_manager().volumes())?;
    verify_secrets(&state, &blueprint).await?;
    verify_packages(&state, &blueprint)?;
    let previous = state.blueprints().get(&name).await.map_err(store_error)?;
    crate::audit::annotate(serde_json::json!({"name": name,
        "old_hash": previous.as_ref().map(crate::audit::blueprint_hash)}));
    if blueprint.name != name {
        let message = format!(
            "blueprint name '{}' in the file does not match '{name}' in the request path",
            blueprint.name
        );
        return Err((
            StatusCode::BAD_REQUEST,
            Json(
                ErrorResponse::named("name_mismatch", message.clone(), blueprint.name)
                    .diagnostic(Some(yaml_path!["name"]), message),
            ),
        ));
    }
    let stored = StoredBlueprint::new(blueprint, permissions_last_preserving_comments(&req.yaml));
    let owner = state.clone();
    let changed_name = name.clone();
    let created = owned_mutation(&state, async move {
        let created = owner.blueprints().upsert_yaml(stored).await.map_err(store_error)?;
        owner.evict_mcp_catalog(&changed_name);
        // Another apply may have committed since the audit read above.
        owner.evict_prepared_packages(&changed_name);
        crate::audit::annotate(
            serde_json::json!({"event": if created { "blueprint_created" } else { "blueprint_replaced" }}),
        );
        crate::metrics::blueprint_apply(created);
        Ok(created)
    }).await?;
    Ok((StatusCode::OK, Json(ApplyResponse { name, created })))
}

#[derive(Debug, Serialize)]
pub struct ShowResponse {
    pub name: String,
    pub yaml: String,
}

/// `GET /v1/blueprints/{name}` — echo a registered blueprint as YAML.
pub async fn show(
    State(state): State<AppState>,
    Path(name): Path<String>,
) -> Result<(StatusCode, Json<ShowResponse>), (StatusCode, Json<ErrorResponse>)> {
    match state
        .blueprints()
        .get_yaml(&name)
        .await
        .map_err(store_error)?
    {
        Some(yaml) => Ok((StatusCode::OK, Json(ShowResponse { yaml, name }))),
        None => Err(not_found(name)),
    }
}

#[derive(Debug, Serialize)]
pub struct PromptResponse {
    pub name: String,
    pub prompt: String,
    /// Descriptions for the discovery tools, keyed by role — the same text the
    /// MCP surface publishes for its equivalents.
    pub tools: ToolDescriptions,
}

#[derive(Debug, Serialize)]
pub struct ToolDescriptions {
    pub search: &'static str,
    pub docs: &'static str,
    /// For harnesses exposing built-ins as one tool (name omitted ⇒ list).
    pub builtins: &'static str,
    pub builtins_list: &'static str,
    pub builtins_docs: &'static str,
    /// Reads the console of a *successful* run, which the execute result omits.
    pub last_run: &'static str,
}

/// `GET /v1/blueprints/{name}/prompt` — the canonical LLM-facing prompt
/// (`llm-prompt.md`) with this blueprint's policy resolved into its
/// placeholders. The same text the MCP surface publishes as the `execute` tool
/// description, so a REST harness teaches its model exactly what an MCP client
/// is taught — one source of truth for both.
pub async fn prompt(
    State(state): State<AppState>,
    Path(name): Path<String>,
) -> Result<(StatusCode, Json<PromptResponse>), (StatusCode, Json<ErrorResponse>)> {
    match state.blueprints().get(&name).await.map_err(store_error)? {
        Some(blueprint) => Ok((
            StatusCode::OK,
            Json(PromptResponse {
                prompt: submilli_shared::prompt::execute_tool_description(
                    &blueprint,
                    submilli_shared::prompt::PromptSurface::Rest,
                ),
                name,
                tools: ToolDescriptions {
                    search: submilli_shared::prompt::tools::PACKAGES_SEARCH,
                    docs: submilli_shared::prompt::tools::PACKAGES_DOCS,
                    builtins: submilli_shared::prompt::tools::BUILTINS_MERGED,
                    builtins_list: submilli_shared::prompt::tools::BUILTINS_LIST,
                    builtins_docs: submilli_shared::prompt::tools::BUILTINS_DOCS,
                    last_run: submilli_shared::prompt::tools::LAST_RUN,
                },
            }),
        )),
        None => Err(not_found(name)),
    }
}

/// `DELETE /v1/blueprints/{name}` — unregister a blueprint.
pub async fn remove(
    State(state): State<AppState>,
    Path(name): Path<String>,
) -> Result<(StatusCode, Json<AddResponse>), (StatusCode, Json<ErrorResponse>)> {
    let previous = match state.blueprints().get(&name).await {
        Ok(previous) => previous,
        Err(error) => {
            crate::blueprint::store_failure_message(error);
            None
        }
    };
    crate::audit::annotate(serde_json::json!({"name": name,
        "old_hash": previous.as_ref().map(crate::audit::blueprint_hash)}));
    let owner = state.clone();
    let changed_name = name.clone();
    owned_mutation(&state, async move {
        let removed = owner.blueprints().remove(&changed_name).await.map_err(store_error)?;
        if !removed {
            return Err(not_found(changed_name));
        }
        owner.wipe_blueprint_sessions(&changed_name).await;
        owner.evict_mcp_service(&changed_name);
        owner.evict_mcp_catalog(&changed_name);
        owner.evict_prepared_packages(&changed_name);
        Ok(())
    }).await?;
    Ok((StatusCode::OK, Json(AddResponse { name })))
}

async fn owned_mutation<T: Send + 'static>(
    state: &AppState,
    work: impl std::future::Future<Output = Result<T, (StatusCode, Json<ErrorResponse>)>>
    + Send
    + 'static,
) -> Result<T, (StatusCode, Json<ErrorResponse>)> {
    state
        .blueprint_mutation(async move {
            let audit = crate::audit::mutation_owner();
            let result = work.await;
            if let Some(audit) = audit {
                audit.finish(match &result {
                    Ok(_) => StatusCode::OK,
                    Err(error) => error.0,
                });
            }
            result
        })
        .await
        .map_err(store_error)?
}

fn store_error(error: StoreError) -> (StatusCode, Json<ErrorResponse>) {
    internal_error(crate::blueprint::store_failure_message(error).into())
}

fn internal_error(message: String) -> (StatusCode, Json<ErrorResponse>) {
    (
        StatusCode::INTERNAL_SERVER_ERROR,
        Json(ErrorResponse::new("internal_error", message)),
    )
}

fn not_found(name: String) -> (StatusCode, Json<ErrorResponse>) {
    (
        StatusCode::NOT_FOUND,
        Json(ErrorResponse::named(
            "not_found",
            format!("blueprint '{name}' is not registered"),
            name,
        )),
    )
}

#[derive(Debug, Serialize)]
pub struct SecretSummary {
    pub name: String,
    pub source: &'static str,
    /// For `store:` declarations, the store key the value is read from — which
    /// is independent of the declaration name, so the console needs it to tell
    /// whether the secret is actually set.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub store_key: Option<String>,
}

/// One list entry: the blueprint's name plus the parsed shape the console's
/// list page renders as columns — names (not just counts) where the UI shows
/// chips, counts where it shows totals.
#[derive(Debug, Serialize)]
pub struct BlueprintSummary {
    pub name: String,
    pub vfs_mode: &'static str,
    pub idle_timeout_secs: u64,
    pub packages: Vec<String>,
    pub secrets: Vec<SecretSummary>,
    pub variables: Vec<String>,
    pub mcp_servers: Vec<String>,
    pub auth_proxy_hosts: Vec<String>,
    pub default_action: Option<submilli_blueprint::DefaultAction>,
    pub caller_count: usize,
    pub rule_count: usize,
    /// Distinct capability names across all callers' rules.
    pub capability_count: usize,
}

impl From<&Blueprint> for BlueprintSummary {
    fn from(b: &Blueprint) -> Self {
        let secrets = b
            .secrets
            .iter()
            .map(|(name, source)| SecretSummary {
                name: name.clone(),
                source: source.kind(),
                store_key: match source {
                    SecretSource::Store(key) => Some(key.clone()),
                    _ => None,
                },
            })
            .collect();
        let capabilities: std::collections::BTreeSet<&str> = b
            .permissions
            .values()
            .flatten()
            .map(|rule| rule.capability.as_str())
            .collect();
        BlueprintSummary {
            name: b.name.clone(),
            vfs_mode: b.vfs.mode_str(),
            idle_timeout_secs: b.idle_timeout.as_secs(),
            packages: b.packages.iter().cloned().collect(),
            secrets,
            variables: b.variables.keys().cloned().collect(),
            mcp_servers: b.mcp.keys().cloned().collect(),
            auth_proxy_hosts: b.auth_proxy.iter().map(|r| r.host.clone()).collect(),
            default_action: b.default_action,
            caller_count: b.permissions.len(),
            rule_count: b.permissions.values().map(Vec::len).sum(),
            capability_count: capabilities.len(),
        }
    }
}

#[derive(Debug, Serialize)]
pub struct ListResponse {
    pub blueprints: Vec<BlueprintSummary>,
}

pub async fn list(
    State(state): State<AppState>,
) -> Result<Json<ListResponse>, (StatusCode, Json<ErrorResponse>)> {
    let blueprints = state
        .blueprints()
        .list_blueprints()
        .await
        .map_err(store_error)?
        .iter()
        .map(BlueprintSummary::from)
        .collect();
    Ok(Json(ListResponse { blueprints }))
}

pub(crate) fn permissions_last_preserving_comments(yaml: &str) -> String {
    let lines = split_lines_preserving_endings(yaml);
    let Some(permissions_start) = lines
        .iter()
        .position(|line| top_level_key(line) == Some("permissions"))
    else {
        return yaml.to_string();
    };

    let permissions_end = lines
        .iter()
        .enumerate()
        .skip(permissions_start + 1)
        .find_map(|(idx, line)| top_level_key(line).map(|_| idx))
        .unwrap_or(lines.len());
    if permissions_end == lines.len() {
        return yaml.to_string();
    }

    let block_start = leading_comment_start(&lines, permissions_start);
    let mut reordered = Vec::with_capacity(lines.len());
    reordered.extend_from_slice(&lines[..block_start]);
    reordered.extend_from_slice(&lines[permissions_end..]);
    ensure_trailing_newline(&mut reordered);
    if !reordered.last().is_some_and(|line| line.trim().is_empty()) {
        reordered.push("\n");
    }
    reordered.extend_from_slice(&lines[block_start..permissions_end]);
    reordered.concat()
}

fn split_lines_preserving_endings(yaml: &str) -> Vec<&str> {
    if yaml.is_empty() {
        Vec::new()
    } else {
        yaml.split_inclusive('\n')
            .chain(yaml.ends_with('\n').then_some(""))
            .filter(|line| !line.is_empty())
            .collect()
    }
}

fn leading_comment_start(lines: &[&str], start: usize) -> usize {
    let mut block_start = start;
    while block_start > 0 && is_blank_or_top_level_comment(lines[block_start - 1]) {
        block_start -= 1;
    }
    block_start
}

fn is_blank_or_top_level_comment(line: &str) -> bool {
    let trimmed = line.trim();
    trimmed.is_empty() || line.starts_with('#')
}

fn ensure_trailing_newline(lines: &mut Vec<&str>) {
    if let Some(last) = lines.last()
        && !last.ends_with('\n')
    {
        lines.push("\n");
    }
}

fn top_level_key(line: &str) -> Option<&str> {
    let first = line.chars().next()?;
    if first.is_whitespace() || first == '#' {
        return None;
    }
    let (key, _) = line.split_once(':')?;
    let key = key.trim();
    (!key.is_empty()).then_some(key)
}

#[cfg(test)]
mod tests {
    use super::permissions_last_preserving_comments;

    #[test]
    fn keeps_permissions_last_and_preserves_comments() {
        let yaml = "\
name: demo

# operator policy
permissions:
  # main script rules
  main:
    - capability: fs.read
      action: allow

mcp:
  linear:
    # endpoint comment
    url: https://mcp.linear.app/mcp
";

        let updated = permissions_last_preserving_comments(yaml);
        assert!(updated.contains("# operator policy\npermissions:"));
        assert!(updated.contains("# endpoint comment"));
        assert!(
            updated.rfind("permissions:").unwrap() > updated.rfind("mcp:").unwrap(),
            "{updated}"
        );
    }
}
