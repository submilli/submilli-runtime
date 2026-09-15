//! REST package- and built-in-discovery endpoints, mirroring the MCP tools:
//!
//! * `GET /v1/packages/search?q=<query>[&blueprint=<name>]` — matching packages
//!   (empty query = all)
//! * `GET /v1/packages/docs?name=<name>[&blueprint=<name>]` — declarations +
//!   description for one
//!
//! `blueprint=` scopes discovery the way the MCP tools are scoped by the bound
//! blueprint: without it these resolve stdlib only, so a caller (the agent
//! server's `search`/`docs` tools) would never see the packages or
//! `@mcp/<server>` entries its own blueprint declares.
//! * `GET /v1/builtins` — the in-scope built-in catalog (`{ types, namespaces }`)
//! * `GET /v1/builtins/docs?name=<name>` — `.d.ts` declarations for one built-in
//!
//! The response bodies come from `crate::packages` (shared with the MCP tools);
//! the REST layer only adds an HTTP status code. Names carry `:` / `@` / `/`, so
//! they ride in the query string rather than a path segment.

use axum::Json;
use axum::extract::{Query, State};
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
use crate::packages::{self, DocLookup};

#[derive(Debug, Serialize)]
pub struct InstalledPackage {
    pub name: String,
    pub version: String,
    pub description: String,
}

#[derive(Debug, Serialize)]
pub struct InstalledResponse {
    pub packages: Vec<InstalledPackage>,
}

/// `GET /v1/packages` — the registry packages installed in this server's store,
/// i.e. the names a blueprint's `packages:` block may declare. An artifact that
/// fails to load is skipped rather than failing the listing.
pub async fn installed(State(state): State<AppState>) -> Json<InstalledResponse> {
    let store = state.package_store();
    let packages = store
        .available_packages()
        .unwrap_or_default()
        .into_iter()
        .filter_map(|name| {
            let artifact = store.load(&name).ok()?;
            Some(InstalledPackage {
                name,
                version: artifact.metadata.package_version,
                description: artifact.metadata.description,
            })
        })
        .collect();
    Json(InstalledResponse { packages })
}

#[derive(Debug, Serialize)]
pub struct UninstallResponse {
    pub name: String,
}

/// `DELETE /v1/packages/{name}` — remove an installed package from the store.
/// Blueprints that declare it keep working until their next prepare, which will
/// fail to load the package — same admin posture as the other package routes.
pub async fn uninstall(
    State(state): State<AppState>,
    axum::extract::Path(name): axum::extract::Path<String>,
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
        return Err(error(
            StatusCode::NOT_FOUND,
            "not_found",
            format!("package '{name}' is not installed"),
        ));
    }
    std::fs::remove_dir_all(&dir).map_err(|err| {
        error(
            StatusCode::INTERNAL_SERVER_ERROR,
            "internal_error",
            format!("removing package '{name}': {err}"),
        )
    })?;
    // Drop any compiled module cached for a blueprint that declared it, so the
    // removal takes effect without a restart.
    for blueprint in state.blueprints().list_blueprints().await {
        if blueprint.packages.contains(&name) {
            state.evict_prepared_packages(&blueprint.name);
        }
    }
    Ok(Json(UninstallResponse { name }))
}

#[derive(Debug, Deserialize)]
pub struct SearchParams {
    #[serde(default)]
    q: String,
    /// Scope the search to a blueprint: folds in its registry packages and its
    /// discovered `@mcp/<server>` packages. Omitted ⇒ stdlib only.
    #[serde(default)]
    blueprint: Option<String>,
}

pub async fn search(
    State(state): State<AppState>,
    Query(params): Query<SearchParams>,
) -> impl IntoResponse {
    crate::metrics::search();
    match blueprint_context(&state, params.blueprint.as_deref()).await {
        Some((blueprint, catalog)) => Json(packages::search_json_with_blueprint(
            &params.q,
            &catalog,
            &blueprint,
            state.package_store(),
            packages::Fetch::Rest,
        )),
        None => Json(packages::search_json(&params.q, packages::Fetch::Rest)),
    }
}

/// Resolve the optional `blueprint=` scope into the blueprint and its MCP
/// catalog. An unknown name degrades to stdlib-only rather than erroring: the
/// caller asked to discover packages, not to assert the blueprint exists.
async fn blueprint_context(
    state: &AppState,
    name: Option<&str>,
) -> Option<(
    submilli_blueprint::Blueprint,
    std::sync::Arc<crate::mcp::McpCatalog>,
)> {
    let name = name?;
    let blueprint = state.blueprints().get(name).await?;
    let catalog = state.mcp_catalog(name, &blueprint).await;
    Some((blueprint, catalog))
}

#[derive(Debug, Deserialize)]
pub struct DocsParams {
    name: String,
    /// Same scoping as search: required to resolve `@mcp/<server>` and this
    /// blueprint's registry packages.
    #[serde(default)]
    blueprint: Option<String>,
}

pub async fn docs(
    State(state): State<AppState>,
    Query(params): Query<DocsParams>,
) -> axum::response::Response {
    // Without a blueprint the endpoint resolves stdlib only — `@mcp/<server>`
    // packages are blueprint-scoped, so `lookup` maps them to `McpUnknown`.
    let lookup = match blueprint_context(&state, params.blueprint.as_deref()).await {
        Some((blueprint, catalog)) => packages::lookup_with_blueprint(
            &params.name,
            &catalog,
            &blueprint,
            state.package_store(),
        ),
        None => packages::lookup(&params.name),
    };
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
    (status, Json(packages::docs_json(&params.name, lookup))).into_response()
}

pub async fn builtins() -> impl IntoResponse {
    crate::metrics::builtins("list");
    Json(packages::builtins_list_json())
}

pub async fn builtin_docs(Query(params): Query<DocsParams>) -> impl IntoResponse {
    crate::metrics::builtins("docs");
    let body = packages::builtin_entry_json(&params.name, &[], &packages::Fetch::Rest);
    let status = if body.get("error").is_some() {
        StatusCode::NOT_FOUND
    } else {
        StatusCode::OK
    };
    (status, Json(body))
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
/// into the server's package store. Admin surface (same unauthenticated posture
/// as `/v1/secrets`; gating lands with SUB-159/160). The fetch + compile is
/// blocking, so it runs on a blocking thread.
pub async fn install(
    State(state): State<AppState>,
    Json(req): Json<InstallRequest>,
) -> Result<Json<InstallResponse>, InstallFailure> {
    let store = state.package_store().clone();
    tokio::task::spawn_blocking(move || install_blocking(&store, req))
        .await
        .map_err(|err| {
            install_err(
                StatusCode::INTERNAL_SERVER_ERROR,
                "internal_error",
                err.to_string(),
            )
        })?
        .map(Json)
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
