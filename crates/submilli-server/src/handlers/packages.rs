//! REST package- and built-in-discovery endpoints, mirroring the MCP tools.
//! Each is nested under the blueprint it answers for, the way an MCP tool is
//! scoped by the blueprint its endpoint names, and 404s on a blueprint that is
//! not registered:
//!
//! * `GET /v1/blueprints/{blueprint}/packages/search?q=<query>` — matching
//!   packages (empty query = all)
//! * `GET /v1/blueprints/{blueprint}/packages/docs?name=<name>` — declarations
//!   + description for one
//! * `GET /v1/blueprints/{blueprint}/builtins` — the built-in catalog
//!   (`{ types, namespaces }`)
//! * `GET /v1/blueprints/{blueprint}/builtins/docs?name=<name>[&name=<name>…]`
//!   — `.d.ts` declarations for the built-ins named
//!
//! The response bodies come from `crate::packages` (shared with the MCP tools);
//! the REST layer only adds an HTTP status code. Names carry `:` / `@` / `/`, so
//! they ride in the query string rather than a path segment.

use axum::Json;
use axum::extract::{Path, Query, RawQuery, State};
use axum::http::{StatusCode, header};
use axum::response::IntoResponse;
use serde::{Deserialize, Serialize};

use submilli_build::{
    DriverError, FetchErrorKind, GithubSource, InstallError, InstallPreparation, Lockfile,
    PackageName, PackageSource, ResolveError, deny_warnings_from_env, load_manifest,
    resolve_github_closure, warning_denial_message,
};
use submilli_shared::github;
use submilli_shared::github::{
    GithubAuth, GithubError, GithubRepoFetcher, GithubToken, TokenSource,
};

use crate::app::AppState;
use crate::compiler_thread;
use crate::packages::{self, DocLookup};

#[derive(Debug, Serialize)]
pub struct InstalledPackage {
    pub name: String,
    pub version: String,
    pub description: String,
    /// The store root this package resolves from.
    pub root: String,
    /// Whether that root is the server's own store (installs and uninstalls
    /// through this API act on it) rather than a read-only fallback.
    pub managed: bool,
}

#[derive(Debug, Serialize)]
pub struct InstalledResponse {
    /// Every root the server searches, in search order: its own store first.
    pub roots: Vec<String>,
    pub packages: Vec<InstalledPackage>,
}

/// `GET /v1/packages` — the registry packages the server can resolve, i.e. the
/// names a blueprint's `packages:` block may declare, each with the root it
/// comes from. An artifact that fails to load is skipped rather than failing
/// the listing.
pub async fn installed(State(state): State<AppState>) -> Json<InstalledResponse> {
    let store = state.package_store();
    let roots = store
        .roots()
        .map(|root| root.display().to_string())
        .collect();
    let packages = store
        .locate_all()
        .into_iter()
        .filter_map(|located| {
            let artifact = store.load(&located.name).ok()?;
            Some(InstalledPackage {
                name: located.name,
                version: artifact.metadata.package_version,
                description: artifact.metadata.description,
                root: located.root.display().to_string(),
                managed: store.owns(&located.root),
            })
        })
        .collect();
    Json(InstalledResponse { roots, packages })
}

#[derive(Debug, Serialize)]
pub struct UninstallResponse {
    pub name: String,
    /// Set when a read-only fallback root still holds a copy, which blueprints
    /// resolve from their next prepare on. The removal itself succeeded.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub still_available_from: Option<String>,
}

/// `DELETE /v1/packages/{name}` — remove an installed package from the server's
/// own store. A package that only exists in a read-only fallback root is
/// refused rather than deleted: the API manages the server's store, not the
/// CLI's. Blueprints that declare it keep working until their next prepare,
/// which then fails to load the package — or, when a fallback copy exists,
/// loads that one; the response says which. Same admin posture as the other
/// package routes.
pub async fn uninstall(
    State(state): State<AppState>,
    Path(name): Path<String>,
) -> Result<Json<UninstallResponse>, (StatusCode, Json<serde_json::Value>)> {
    let store = state.package_store();
    let error = |status: StatusCode, error: &str, message: String| {
        (
            status,
            Json(serde_json::json!({ "error": error, "message": message })),
        )
    };
    let dir = store
        .package_dir(&name)
        .map_err(|err| error(StatusCode::BAD_REQUEST, "invalid_name", err.to_string()))?;
    if !dir.is_dir() {
        if let Ok(Some(elsewhere)) = store.locate(&name) {
            return Err(error(
                StatusCode::CONFLICT,
                "not_managed",
                format!(
                    "package '{name}' is served from the read-only fallback store {}; this API \
                     only manages {}, so remove it from that store on the host",
                    elsewhere.root.display(),
                    store.root().display()
                ),
            ));
        }
        return Err(error(
            StatusCode::NOT_FOUND,
            "not_found",
            format!("package '{name}' is not installed"),
        ));
    }
    let removed = std::fs::remove_dir_all(&dir);
    // Drop every cached module set so the removal takes effect without a
    // restart, even a partial one that left the directory incomplete.
    // Blueprints that declare the package are not the only ones affected: it
    // may have been a transitive dependency, or the owned copy that shadowed a
    // fallback one.
    state.evict_all_prepared_packages();
    removed.map_err(|err| {
        error(
            StatusCode::INTERNAL_SERVER_ERROR,
            "internal_error",
            format!("removing package '{name}': {err}"),
        )
    })?;
    let still_available_from = store
        .locate(&name)
        .ok()
        .flatten()
        .map(|located| located.root.display().to_string());
    Ok(Json(UninstallResponse {
        name,
        still_available_from,
    }))
}

fn docs_response(name: &str, lookup: DocLookup) -> axum::response::Response {
    let status = match &lookup {
        DocLookup::Host { .. }
        | DocLookup::Mcp { .. }
        | DocLookup::Registry { .. }
        | DocLookup::Builtin { .. } => StatusCode::OK,
        DocLookup::McpUnknown | DocLookup::Unknown { .. } => StatusCode::NOT_FOUND,
    };
    crate::metrics::docs(status == StatusCode::OK);
    // Same preference as the MCP tool: registry and `@mcp/<server>` packages
    // have a readme, so they render as markdown — a model reads that far better
    // than the same text escaped inside a JSON string. Everything else is JSON.
    if let Some(markdown) = packages::docs_markdown(&lookup) {
        return (
            status,
            [(header::CONTENT_TYPE, "text/markdown; charset=utf-8")],
            markdown,
        )
            .into_response();
    }
    (status, Json(packages::docs_json(name, lookup))).into_response()
}

type BlueprintMiss = (StatusCode, Json<serde_json::Value>);

/// The blueprint a nested discovery route names, with its MCP catalog.
async fn registered_blueprint(
    state: &AppState,
    name: &str,
) -> Result<
    (
        submilli_blueprint::Blueprint,
        std::sync::Arc<crate::mcp::McpCatalog>,
    ),
    BlueprintMiss,
> {
    match state
        .blueprints()
        .get(name)
        .await
        .map_err(crate::blueprint::store_failure_response)?
    {
        Some(blueprint) => {
            let catalog = state.mcp_catalog(name, &blueprint).await;
            Ok((blueprint, catalog))
        }
        None => Err((
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({
                "error": "not_found",
                "message": format!("blueprint '{name}' is not registered"),
                "name": name,
            })),
        )),
    }
}

#[derive(Debug, Deserialize)]
pub struct BlueprintSearchParams {
    #[serde(default)]
    q: String,
}

/// `GET /v1/blueprints/{blueprint}/packages/search` — what the blueprint's MCP
/// `packages__search` tool answers.
pub async fn blueprint_search(
    State(state): State<AppState>,
    Path(name): Path<String>,
    Query(params): Query<BlueprintSearchParams>,
) -> Result<Json<serde_json::Value>, BlueprintMiss> {
    let (blueprint, catalog) = registered_blueprint(&state, &name).await?;
    crate::metrics::search();
    Ok(Json(packages::search_json_with_blueprint(
        &params.q,
        &catalog,
        &blueprint,
        state.package_store(),
        packages::Fetch::Rest(&name),
    )))
}

#[derive(Debug, Deserialize)]
pub struct BlueprintDocsParams {
    name: String,
}

/// `GET /v1/blueprints/{blueprint}/packages/docs` — what the blueprint's MCP
/// `packages__docs` tool answers.
pub async fn blueprint_docs(
    State(state): State<AppState>,
    Path(name): Path<String>,
    Query(params): Query<BlueprintDocsParams>,
) -> Result<axum::response::Response, BlueprintMiss> {
    let (blueprint, catalog) = registered_blueprint(&state, &name).await?;
    let lookup =
        packages::lookup_with_blueprint(&params.name, &catalog, &blueprint, state.package_store());
    Ok(docs_response(&params.name, lookup))
}

/// `GET /v1/blueprints/{blueprint}/builtins` — the built-in catalog. It is the
/// same for every blueprint; the route is nested so a harness addresses one
/// blueprint throughout.
pub async fn blueprint_builtins(
    State(state): State<AppState>,
    Path(name): Path<String>,
) -> Result<Json<serde_json::Value>, BlueprintMiss> {
    registered_blueprint(&state, &name).await?;
    crate::metrics::builtins("list");
    Ok(Json(packages::builtins_list_json()))
}

/// `GET /v1/blueprints/{blueprint}/builtins/docs?name=A&name=B` — what the
/// blueprint's MCP `builtins__docs` tool answers: one entry per `name`, an
/// unknown one reported inline rather than failing the batch.
pub async fn blueprint_builtin_docs(
    State(state): State<AppState>,
    Path(name): Path<String>,
    RawQuery(query): RawQuery,
) -> Result<Json<serde_json::Value>, BlueprintMiss> {
    let (blueprint, catalog) = registered_blueprint(&state, &name).await?;
    crate::metrics::builtins("docs");
    // `Query` keeps only one value of a repeated key, so read them all here.
    let names: Vec<String> = url::form_urlencoded::parse(query.unwrap_or_default().as_bytes())
        .filter(|(key, _)| key == "name")
        .map(|(_, value)| value.into_owned())
        .collect();
    Ok(Json(packages::builtins_docs_json(
        &names,
        &packages::mcp_package_names(&catalog),
        &packages::Fetch::Rest(&name),
        submilli_shared::library_visibility::LibraryVisibility::for_blueprint(&blueprint),
    )))
}

#[derive(Debug, Serialize)]
pub struct InstallErrorBody {
    pub error: &'static str,
    pub message: String,
    pub warnings: Vec<String>,
}

type InstallFailure = (StatusCode, Json<InstallErrorBody>);

fn install_err(status: StatusCode, error: &'static str, message: String) -> InstallFailure {
    (
        status,
        Json(InstallErrorBody {
            error,
            message,
            warnings: Vec::new(),
        }),
    )
}

#[derive(Debug, Deserialize)]
pub struct InstallRequest {
    /// GitHub repo: `org/repo`, `github.com/org/repo`, or a full URL.
    pub url: String,
    /// Pin to this commit (or ref). Resolved from the URL's default branch when
    /// omitted.
    #[serde(default)]
    pub sha: Option<String>,
    /// Install only this package; omit to install every package the repo declares.
    #[serde(default)]
    pub package: Option<String>,
    /// Re-install over a package already present at a different commit.
    #[serde(default)]
    pub upgrade: bool,
    #[serde(default)]
    pub deny_warnings: bool,
}

#[derive(Debug, Serialize)]
pub struct InstallResponse {
    pub sha: String,
    pub installed: Vec<String>,
    pub up_to_date: Vec<String>,
    pub warnings: Vec<String>,
}

/// `POST /v1/packages/install` — fetch a GitHub repo, compile it, and install
/// into the server's package store. The fetch + compile blocks, so it runs
/// off the async workers, on a compiler-sized thread because it compiles
/// package sources.
pub async fn install(
    State(state): State<AppState>,
    Json(req): Json<InstallRequest>,
) -> Result<Json<InstallResponse>, InstallFailure> {
    let store = state.package_store().clone();
    let token_file = state.github_token_file().map(std::path::Path::to_path_buf);
    let installer_state = state.clone();
    tokio::task::spawn_blocking(move || {
        let install = || install_with_server_token(&store, req, token_file);
        let outcome = match compiler_thread::run(install) {
            Ok(outcome) => outcome,
            Err(error) => Err(internal_install_error(error)),
        };
        // Anything prepared before this install may now resolve differently:
        // a dependency the closure installed (which the response's `installed`
        // field does not list), or an owned copy now shadowing a fallback one.
        // A failed install can have written dependencies before it failed,
        // and a client that hangs up does not stop this task, so the eviction
        // rides with the install rather than with the request.
        installer_state.evict_all_prepared_packages();
        outcome
    })
    .await
    .map_err(internal_install_error)?
    .map(Json)
}

fn internal_install_error(error: impl std::fmt::Display) -> InstallFailure {
    install_err(
        StatusCode::INTERNAL_SERVER_ERROR,
        "internal_error",
        error.to_string(),
    )
}

/// Install with the server's own GitHub token, saying in the log when GitHub
/// rejected it and the fetch went on without.
fn install_with_server_token(
    store: &submilli_build::PackageStore,
    req: InstallRequest,
    token_file: Option<std::path::PathBuf>,
) -> Result<InstallResponse, InstallFailure> {
    let auth = github_auth(token_file)?;
    let outcome = install_blocking(store, req, &auth);
    if auth.token_rejected() {
        tracing::warn!(
            "GitHub rejected {} (expired or revoked); packages were fetched without it",
            auth.source().describe()
        );
    }
    outcome
}

/// The server's own GitHub token, read fresh so a replaced file rotates it.
/// Never the caller's: an install request carries no credential.
fn github_auth(token_file: Option<std::path::PathBuf>) -> Result<GithubAuth, InstallFailure> {
    let Some(path) = token_file else {
        return Ok(GithubAuth::new(None, TokenSource::ServerUnconfigured));
    };
    let token = GithubToken::read_file(&path).map_err(|err| {
        install_err(
            StatusCode::INTERNAL_SERVER_ERROR,
            "github_token_unavailable",
            err.to_string(),
        )
    })?;
    Ok(GithubAuth::new(Some(token), TokenSource::ServerFile(path)))
}

fn install_blocking(
    store: &submilli_build::PackageStore,
    req: InstallRequest,
    auth: &GithubAuth,
) -> Result<InstallResponse, InstallFailure> {
    let mut spec = github::parse_spec(&req.url)
        .map_err(|err| install_err(StatusCode::BAD_REQUEST, "invalid_url", err.to_string()))?;
    if let Some(sha) = req.sha {
        spec.git_ref = Some(sha);
    }
    let resolved = spec
        .resolve(auth)
        .map_err(|err| map_github_error(err, "resolve_failed"))?;
    let dir = resolved
        .download(auth)
        .map_err(|err| map_github_error(err, "download_failed"))?;

    let source = PackageSource::Github(GithubSource {
        org: resolved.org.clone(),
        repo: resolved.repo.clone(),
        sha: resolved.sha.clone(),
        source_hash: None,
    });

    // Resolve the repo's GitHub-dependency closure (network), install it
    // deps-first, then install the requested package(s).
    let manifest_path = dir.path().join("submilli.toml");
    let manifest = load_manifest(&manifest_path).map_err(|diagnostics| {
        install_err(
            StatusCode::BAD_REQUEST,
            "invalid_manifest",
            diagnostics
                .iter()
                .map(|diag| diag.message.clone())
                .collect::<Vec<_>>()
                .join("; "),
        )
    })?;
    let existing_lock = Lockfile::read(dir.path()).ok().flatten();
    let closure = resolve_github_closure(
        store,
        &manifest,
        &GithubRepoFetcher::new(auth),
        existing_lock.as_ref(),
    )
    .map_err(map_resolve_error)?;
    let mut preparation = InstallPreparation::new(store).map_err(map_install_error)?;
    preparation
        .prepare_plan(&closure.plan, req.upgrade)
        .map_err(map_install_error)?;

    let only = req.package.map(PackageName::new);
    let report = preparation
        .prepare_repo(dir.path(), only.as_ref(), &source, req.upgrade)
        .map_err(map_install_error)?;

    publish_install(preparation, report, resolved.sha, req.deny_warnings)
}

fn publish_install(
    preparation: InstallPreparation,
    report: submilli_build::InstallReport,
    sha: String,
    deny_warnings: bool,
) -> Result<InstallResponse, InstallFailure> {
    let warnings = preparation.warnings.clone();
    preparation
        .publish(deny_warnings || deny_warnings_from_env())
        .map_err(map_install_error)?;
    Ok(InstallResponse {
        warnings,
        sha,
        installed: report
            .installed
            .iter()
            .map(|pkg| pkg.name.as_str().to_string())
            .collect(),
        up_to_date: report
            .up_to_date
            .iter()
            .map(|name| name.as_str().to_string())
            .collect(),
    })
}

/// A GitHub fetch failure. `upstream_failure_code` is the code for a failed
/// transfer or an unexpected answer from GitHub.
fn map_github_error(err: GithubError, upstream_failure_code: &'static str) -> InstallFailure {
    let (status, code) = match &err {
        GithubError::InvalidSpec(_) => (StatusCode::BAD_REQUEST, "invalid_url"),
        GithubError::Access(_)
        | GithubError::RateLimited(_)
        | GithubError::Resolve(_)
        | GithubError::Download(_) => github_failure(err.kind(), upstream_failure_code),
    };
    install_err(status, code, err.to_string())
}

/// The status and code for a GitHub failure of `kind`, the same for the
/// repository being installed and for any of its dependencies.
fn github_failure(kind: FetchErrorKind, failed_code: &'static str) -> (StatusCode, &'static str) {
    match kind {
        FetchErrorKind::Access => (StatusCode::BAD_REQUEST, "github_access"),
        FetchErrorKind::RateLimited => (StatusCode::SERVICE_UNAVAILABLE, "github_rate_limited"),
        FetchErrorKind::Failed => (StatusCode::BAD_GATEWAY, failed_code),
    }
}

fn map_resolve_error(err: ResolveError) -> InstallFailure {
    match err {
        ResolveError::Fetch { ref source, .. } => {
            let (status, code) = github_failure(source.kind, "dependency_fetch_failed");
            install_err(status, code, err.to_string())
        }
        ResolveError::ShaConflict { .. } => {
            install_err(StatusCode::CONFLICT, "dependency_conflict", err.to_string())
        }
        ResolveError::Cycle { .. }
        | ResolveError::MissingPackageInRepo { .. }
        | ResolveError::Manifest { .. } => install_err(
            StatusCode::BAD_REQUEST,
            "invalid_dependency",
            err.to_string(),
        ),
    }
}

fn map_install_error(err: InstallError) -> InstallFailure {
    match err {
        InstallError::WarningsDenied { warnings } => (
            StatusCode::BAD_REQUEST,
            Json(InstallErrorBody {
                error: "warnings_denied",
                message: warning_denial_message(warnings.len()),
                warnings,
            }),
        ),
        InstallError::Preparation(error) => internal_install_error(error),
        InstallError::NoManifest { repo_dir } => install_err(
            StatusCode::BAD_REQUEST,
            "no_manifest",
            format!("{} has no submilli.toml at its root", repo_dir.display()),
        ),
        InstallError::Manifest { diagnostics, .. } => install_err(
            StatusCode::BAD_REQUEST,
            "invalid_manifest",
            diagnostics
                .iter()
                .map(|diag| diag.message.clone())
                .collect::<Vec<_>>()
                .join("; "),
        ),
        InstallError::Driver(DriverError::Compile { rendered, .. }) => {
            install_err(StatusCode::BAD_REQUEST, "compile_failed", rendered)
        }
        InstallError::Driver(err) => {
            install_err(StatusCode::BAD_REQUEST, "build_failed", err.to_string())
        }
        InstallError::ScopeMismatch {
            org,
            repo,
            packages,
        } => install_err(
            StatusCode::BAD_REQUEST,
            "scope_mismatch",
            format!(
                "packages [{}] from github.com/{org}/{repo} must be scoped `@{org}/...`",
                packages
                    .iter()
                    .map(|name| name.as_str().to_string())
                    .collect::<Vec<_>>()
                    .join(", ")
            ),
        ),
        InstallError::Conflict {
            incoming,
            conflicts,
        } => install_err(
            StatusCode::CONFLICT,
            "already_installed",
            format!(
                "{} already installed at a different commit; retry with upgrade to replace with {incoming}",
                conflicts
                    .iter()
                    .map(|conflict| conflict.name.as_str().to_string())
                    .collect::<Vec<_>>()
                    .join(", ")
            ),
        ),
        InstallError::Store(err) => install_err(
            StatusCode::INTERNAL_SERVER_ERROR,
            "store_error",
            err.to_string(),
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn github_failures_map_to_their_codes() {
        let code = |err, failed| map_github_error(err, failed).1.0.error;
        assert_eq!(
            code(GithubError::Access("private".into()), "resolve_failed"),
            "github_access"
        );
        assert_eq!(
            code(GithubError::RateLimited("later".into()), "resolve_failed"),
            "github_rate_limited"
        );
        assert_eq!(
            code(GithubError::Download("reset".into()), "download_failed"),
            "download_failed"
        );
    }

    #[test]
    fn a_dependency_fetch_keeps_its_kind() {
        let fetch = |kind| ResolveError::Fetch {
            name: PackageName::new("@acme/crm"),
            url: "github.com/acme/crm".into(),
            sha: "0".repeat(40),
            source: submilli_build::FetchError::with_kind(kind, "message"),
        };
        let code = |kind| map_resolve_error(fetch(kind)).1.0.error;
        assert_eq!(code(FetchErrorKind::Access), "github_access");
        assert_eq!(code(FetchErrorKind::RateLimited), "github_rate_limited");
        assert_eq!(code(FetchErrorKind::Failed), "dependency_fetch_failed");
    }

    #[test]
    fn the_server_token_is_read_from_its_file_on_each_install() {
        let unconfigured = github_auth(None).unwrap();
        assert_eq!(unconfigured.source(), &TokenSource::ServerUnconfigured);
        assert!(unconfigured.token().is_none());

        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("token");
        let missing = github_auth(Some(path.clone())).unwrap_err();
        assert_eq!(missing.0, StatusCode::INTERNAL_SERVER_ERROR);
        assert_eq!(missing.1.0.error, "github_token_unavailable");

        std::fs::write(&path, "ghp_first\n").unwrap();
        let first = github_auth(Some(path.clone())).unwrap();
        assert_eq!(first.token().unwrap().secret(), "ghp_first");
        std::fs::write(&path, "ghp_rotated\n").unwrap();
        let rotated = github_auth(Some(path.clone())).unwrap();
        assert_eq!(rotated.token().unwrap().secret(), "ghp_rotated");
        assert_eq!(rotated.source(), &TokenSource::ServerFile(path));
    }
}

#[cfg(test)]
mod warning_policy_tests {
    use super::*;

    #[test]
    fn deny_warnings_request_defaults_to_false_and_response_preserves_diagnostics() {
        let request: InstallRequest = serde_json::from_str(r#"{"url":"acme/billing"}"#).unwrap();
        assert!(!request.deny_warnings);
        let request: InstallRequest =
            serde_json::from_str(r#"{"url":"acme/billing","deny_warnings":true}"#).unwrap();
        assert!(request.deny_warnings);
        let warnings = vec!["warning: capability mismatch\n  --> src/lib.ts:1:1\n".to_string()];
        let (status, Json(body)) = map_install_error(InstallError::WarningsDenied {
            warnings: warnings.clone(),
        });
        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert_eq!(body.error, "warnings_denied");
        assert_eq!(
            body.message,
            "1 warning(s) treated as errors (--deny-warnings)"
        );
        assert_eq!(body.warnings, warnings);
        let response = InstallResponse {
            sha: "commit".into(),
            installed: Vec::new(),
            up_to_date: Vec::new(),
            warnings: warnings.clone(),
        };
        assert_eq!(
            serde_json::to_value(response).unwrap()["warnings"],
            serde_json::json!(warnings)
        );
    }
}

#[cfg(test)]
mod strict_install_tests {
    use super::*;

    #[test]
    fn deny_warnings_server_environment_overrides_false_request() {
        const CHILD: &str = "SUBMILLI_TEST_STRICT_INSTALL_CHILD";
        if std::env::var_os(CHILD).is_none() {
            let output = std::process::Command::new(std::env::current_exe().unwrap())
                .args(["--exact", "handlers::packages::strict_install_tests::deny_warnings_server_environment_overrides_false_request", "--nocapture"])
                .env(CHILD, "1").env("SUBMILLI_DENY_WARNINGS", "1").output().unwrap();
            assert!(
                output.status.success(),
                "{}",
                String::from_utf8_lossy(&output.stderr)
            );
            assert!(String::from_utf8_lossy(&output.stdout).contains("1 passed"));
            return;
        }
        let directory = tempfile::tempdir().unwrap();
        let repo = directory.path().join("repo");
        std::fs::create_dir_all(repo.join("src")).unwrap();
        std::fs::create_dir_all(repo.join("docs")).unwrap();
        std::fs::write(repo.join("submilli.toml"), "[[package]]\nname = \"@acme/warned\"\nversion = \"0.1.0\"\ndescription = \"Warnings fixture.\"\n").unwrap();
        std::fs::write(
            repo.join("src/lib.ts"),
            "export function hello(): number { return 1; }\n",
        )
        .unwrap();
        std::fs::write(repo.join("docs/readme.md"), "# Fixture\n").unwrap();
        let store = submilli_build::PackageStore::new(directory.path().join("store"));
        let mut preparation = InstallPreparation::new(&store).unwrap();
        let source = PackageSource::Github(GithubSource {
            org: "acme".into(),
            repo: "warned".into(),
            sha: "0".repeat(40),
            source_hash: None,
        });
        let report = preparation
            .prepare_repo(&repo, None, &source, false)
            .unwrap();
        let request: InstallRequest =
            serde_json::from_str(r#"{"url":"acme/warned","deny_warnings":false}"#).unwrap();
        let (status, Json(body)) =
            publish_install(preparation, report, "0".repeat(40), request.deny_warnings)
                .unwrap_err();
        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert_eq!(body.error, "warnings_denied");
        assert!(!body.warnings.is_empty());
        assert!(!store.root().exists());
    }
}
