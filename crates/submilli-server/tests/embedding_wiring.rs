//! End-to-end tests for `submilli:embedding` bound to a server execution.
//!
//! The provider and the embedding budget must reach both execution routes, the
//! server-wide ceiling must be one budget shared across executions and separate
//! from the `submilli:llm` one, and a cancelled run must leave the server-wide
//! counter holding exactly what was sent.
//!
//! Dispatch is faked at the [`EmbeddingDispatch`] seam, so no provider is
//! contacted. Every alias is Hugging Face so a call spans several requests
//! (32 texts each) with an estimate of one token per three bytes.

use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use axum::body::Body;
use axum::http::{HeaderMap, Request, StatusCode};
use http_body_util::BodyExt;
use serde_json::{Value, json};
use submilli_blueprint::Blueprint;
use submilli_server::blueprint::InMemoryBlueprintStore;
use submilli_server::config::ServerConfig;
use submilli_server::{AppState, app};
use submilli_shared::embedding::{
    DispatchFailure, DispatchResponse, DispatchRow, EmbeddingDispatch, EmbeddingRequest,
};
use submilli_shared::llm::{
    ModelDispatch, ModelRequest, ProviderFailure, ProviderResponse, ProviderUsage, StopReason,
};
use tower::ServiceExt;

const BLUEPRINT: &str = "embed";
const DIMENSIONS: usize = 4;

fn blueprint() -> Blueprint {
    submilli_blueprint::parse(
        "name: embed\n\
         default: allow\n\
         llm:\n  providers:\n    fake:\n      type: anthropic\n  models:\n    test-model:\n      provider: fake\n\
         embedding:\n  providers:\n    hf:\n      type: huggingface\n      base_url: https://hf.example.com\n  models:\n    docs:\n      provider: hf\n      model: bge\n      dimensions: 4\n",
    )
    .expect("blueprint parses")
}

/// A dispatch that answers every request with unit vectors and records how many
/// requests and texts reached it. `usage` is what it reports per request: `None`
/// is a provider that reports nothing, whose spend stays indeterminate.
struct Fake {
    requests: Arc<AtomicUsize>,
    usage: Option<u64>,
    /// When set, every request parks here after signalling `arrived`, until
    /// a permit is added.
    gate: Option<Gate>,
}

#[derive(Clone)]
struct Gate {
    arrived: Arc<tokio::sync::Notify>,
    release: Arc<tokio::sync::Semaphore>,
}

impl EmbeddingDispatch for Fake {
    fn dispatch<'a>(
        &'a self,
        request: EmbeddingRequest<'a>,
    ) -> Pin<Box<dyn Future<Output = Result<DispatchResponse, DispatchFailure>> + Send + 'a>> {
        self.requests.fetch_add(1, Ordering::SeqCst);
        let rows: Vec<DispatchRow> = request
            .texts
            .iter()
            .map(|_| DispatchRow {
                index: None,
                values: (0..DIMENSIONS)
                    .map(|i| if i == 0 { 1.0 } else { 0.0 })
                    .collect(),
            })
            .collect();
        let usage = self.usage;
        let gate = self.gate.clone();
        Box::pin(async move {
            if let Some(gate) = gate {
                gate.arrived.notify_waiters();
                let permit = gate
                    .release
                    .acquire()
                    .await
                    .expect("the release semaphore must stay open");
                permit.forget();
            }
            Ok(DispatchResponse { rows, usage })
        })
    }
}

fn dispatch(usage: Option<u64>) -> (Arc<dyn EmbeddingDispatch>, Arc<AtomicUsize>) {
    let requests = Arc::new(AtomicUsize::new(0));
    (
        Arc::new(Fake {
            requests: Arc::clone(&requests),
            usage,
            gate: None,
        }),
        requests,
    )
}

fn gated(gate: &Gate, usage: Option<u64>) -> (Arc<dyn EmbeddingDispatch>, Arc<AtomicUsize>) {
    let requests = Arc::new(AtomicUsize::new(0));
    (
        Arc::new(Fake {
            requests: Arc::clone(&requests),
            usage,
            gate: Some(gate.clone()),
        }),
        requests,
    )
}

fn new_gate() -> Gate {
    Gate {
        arrived: Arc::new(tokio::sync::Notify::new()),
        release: Arc::new(tokio::sync::Semaphore::new(0)),
    }
}

/// The `submilli:llm` side of the blueprint, answering every call.
fn llm_dispatch() -> Arc<dyn ModelDispatch> {
    struct AlwaysOk;
    impl ModelDispatch for AlwaysOk {
        fn dispatch<'a>(
            &'a self,
            _request: ModelRequest<'a>,
        ) -> Pin<Box<dyn Future<Output = Result<ProviderResponse, ProviderFailure>> + Send + 'a>>
        {
            Box::pin(async move {
                Ok(ProviderResponse {
                    text: Some("answer".to_string()),
                    stop_reason: StopReason::Stop,
                    usage: ProviderUsage::reported(1.0, 1.0),
                })
            })
        }
    }
    Arc::new(AlwaysOk)
}

/// A script embedding `count` three-byte texts (one estimated token each),
/// catching so a refusal reads as a value.
fn embed_script(count: usize) -> String {
    format!(
        r#"import embedding from "submilli:embedding";
function main(): string {{
    try {{
        const texts: string[] = [];
        for (let i = 0; i < {count}; i++) {{ texts.push("abc"); }}
        const r = embedding.embed("docs", texts, "document");
        return "OK:" + r.count.toString() + ":" + r.dimensions.toString() + ":" + r.vector(0)[0].toString();
    }} catch (e: Error) {{
        return e.message;
    }}
}}"#
    )
}

const LLM_CALL: &str = r#"import llm from "submilli:llm";
function main(): string {
    try {
        const t = llm.call("test-model", "hi").text;
        return "OK:" + (t === null ? "NULL" : (t as string));
    } catch (e: Error) {
        return e.message;
    }
}"#;

const SERVER_REFUSAL: &str = "server embedding token budget";

struct Harness {
    state: AppState,
    _vfs_root: tempfile::TempDir,
}

impl Harness {
    /// A harness with a working dispatch and default budgets.
    fn new() -> Self {
        Self::with_config(|c| c, dispatch(None).0)
    }

    fn with_config(
        tweak: impl FnOnce(ServerConfig) -> ServerConfig,
        dispatch: Arc<dyn EmbeddingDispatch>,
    ) -> Self {
        let vfs_root = tempfile::tempdir().expect("vfs root");
        let config = ServerConfig {
            blueprints: Some(Arc::new(
                InMemoryBlueprintStore::seed(vec![blueprint()]).expect("seed blueprints"),
            )),
            session_storage_root: Some(vfs_root.path().to_path_buf()),
            llm_dispatch: Some(llm_dispatch()),
            embedding_dispatch: Some(dispatch),
            ..ServerConfig::default()
        };
        Self {
            state: futures::executor::block_on(AppState::new(tweak(config))).expect("AppState"),
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

fn result_of(body: &Value) -> String {
    ok(body)["result"]
        .as_str()
        .expect("a string result")
        .to_string()
}

/// The provider must reach both routes; a provider wired into only one is the
/// failure this wiring exists to prevent.
#[tokio::test]
async fn a_configured_provider_reaches_both_execution_routes() {
    let h = Harness::new();
    let script = embed_script(2);
    let expected = "OK:2:4:1";

    let session = h.create_session(BLUEPRINT).await;
    assert_eq!(
        result_of(&h.rest(&session, &script).await),
        expected,
        "REST"
    );

    let mcp_session = h.mcp_handshake(BLUEPRINT).await;
    assert_eq!(
        result_of(&h.mcp(BLUEPRINT, &mcp_session, &script).await),
        expected,
        "MCP"
    );

    assert_eq!(result_of(&h.one_shot(&script).await), expected, "one-shot");
}

/// The real HTTP dispatch is the default; with none overridden, `models()` still
/// resolves, which proves a provider was built from the blueprint.
#[tokio::test]
async fn the_default_provider_is_installed_when_no_dispatch_is_overridden() {
    let vfs_root = tempfile::tempdir().expect("vfs root");
    let state = AppState::new(ServerConfig {
        blueprints: Some(Arc::new(
            InMemoryBlueprintStore::seed(vec![blueprint()]).expect("seed blueprints"),
        )),
        session_storage_root: Some(vfs_root.path().to_path_buf()),
        ..ServerConfig::default()
    })
    .await
    .expect("AppState");
    let h = Harness {
        state,
        _vfs_root: vfs_root,
    };
    let session = h.create_session(BLUEPRINT).await;
    let listed = result_of(
        &h.rest(
            &session,
            r#"import embedding from "submilli:embedding";
function main(): string {
    const names: string[] = [];
    for (const m of embedding.models()) { names.push(m.name); }
    return names.join(",");
}"#,
        )
        .await,
    );
    assert_eq!(listed, "docs");
}

/// AE6: the server-wide embedding ceiling is one budget shared by concurrent
/// sessions. The first execution is parked in the provider so its reservation is
/// still charged when the second reserves; a sequential version would pass
/// against a per-session budget too.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn the_aggregate_budget_is_shared_across_two_concurrent_sessions() {
    // Ten tokens per call: room for one at a time.
    let gate = new_gate();
    let (dispatch, requests) = gated(&gate, None);
    let h = Arc::new(Harness::with_config(
        |c| ServerConfig {
            max_embedding_tokens: Some(15),
            ..c
        },
        dispatch,
    ));
    let first = h.create_session(BLUEPRINT).await;
    let second = h.create_session(BLUEPRINT).await;

    let parked = {
        let h = Arc::clone(&h);
        let wait = gate.arrived.notified();
        let task = tokio::spawn(async move { h.rest(&first, &embed_script(10)).await });
        wait.await;
        task
    };

    let refused = tokio::time::timeout(
        std::time::Duration::from_secs(10),
        h.rest(&second, &embed_script(10)),
    )
    .await
    .expect("the second call should be refused before dispatch, not parked");
    let refused = result_of(&refused);
    assert!(refused.contains(SERVER_REFUSAL), "{refused}");
    assert!(refused.contains("--max-embedding-tokens"), "{refused}");

    gate.release.add_permits(1);
    let allowed = result_of(&parked.await.expect("the parked execution"));
    assert!(allowed.starts_with("OK:"), "{allowed}");
    assert_eq!(
        requests.load(Ordering::SeqCst),
        1,
        "the refused call must not have been dispatched"
    );
}

/// Exhausting the embedding budget leaves the `llm.call` budget untouched.
#[tokio::test]
async fn exhausting_the_embedding_budget_leaves_llm_calls_working() {
    // No reported usage: every call's estimate stays held, so the aggregate
    // fills after a few one-shots with no execution still live.
    let (dispatch, _) = dispatch(None);
    let h = Harness::with_config(
        |c| ServerConfig {
            max_embedding_tokens: Some(25),
            ..c
        },
        dispatch,
    );
    let mut refusal = None;
    for _ in 0..6 {
        let run = result_of(&h.one_shot(&embed_script(10)).await);
        if !run.starts_with("OK:") {
            refusal = Some(run);
            break;
        }
    }
    let refusal = refusal.expect("held spend must fill the embedding aggregate");
    assert!(refusal.contains(SERVER_REFUSAL), "{refusal}");

    assert_eq!(result_of(&h.one_shot(LLM_CALL).await), "OK:answer");
}

/// A Hugging Face call reports no usage, so its estimate stays held on the
/// server-wide counter after its run ends — and a provider that reports usage
/// returns it.
#[tokio::test]
async fn unreported_usage_stays_held_after_the_run_and_reported_usage_returns() {
    // 30 tokens held of 40 leaves room for 10, not 11.
    let (dispatch, _) = dispatch(None);
    let h = Harness::with_config(
        |c| ServerConfig {
            max_embedding_tokens: Some(40),
            ..c
        },
        dispatch,
    );
    assert!(result_of(&h.one_shot(&embed_script(30)).await).starts_with("OK:"));
    let refused = result_of(&h.one_shot(&embed_script(11)).await);
    assert!(refused.contains(SERVER_REFUSAL), "{refused}");
    assert!(result_of(&h.one_shot(&embed_script(10)).await).starts_with("OK:"));

    // The same sequence with reported usage never accumulates.
    let (dispatch, _) = dispatch_reporting(30);
    let h = Harness::with_config(
        |c| ServerConfig {
            max_embedding_tokens: Some(40),
            ..c
        },
        dispatch,
    );
    for i in 0..5 {
        let run = result_of(&h.one_shot(&embed_script(30)).await);
        assert!(
            run.starts_with("OK:"),
            "one-shot {i} was refused, so reported usage was not returned: {run}"
        );
    }
}

fn dispatch_reporting(per_request: u64) -> (Arc<dyn EmbeddingDispatch>, Arc<AtomicUsize>) {
    // 30 one-token texts fit one request (32 per request).
    dispatch(Some(per_request))
}

/// A request whose client goes away mid-embed is not cancelled: the server owns
/// the run independently of the connection. While the run is parked its sent
/// spend stays charged on the server-wide counter, and once it finishes it
/// settles every sub-batch against the provider's reported usage.
///
/// 33 one-token texts split into a request of 32 and one of 1; concurrency 1
/// parks the first while the second is still unsent. The provider reports one
/// token per request, less than the 32-token estimate of the first, so a run
/// that skipped settlement would leave all 33 estimated tokens held, while a
/// settled run returns the aggregate to zero, leaving room for a 40-token call.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_dropped_request_keeps_its_spend_while_running_and_settles_when_it_ends() {
    let gate = new_gate();
    let (dispatch, _) = gated(&gate, Some(1));
    let h = Arc::new(Harness::with_config(
        |c| ServerConfig {
            max_embedding_tokens: Some(40),
            max_embedding_concurrency: Some(1),
            ..c
        },
        dispatch,
    ));
    let session = h.create_session(BLUEPRINT).await;

    let task = {
        let h = Arc::clone(&h);
        let session = session.clone();
        let wait = gate.arrived.notified();
        let task = tokio::spawn(async move { h.rest(&session, &embed_script(33)).await });
        wait.await;
        task
    };
    task.abort();
    assert!(task.await.is_err(), "the request was cancelled");

    // The first request is parked and its run is alive. The dropped HTTP request
    // did not release the 33 reserved tokens: 33 + 8 passes 40, so eight are refused.
    let refused = result_of(&h.one_shot(&embed_script(8)).await);
    assert!(
        refused.contains(SERVER_REFUSAL),
        "the dropped request's sent spend must stay charged while its run is alive: {refused}"
    );

    // The run outlives its dropped request. Let both sub-batches through.
    gate.release.add_permits(16);

    // A 40-token call fits only once the run has ended and settled to the
    // reported usage (the reported spend returns to the aggregate when the run's
    // budget drops). Skipped settlement would leave 33 held and refuse it
    // forever. Bounded by a timeout, not a sleep: the condition is the settlement.
    let admitted = tokio::time::timeout(std::time::Duration::from_secs(20), async {
        loop {
            let run = result_of(&h.one_shot(&embed_script(40)).await);
            if run.starts_with("OK:") {
                return;
            }
            assert!(run.contains(SERVER_REFUSAL), "{run}");
            tokio::task::yield_now().await;
            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        }
    })
    .await;
    assert!(
        admitted.is_ok(),
        "the dropped request's run never settled its reservation"
    );
}
