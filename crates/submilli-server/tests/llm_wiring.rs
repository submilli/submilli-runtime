//! End-to-end tests for `submilli:llm` bound to a server execution.
//!
//! The claim under test is that the provider and the token budget reach *both*
//! execution routes, that the server-wide ceiling is one budget shared across
//! every live execution, and that a one-shot `POST /v1/execute` releases its
//! reservation when its transient session drops.
//!
//! Dispatch is faked at the [`ModelDispatch`] seam — the same seam
//! `submilli-shared`'s own tests drive — so these exercise the wiring with zero
//! live provider calls.

use std::collections::BTreeMap;
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use axum::body::Body;
use axum::http::{HeaderMap, Request, StatusCode};
use http_body_util::BodyExt;
use serde_json::{Value, json};
use submilli_blueprint::{
    Action, Blueprint, LlmConfig, LlmModelDecl, LlmProviderDecl, PermissionRule, VfsConfig,
};
use submilli_server::blueprint::InMemoryBlueprintStore;
use submilli_server::config::ServerConfig;
use submilli_server::{AppState, LlmLimits, app};
use submilli_shared::llm::{
    ModelDispatch, ModelRequest, ProviderFailure, ProviderResponse, ProviderUsage, StopReason,
};
use tower::ServiceExt;

const BLUEPRINT: &str = "llm";
const MODEL: &str = "test-model";

/// Output tokens reserved per prompt in these tests.
///
/// The reservation is `estimated_input + output_cap × prompt_count`, and the
/// stdlib reserves against [`LlmLimits::default_output_cap`], so pinning it
/// small keeps a budget expressible in whole calls. At the production default
/// (64k) every ceiling below would have to be six figures to admit one call,
/// which reads as a magic number rather than as "room for one".
const OUTPUT_CAP: u64 = 1_000;

/// Limits with a small, legible output cap. Every other bound stays at its
/// default, so a test that trips one is tripping the bound it names.
fn limits(per_execution_tokens: u64) -> LlmLimits {
    LlmLimits {
        per_execution_tokens,
        default_output_cap: OUTPUT_CAP,
        ..LlmLimits::default()
    }
}

/// The server is deny-by-default, so `llm.call` must be granted before the
/// budget behaviour under test is reached.
fn allow_llm() -> BTreeMap<String, Vec<PermissionRule>> {
    BTreeMap::from([(
        "main".to_string(),
        vec![PermissionRule {
            capability: "llm.call".into(),
            filter: None,
            action: Action::Allow,
        }],
    )])
}

/// A blueprint declaring one provider and one model. Declaration is
/// authoritative (KTD9), so the model must be here for a call to resolve.
fn blueprint(name: &str) -> Blueprint {
    Blueprint {
        name: name.into(),
        vfs: VfsConfig::None,
        permissions: allow_llm(),
        llm: LlmConfig {
            providers: BTreeMap::from([(
                "fake".to_string(),
                LlmProviderDecl {
                    provider_type: "anthropic".into(),
                    base_url: None,
                    api_key: None,
                    supports_structured_outputs: true,
                    options: BTreeMap::new(),
                },
            )]),
            models: BTreeMap::from([(
                MODEL.to_string(),
                LlmModelDecl {
                    provider: "fake".into(),
                    context_window: None,
                    output_reserve: Some(1_000),
                    description: None,
                },
            )]),
        },
        ..Default::default()
    }
}

/// A dispatch that always answers, counting how many times it was reached.
///
/// Reporting usage of exactly `reported_tokens` keeps reconciliation
/// deterministic, so a budget assertion is about the wiring rather than about a
/// token estimator.
struct AlwaysOk {
    calls: Arc<AtomicUsize>,
    reported_tokens: f64,
}

impl ModelDispatch for AlwaysOk {
    fn dispatch<'a>(
        &'a self,
        _request: ModelRequest<'a>,
    ) -> Pin<Box<dyn Future<Output = Result<ProviderResponse, ProviderFailure>> + Send + 'a>> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        let usage = ProviderUsage::reported(self.reported_tokens, self.reported_tokens);
        Box::pin(async move {
            Ok(ProviderResponse {
                text: Some("answer".to_string()),
                stop_reason: StopReason::Stop,
                usage,
            })
        })
    }
}

/// A dispatch that parks the caller until it is released.
///
/// This is what makes the aggregate-sharing test non-vacuous: it holds an
/// execution's reservation open — the execution cannot return, so its budget
/// cannot drop — while a second execution tries to reserve against the same
/// aggregate.
struct Gated {
    /// Signalled once a call has arrived and is parked.
    arrived: Arc<tokio::sync::Notify>,
    /// Awaited by the parked call; the test resolves it to let the call finish.
    release: Arc<tokio::sync::Semaphore>,
    calls: Arc<AtomicUsize>,
}

impl ModelDispatch for Gated {
    fn dispatch<'a>(
        &'a self,
        _request: ModelRequest<'a>,
    ) -> Pin<Box<dyn Future<Output = Result<ProviderResponse, ProviderFailure>> + Send + 'a>> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        self.arrived.notify_waiters();
        Box::pin(async move {
            let permit = self
                .release
                .acquire()
                .await
                .expect("the release semaphore must stay open");
            permit.forget();
            Ok(ProviderResponse {
                text: Some("answer".to_string()),
                stop_reason: StopReason::Stop,
                usage: ProviderUsage::reported(1.0, 1.0),
            })
        })
    }
}

fn dispatch(reported_tokens: f64) -> (Arc<dyn ModelDispatch>, Arc<AtomicUsize>) {
    let calls = Arc::new(AtomicUsize::new(0));
    (
        Arc::new(AlwaysOk {
            calls: Arc::clone(&calls),
            reported_tokens,
        }),
        calls,
    )
}

/// `text` is `string | null` — a completion cut off at the output cap is `ok:
/// false` and still carries partial text — so every script narrows it.
const CALL: &str = r#"import llm from "submilli:llm";
function main(): string {
    const t = llm.call("test-model", "hi").text;
    return t === null ? "NULL" : (t as string);
}"#;

/// Catches, so a refusal is readable as a value rather than as a trap.
const CATCH: &str = r#"import llm from "submilli:llm";
function main(): string {
    try {
        const t = llm.call("test-model", "hi").text;
        return "OK:" + (t === null ? "NULL" : (t as string));
    } catch (e: Error) {
        return e.message;
    }
}"#;

struct Harness {
    state: AppState,
    _vfs_root: tempfile::TempDir,
}

impl Harness {
    /// A harness with a working dispatch and default budgets.
    fn new() -> Self {
        Self::with_config(|c| c, dispatch(1.0).0)
    }

    fn with_config(
        tweak: impl FnOnce(ServerConfig) -> ServerConfig,
        dispatch: Arc<dyn ModelDispatch>,
    ) -> Self {
        Self::build(tweak, Some(dispatch))
    }

    fn build(
        tweak: impl FnOnce(ServerConfig) -> ServerConfig,
        dispatch: Option<Arc<dyn ModelDispatch>>,
    ) -> Self {
        let vfs_root = tempfile::tempdir().expect("vfs root");
        let config = ServerConfig {
            blueprints: Some(Arc::new(InMemoryBlueprintStore::seed(vec![blueprint(
                BLUEPRINT,
            )]))),
            session_storage_root: Some(vfs_root.path().to_path_buf()),
            llm_dispatch: dispatch,
            ..ServerConfig::default()
        };
        Self {
            state: AppState::new(tweak(config)).expect("AppState"),
            _vfs_root: vfs_root,
        }
    }

    async fn send(&self, req: Request<Body>) -> (StatusCode, HeaderMap, Value) {
        let resp = app(self.state.clone()).oneshot(req).await.unwrap();
        let status = resp.status();
        let headers = resp.headers().clone();
        let content_type = headers
            .get("content-type")
            .and_then(|v| v.to_str().ok())
            .unwrap_or("")
            .to_string();
        let bytes = resp.into_body().collect().await.unwrap().to_bytes();
        (status, headers, parse_body(&content_type, &bytes))
    }

    async fn post(&self, uri: &str, body: Value) -> (StatusCode, Value) {
        let req = Request::builder()
            .method("POST")
            .uri(uri)
            .header("host", "localhost")
            .header("content-type", "application/json")
            .header("accept", "application/json, text/event-stream")
            .body(Body::from(body.to_string()))
            .unwrap();
        let (status, _, body) = self.send(req).await;
        (status, body)
    }

    async fn get(&self, uri: &str) -> (StatusCode, Value) {
        let req = Request::builder()
            .method("GET")
            .uri(uri)
            .header("host", "localhost")
            .body(Body::empty())
            .unwrap();
        let (status, _, body) = self.send(req).await;
        (status, body)
    }

    async fn active_sessions(&self) -> u64 {
        self.get("/v1/status").await.1["active_sessions"]
            .as_u64()
            .expect("active_sessions count")
    }

    async fn create_session(&self, name: &str) -> String {
        let (status, body) = self
            .post("/v1/sessions", json!({ "blueprint": name }))
            .await;
        assert_eq!(status, StatusCode::OK, "create session: {body}");
        body["session_id"].as_str().expect("session id").to_string()
    }

    async fn rest(&self, session: &str, code: &str) -> Value {
        let (status, body) = self
            .post(
                &format!("/v1/sessions/{session}/execute"),
                json!({ "code": code }),
            )
            .await;
        assert_eq!(status, StatusCode::OK, "rest execute: {body}");
        body
    }

    async fn one_shot(&self, code: &str) -> Value {
        let (status, body) = self
            .post(
                "/v1/execute",
                json!({ "code": code, "blueprint": BLUEPRINT }),
            )
            .await;
        assert_eq!(status, StatusCode::OK, "one-shot execute: {body}");
        body
    }

    async fn mcp(&self, name: &str, session: &str, code: &str) -> Value {
        let req = Request::builder()
            .method("POST")
            .uri(format!("/mcp/{name}"))
            .header("host", "localhost")
            .header("content-type", "application/json")
            .header("accept", "application/json, text/event-stream")
            .header("mcp-session-id", session)
            .body(Body::from(
                json!({
                    "jsonrpc": "2.0",
                    "id": 2,
                    "method": "tools/call",
                    "params": {
                        "name": "submilli__typescript__execute",
                        "arguments": { "code": code },
                    }
                })
                .to_string(),
            ))
            .unwrap();
        let (status, _, body) = self.send(req).await;
        assert_eq!(status, StatusCode::OK, "mcp execute: {body}");
        let text = body["result"]["content"][0]["text"]
            .as_str()
            .unwrap_or_else(|| panic!("mcp tool result: {body}"));
        serde_json::from_str(text).unwrap_or_else(|_| json!({ "raw": text }))
    }

    async fn mcp_handshake(&self, name: &str) -> String {
        let req = Request::builder()
            .method("POST")
            .uri(format!("/mcp/{name}"))
            .header("host", "localhost")
            .header("content-type", "application/json")
            .header("accept", "application/json, text/event-stream")
            .body(Body::from(
                json!({
                    "jsonrpc": "2.0",
                    "id": 1,
                    "method": "initialize",
                    "params": {
                        "protocolVersion": "2025-06-18",
                        "capabilities": {},
                        "clientInfo": { "name": "test", "version": "0" }
                    }
                })
                .to_string(),
            ))
            .unwrap();
        let (status, headers, body) = self.send(req).await;
        assert_eq!(status, StatusCode::OK, "initialize: {body}");
        let session = headers
            .get("mcp-session-id")
            .expect("initialize must assign a session id")
            .to_str()
            .unwrap()
            .to_string();
        let (status, _) = self
            .post_to(
                &format!("/mcp/{name}"),
                json!({ "jsonrpc": "2.0", "method": "notifications/initialized" }),
                &session,
            )
            .await;
        assert_eq!(status, StatusCode::ACCEPTED, "initialized notification");
        session
    }

    async fn post_to(&self, uri: &str, body: Value, session: &str) -> (StatusCode, Value) {
        let req = Request::builder()
            .method("POST")
            .uri(uri)
            .header("host", "localhost")
            .header("content-type", "application/json")
            .header("accept", "application/json, text/event-stream")
            .header("mcp-session-id", session)
            .body(Body::from(body.to_string()))
            .unwrap();
        let (status, _, body) = self.send(req).await;
        (status, body)
    }
}

fn parse_body(content_type: &str, bytes: &[u8]) -> Value {
    if bytes.is_empty() {
        return Value::Null;
    }
    if content_type.starts_with("text/event-stream") {
        let text = String::from_utf8_lossy(bytes);
        let data = text
            .lines()
            .filter_map(|l| l.strip_prefix("data:"))
            .next_back()
            .unwrap_or("")
            .trim();
        return serde_json::from_str(data).unwrap_or(Value::Null);
    }
    serde_json::from_slice(bytes).unwrap_or(Value::Null)
}

fn ok(body: &Value) -> &Value {
    assert!(body["error"].is_null(), "execution failed: {body}");
    body
}

/// The provider must reach both routes. A provider wired into only one is
/// exactly the failure this wiring exists to prevent, so MCP is asserted
/// separately rather than assumed.
#[tokio::test]
async fn a_configured_provider_reaches_both_execution_routes() {
    let h = Harness::new();

    let session = h.create_session(BLUEPRINT).await;
    assert_eq!(
        ok(&h.rest(&session, CALL).await)["result"],
        json!("answer"),
        "REST: the configured provider must dispatch"
    );

    let mcp_session = h.mcp_handshake(BLUEPRINT).await;
    assert_eq!(
        ok(&h.mcp(BLUEPRINT, &mcp_session, CALL).await)["result"],
        json!("answer"),
        "MCP: the configured provider must dispatch"
    );

    assert_eq!(
        ok(&h.one_shot(CALL).await)["result"],
        json!("answer"),
        "one-shot: the configured provider must dispatch"
    );
}

/// R12: with no dispatch installed, a program gets the *catchable*
/// configuration error naming the model — not an internal failure.
#[tokio::test]
async fn no_provider_configured_is_a_catchable_configuration_error_on_both_routes() {
    let h = Harness::build(|c| c, None);

    let session = h.create_session(BLUEPRINT).await;
    let over_rest = ok(&h.rest(&session, CATCH).await)["result"]
        .as_str()
        .expect("a string result")
        .to_string();
    assert!(
        over_rest.contains("no model provider is configured")
            || over_rest.contains("not configured"),
        "REST: the refusal must say the provider is unconfigured: {over_rest}"
    );
    assert!(
        over_rest.contains(MODEL),
        "REST: the refusal must name the model: {over_rest}"
    );

    let mcp_session = h.mcp_handshake(BLUEPRINT).await;
    let over_mcp = ok(&h.mcp(BLUEPRINT, &mcp_session, CATCH).await)["result"]
        .as_str()
        .expect("a string result")
        .to_string();
    assert!(
        over_mcp.contains(MODEL),
        "MCP: the refusal must name the model: {over_mcp}"
    );
}

/// R9: the server-wide ceiling is *one* budget, shared across concurrent
/// sessions — not a per-session copy of the same number.
///
/// **The concurrency is load-bearing, and a sequential version of this test is
/// vacuous.** A budget releases on `Drop`, so once the first execution returns
/// its reservation is gone and a second call fits whatever the aggregate is —
/// the test would pass against a per-session budget too. So the second call must
/// be *in flight* while the first still holds its reservation: the fake dispatch
/// blocks until both executions have reached it, and only then answers.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn the_aggregate_budget_is_shared_across_two_concurrent_sessions() {
    // One call reserves OUTPUT_CAP plus its input estimate, so 1500 admits one
    // and refuses a second taken while the first is still held.
    const AGGREGATE: u64 = 1_500;
    let calls = Arc::new(AtomicUsize::new(0));
    let arrived = Arc::new(tokio::sync::Notify::new());
    let release = Arc::new(tokio::sync::Semaphore::new(0));
    let gated: Arc<dyn ModelDispatch> = Arc::new(Gated {
        arrived: Arc::clone(&arrived),
        release: Arc::clone(&release),
        calls: Arc::clone(&calls),
    });
    let h = Arc::new(Harness::with_config(
        |c| ServerConfig {
            max_llm_tokens: Some(AGGREGATE),
            llm_limits: limits(1_000_000),
            ..c
        },
        gated,
    ));

    let first_session = h.create_session(BLUEPRINT).await;
    let second_session = h.create_session(BLUEPRINT).await;

    // Park the first execution inside the provider, so its reservation is still
    // charged against the aggregate when the second one reserves.
    let parked = {
        let h = Arc::clone(&h);
        let session = first_session.clone();
        let wait = arrived.notified();
        let task = tokio::spawn(async move { h.rest(&session, CATCH).await });
        wait.await;
        task
    };

    // Bounded, so an unshared budget fails this assertion rather than hanging:
    // a second execution that is *admitted* parks in the gated provider forever,
    // and a hang reads as flakiness instead of as the bug it is.
    let second = tokio::time::timeout(
        std::time::Duration::from_secs(10),
        h.rest(&second_session, CATCH),
    )
    .await
    .expect(
        "the second concurrent call should have been refused before dispatch; it instead \
         reached the provider and parked, which means it got its own copy of the \
         server-wide ceiling",
    );
    let refused = ok(&second)["result"].as_str().expect("string").to_string();
    assert!(
        !refused.starts_with("OK:"),
        "a second concurrent session must not get its own copy of the server-wide \
         ceiling: {refused}"
    );

    // Let the parked call finish, and confirm it was the one that held the budget.
    release.add_permits(1);
    let allowed = ok(&parked.await.expect("the parked execution"))["result"]
        .as_str()
        .expect("string")
        .to_string();
    assert!(
        allowed.starts_with("OK:"),
        "the first session's call must fit the aggregate: {allowed}"
    );
    // Which ceiling refused decides what to do about it: this execution's own
    // spend is not what is in the way, so a message reading like the
    // per-execution limit would send the caller to shrink work that cannot help.
    assert!(
        refused.contains("server token budget"),
        "the refusal must name the aggregate, not the per-execution ceiling: {refused}"
    );
    assert!(
        refused.contains("--max-llm-tokens"),
        "the aggregate refusal must point at the operator's knob: {refused}"
    );

    // The refusal is charged before dispatch, so the second call never reached
    // the provider — this is what makes it a ceiling rather than an after-the-fact
    // accounting of spend that already happened.
    assert_eq!(
        calls.load(Ordering::SeqCst),
        1,
        "the refused call must not have been dispatched"
    );
}

/// The per-execution ceiling is not the aggregate: a single run is bounded even
/// when the server-wide budget has ample headroom.
#[tokio::test]
async fn the_per_execution_ceiling_bounds_one_run_below_the_aggregate() {
    let (dispatch, calls) = dispatch(1.0);
    let h = Harness::with_config(
        |c| ServerConfig {
            // Ample aggregate, tight per-execution ceiling: only the latter can
            // be what refuses.
            max_llm_tokens: Some(1_000_000),
            llm_limits: limits(500),
            ..c
        },
        dispatch,
    );

    let session = h.create_session(BLUEPRINT).await;
    let refused = ok(&h.rest(&session, CATCH).await)["result"]
        .as_str()
        .expect("string")
        .to_string();
    assert!(
        refused.contains("execution token budget"),
        "the per-execution ceiling must be what refuses: {refused}"
    );
    assert_eq!(
        calls.load(Ordering::SeqCst),
        0,
        "a call over the per-execution ceiling must not be dispatched"
    );
}

/// A one-shot's reservation returns to the aggregate when its run ends, so the
/// stateless route can be used repeatedly without ratcheting the server-wide
/// budget to its cap.
///
/// A successful call reconciles its own over-reservation down to reported usage
/// before returning, so this covers the reconcile path. The `Drop` path — an
/// execution that ends *without* reconciling — is
/// [`a_trapped_run_releases_its_reservation_on_drop`].
#[tokio::test]
async fn a_one_shot_releases_its_reservation_when_its_transient_session_drops() {
    const AGGREGATE: u64 = 1_500;
    let (dispatch, calls) = dispatch(1.0);
    let h = Harness::with_config(
        |c| ServerConfig {
            max_llm_tokens: Some(AGGREGATE),
            llm_limits: limits(1_000_000),
            ..c
        },
        dispatch,
    );

    for i in 0..8 {
        let run = ok(&h.one_shot(CATCH).await)["result"]
            .as_str()
            .expect("string")
            .to_string();
        assert!(
            run.starts_with("OK:"),
            "one-shot {i} was refused, so an earlier one never released its tokens: {run}"
        );
        assert_eq!(
            h.active_sessions().await,
            0,
            "one-shot {i} left its session registered"
        );
    }

    // The budget the one-shots would otherwise have exhausted is intact, so a
    // real session can still call — the failure an operator actually sees.
    let session = h.create_session(BLUEPRINT).await;
    let after = ok(&h.rest(&session, CATCH).await)["result"]
        .as_str()
        .expect("string")
        .to_string();
    assert!(
        after.starts_with("OK:"),
        "a real session must still fit the budget after eight one-shots: {after}"
    );
    assert_eq!(
        calls.load(Ordering::SeqCst),
        9,
        "every call should have reached the provider"
    );
}

/// Indeterminate spend is charged to the aggregate and **stays** charged when
/// the execution ends — the deliberate exception to release-on-drop.
///
/// A provider that reports no usage may still have billed, so `null` means
/// indeterminate, not free. Releasing it at teardown would let a caller who can
/// induce null usage spend past the server-wide ceiling: N executions each under
/// the per-execution ceiling, none of them ever tripping the aggregate.
///
/// So this asserts the *opposite* of the reconcile test above, and the pair is
/// what pins the distinction: reported usage comes back, held reserve does not.
#[tokio::test]
async fn indeterminate_spend_stays_charged_against_the_aggregate() {
    const AGGREGATE: u64 = 4_000;
    // Reports no usage at all, so every call's whole share becomes held reserve.
    let (dispatch, calls) = dispatch(f64::NAN);
    let h = Harness::with_config(
        |c| ServerConfig {
            max_llm_tokens: Some(AGGREGATE),
            llm_limits: limits(1_000_000),
            ..c
        },
        dispatch,
    );

    // Each call holds ~OUTPUT_CAP, so the aggregate is exhausted after a few —
    // even though no execution is still live. Drive one-shots until one is
    // refused, which must happen well inside this bound.
    let mut refusal = None;
    for _ in 0..12 {
        let run = ok(&h.one_shot(CATCH).await)["result"]
            .as_str()
            .expect("string")
            .to_string();
        if !run.starts_with("OK:") {
            refusal = Some(run);
            break;
        }
    }

    let refusal = refusal.expect(
        "indeterminate spend must accumulate against the aggregate; every call was \
         admitted, so held reserve was released at teardown and the server-wide \
         ceiling can be escaped by inducing null usage",
    );
    assert!(
        refusal.contains("server token budget"),
        "the aggregate must be what refuses once held reserve fills it: {refusal}"
    );
    assert!(
        calls.load(Ordering::SeqCst) > 0,
        "the calls that filled the budget must have reached the provider"
    );
}

/// Each execution gets its own per-execution ceiling: spend does not accumulate
/// across executes in one session the way session-KV bytes do.
///
/// **Indeterminate usage is what makes this non-vacuous.** A reported-usage call
/// reconciles almost its whole reservation back, so a budget wrongly cached on
/// the session would still admit many executes and the test would prove nothing.
/// Held reserve is never returned, so under a cached budget the second execute
/// is refused — which is exactly the per-execution ceiling silently becoming a
/// per-session one.
#[tokio::test]
async fn each_execute_gets_a_fresh_per_execution_budget() {
    let (dispatch, calls) = dispatch(f64::NAN);
    let h = Harness::with_config(
        |c| ServerConfig {
            max_llm_tokens: Some(1_000_000),
            // Room for exactly one call per execution, and no more.
            llm_limits: limits(1_500),
            ..c
        },
        dispatch,
    );

    let session = h.create_session(BLUEPRINT).await;
    for i in 0..4 {
        let run = ok(&h.rest(&session, CATCH).await)["result"]
            .as_str()
            .expect("string")
            .to_string();
        assert!(
            run.starts_with("OK:"),
            "execute {i} was refused, so the per-execution budget is behaving like a \
             per-session one: {run}"
        );
    }
    assert_eq!(calls.load(Ordering::SeqCst), 4);
}
