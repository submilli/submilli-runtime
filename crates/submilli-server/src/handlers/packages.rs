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
    DriverError, GithubSource, InstallError, Lockfile, PackageName, PackageSource, ResolveError,
    install_from_dir, install_plan, load_manifest, resolve_github_closure,
};
use submilli_shared::github;
use submilli_shared::github::GithubRepoFetcher;

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
    match state.blueprints().get(name).await {
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
}

type InstallFailure = (StatusCode, Json<InstallErrorBody>);

fn install_err(status: StatusCode, error: &'static str, message: String) -> InstallFailure {
    (status, Json(InstallErrorBody { error, message }))
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
}

#[derive(Debug, Serialize)]
pub struct InstallResponse {
    pub sha: String,
    pub installed: Vec<String>,
    pub up_to_date: Vec<String>,
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
    let installer_state = state.clone();
    tokio::task::spawn_blocking(move || {
        let outcome = match compiler_thread::run(|| install_blocking(&store, req)) {
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

fn install_blocking(
    store: &submilli_build::PackageStore,
    req: InstallRequest,
) -> Result<InstallResponse, InstallFailure> {
    let mut spec = github::parse_spec(&req.url)
        .map_err(|err| install_err(StatusCode::BAD_REQUEST, "invalid_url", err.to_string()))?;
    if let Some(sha) = req.sha {
        spec.git_ref = Some(sha);
    }
    let resolved = spec
        .resolve()
        .map_err(|err| install_err(StatusCode::BAD_GATEWAY, "resolve_failed", err.to_string()))?;
    let dir = resolved
        .download()
        .map_err(|err| install_err(StatusCode::BAD_GATEWAY, "download_failed", err.to_string()))?;

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
    let closure =
        resolve_github_closure(store, &manifest, &GithubRepoFetcher, existing_lock.as_ref())
            .map_err(map_resolve_error)?;
    install_plan(store, &closure.plan, req.upgrade).map_err(map_install_error)?;

    let only = req.package.map(PackageName::new);
    let report = install_from_dir(store, dir.path(), only.as_ref(), &source, req.upgrade)
        .map_err(map_install_error)?;

    Ok(InstallResponse {
        sha: resolved.sha,
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

fn map_resolve_error(err: ResolveError) -> InstallFailure {
    match err {
        ResolveError::Fetch { .. } => install_err(
            StatusCode::BAD_GATEWAY,
            "dependency_fetch_failed",
            err.to_string(),
        ),
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
