//! The running playground: the embedded server and the control listener, both on
//! loopback, in this process.
//!
//! The server listener is `submilli-server` itself under the playground's tokens.
//! The control listener serves the page and the control API; the routes later
//! steps add go into [`ControlRoutes`].

use std::fs::File;
use std::net::{Ipv4Addr, SocketAddr};
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};

use anyhow::{Context, Result, bail};
use axum::Json;
use axum::Router;
use axum::body::Body;
use axum::extract::{Request, State};
use axum::http::header::{CONTENT_TYPE, X_CONTENT_TYPE_OPTIONS};
use axum::http::{HeaderValue, StatusCode};
use axum::middleware::{self, Next};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use submilli_server::audit::AuditConfig;
use submilli_server::{AppState, AuthConfig, RunTelemetry, ServerConfig};
use tokio::sync::Notify;

use super::control_auth::{self, Caller, ControlAuth, LoginRefusal};
use super::project::Project;
use super::state::{APP_TOKEN, Lock, StateDir};
use super::store::redact::WatchedSecretStore;
use super::store::{KnownSecrets, Recorder, Store};
use super::{Egress, ReadyRecord};

/// How long in-flight requests may finish after a stop before they are dropped.
const SHUTDOWN_GRACE: Duration = Duration::from_secs(5);
/// How long the control listener may take to close once the server has drained.
const CONTROL_CLOSE_BUDGET: Duration = Duration::from_secs(2);

pub(crate) struct HostOptions {
    pub(crate) project: Project,
    pub(crate) egress: Egress,
    pub(crate) nonce: String,
    /// Held until the instance is ready, so a concurrent start waits and attaches.
    pub(crate) start_lock: Option<File>,
    /// Print the ready record once serving, as `--foreground` does; a detached
    /// child's output goes to its log, and the parent prints the record instead.
    pub(crate) print: Option<super::Output>,
}

/// What the control listener reports about this instance. Built once at start.
#[derive(Clone, Serialize)]
pub(crate) struct Description {
    pub(crate) project: PathBuf,
    pub(crate) blueprint: BlueprintInfo,
    pub(crate) url: String,
    pub(crate) server_url: String,
    pub(crate) pid: u32,
    pub(crate) app_token_file: PathBuf,
    pub(crate) state_dir: PathBuf,
    pub(crate) log_file: PathBuf,
    pub(crate) egress_grants: Vec<String>,
    pub(crate) provider: Option<&'static str>,
    pub(crate) model: Option<String>,
}

#[derive(Clone, Serialize)]
pub(crate) struct BlueprintInfo {
    pub(crate) name: String,
    pub(crate) path: PathBuf,
}

/// Serve until a stop, a signal, or `POST /v1/shutdown`, then drain and remove
/// the lock and the ready file.
pub(crate) fn serve(options: HostOptions) -> Result<()> {
    restrict_new_files();
    let state_dir = StateDir::for_project(&options.project.root);
    state_dir.create()?;
    let tokens = state_dir.tokens()?;
    let blueprint_yaml = std::fs::read_to_string(&options.project.blueprint)
        .with_context(|| format!("reading {}", options.project.blueprint.display()))?;
    let blueprint = submilli_blueprint::parse(&blueprint_yaml)
        .with_context(|| format!("parsing {}", options.project.blueprint.display()))?;
    let admin = state_dir.admin_token()?;
    let secrets = KnownSecrets::default();
    let store = Store::open(&state_dir.store_dir()).context("opening the run store")?;
    let secret_store = Arc::new(WatchedSecretStore::new(
        crate::commands::local::open_secret_store()?,
        secrets.clone(),
    ));
    let config = server_config(
        &state_dir,
        &options.egress,
        tokens.clone(),
        Some(secret_store),
        Some(Arc::new(Recorder::new(Arc::new(store), secrets))),
    );
    let runtime = submilli_server::runtime(&config).context("starting the async runtime")?;
    let result = runtime.block_on(run(
        options,
        state_dir,
        config,
        tokens,
        Registration {
            name: blueprint.name.clone(),
            yaml: blueprint_yaml,
            admin,
        },
    ));
    // Teardown is bounded: a stray connection task must not keep the process up.
    runtime.shutdown_timeout(Duration::from_millis(250));
    result
}

struct Registration {
    name: String,
    yaml: String,
    admin: String,
}

async fn run(
    options: HostOptions,
    state_dir: StateDir,
    mut config: ServerConfig,
    tokens: Vec<submilli_server::ApiToken>,
    blueprint: Registration,
) -> Result<()> {
    config.blueprints = Some(submilli_server::prepare_blueprint_store(&config).await?);
    let state = AppState::new(config)?;
    state.boot().await?;
    register_blueprint(&state, &blueprint).await?;

    let server = tokio::net::TcpListener::bind(SocketAddr::from((Ipv4Addr::LOCALHOST, 0)))
        .await
        .context("binding the server listener")?;
    let control = tokio::net::TcpListener::bind(SocketAddr::from((Ipv4Addr::LOCALHOST, 0)))
        .await
        .context("binding the control listener")?;
    let server_port = server.local_addr()?.port();
    let control_port = control.local_addr()?.port();

    let description = describe(
        &options,
        &state_dir,
        &blueprint.name,
        control_port,
        server_port,
    );
    let auth = Arc::new(ControlAuth::new(tokens));
    let shared = ControlState(Arc::new(ControlInner {
        auth: Arc::clone(&auth),
        port: control_port,
        nonce: options.nonce.clone(),
        server_shutdown: state.shutdown_signal(),
        description: description.clone(),
    }));
    let control_stop = Arc::new(Notify::new());
    let control_task = tokio::spawn({
        let control_stop = Arc::clone(&control_stop);
        let router = ControlRoutes::new().router(shared);
        async move {
            axum::serve(control, router)
                .with_graceful_shutdown(async move { control_stop.notified().await })
                .await
        }
    });

    let lock = Lock {
        pid: std::process::id(),
        control_port,
        server_port,
        nonce: options.nonce.clone(),
    };
    let served = async {
        // The lock first, so the ready file never names an instance without one.
        state_dir.write_lock(&lock)?;
        state_dir.write_ready(&lock)?;
        drop(options.start_lock);
        if let Some(output) = options.print {
            let code = auth.mint_login_code(Instant::now())?;
            let record = ReadyRecord::new(description_value(&description)?, &code, false);
            record.print(output);
        }
        submilli_server::serve_embedded(server, state, SHUTDOWN_GRACE).await
    }
    .await;

    auth.drop_sessions();
    control_stop.notify_one();
    if tokio::time::timeout(CONTROL_CLOSE_BUDGET, control_task)
        .await
        .is_err()
    {
        warn("the control listener did not close in time");
    }
    state_dir.remove_if_ours(&options.nonce);
    served
}

async fn register_blueprint(state: &AppState, blueprint: &Registration) -> Result<()> {
    use tower::ServiceExt;
    // Create-or-replace: a restart applies the file as it is now.
    let mut uri = url::Url::parse("http://playground/v1/blueprints")
        .context("building the blueprint registration")?;
    uri.path_segments_mut()
        .map_err(|()| anyhow::anyhow!("building the blueprint registration"))?
        .push(&blueprint.name);
    let request = Request::builder()
        .method("PUT")
        .uri(uri.path())
        .header(CONTENT_TYPE, "application/json")
        .header(
            axum::http::header::AUTHORIZATION,
            format!("Bearer {}", blueprint.admin),
        )
        .body(Body::from(json!({ "yaml": blueprint.yaml }).to_string()))
        .context("building the blueprint registration")?;
    let response = submilli_server::app(state.clone())
        .oneshot(request)
        .await
        .context("registering the blueprint")?;
    let status = response.status();
    if status.is_success() {
        return Ok(());
    }
    let body = axum::body::to_bytes(response.into_body(), 1 << 20)
        .await
        .unwrap_or_default();
    let message = serde_json::from_slice::<Value>(&body)
        .ok()
        .and_then(|body| body["message"].as_str().map(str::to_owned))
        .unwrap_or_else(|| String::from_utf8_lossy(&body).into_owned());
    bail!(
        "the server refused blueprint `{}` ({}): {message}",
        blueprint.name,
        status.as_u16()
    )
}

fn server_config(
    state_dir: &StateDir,
    egress: &Egress,
    tokens: Vec<submilli_server::ApiToken>,
    secret_store: Option<Arc<dyn submilli_server::SecretStore>>,
    run_recorder: Option<Arc<dyn submilli_server::record::RunRecorderFactory>>,
) -> ServerConfig {
    ServerConfig {
        // Main's audit writes whole permission-check contexts, unredacted, to the
        // log: a second copy of run data outside the playground's own store.
        audit: AuditConfig {
            enabled: false,
            ..AuditConfig::default()
        },
        auth: AuthConfig::Tokens(tokens),
        network_policy: egress.policy(),
        blueprint_dir: Some(state_dir.blueprints_dir()),
        session_store_dir: Some(state_dir.sessions_dir()),
        session_storage_root: Some(state_dir.session_vfs_dir()),
        ephemeral_storage_root: Some(state_dir.ephemeral_vfs_dir()),
        managed_volume_root: Some(state_dir.volumes_dir()),
        // The developer's own stores, so `build publish-local` and `secret put`
        // keep working.
        package_store_root: Some(submilli_build::default_package_store_dir()),
        secret_store,
        // Every run the server executes, whoever sent it, goes to the playground's store.
        run_recorder,
        // No run content leaves the machine, whatever the CLI's telemetry setting.
        run_telemetry: RunTelemetry::Off,
        ..ServerConfig::default()
    }
}

fn describe(
    options: &HostOptions,
    state_dir: &StateDir,
    blueprint: &str,
    control_port: u16,
    server_port: u16,
) -> Description {
    let (provider, model) = detect_provider();
    Description {
        project: options.project.root.clone(),
        blueprint: BlueprintInfo {
            name: blueprint.to_owned(),
            path: options.project.blueprint.clone(),
        },
        url: format!("http://127.0.0.1:{control_port}/"),
        server_url: format!("http://127.0.0.1:{server_port}"),
        pid: std::process::id(),
        app_token_file: state_dir.token_path(APP_TOKEN),
        state_dir: state_dir.root().to_path_buf(),
        log_file: state_dir.log_path(),
        egress_grants: options.egress.grants(),
        provider,
        model,
    }
}

/// The bring-your-own-key provider the environment names when the playground
/// starts. Only which variable is set is read here, never its value.
fn detect_provider() -> (Option<&'static str>, Option<String>) {
    let set = |key: &str| std::env::var_os(key).is_some_and(|value| !value.is_empty());
    let provider = [
        ("ANTHROPIC_API_KEY", "anthropic"),
        ("OPENAI_API_KEY", "openai"),
        ("GEMINI_API_KEY", "google"),
        ("GOOGLE_API_KEY", "google"),
    ]
    .into_iter()
    .find_map(|(key, provider)| set(key).then_some(provider));
    let model = std::env::var("SUBMILLI_PLAYGROUND_MODEL")
        .ok()
        .filter(|model| !model.trim().is_empty());
    (provider, model)
}

pub(crate) fn description_value(description: &Description) -> Result<Value> {
    serde_json::to_value(description).context("encoding the playground's description")
}

/// Files the server creates under the state directory must be owner-only too.
fn restrict_new_files() {
    // SAFETY: umask takes and returns a plain mode and touches no memory.
    unsafe {
        libc::umask(0o077);
    }
}

/// The detached child's stderr is its log; nothing else reports this.
fn warn(message: &str) {
    use std::io::Write;
    let _ = writeln!(std::io::stderr().lock(), "warning: {message}");
}

#[derive(Clone)]
struct ControlState(Arc<ControlInner>);

struct ControlInner {
    auth: Arc<ControlAuth>,
    port: u16,
    nonce: String,
    server_shutdown: Arc<Notify>,
    description: Description,
}

/// The control listener's routes, by who may call them. Later steps add their
/// routes to the group whose callers they accept.
pub(crate) struct ControlRoutes {
    /// No credential: the page, the login-code exchange, and the nonce challenge.
    public: Router<ControlState>,
    /// The admin token only: the CLI.
    admin: Router<ControlState>,
    /// The admin token or a browser session: the CLI and the page.
    viewer: Router<ControlState>,
}

impl ControlRoutes {
    fn new() -> Self {
        Self {
            public: Router::new()
                .route("/", get(page))
                .route("/api/challenge", post(challenge))
                .route("/api/login", post(login)),
            admin: Router::new()
                .route("/api/login-codes", post(mint_login_code))
                .route("/api/stop", post(stop)),
            viewer: Router::new().route("/api/status", get(status)),
        }
    }

    fn router(self, state: ControlState) -> Router {
        let admin = self
            .admin
            .route_layer(middleware::from_fn_with_state(state.clone(), require_admin));
        let viewer = self.viewer.route_layer(middleware::from_fn_with_state(
            state.clone(),
            require_viewer,
        ));
        Router::new()
            .merge(self.public)
            .merge(admin)
            .merge(viewer)
            .fallback(not_found)
            .layer(middleware::from_fn_with_state(state.clone(), same_origin))
            .layer(middleware::from_fn(no_sniff))
            .with_state(state)
    }
}

fn error(status: StatusCode, code: &'static str, message: &str) -> Response {
    (status, Json(json!({ "error": code, "message": message }))).into_response()
}

fn unauthorized() -> Response {
    error(
        StatusCode::UNAUTHORIZED,
        "unauthorized",
        "send `Authorization: Bearer <token>` with the playground's admin token or a browser \
         session token",
    )
}

fn forbidden() -> Response {
    error(
        StatusCode::FORBIDDEN,
        "forbidden",
        "this token may not use this control route",
    )
}

async fn no_sniff(request: Request, next: Next) -> Response {
    let mut response = next.run(request).await;
    response
        .headers_mut()
        .insert(X_CONTENT_TYPE_OPTIONS, HeaderValue::from_static("nosniff"));
    response
}

async fn same_origin(State(state): State<ControlState>, request: Request, next: Next) -> Response {
    if !control_auth::same_origin(request.headers(), state.0.port) {
        return error(
            StatusCode::FORBIDDEN,
            "forbidden_origin",
            "the control listener answers only requests addressed to it from its own origin",
        );
    }
    next.run(request).await
}

async fn require_admin(
    State(state): State<ControlState>,
    request: Request,
    next: Next,
) -> Response {
    match state.0.auth.caller(request.headers(), Instant::now()) {
        Some(Caller::Admin) => next.run(request).await,
        Some(_) => forbidden(),
        None => unauthorized(),
    }
}

async fn require_viewer(
    State(state): State<ControlState>,
    request: Request,
    next: Next,
) -> Response {
    match state.0.auth.caller(request.headers(), Instant::now()) {
        Some(Caller::Admin | Caller::Browser) => next.run(request).await,
        Some(Caller::Token(_)) => forbidden(),
        None => unauthorized(),
    }
}

/// An unknown control route answers 401 to a caller with no credential, so the
/// route table is not readable without one.
async fn not_found(State(state): State<ControlState>, request: Request) -> Response {
    if request.uri().path().starts_with("/api/")
        && state
            .0
            .auth
            .caller(request.headers(), Instant::now())
            .is_none()
    {
        return unauthorized();
    }
    error(StatusCode::NOT_FOUND, "not_found", "no such control route")
}

#[derive(Deserialize)]
struct ChallengeRequest {
    challenge: String,
}

async fn challenge(
    State(state): State<ControlState>,
    Json(request): Json<ChallengeRequest>,
) -> Response {
    match control_auth::challenge_response(&state.0.nonce, &request.challenge) {
        Some(response) => Json(json!({ "response": response })).into_response(),
        None => error(
            StatusCode::BAD_REQUEST,
            "invalid_challenge",
            "send 32 random bytes as 64 hex characters",
        ),
    }
}

#[derive(Deserialize)]
struct LoginRequest {
    code: String,
}

async fn login(State(state): State<ControlState>, Json(request): Json<LoginRequest>) -> Response {
    match state.0.auth.exchange(&request.code, Instant::now()) {
        Ok(token) => Json(json!({
            "session_token": token,
            "expires_in_secs": control_auth::SESSION_TTL.as_secs(),
        }))
        .into_response(),
        Err(LoginRefusal::Invalid) => error(
            StatusCode::BAD_REQUEST,
            "invalid_login_code",
            "the login code is unknown, used, or expired; run `submilli playground open` for a \
             new link",
        ),
        Err(LoginRefusal::RateLimited) => error(
            StatusCode::TOO_MANY_REQUESTS,
            "rate_limited",
            "too many failed logins; wait a minute",
        ),
    }
}

async fn mint_login_code(State(state): State<ControlState>) -> Response {
    match state.0.auth.mint_login_code(Instant::now()) {
        Ok(code) => Json(json!({
            "code": code,
            "expires_in_secs": control_auth::LOGIN_CODE_TTL.as_secs(),
        }))
        .into_response(),
        Err(failure) => error(
            StatusCode::INTERNAL_SERVER_ERROR,
            "internal",
            &failure.to_string(),
        ),
    }
}

async fn status(State(state): State<ControlState>) -> Response {
    let Ok(mut description) = description_value(&state.0.description) else {
        return error(
            StatusCode::INTERNAL_SERVER_ERROR,
            "internal",
            "cannot encode the playground's description",
        );
    };
    if let Some(object) = description.as_object_mut() {
        object.insert("running".into(), json!(true));
        object.insert(
            "browser_sessions".into(),
            json!(state.0.auth.browser_sessions(Instant::now())),
        );
    }
    Json(description).into_response()
}

async fn stop(State(state): State<ControlState>) -> Response {
    state.0.server_shutdown.notify_one();
    (StatusCode::ACCEPTED, Json(json!({ "stopping": true }))).into_response()
}

/// A stand-in until the page arrives: it trades the link's login code for a
/// browser session, keeps the token out of the address bar, and shows the status.
async fn page() -> Response {
    (
        [(
            CONTENT_TYPE,
            HeaderValue::from_static("text/html; charset=utf-8"),
        )],
        PAGE,
    )
        .into_response()
}

const PAGE: &str = r#"<!doctype html>
<html lang="en"><head><meta charset="utf-8"><meta name="viewport" content="width=device-width, initial-scale=1">
<title>Submilli playground</title></head>
<body><h1>Submilli playground</h1><pre id="status">Signing in...</pre>
<script>
(async () => {
  const out = document.getElementById("status");
  const code = new URLSearchParams(location.hash.slice(1)).get("login");
  history.replaceState(null, "", location.pathname + location.search);
  try {
    if (code) {
      const r = await fetch("/api/login", { method: "POST", headers: { "content-type": "application/json" }, body: JSON.stringify({ code }) });
      if (r.ok) localStorage.setItem("submilli-playground-session", (await r.json()).session_token);
    }
    const token = localStorage.getItem("submilli-playground-session");
    const r = await fetch("/api/status", { headers: token ? { authorization: "Bearer " + token } : {} });
    if (r.status === 401) { localStorage.removeItem("submilli-playground-session"); out.textContent = "Run `submilli playground open` for a new link."; return; }
    out.textContent = JSON.stringify(await r.json(), null, 2);
  } catch (e) { out.textContent = String(e); }
})();
</script></body></html>
"#;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_playground_turns_off_run_telemetry_and_the_audit_trail() {
        let dir = tempfile::tempdir().unwrap();
        let state = StateDir::for_project(dir.path());
        state.create().unwrap();
        let tokens = state.tokens().unwrap();
        let config = server_config(&state, &Egress::default(), tokens, None, None);
        assert_eq!(config.run_telemetry, RunTelemetry::Off);
        assert!(!config.audit.enabled);
        assert!(matches!(config.auth, AuthConfig::Tokens(ref tokens) if tokens.len() == 5));
    }

    #[test]
    fn egress_denies_loopback_unless_granted() {
        let denied = Egress::default();
        assert!(denied.grants().is_empty());
        let granted = Egress {
            allow_localhost: true,
            allow_private: false,
            allow_ip: vec!["10.1.0.0/16".parse().unwrap()],
        };
        assert_eq!(
            granted.grants(),
            ["allow-localhost", "allow-ip 10.1.0.0/16"]
        );
        let _: submilli_server::NetworkPolicy = granted.policy();
    }
}
