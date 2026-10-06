//! Recorded-world connectors: a run's HTTP client, MCP transport and model provider
//! answered from a recorded run, enforcing the current blueprint and stopping the run at
//! the first call with nothing recorded.
//!
//! The end-to-end tests record a program with the real call log and the real auth proxy,
//! then run a program against the recording through the same host functions. The runner's
//! own path (and so `test_program`) comes later; `run` here stands in for its cancel
//! handling.

use std::collections::{BTreeMap, HashMap};
use std::sync::{Arc, Mutex};

use interpreter::runtime::call_log::{MASKED, Payload, mask_headers, mask_url};
use interpreter::runtime::mcp::request_digest as mcp_digest;
use interpreter::runtime::{
    BodyCopy, CallOutcome, CallRecord, DecisionLog, DecisionLogConfig, DecisionLogOutput,
    FailureReason, LlmCallError, LlmFailure, LlmModel, LlmOutcome, LlmProvider, McpCallError,
    McpTransport, PayloadRecord, StoreData, Vfs, dispatch_main_async,
    install_runtime_host_functions, install_runtime_store_bound, install_tenant_limits,
};
use interpreter::stdlib::http::NetworkPolicy;
use interpreter::stdlib::http::transport::{DownloadMeta, DownloadProgress};
use interpreter::stdlib::http::{
    HttpClient, HttpError, HttpRequest, HttpResponse, RecordedRequest, RedirectHop,
};
use interpreter::{FileId, RuntimeConfig, compile_script};
use serde_json::{Value, json};
use submilli_blueprint::{Blueprint, McpServer, VarBindings};
use submilli_server::record::replay::Miss;
use submilli_server::record::{
    Cassette, LiveReach, MissReason, RecordedHttpClient, RecordedLlmProvider, RecordedMcpTransport,
    RecordedRun, ReplayReport,
};
use submilli_shared::mcp::StreamableHttpTransport;
use submilli_shared::{BlueprintAuthProxy, PolicyCheck};
use tokio::sync::oneshot;
use wasmtime::{Linker, Module};

// ---- the harness -----------------------------------------------------------------------

/// The live world a program is recorded against: answers each request with its path and how
/// many times that path was asked, and keeps what it was sent.
#[derive(Default)]
struct Live {
    seen: Mutex<Vec<HttpRequest>>,
    asked: Mutex<HashMap<String, u32>>,
    /// Asks for [`HttpRequest::recorded_as`], and keeps what it was given.
    capture: bool,
    recorded: Mutex<Vec<Option<RecordedRequest>>>,
    /// `/start` redirects here, through the request's guard.
    redirect_to: Option<&'static str>,
    /// Panics when asked anything: a run that must not reach outside.
    forbidden: bool,
    /// Cuts every response body to this many bytes after the program saw it whole.
    body: Option<String>,
}

#[async_trait::async_trait]
impl HttpClient for Live {
    fn wants_recorded_request(&self) -> bool {
        self.capture
    }

    async fn send(&self, req: &HttpRequest) -> Result<HttpResponse, HttpError> {
        assert!(!self.forbidden, "an outbound request was made: {}", req.url);
        self.seen.lock().unwrap().push(req.clone());
        self.recorded.lock().unwrap().push(req.recorded_as.clone());
        let url = url::Url::parse(&req.url).unwrap();
        let path = url.path().to_owned();
        if path == "/down" {
            return Err(HttpError::Timeout);
        }
        let mut final_url = req.url.clone();
        let mut path = path;
        if let (Some(target), Some(guard), "/start") =
            (self.redirect_to, req.redirect_guard.as_ref(), path.as_str())
        {
            let hop_url = url::Url::parse(target).unwrap();
            guard
                .authorize(&RedirectHop {
                    method: "GET",
                    url: &hop_url,
                    method_rewritten: false,
                    body_len: 0,
                })
                .map_err(HttpError::PermissionDenied)?;
            target.clone_into(&mut final_url);
            path = hop_url.path().to_owned();
        }
        let n = {
            let mut asked = self.asked.lock().unwrap();
            let n = asked.entry(path.clone()).or_insert(0);
            *n += 1;
            *n
        };
        Ok(HttpResponse {
            status: 200,
            status_text: "OK".into(),
            headers: vec![
                ("set-cookie".into(), "session=hidden".into()),
                ("content-type".into(), "text/plain".into()),
            ],
            body: self
                .body
                .clone()
                .unwrap_or_else(|| format!("{path}:{n}"))
                .into_bytes(),
            final_url,
        })
    }

    async fn download(
        &self,
        _req: &HttpRequest,
        _writer: &mut (dyn std::io::Write + Send),
    ) -> Result<DownloadMeta, HttpError> {
        unreachable!("not used")
    }
}

#[tokio::test]
async fn a_recorded_refusal_from_configuration_is_not_replayed_over_todays() {
    let mut setup = Setup::new(ALLOW_ALL, Arc::new(Live::default()));
    setup.llm = Some(Arc::new(FakeLlm {
        calls: Mutex::new(0),
    }));
    let recorded = run(TRANSPORT_DOWN, setup).await;
    for kind in [
        "unknown-model",
        "not-configured",
        "unauthorized",
        "budget-exceeded",
        "prompt-bounds-exceeded",
        "from-the-future",
    ] {
        let mut recording = recorded.recorded(TRANSPORT_DOWN);
        for call in &mut recording.calls {
            let mut response = call.response.as_deref().cloned().expect("response");
            response.meta["call_error"]["kind"] = json!(kind);
            call.response = Some(Box::new(response));
        }
        let (cancel, mut requested) = oneshot::channel();
        let cassette = Cassette::new(&recording, cancel);
        let declared = Arc::new(FakeLlm {
            calls: Mutex::new(0),
        });
        let provider = RecordedLlmProvider::new(cassette.clone(), declared.clone());
        let prompts = ["transport down".to_owned()];
        // The model is declared today, so the recorded refusal does not answer for it.
        provider
            .call("open", &prompts, None)
            .await
            .expect_err("stopped");
        assert!(requested.try_recv().is_ok(), "{kind}");
        let report = cassette.report();
        assert_eq!(
            miss_of(&report).reason,
            MissReason::RecordedFailure,
            "{kind}"
        );
        assert_eq!(*declared.calls.lock().unwrap(), 0);

        // Live goes to the provider, which decides today.
        let (cancel, _requested) = oneshot::channel();
        let cassette = Cassette::new(&recording, cancel);
        let provider = RecordedLlmProvider::new(cassette.clone(), declared.clone()).with_live();
        let error = provider
            .call("open", &prompts, None)
            .await
            .expect_err("the provider's own failure");
        assert!(matches!(error, LlmCallError::Transport { .. }), "{kind}");
        assert_eq!(*declared.calls.lock().unwrap(), 1);
        assert_eq!(cassette.report().went_live.len(), 1);
    }
}

/// Answers every prompt `answer`, except `refuse me`, which it refuses with a failure, and
/// `transport down`, which fails the whole call.
struct FakeLlm {
    calls: Mutex<u32>,
}

impl LlmProvider for FakeLlm {
    fn call<'a>(
        &'a self,
        model: &'a str,
        prompts: &'a [String],
        _schema_json: Option<&'a str>,
    ) -> std::pin::Pin<
        Box<dyn std::future::Future<Output = Result<Vec<LlmOutcome>, LlmCallError>> + Send + 'a>,
    > {
        *self.calls.lock().unwrap() += 1;
        if prompts.iter().any(|prompt| prompt == "transport down") {
            let error = LlmCallError::Transport {
                model: model.to_owned(),
                detail: "connection reset".to_owned(),
            };
            return Box::pin(async move { Err(error) });
        }
        let outcomes = prompts
            .iter()
            .map(|prompt| {
                if prompt == "refuse locally" {
                    let failure =
                        LlmFailure::new(FailureReason::Transport, "no route").refused_locally();
                    LlmOutcome::failed(failure, None::<String>)
                } else if prompt == "refuse me" {
                    let failure = LlmFailure::new(FailureReason::ContentFiltered, "filtered")
                        .with_status(400)
                        .with_finish_reason("safety");
                    LlmOutcome::failed(failure, Some("partial")).with_usage(Some(3), None)
                } else {
                    LlmOutcome::success("answer").with_usage(Some(10), Some(10))
                }
            })
            .collect();
        Box::pin(async move { Ok(outcomes) })
    }

    fn models<'a>(
        &'a self,
    ) -> std::pin::Pin<
        Box<dyn std::future::Future<Output = Result<Vec<LlmModel>, LlmCallError>> + Send + 'a>,
    > {
        Box::pin(async { Ok(vec![LlmModel::new("open")]) })
    }

    fn output_reserve(&self, _model: &str) -> Option<u64> {
        Some(64)
    }
}

struct Setup {
    blueprint: String,
    secrets: BTreeMap<String, String>,
    http: Arc<dyn HttpClient>,
    llm: Option<Arc<dyn LlmProvider>>,
    log: DecisionLogConfig,
    /// The run's cancel signal, as the runner holds it.
    cancel: Option<oneshot::Receiver<()>>,
}

impl Setup {
    fn new(blueprint: &str, http: Arc<dyn HttpClient>) -> Self {
        Self {
            blueprint: blueprint.to_owned(),
            secrets: BTreeMap::new(),
            http,
            llm: None,
            log: DecisionLogConfig::default(),
            cancel: None,
        }
    }
}

struct Outcome {
    result: Result<String, String>,
    /// The run ended because its cancel signal fired.
    cancelled: bool,
    log: DecisionLogOutput,
}

impl Outcome {
    /// The run as a recording.
    fn recorded(&self, source: &str) -> RecordedRun {
        RecordedRun {
            execution_id: "run-1".into(),
            blueprint_name: "bp".into(),
            blueprint_hash: None,
            code: Some(source.to_owned()),
            session_id: None,
            variables: VarBindings::new(),
            decisions: self.log.records.clone(),
            calls: self.log.calls.clone(),
            log_truncated: self.log.truncated,
            mcp_catalog: None,
        }
    }

    /// What the policy decided, in order: the decision list a test run must reproduce.
    fn decisions(&self) -> Vec<(String, String, bool, Option<usize>)> {
        self.log
            .records
            .iter()
            .map(|record| {
                (
                    record.caller.clone(),
                    record.capability.clone(),
                    record.allowed,
                    record.rule,
                )
            })
            .collect()
    }
}

/// Runs `source` as the server's runner would: the policy and auth proxy of the blueprint,
/// the call log, and the cancel signal winning over a guest that carries on.
async fn run(source: &str, setup: Setup) -> Outcome {
    let blueprint = Arc::new(submilli_blueprint::parse(&setup.blueprint).expect("blueprint"));
    let cfg = RuntimeConfig::default();
    let engine = cfg.engine_async().expect("engine");
    let script = compile_script(source, "main.ts", FileId(0), &[], &[])
        .unwrap_or_else(|d| panic!("script: {d:#?}"));

    let mut data = StoreData::with_vfs(Vfs::tempdir().expect("tempdir"));
    data.install_type_info(script.type_info.clone());
    data.http_client = setup.http;
    data.auth_proxy = Arc::new(BlueprintAuthProxy::with_harness(
        Arc::clone(&blueprint),
        None,
        Arc::new(setup.secrets),
    ));
    data.security_check = Arc::new(PolicyCheck::new(blueprint));
    if let Some(llm) = setup.llm {
        data.llm_provider = Some(llm);
    }
    let log = DecisionLog::install(&mut data, setup.log);

    let mut store = cfg.store_async(&engine, data).expect("store");
    install_tenant_limits(&mut store);
    let mut linker = Linker::<StoreData>::new(&engine);
    install_runtime_host_functions(&mut linker).expect("host functions");
    install_runtime_store_bound(&mut linker, &mut store).expect("store-bound functions");
    let module = Module::new(&engine, &script.wasm).expect("module");
    let instance = linker
        .instantiate_async(&mut store, &module)
        .await
        .expect("instantiate");

    let execution = async {
        dispatch_main_async(&mut store, &instance)
            .await
            .map(Option::unwrap_or_default)
            .map_err(|error| format!("{error:#}"))
    };
    // As `run_inner` does: only a sent cancel counts.
    let cancel_sent = async {
        if let Some(requested) = setup.cancel
            && requested.await.is_ok()
        {
            return;
        }
        std::future::pending::<()>().await;
    };
    let (result, cancelled) = tokio::select! {
        biased;
        () = cancel_sent => (Err("execution cancelled".to_owned()), true),
        result = execution => (result, false),
    };
    Outcome {
        result,
        cancelled,
        log: log.finish(),
    }
}

const ALLOW_ALL: &str = "name: bp\ndefault: allow\n";

/// A blueprint that allows everything but `http.get` to `host`.
fn deny_get_to(host: &str) -> String {
    format!(
        "name: bp\ndefault: allow\npermissions:\n  main:\n    - capability: http.get\n      filter: host == \"{host}\"\n      action: deny\n"
    )
}

/// A blueprint that allows `http.get` to `hosts` only.
fn allow_get_to(hosts: &[&str]) -> String {
    let rules: String = hosts
        .iter()
        .map(|host| {
            format!(
                "    - capability: http.get\n      filter: host == \"{host}\"\n      action: allow\n"
            )
        })
        .collect();
    format!("name: bp\ndefault: deny\npermissions:\n  main:\n{rules}")
}

/// The test run's world: recorded connectors over `recording`, and the cancel receiver.
fn replay_setup(blueprint: &str, recording: &RecordedRun) -> (Setup, Arc<Cassette>) {
    let (cancel, requested) = oneshot::channel();
    let cassette = Cassette::new(recording, cancel);
    let mut setup = Setup::new(
        blueprint,
        Arc::new(RecordedHttpClient::new(cassette.clone())),
    );
    setup.cancel = Some(requested);
    (setup, cassette)
}

fn miss_of(report: &ReplayReport) -> &Miss {
    report.miss.as_ref().expect("the run stopped at a miss")
}

const FETCH: &str = r#"
import { get } from "submilli:http";
function main(): string {
  const headers = new Map<string, string>([["X-Trace", "a"], ["Authorization", "Bearer stale"]]);
  return get("https://api.test/v1/items?limit=2", headers).body;
}
"#;

// ---- the digest pin ----------------------------------------------------------------------

const AUTH_BLUEPRINT: &str = "\
name: bp
default: allow
secrets:
  K: { harness: {} }
auth_proxy:
  - host: api.test
    headers:
      X-Api-Version: \"2\"
      Authorization: \"Bearer ${secrets.K}\"
    query:
      api_key: \"${secrets.K}\"
";

#[tokio::test]
async fn the_digest_the_call_log_recorded_reaches_the_transport_through_the_auth_proxy() {
    let live = Arc::new(Live {
        capture: true,
        ..Live::default()
    });
    let mut setup = Setup::new(AUTH_BLUEPRINT, live.clone());
    setup.secrets.insert("K".into(), "tok-1".into());
    let recorded = run(FETCH, setup).await;
    assert_eq!(recorded.result, Ok("/v1/items:1".to_owned()));

    let call = &recorded.log.calls[0];
    let request = call.request.as_ref().expect("the request");
    let seen = live.seen.lock().unwrap()[0].clone();
    // The proxy added headers (beside the script's own credential, which it matches by
    // exact name), reordered them, and appended a query parameter: the request the
    // transport holds is not the one the log keyed.
    assert!(seen.url.contains("api_key=tok-1"), "{}", seen.url);
    assert_eq!(
        seen.headers
            .iter()
            .map(|(n, _)| n.as_str())
            .collect::<Vec<_>>(),
        ["X-Trace", "Authorization", "authorization", "x-api-version"]
    );
    // Deriving the digest again from what the transport holds would not match.
    let (headers, _) = mask_headers(&seen.headers);
    let rederived = Payload::meta(json!({
        "method": seen.method,
        "url": mask_url(&seen.url),
        "headers": headers,
    }))
    .with_body(&seen.body)
    .digest();
    assert_ne!(rederived, request.digest);
    // What the host function handed the transport is the call log's own.
    let given = live.recorded.lock().unwrap()[0]
        .clone()
        .expect("recorded_as");
    assert_eq!(given.digest, request.digest);
    assert_eq!(
        Some(given.masked_url.as_str()),
        request.meta["url"].as_str()
    );
}

#[tokio::test]
async fn a_refreshed_auth_header_still_matches() {
    let live = Arc::new(Live::default());
    let mut setup = Setup::new(AUTH_BLUEPRINT, live);
    setup.secrets.insert("K".into(), "tok-1".into());
    let recorded = run(FETCH, setup).await;

    let recording = recorded.recorded(FETCH);
    let (mut setup, cassette) = replay_setup(AUTH_BLUEPRINT, &recording);
    // The credential was refreshed since the recording.
    setup.secrets.insert("K".into(), "tok-2".into());
    let replayed = run(FETCH, setup).await;

    assert_eq!(replayed.result, Ok("/v1/items:1".to_owned()));
    let report = cassette.report();
    assert!(report.miss.is_none(), "{:?}", report.miss);
    assert_eq!(report.served.len(), 1);
    assert_eq!(report.served[0].source_call_index, 0);
    // The served call's digest is the one the test run's own log records for it.
    assert_eq!(
        replayed.log.calls[0]
            .request
            .as_ref()
            .map(|request| request.digest.as_str()),
        report.served[0].request_digests.first().map(String::as_str)
    );
}

// ---- matching ----------------------------------------------------------------------------

#[tokio::test]
async fn recordings_of_one_request_are_served_in_order_whatever_order_the_program_asks() {
    let source = r#"
import { get } from "submilli:http";
function main(): string {
  return [get("https://x.test/a").body, get("https://x.test/b").body, get("https://x.test/a").body].join(",");
}
"#;
    let recorded = run(source, Setup::new(ALLOW_ALL, Arc::new(Live::default()))).await;
    assert_eq!(recorded.result, Ok("/a:1,/b:1,/a:2".to_owned()));

    let reordered = r#"
import { get } from "submilli:http";
function main(): string {
  const b = get("https://x.test/b").body;
  const a1 = get("https://x.test/a").body;
  const a2 = get("https://x.test/a").body;
  return [b, a1, a2].join(",");
}
"#;
    let (setup, cassette) = replay_setup(ALLOW_ALL, &recorded.recorded(source));
    let replayed = run(reordered, setup).await;
    assert_eq!(replayed.result, Ok("/b:1,/a:1,/a:2".to_owned()));
    let served: Vec<u64> = cassette
        .report()
        .served
        .iter()
        .map(|served| served.source_call_index)
        .collect();
    assert_eq!(served, [1, 0, 2]);
}

#[tokio::test]
async fn a_request_that_differs_stops_the_run_and_cannot_be_caught() {
    let recorded = run(
        r#"
import { get } from "submilli:http";
function main(): string {
  return get("https://x.test/a", new Map<string, string>([["X-Page", "1"]])).body;
}
"#,
        Setup::new(ALLOW_ALL, Arc::new(Live::default())),
    )
    .await;
    let source = r#"
import { get } from "submilli:http";
function main(): string {
  try {
    return get("https://x.test/a", new Map<string, string>([["X-Page", "2"]])).body;
  } catch (e) { return "caught"; }
}
"#;
    let (setup, cassette) = replay_setup(ALLOW_ALL, &recorded.recorded(""));
    let replayed = run(source, setup).await;

    assert!(replayed.cancelled, "{:?}", replayed.result);
    assert_ne!(replayed.result, Ok("caught".to_owned()));
    let report = cassette.report();
    let miss = miss_of(&report);
    assert_eq!(miss.reason, MissReason::RequestDiffers);
    assert_eq!(miss.key, "http GET https://x.test/a");
    assert_eq!(
        miss.nearest.as_ref().map(|n| (n.call_index, n.used)),
        Some((0, false))
    );
    assert!(report.served.is_empty());
}

#[tokio::test]
async fn a_call_the_recording_never_made_stops_at_it_after_serving_the_first() {
    // The second call was denied when recorded; a new rule allows it.
    let source = r#"
import { get } from "submilli:http";
function main(): string {
  const a = get("https://a.test/one").body;
  let b = "none";
  try { b = get("https://b.test/two").body; } catch (e) { b = "denied"; }
  return a + "|" + b;
}
"#;
    let recorded = run(
        source,
        Setup::new(&allow_get_to(&["a.test"]), Arc::new(Live::default())),
    )
    .await;
    assert_eq!(recorded.result, Ok("/one:1|denied".to_owned()));
    let recording = recorded.recorded(source);

    let (setup, cassette) = replay_setup(&allow_get_to(&["a.test", "b.test"]), &recording);
    let replayed = run(source, setup).await;

    assert!(replayed.cancelled);
    let report = cassette.report();
    assert_eq!(report.served.len(), 1);
    let miss = miss_of(&report);
    assert_eq!(miss.reason, MissReason::NoRecording);
    assert_eq!(miss.key, "http GET https://b.test/two");
    // The test run's own log ends with the stopping call, unfinished.
    let stopped = replayed.log.calls.last().expect("the stopping call");
    assert_eq!(stopped.outcome, Some(CallOutcome::Unfinished));
    assert_eq!(stopped.capability, "http.get");
}

#[tokio::test]
async fn an_unchanged_program_and_blueprint_decide_alike_and_reach_nowhere() {
    let source = r#"
import { get, post } from "submilli:http";
function main(): string {
  const a = get("https://a.test/one").body;
  const b = post("https://b.test/two", "payload").body;
  return a + b;
}
"#;
    let recorded = run(source, Setup::new(ALLOW_ALL, Arc::new(Live::default()))).await;
    let (setup, cassette) = replay_setup(ALLOW_ALL, &recorded.recorded(source));
    let replayed = run(source, setup).await;

    assert_eq!(replayed.result, recorded.result);
    assert_eq!(replayed.decisions(), recorded.decisions());
    assert!(!replayed.cancelled);
    assert_eq!(cassette.report().served.len(), 2);
}

#[tokio::test]
async fn a_call_that_is_now_denied_does_not_use_up_a_recording_later_calls_need() {
    let source = r#"
import { get } from "submilli:http";
function main(): string {
  const a = get("https://a.test/x").body;
  let b = "none";
  try { b = get("https://b.test/x").body; } catch (e) { b = "denied"; }
  const c = get("https://c.test/x").body;
  return [a, b, c].join(",");
}
"#;
    let recorded = run(source, Setup::new(ALLOW_ALL, Arc::new(Live::default()))).await;
    assert_eq!(recorded.result, Ok("/x:1,/x:2,/x:3".to_owned()));
    let (setup, cassette) = replay_setup(&deny_get_to("b.test"), &recorded.recorded(source));
    let replayed = run(source, setup).await;

    assert_eq!(replayed.result, Ok("/x:1,denied,/x:3".to_owned()));
    let served: Vec<u64> = cassette
        .report()
        .served
        .iter()
        .map(|served| served.source_call_index)
        .collect();
    assert_eq!(served, [0, 2]);
}

#[tokio::test]
async fn a_recorded_transport_failure_is_raised_again_from_its_kind() {
    let source = r#"
import { get } from "submilli:http";
function main(): string {
  try { get("https://x.test/down"); } catch (e) { return "failed"; }
  return "answered";
}
"#;
    let recorded = run(source, Setup::new(ALLOW_ALL, Arc::new(Live::default()))).await;
    assert_eq!(recorded.result, Ok("failed".to_owned()));
    let (setup, cassette) = replay_setup(ALLOW_ALL, &recorded.recorded(source));
    let replayed = run(source, setup).await;
    assert_eq!(replayed.result, Ok("failed".to_owned()));
    assert!(cassette.report().miss.is_none());
}

#[tokio::test]
async fn masked_header_values_are_served_as_masked() {
    let source = r#"
import { get } from "submilli:http";
function main(): string {
  const r = get("https://x.test/a");
  return String(r.headers.get("set-cookie")) + "," + String(r.headers.get("content-type"));
}
"#;
    let recorded = run(source, Setup::new(ALLOW_ALL, Arc::new(Live::default()))).await;
    assert_eq!(recorded.result, Ok("session=hidden,text/plain".to_owned()));
    let (setup, _) = replay_setup(ALLOW_ALL, &recorded.recorded(source));
    let replayed = run(source, setup).await;
    assert_eq!(replayed.result, Ok(format!("{MASKED},text/plain")));
}

// ---- incomplete recordings ---------------------------------------------------------------

#[tokio::test]
async fn a_truncated_body_is_a_recording_incomplete() {
    let source = r#"
import { get } from "submilli:http";
function main(): string { return get("https://x.test/a").body; }
"#;
    let mut setup = Setup::new(
        ALLOW_ALL,
        Arc::new(Live {
            body: Some("0123456789".into()),
            ..Live::default()
        }),
    );
    setup.log.max_payload_bytes = 4;
    let recorded = run(source, setup).await;
    assert_eq!(recorded.result, Ok("0123456789".to_owned()));

    let (setup, cassette) = replay_setup(ALLOW_ALL, &recorded.recorded(source));
    let replayed = run(source, setup).await;
    assert!(replayed.cancelled);
    let report = cassette.report();
    assert_eq!(miss_of(&report).reason, MissReason::RecordingIncomplete);
    // The recording was matched, so it is the nearest one, and unused.
    assert_eq!(
        miss_of(&report).nearest.as_ref().map(|n| n.used),
        Some(false)
    );
}

#[tokio::test]
async fn a_recording_kept_as_a_digest_alone_is_a_recording_incomplete() {
    let source = r#"
import { get } from "submilli:http";
function main(): string { return get("https://x.test/a").body; }
"#;
    let recorded = run(source, Setup::new(ALLOW_ALL, Arc::new(Live::default()))).await;
    let mut recording = recorded.recorded(source);
    recording.calls = recording
        .calls
        .iter()
        .map(CallRecord::without_bodies)
        .collect();
    let (setup, cassette) = replay_setup(ALLOW_ALL, &recording);
    let replayed = run(source, setup).await;
    // The request keeps its digest, so the call is recognized; its response is gone.
    assert!(replayed.cancelled);
    let report = cassette.report();
    assert_eq!(miss_of(&report).reason, MissReason::RecordingIncomplete);
}

// ---- redirects ---------------------------------------------------------------------------

const REDIRECTED: &str = r#"
import { get } from "submilli:http";
function main(): string {
  try { return get("https://api.test/start").body; } catch (e) { return "refused"; }
}
"#;

fn redirecting() -> Arc<Live> {
    Arc::new(Live {
        redirect_to: Some("https://cdn.test/landing"),
        ..Live::default()
    })
}

#[tokio::test]
async fn a_recorded_redirect_is_served_once_its_hops_pass_the_current_blueprint() {
    let recorded = run(REDIRECTED, Setup::new(ALLOW_ALL, redirecting())).await;
    assert_eq!(recorded.result, Ok("/landing:1".to_owned()));
    let (setup, cassette) = replay_setup(ALLOW_ALL, &recorded.recorded(REDIRECTED));
    let replayed = run(REDIRECTED, setup).await;
    assert_eq!(replayed.result, Ok("/landing:1".to_owned()));
    assert!(cassette.report().miss.is_none());
    // The hop was authorized again: the test run records its own decision for it.
    assert_eq!(replayed.decisions(), recorded.decisions());
}

#[tokio::test]
async fn a_narrowed_redirect_target_is_refused_by_the_guard_as_a_live_client_would() {
    let recorded = run(REDIRECTED, Setup::new(ALLOW_ALL, redirecting())).await;
    let (setup, cassette) = replay_setup(&deny_get_to("cdn.test"), &recorded.recorded(REDIRECTED));
    let replayed = run(REDIRECTED, setup).await;

    assert_eq!(replayed.result, Ok("refused".to_owned()));
    assert!(!replayed.cancelled);
    assert!(cassette.report().miss.is_none());
    let hop = replayed
        .log
        .records
        .iter()
        .find(|record| {
            matches!(
                record.entry_path,
                interpreter::runtime::EntryPath::RedirectHop { .. }
            )
        })
        .expect("the hop's decision");
    assert!(!hop.allowed);
}

// ---- model calls -------------------------------------------------------------------------

const BATCH: &str = r#"
import llm from "submilli:llm";
function main(): string {
  const done = llm.batch("open", ["fine", "refuse me"]);
  return done.map((c) => c.ok ? "ok" : (c.reason ?? "?")).join(",");
}
"#;

#[tokio::test]
async fn a_model_batch_is_served_whole_with_each_outcomes_own_failure() {
    let provider = Arc::new(FakeLlm {
        calls: Mutex::new(0),
    });
    let mut setup = Setup::new(ALLOW_ALL, Arc::new(Live::default()));
    setup.llm = Some(provider.clone());
    let recorded = run(BATCH, setup).await;
    assert_eq!(recorded.result, Ok("ok,content-filtered".to_owned()));
    assert_eq!(*provider.calls.lock().unwrap(), 1);

    let (cancel, requested) = oneshot::channel();
    let cassette = Cassette::new(&recorded.recorded(BATCH), cancel);
    let declared = Arc::new(FakeLlm {
        calls: Mutex::new(0),
    });
    let mut setup = Setup::new(ALLOW_ALL, Arc::new(Live::default()));
    setup.llm = Some(Arc::new(RecordedLlmProvider::new(
        cassette.clone(),
        declared.clone(),
    )));
    setup.cancel = Some(requested);
    let replayed = run(BATCH, setup).await;

    assert_eq!(replayed.result, Ok("ok,content-filtered".to_owned()));
    // The provider that declares the models was never asked for a completion.
    assert_eq!(*declared.calls.lock().unwrap(), 0);
    assert_eq!(cassette.report().served.len(), 1);
    // The test run's call log records what the source's did.
    let (source, test) = (&recorded.log.calls[0], &replayed.log.calls[0]);
    let response = |call: &CallRecord| {
        let response = call.response.as_ref().expect("the reply");
        (response.meta.clone(), response.digest.clone())
    };
    assert_eq!(response(source), response(test));
    assert_eq!(source.usage, test.usage);
}

#[tokio::test]
async fn a_recorded_cancelled_outcome_is_a_miss_as_the_cancel_was_the_source_runs() {
    let mut setup = Setup::new(ALLOW_ALL, Arc::new(Live::default()));
    setup.llm = Some(Arc::new(FakeLlm {
        calls: Mutex::new(0),
    }));
    let recorded = run(BATCH, setup).await;
    let mut recording = recorded.recorded(BATCH);
    for call in &mut recording.calls {
        let mut response = call.response.as_deref().cloned().expect("response");
        response.meta["failures"][1]["kind"] = json!("cancelled");
        call.response = Some(Box::new(response));
    }
    let (cancel, requested) = oneshot::channel();
    let cassette = Cassette::new(&recording, cancel);
    let mut setup = Setup::new(ALLOW_ALL, Arc::new(Live::default()));
    setup.llm = Some(Arc::new(RecordedLlmProvider::new(
        cassette.clone(),
        Arc::new(FakeLlm {
            calls: Mutex::new(0),
        }),
    )));
    setup.cancel = Some(requested);
    let replayed = run(BATCH, setup).await;
    assert!(replayed.cancelled);
    assert_eq!(
        miss_of(&cassette.report()).reason,
        MissReason::RecordedFailure
    );
}

#[tokio::test]
async fn a_local_refusal_is_recorded_as_local_and_is_a_miss_on_replay() {
    const LOCAL: &str = r#"
import llm from "submilli:llm";
function main(): string {
  return llm.batch("open", ["fine", "refuse locally"]).map((c) => c.ok ? "ok" : (c.reason ?? "?")).join(",");
}
"#;
    let mut setup = Setup::new(ALLOW_ALL, Arc::new(Live::default()));
    setup.llm = Some(Arc::new(FakeLlm {
        calls: Mutex::new(0),
    }));
    let recorded = run(LOCAL, setup).await;
    // The guest saw a transport failure, as ever.
    assert_eq!(recorded.result, Ok("ok,transport".to_owned()));
    let response = recorded.log.calls[0].response.as_ref().expect("reply");
    assert_eq!(response.meta["failures"][1]["kind"], json!("local"));

    let (cancel, requested) = oneshot::channel();
    let cassette = Cassette::new(&recorded.recorded(LOCAL), cancel);
    let mut setup = Setup::new(ALLOW_ALL, Arc::new(Live::default()));
    setup.llm = Some(Arc::new(RecordedLlmProvider::new(
        cassette.clone(),
        Arc::new(FakeLlm {
            calls: Mutex::new(0),
        }),
    )));
    setup.cancel = Some(requested);
    let replayed = run(LOCAL, setup).await;
    assert!(replayed.cancelled);
    assert_eq!(
        miss_of(&cassette.report()).reason,
        MissReason::RecordedFailure
    );
}

#[tokio::test]
async fn only_a_recorded_outcome_failure_from_the_provider_is_served() {
    let mut setup = Setup::new(ALLOW_ALL, Arc::new(Live::default()));
    setup.llm = Some(Arc::new(FakeLlm {
        calls: Mutex::new(0),
    }));
    let recorded = run(BATCH, setup).await;
    for (kind, status, served) in [
        ("rate-limited", json!(429), true),
        ("provider-unavailable", json!(503), true),
        ("transport", json!(null), true),
        ("request-rejected", json!(400), true),
        ("request-rejected", json!(401), false),
        ("request-rejected", json!(403), false),
        ("cancelled", json!(null), false),
        ("local", json!(null), false),
        ("from-the-future", json!(null), false),
    ] {
        let mut recording = recorded.recorded(BATCH);
        for call in &mut recording.calls {
            let mut response = call.response.as_deref().cloned().expect("response");
            response.meta["failures"][1]["kind"] = json!(kind);
            response.meta["failures"][1]["status"] = status.clone();
            call.response = Some(Box::new(response));
        }
        let (cancel, requested) = oneshot::channel();
        let cassette = Cassette::new(&recording, cancel);
        let mut setup = Setup::new(ALLOW_ALL, Arc::new(Live::default()));
        setup.llm = Some(Arc::new(RecordedLlmProvider::new(
            cassette.clone(),
            Arc::new(FakeLlm {
                calls: Mutex::new(0),
            }),
        )));
        setup.cancel = Some(requested);
        let replayed = run(BATCH, setup).await;
        if served {
            assert_eq!(replayed.result, Ok(format!("ok,{kind}")), "{kind} {status}");
        } else {
            assert!(replayed.cancelled, "{kind} {status}");
            assert_eq!(
                miss_of(&cassette.report()).reason,
                MissReason::RecordedFailure,
                "{kind} {status}"
            );
        }
    }
}

#[tokio::test]
async fn a_model_call_with_other_prompts_stops_the_run() {
    let mut setup = Setup::new(ALLOW_ALL, Arc::new(Live::default()));
    setup.llm = Some(Arc::new(FakeLlm {
        calls: Mutex::new(0),
    }));
    let recorded = run(BATCH, setup).await;

    let other = BATCH.replace("fine", "other");
    let (cancel, requested) = oneshot::channel();
    let cassette = Cassette::new(&recorded.recorded(BATCH), cancel);
    let mut setup = Setup::new(ALLOW_ALL, Arc::new(Live::default()));
    setup.llm = Some(Arc::new(RecordedLlmProvider::new(
        cassette.clone(),
        Arc::new(FakeLlm {
            calls: Mutex::new(0),
        }),
    )));
    setup.cancel = Some(requested);
    let replayed = run(&other, setup).await;
    assert!(replayed.cancelled);
    let report = cassette.report();
    assert_eq!(miss_of(&report).reason, MissReason::RequestDiffers);
    assert_eq!(miss_of(&report).key, "llm open");
}

// ---- direct: MCP, downloads, and the cancel signal ---------------------------------------

fn payload(meta: Value, body: Option<&str>, digest: &str) -> PayloadRecord {
    PayloadRecord {
        meta,
        body: body.map(|body| BodyCopy::Text(body.to_owned())),
        digest: digest.to_owned(),
        bytes: body.map_or(0, str::len) as u64,
        truncated: false,
        masked_headers: Vec::new(),
    }
}

fn call(
    call_index: u64,
    capability: &str,
    request: PayloadRecord,
    response: Option<PayloadRecord>,
) -> CallRecord {
    CallRecord {
        call_index,
        caller: "main".into(),
        capability: capability.into(),
        started_micros: 0,
        ended_micros: Some(1),
        outcome: Some(if response.is_some() {
            CallOutcome::Returned
        } else {
            CallOutcome::Unfinished
        }),
        line: None,
        request: Some(Box::new(request)),
        response: response.map(Box::new),
        usage: None,
    }
}

fn recording_of(calls: Vec<CallRecord>) -> RecordedRun {
    RecordedRun {
        execution_id: "run-1".into(),
        blueprint_name: "bp".into(),
        blueprint_hash: None,
        code: None,
        session_id: None,
        variables: VarBindings::new(),
        decisions: Vec::new(),
        calls,
        log_truncated: false,
        mcp_catalog: None,
    }
}

/// A blueprint declaring the MCP server `files`.
fn files_blueprint() -> Arc<Blueprint> {
    Arc::new(Blueprint {
        name: "bp".into(),
        mcp: BTreeMap::from([(
            "files".to_string(),
            McpServer {
                transport: "streamable_http".into(),
                url: "http://127.0.0.1:9/mcp".into(),
                headers: BTreeMap::new(),
                auth: None,
            },
        )]),
        ..Default::default()
    })
}

fn mcp_call(index: u64, args: &str, response: PayloadRecord) -> CallRecord {
    let request = payload(
        json!({ "server": "files", "tool": "read" }),
        Some(args),
        &mcp_digest("files", "read", args),
    );
    call(index, "mcp.files", request, Some(response))
}

async fn mcp(transport: &RecordedMcpTransport, args: &str) -> interpreter::runtime::McpOutcome {
    transport.call("files", "read", args).await
}

#[tokio::test]
async fn mcp_responses_are_rebuilt_through_the_bounded_path_and_served_in_order() {
    let run = recording_of(vec![
        mcp_call(
            0,
            r#"{"p":1}"#,
            payload(Value::Null, Some(r#"{"text":"one"}"#), "r0"),
        ),
        mcp_call(
            1,
            r#"{"p":1}"#,
            payload(Value::Null, Some(r#"{"text":"two"}"#), "r1"),
        ),
        mcp_call(2, r#"{"p":2}"#, payload(Value::Null, Some("[1,2]"), "r2")),
    ]);
    let (cancel, mut requested) = oneshot::channel();
    let cassette = Cassette::new(&run, cancel);
    let transport = RecordedMcpTransport::new(cassette.clone(), files_blueprint());

    let value = |outcome: interpreter::runtime::McpOutcome| match outcome.result {
        Ok(response) => format!("{response:?}"),
        Err(error) => panic!("{error:?}"),
    };
    assert!(value(mcp(&transport, r#"{"p":2}"#).await).contains("Array"));
    assert!(value(mcp(&transport, r#"{"p":1}"#).await).contains("one"));
    let second = mcp(&transport, r#"{"p":1}"#).await;
    assert_eq!(second.received_bytes, r#"{"text":"two"}"#.len() as u64);
    assert!(value(second).contains("two"));
    assert!(requested.try_recv().is_err(), "nothing missed");

    let served: Vec<u64> = cassette
        .report()
        .served
        .iter()
        .map(|served| served.source_call_index)
        .collect();
    assert_eq!(served, [2, 0, 1]);

    // A fourth call has nothing left to answer it.
    let missed = mcp(&transport, r#"{"p":1}"#).await;
    assert!(matches!(missed.result, Err(McpCallError::Internal { .. })));
    assert!(
        requested.try_recv().is_ok(),
        "the miss fired the cancel signal"
    );
    assert_eq!(miss_of(&cassette.report()).reason, MissReason::NoRecording);
}

#[tokio::test]
async fn an_mcp_failure_that_is_not_an_answer_from_outside_is_a_miss_as_todays_configuration_decides()
 {
    for kind in ["local", "auth-expired", "internal", "from-the-future"] {
        let run = recording_of(vec![mcp_call(
            0,
            "1",
            payload(
                json!({ "kind": kind, "message": "no OAuth credential", "status": null }),
                None,
                "f",
            ),
        )]);
        let (cancel, mut requested) = oneshot::channel();
        let cassette = Cassette::new(&run, cancel);
        let transport = RecordedMcpTransport::new(cassette.clone(), files_blueprint());
        assert!(
            matches!(
                mcp(&transport, "1").await.result,
                Err(McpCallError::Internal { .. })
            ),
            "{kind}"
        );
        assert!(requested.try_recv().is_ok(), "{kind}");
        assert_eq!(
            miss_of(&cassette.report()).reason,
            MissReason::RecordedFailure,
            "{kind}"
        );
    }
}

#[tokio::test]
async fn a_recorded_token_endpoint_401_or_403_is_a_miss_as_todays_credential_decides() {
    for status in [401, 403] {
        let run = recording_of(vec![mcp_call(
            0,
            "1",
            payload(
                json!({ "kind": "upstream", "message": "denied", "status": status }),
                None,
                "f",
            ),
        )]);
        let (cancel, mut requested) = oneshot::channel();
        let cassette = Cassette::new(&run, cancel);
        let transport = RecordedMcpTransport::new(cassette.clone(), files_blueprint());
        assert!(matches!(
            mcp(&transport, "1").await.result,
            Err(McpCallError::Internal { .. })
        ));
        assert!(requested.try_recv().is_ok(), "{status}");
        assert_eq!(
            miss_of(&cassette.report()).reason,
            MissReason::RecordedFailure,
            "{status}"
        );
    }
}

#[tokio::test]
async fn mcp_failures_are_rebuilt_from_their_kind_and_an_unnamed_one_is_a_miss() {
    let failure = |meta: Value| payload(meta, None, "f");
    let run = recording_of(vec![
        mcp_call(
            0,
            "1",
            failure(json!({ "kind": "upstream", "message": "bad gateway", "status": 502 })),
        ),
        mcp_call(
            1,
            "2",
            failure(json!({ "kind": "transport", "message": "reset", "status": null })),
        ),
        mcp_call(
            2,
            "3",
            failure(
                json!({ "kind": "transport", "message": "token endpoint reset", "status": null }),
            ),
        ),
        mcp_call(
            3,
            "4",
            failure(json!({ "kind": "mcp", "message": "no such file", "status": null })),
        ),
        mcp_call(
            4,
            "5",
            failure(json!({ "kind": "response-too-large", "message": "", "status": null })),
        ),
        mcp_call(5, "6", failure(json!({ "error": "Transport(\"old\")" }))),
    ]);
    let (cancel, mut requested) = oneshot::channel();
    let cassette = Cassette::new(&run, cancel);
    let transport = RecordedMcpTransport::new(cassette.clone(), files_blueprint());

    assert!(matches!(
        mcp(&transport, "1").await.result,
        Err(McpCallError::Upstream { status: 502, body }) if body == "bad gateway"
    ));
    assert!(matches!(
        mcp(&transport, "2").await.result,
        Err(McpCallError::Transport(detail)) if detail == "reset"
    ));
    assert!(matches!(
        mcp(&transport, "3").await.result,
        Err(McpCallError::Transport(detail)) if detail == "token endpoint reset"
    ));
    assert!(matches!(
        mcp(&transport, "4").await.result,
        Err(McpCallError::Mcp { message }) if message == "no such file"
    ));
    assert!(matches!(
        mcp(&transport, "5").await.result,
        Err(McpCallError::ResponseTooLarge)
    ));
    assert!(requested.try_recv().is_err());

    // A failure recorded without a kind cannot be raised again.
    assert!(mcp(&transport, "6").await.result.is_err());
    assert_eq!(
        miss_of(&cassette.report()).reason,
        MissReason::RecordedFailure
    );
    assert!(requested.try_recv().is_ok());
}

fn http_request(url: &str, recorded: Option<RecordedRequest>) -> HttpRequest {
    HttpRequest {
        method: "GET".into(),
        url: url.into(),
        headers: Vec::new(),
        body: Vec::new(),
        timeout_ms: 1000,
        max_response_size: 1024,
        decompress: false,
        transport_policy: None,
        redirect_guard: None,
        recorded_as: recorded,
    }
}

#[tokio::test]
async fn a_download_always_misses_and_fires_the_cancel_signal() {
    let request = payload(
        json!({ "method": "GET", "url": "https://x.test/big.bin", "headers": [] }),
        Some(""),
        "d0",
    );
    let response = payload(
        json!({ "status": 200, "path": "/big.bin", "bytes_written": 9 }),
        None,
        "r",
    );
    let run = recording_of(vec![call(0, "http.download", request, Some(response))]);
    let (cancel, mut requested) = oneshot::channel();
    let cassette = Cassette::new(&run, cancel);
    let client = RecordedHttpClient::new(cassette.clone());

    let recorded = RecordedRequest {
        masked_url: "https://x.test/big.bin".into(),
        digest: "d0".into(),
    };
    let mut sink = Vec::new();
    let error = client
        .download(
            &http_request("https://x.test/big.bin", Some(recorded)),
            &mut sink,
        )
        .await
        .expect_err("a download is never answered");
    assert!(matches!(error, HttpError::Internal(_)), "{error}");

    let report = cassette.report();
    let miss = miss_of(&report);
    assert_eq!(miss.reason, MissReason::Download);
    assert_eq!(miss.nearest.as_ref().map(|n| n.call_index), Some(0));
    assert!(requested.try_recv().is_ok());
    assert!(sink.is_empty());
}

#[tokio::test]
async fn a_download_is_not_taken_for_a_get_of_the_same_request() {
    let meta = json!({ "method": "GET", "url": "https://x.test/f", "headers": [] });
    let run = recording_of(vec![call(
        0,
        "http.download",
        payload(meta, Some(""), "same"),
        Some(payload(json!({ "status": 200 }), None, "r")),
    )]);
    let (cancel, _requested) = oneshot::channel();
    let cassette = Cassette::new(&run, cancel);
    let client = RecordedHttpClient::new(cassette.clone());
    let recorded = RecordedRequest {
        masked_url: "https://x.test/f".into(),
        digest: "same".into(),
    };
    let error = client
        .send(&http_request("https://x.test/f", Some(recorded)))
        .await
        .expect_err("nothing recorded for a get");
    assert!(matches!(error, HttpError::Internal(_)));
    assert_eq!(miss_of(&cassette.report()).reason, MissReason::NoRecording);
}

#[tokio::test]
async fn a_request_that_carries_no_record_of_what_the_program_sent_misses() {
    let (cancel, mut requested) = oneshot::channel();
    let cassette = Cassette::new(&recording_of(Vec::new()), cancel);
    let client = RecordedHttpClient::new(cassette.clone());
    // A git fetch builds its own request; no host function keyed it.
    client
        .send(&http_request("https://x.test/info/refs", None))
        .await
        .expect_err("unrecorded");
    assert_eq!(miss_of(&cassette.report()).reason, MissReason::NoRecording);
    assert!(requested.try_recv().is_ok());
}

#[tokio::test]
async fn only_the_first_miss_is_the_stop() {
    let (cancel, _requested) = oneshot::channel();
    let cassette = Cassette::new(&recording_of(Vec::new()), cancel);
    let client = RecordedHttpClient::new(cassette.clone());
    for path in ["first", "second"] {
        let url = format!("https://x.test/{path}");
        let recorded = RecordedRequest {
            masked_url: url.clone(),
            digest: "d".into(),
        };
        client
            .send(&http_request(&url, Some(recorded)))
            .await
            .expect_err("no recording");
    }
    assert_eq!(
        miss_of(&cassette.report()).key,
        "http GET https://x.test/first"
    );
}

// ---- a proxy-injected credential never reaches a miss -----------------------------------

#[tokio::test]
async fn a_download_miss_names_the_programs_url_and_not_the_one_the_auth_proxy_rewrote() {
    let request = payload(
        json!({ "method": "GET", "url": "https://x.test/big.bin", "headers": [] }),
        Some(""),
        "d0",
    );
    let response = payload(json!({ "bytes_written": 9 }), None, "r");
    let run = recording_of(vec![call(0, "http.download", request, Some(response))]);
    let (cancel, _requested) = oneshot::channel();
    let cassette = Cassette::new(&run, cancel);
    let client = RecordedHttpClient::new(cassette.clone());

    // The proxy added `appid`, a name no mask knows, to the URL the transport holds.
    let recorded = RecordedRequest {
        masked_url: "https://x.test/big.bin".into(),
        digest: "d0".into(),
    };
    let error = client
        .download(
            &http_request("https://x.test/big.bin?appid=SECRET-VALUE", Some(recorded)),
            &mut Vec::new(),
        )
        .await
        .expect_err("a download is never answered");
    assert!(!error.to_string().contains("SECRET"), "{error}");
    let report = cassette.report();
    let miss = miss_of(&report);
    assert_eq!(miss.key, "http GET https://x.test/big.bin");
    assert_eq!(
        miss.nearest.as_ref().map(|nearest| nearest.call_index),
        Some(0),
        "the recorded download is the nearest"
    );
    assert!(!serde_json::to_string(&report).unwrap().contains("SECRET"));
}

#[tokio::test]
async fn a_request_with_no_record_is_keyed_without_its_query_or_userinfo() {
    let (cancel, _requested) = oneshot::channel();
    let cassette = Cassette::new(&recording_of(Vec::new()), cancel);
    let client = RecordedHttpClient::new(cassette.clone());
    let error = client
        .send(&http_request(
            "https://user:hunter2@x.test/info/refs?service=git&token=SECRET",
            None,
        ))
        .await
        .expect_err("unrecorded");
    let report = cassette.report();
    let text = format!("{error} {}", serde_json::to_string(&report).unwrap());
    assert!(
        !text.contains("SECRET") && !text.contains("hunter2"),
        "{text}"
    );
    assert_eq!(
        miss_of(&report).key,
        format!("http GET https://{MASKED}@x.test/info/refs")
    );
}

/// Reports the wire bytes of a download through the progress it is given.
struct ProgressSpy;

#[async_trait::async_trait]
impl HttpClient for ProgressSpy {
    async fn send(&self, _req: &HttpRequest) -> Result<HttpResponse, HttpError> {
        unreachable!("not used")
    }

    async fn download(
        &self,
        _req: &HttpRequest,
        _writer: &mut (dyn std::io::Write + Send),
    ) -> Result<DownloadMeta, HttpError> {
        unreachable!("a counted download goes through the progress")
    }

    async fn download_with_progress(
        &self,
        _req: &HttpRequest,
        _writer: &mut (dyn std::io::Write + Send),
        progress: &DownloadProgress,
    ) -> Result<DownloadMeta, HttpError> {
        progress.received(42);
        Ok(DownloadMeta {
            status: 200,
            status_text: "OK".into(),
            headers: Vec::new(),
            final_url: "https://x.test/f".into(),
            bytes_written: 42,
        })
    }
}

#[tokio::test]
async fn a_live_download_is_counted_through_its_progress() {
    let (cancel, _requested) = oneshot::channel();
    let cassette = Cassette::new(&recording_of(Vec::new()), cancel);
    let client = RecordedHttpClient::new(cassette.clone()).with_live(
        Arc::new(ProgressSpy),
        submilli_server::record::LiveReach::Everything,
    );
    let progress = DownloadProgress::default();
    let recorded = RecordedRequest {
        masked_url: "https://x.test/f".into(),
        digest: "d".into(),
    };
    client
        .download_with_progress(
            &http_request("https://x.test/f", Some(recorded)),
            &mut Vec::new(),
            &progress,
        )
        .await
        .expect("the live download");
    assert_eq!(progress.bytes_received(), 42);
    assert_eq!(cassette.report().went_live.len(), 1);
}

// ---- redirect chains ---------------------------------------------------------------------

/// `recording` with its one redirect hop replaced by hops numbered `indexes`, each shown to
/// the guard as the same address.
fn with_hops(mut recording: RecordedRun, indexes: &[u32]) -> RecordedRun {
    let is_hop = |record: &interpreter::runtime::DecisionRecord| {
        matches!(
            record.entry_path,
            interpreter::runtime::EntryPath::RedirectHop { .. }
        )
    };
    let template = recording
        .decisions
        .iter()
        .find(|record| is_hop(record))
        .expect("the recorded hop")
        .clone();
    recording.decisions.retain(|record| !is_hop(record));
    for &index in indexes {
        let mut hop = template.clone();
        let interpreter::runtime::EntryPath::RedirectHop {
            parent_call_index, ..
        } = hop.entry_path
        else {
            unreachable!("the template is a hop")
        };
        hop.entry_path = interpreter::runtime::EntryPath::RedirectHop {
            parent_call_index,
            index,
        };
        recording.decisions.push(hop);
    }
    recording
}

async fn replay_redirect(recording: &RecordedRun) -> (Outcome, ReplayReport) {
    let (setup, cassette) = replay_setup(ALLOW_ALL, recording);
    let replayed = run(REDIRECTED, setup).await;
    (replayed, cassette.report())
}

#[tokio::test]
async fn a_chain_numbered_from_its_first_hop_ending_where_the_response_came_from_is_served() {
    let recorded = run(REDIRECTED, Setup::new(ALLOW_ALL, redirecting())).await;
    let recording = with_hops(recorded.recorded(REDIRECTED), &[0, 1, 2]);
    let (replayed, report) = replay_redirect(&recording).await;
    assert_eq!(replayed.result, Ok("/landing:1".to_owned()));
    assert!(report.miss.is_none(), "{:?}", report.miss);
}

#[tokio::test]
async fn a_chain_missing_a_hop_is_a_recording_incomplete() {
    let recorded = run(REDIRECTED, Setup::new(ALLOW_ALL, redirecting())).await;
    for indexes in [&[0, 2][..], &[1][..]] {
        let recording = with_hops(recorded.recorded(REDIRECTED), indexes);
        let (replayed, report) = replay_redirect(&recording).await;
        assert!(replayed.cancelled, "{indexes:?}");
        assert_eq!(
            miss_of(&report).reason,
            MissReason::RecordingIncomplete,
            "{indexes:?}"
        );
    }
}

#[tokio::test]
async fn a_chain_that_ends_elsewhere_than_the_response_is_a_recording_incomplete() {
    let recorded = run(REDIRECTED, Setup::new(ALLOW_ALL, redirecting())).await;
    let mut recording = with_hops(recorded.recorded(REDIRECTED), &[0]);
    for record in &mut recording.decisions {
        if matches!(
            record.entry_path,
            interpreter::runtime::EntryPath::RedirectHop { .. }
        ) {
            record.context["path"] = json!("/elsewhere");
        }
    }
    let (replayed, report) = replay_redirect(&recording).await;
    assert!(replayed.cancelled);
    assert_eq!(miss_of(&report).reason, MissReason::RecordingIncomplete);
}

#[tokio::test]
async fn a_redirected_response_from_a_truncated_log_is_a_recording_incomplete() {
    let recorded = run(REDIRECTED, Setup::new(ALLOW_ALL, redirecting())).await;
    let mut recording = recorded.recorded(REDIRECTED);
    recording.log_truncated = true;
    let (replayed, report) = replay_redirect(&recording).await;
    assert!(replayed.cancelled);
    assert_eq!(miss_of(&report).reason, MissReason::RecordingIncomplete);
}

/// `recording` with its one call's response replaced by `meta`, a failure.
fn with_failed_response(mut recording: RecordedRun, meta: Value) -> RecordedRun {
    for call in &mut recording.calls {
        call.response = Some(Box::new(payload(meta.clone(), None, "failed")));
    }
    recording
}

/// The indexes of the redirect hops the run decided, in order.
fn hop_indexes(outcome: &Outcome) -> Vec<u32> {
    outcome
        .log
        .records
        .iter()
        .filter_map(|record| match record.entry_path {
            interpreter::runtime::EntryPath::RedirectHop { index, .. } => Some(index),
            _ => None,
        })
        .collect()
}

#[tokio::test]
async fn a_recording_that_cannot_be_used_leaves_no_hop_decisions_for_the_live_call() {
    let source = Setup::new(
        ALLOW_ALL,
        Arc::new(Live {
            redirect_to: Some("https://cdn.test/landing"),
            body: Some("0123456789".into()),
            ..Live::default()
        }),
    );
    let mut source = source;
    source.log.max_payload_bytes = 4;
    let recorded = run(REDIRECTED, source).await;
    // Two hops are recorded, and the body is cut: the recording cannot answer.
    let recording = with_hops(recorded.recorded(REDIRECTED), &[0, 1]);
    let (cancel, _requested) = oneshot::channel();
    let cassette = Cassette::new(&recording, cancel);
    let client =
        RecordedHttpClient::new(cassette.clone()).with_live(redirecting(), LiveReach::Reads);
    let replayed = run(REDIRECTED, Setup::new(ALLOW_ALL, Arc::new(client))).await;

    assert_eq!(replayed.result, Ok("/landing:1".to_owned()));
    let report = cassette.report();
    assert_eq!(report.went_live.len(), 1);
    assert_eq!(report.went_live[0].reason, MissReason::RecordingIncomplete);
    assert_eq!(hop_indexes(&replayed), [0], "only the live chain's own hop");
}

#[tokio::test]
async fn a_redirect_back_to_the_same_address_needs_an_uncut_log() {
    let live = || {
        Arc::new(Live {
            redirect_to: Some("https://api.test/start"),
            ..Live::default()
        })
    };
    let recorded = run(REDIRECTED, Setup::new(ALLOW_ALL, live())).await;
    let recording = recorded.recorded(REDIRECTED);
    let (replayed, report) = replay_redirect(&recording).await;
    assert_eq!(replayed.result, Ok("/start:1".to_owned()));
    assert!(report.miss.is_none(), "{:?}", report.miss);

    let mut cut = recording;
    cut.log_truncated = true;
    let (replayed, report) = replay_redirect(&cut).await;
    assert!(replayed.cancelled);
    assert_eq!(miss_of(&report).reason, MissReason::RecordingIncomplete);
}

#[tokio::test]
async fn a_recorded_failure_after_a_gap_in_its_hops_is_a_recording_incomplete() {
    let recorded = run(REDIRECTED, Setup::new(ALLOW_ALL, redirecting())).await;
    let failed = json!({ "kind": "timeout", "error": "request timed out" });
    for (indexes, usable) in [(&[0, 1][..], true), (&[0, 2][..], false), (&[1][..], false)] {
        let recording = with_failed_response(
            with_hops(recorded.recorded(REDIRECTED), indexes),
            failed.clone(),
        );
        let (replayed, report) = replay_redirect(&recording).await;
        if usable {
            assert_eq!(replayed.result, Ok("refused".to_owned()), "{indexes:?}");
            assert!(report.miss.is_none(), "{indexes:?}");
        } else {
            assert!(replayed.cancelled, "{indexes:?}");
            assert_eq!(
                miss_of(&report).reason,
                MissReason::RecordingIncomplete,
                "{indexes:?}"
            );
        }
    }
}

/// Replays `recording` with `redirecting()` as the live client for reads.
async fn replay_reads_live(recording: &RecordedRun) -> (Outcome, Arc<Cassette>) {
    let (cancel, _requested) = oneshot::channel();
    let cassette = Cassette::new(recording, cancel);
    let client =
        RecordedHttpClient::new(cassette.clone()).with_live(redirecting(), LiveReach::Reads);
    let replayed = run(REDIRECTED, Setup::new(ALLOW_ALL, Arc::new(client))).await;
    (replayed, cassette)
}

/// A recorded failure that is a miss: the run stops at it with `RecordedFailure`, and where
/// reads may go live the live chain's one hop is the only decision, whether or not the
/// recording kept hops.
async fn assert_failure_is_a_miss(meta: Value) {
    let recorded = run(REDIRECTED, Setup::new(ALLOW_ALL, redirecting())).await;
    let mut bare = recorded.recorded(REDIRECTED);
    bare.decisions.clear();
    let with_hops = with_hops(recorded.recorded(REDIRECTED), &[0, 1]);
    for recording in [bare, with_hops] {
        let recording = with_failed_response(recording, meta.clone());
        let (replayed, report) = replay_redirect(&recording).await;
        assert!(replayed.cancelled, "{meta}");
        assert_eq!(
            miss_of(&report).reason,
            MissReason::RecordedFailure,
            "{meta}"
        );
        assert!(hop_indexes(&replayed).is_empty(), "{meta}");

        // In a mode that goes live, today's policy decides at the live client.
        let (replayed, cassette) = replay_reads_live(&recording).await;
        assert_eq!(replayed.result, Ok("/landing:1".to_owned()), "{meta}");
        assert_eq!(cassette.report().went_live.len(), 1, "{meta}");
        assert_eq!(hop_indexes(&replayed), [0], "only the live chain's own hop");
    }
}

#[tokio::test]
async fn a_recorded_egress_refusal_is_not_replayed_over_todays_network_policy() {
    assert_failure_is_a_miss(
        json!({ "kind": "egress-denied", "error": "network error: blocked address" }),
    )
    .await;
}

#[tokio::test]
async fn only_a_recorded_failure_from_outside_is_served() {
    let recorded = run(REDIRECTED, Setup::new(ALLOW_ALL, redirecting())).await;
    for (meta, served) in [
        (
            json!({ "kind": "network", "error": "network error: reset" }),
            true,
        ),
        (
            json!({ "kind": "timeout", "error": "request timed out" }),
            true,
        ),
        (json!({ "kind": "policy", "error": "blocked" }), false),
        (json!({ "kind": "internal", "error": "setup" }), false),
        (
            json!({ "kind": "unsupported-method", "error": "unsupported HTTP method: X" }),
            false,
        ),
        (json!({ "kind": "other", "error": "disk full" }), false),
        (json!({ "kind": "from-the-future", "error": "?" }), false),
        (json!({ "error": "no kind" }), false),
    ] {
        let recording = with_failed_response(recorded.recorded(REDIRECTED), meta.clone());
        let (replayed, report) = replay_redirect(&recording).await;
        if served {
            assert_eq!(replayed.result, Ok("refused".to_owned()), "{meta}");
            assert!(report.miss.is_none(), "{meta}");
        } else {
            assert!(replayed.cancelled, "{meta}");
            assert_eq!(
                miss_of(&report).reason,
                MissReason::RecordedFailure,
                "{meta}"
            );
            assert!(hop_indexes(&replayed).is_empty(), "{meta}");
        }
    }
}

#[tokio::test]
async fn a_recorded_denial_at_a_hop_is_served_as_a_denial_while_the_blueprint_still_denies_it() {
    let deny = deny_get_to("cdn.test");
    let recorded = run(REDIRECTED, Setup::new(&deny, redirecting())).await;
    assert_eq!(recorded.result, Ok("refused".to_owned()));
    let recording = recorded.recorded(REDIRECTED);

    let (setup, cassette) = replay_setup(&deny, &recording);
    let replayed = run(REDIRECTED, setup).await;
    assert_eq!(replayed.result, Ok("refused".to_owned()));
    assert!(!replayed.cancelled);
    assert!(cassette.report().miss.is_none());
    assert_eq!(hop_indexes(&replayed), [0]);

    // Once the hop is allowed, the recording says nothing about what comes after it.
    let (replayed, report) = replay_redirect(&recording).await;
    assert!(replayed.cancelled);
    assert_eq!(miss_of(&report).reason, MissReason::RecordedFailure);
}

#[tokio::test]
async fn a_recording_with_unreadable_response_parts_is_incomplete_before_any_hop_is_decided() {
    let recorded = run(REDIRECTED, Setup::new(ALLOW_ALL, redirecting())).await;
    for (field, value) in [
        ("status", json!("ok")),
        ("status_text", json!(5)),
        ("headers", json!("none")),
        ("url", json!(null)),
    ] {
        let mut recording = with_hops(recorded.recorded(REDIRECTED), &[0, 1]);
        for call in &mut recording.calls {
            if let Some(response) = call.response.as_mut() {
                response.meta[field] = value.clone();
            }
        }
        let (replayed, report) = replay_redirect(&recording).await;
        assert!(replayed.cancelled, "{field}");
        assert_eq!(
            miss_of(&report).reason,
            MissReason::RecordingIncomplete,
            "{field}"
        );
        assert!(hop_indexes(&replayed).is_empty(), "{field}");
    }
}

#[tokio::test]
async fn a_recorded_too_large_failure_is_a_miss_as_todays_limit_decides() {
    assert_failure_is_a_miss(json!({ "kind": "too-large", "error": "response too large" })).await;
}

// ---- the current blueprint's declarations ------------------------------------------------

#[tokio::test]
async fn a_server_declared_over_stdio_is_refused_as_the_live_transport_refuses_it() {
    let run = recording_of(vec![mcp_call(
        0,
        "1",
        payload(Value::Null, Some(r#"{"text":"one"}"#), "r0"),
    )]);
    let (cancel, _requested) = oneshot::channel();
    let cassette = Cassette::new(&run, cancel);
    let mut blueprint = Arc::try_unwrap(files_blueprint()).expect("unshared");
    blueprint.mcp.get_mut("files").unwrap().transport = "stdio".into();
    let blueprint = Arc::new(blueprint);
    let recorded = RecordedMcpTransport::new(cassette.clone(), blueprint.clone());
    let live = StreamableHttpTransport::new(
        "bp".into(),
        blueprint,
        None,
        None,
        Arc::new(NetworkPolicy::deny_private()),
    )
    .call("files", "read", "1")
    .await
    .result;
    let refused = mcp(&recorded, "1").await.result;
    assert!(matches!(refused, Err(McpCallError::Local(_))));
    assert_eq!(format!("{refused:?}"), format!("{live:?}"));
    let report = cassette.report();
    assert!(report.miss.is_none() && report.served.is_empty());
}

#[tokio::test]
async fn a_server_the_blueprint_no_longer_declares_is_refused_as_the_live_transport_refuses_it() {
    let run = recording_of(vec![mcp_call(
        0,
        "1",
        payload(Value::Null, Some(r#"{"text":"one"}"#), "r0"),
    )]);
    let (cancel, mut requested) = oneshot::channel();
    let cassette = Cassette::new(&run, cancel);
    let undeclared = Arc::new(Blueprint {
        name: "bp".into(),
        ..Default::default()
    });
    let transport = RecordedMcpTransport::new(cassette.clone(), undeclared.clone());

    let replayed = mcp(&transport, "1").await.result;
    let live = StreamableHttpTransport::new(
        "bp".into(),
        undeclared,
        None,
        None,
        Arc::new(NetworkPolicy::deny_private()),
    )
    .call("files", "read", "1")
    .await
    .result;
    assert!(matches!(replayed, Err(McpCallError::Local(_))));
    assert_eq!(format!("{replayed:?}"), format!("{live:?}"));
    // A refusal like any other, not a stop: nothing was missed or used up.
    let report = cassette.report();
    assert!(report.miss.is_none() && report.served.is_empty());
    assert!(requested.try_recv().is_err());
}

#[tokio::test]
async fn a_model_the_blueprint_no_longer_declares_is_refused_as_the_provider_refuses_it() {
    let mut setup = Setup::new(ALLOW_ALL, Arc::new(Live::default()));
    setup.llm = Some(Arc::new(FakeLlm {
        calls: Mutex::new(0),
    }));
    let recorded = run(BATCH, setup).await;
    let (cancel, mut requested) = oneshot::channel();
    let cassette = Cassette::new(&recorded.recorded(BATCH), cancel);
    let provider = RecordedLlmProvider::new(
        cassette.clone(),
        Arc::new(FakeLlm {
            calls: Mutex::new(0),
        }),
    );
    let prompts = ["fine".to_owned(), "refuse me".to_owned()];

    let error = provider
        .call("gone", &prompts, None)
        .await
        .expect_err("refused");
    assert_eq!(
        error,
        LlmCallError::UnknownModel {
            model: "gone".into(),
            available: vec!["open".into()],
        }
    );
    let report = cassette.report();
    assert!(report.miss.is_none() && report.served.is_empty());
    assert!(requested.try_recv().is_err());
    // The recorded model is still served.
    provider.call("open", &prompts, None).await.expect("served");
    assert_eq!(cassette.report().served.len(), 1);
}

// ---- a whole-call model failure ----------------------------------------------------------

const TRANSPORT_DOWN: &str = r#"
import llm from "submilli:llm";
function main(): string {
  try { llm.call("open", "transport down"); } catch (e) { return "caught"; }
  return "answered";
}
"#;

#[tokio::test]
async fn a_model_call_that_failed_whole_fails_the_same_way_without_the_provider() {
    let mut setup = Setup::new(ALLOW_ALL, Arc::new(Live::default()));
    setup.llm = Some(Arc::new(FakeLlm {
        calls: Mutex::new(0),
    }));
    let recorded = run(TRANSPORT_DOWN, setup).await;
    assert_eq!(recorded.result, Ok("caught".to_owned()));

    let (cancel, requested) = oneshot::channel();
    let cassette = Cassette::new(&recorded.recorded(TRANSPORT_DOWN), cancel);
    let declared = Arc::new(FakeLlm {
        calls: Mutex::new(0),
    });
    let mut setup = Setup::new(ALLOW_ALL, Arc::new(Live::default()));
    setup.llm = Some(Arc::new(RecordedLlmProvider::new(
        cassette.clone(),
        declared.clone(),
    )));
    setup.cancel = Some(requested);
    let replayed = run(TRANSPORT_DOWN, setup).await;

    assert_eq!(replayed.result, Ok("caught".to_owned()));
    assert!(!replayed.cancelled);
    assert_eq!(*declared.calls.lock().unwrap(), 0, "the provider never ran");
    let report = cassette.report();
    assert!(report.miss.is_none(), "{:?}", report.miss);
    assert_eq!(report.served.len(), 1);
    let response = |outcome: &Outcome| outcome.log.calls[0].response.as_ref().unwrap().meta.clone();
    assert_eq!(response(&recorded), response(&replayed));
}
