//! Decision records: what the per-run recorder sees at the policy and invariant seams, and
//! that installing it changes nothing a program, its embedder's audit, or its fuel can see.

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

use interpreter::runtime::security::AuditDecision;
use interpreter::runtime::{
    Access, BodyCopy, CallOutcome, CallRecord, CheckOutcome, DecisionAction, DecisionCause,
    DecisionExplanation, DecisionLog, DecisionLogConfig, DecisionLogOutput, DecisionRecord,
    EmbeddingBatch, EmbeddingError, EmbeddingLimits, EmbeddingModel, EmbeddingProvider,
    EmbeddingTokenBudget, EntryPath, ExecutionTokenBudget, FailureReason, FailureReasonRecord,
    FailureRecord, InMemorySessionKv, LinkedPackageModule, LlmCallError, LlmFailure, LlmLimits,
    LlmModel, LlmOutcome, LlmProvider, MountSpec, NearMissRecord, RecordObserver, RuleCitation,
    SecurityCheck, SessionKvLimits, SharedTokenBudget, StoreData, Vfs,
    install_package_modules_async, install_runtime_host_functions, install_runtime_store_bound,
    install_tenant_limits, limits::ExecutionUsage,
};
use interpreter::stdlib::git::GitConfig;
use interpreter::stdlib::http::transport::{
    DownloadMeta, EgressAt, HttpClient, HttpError, HttpRequest, HttpResponse, RedirectHop,
};
use interpreter::{
    CompiledPackage, FileId, ModulePath, PackageSourceModule, RuntimeConfig,
    compile_package_with_transitive, compile_script, dispatch_main_async,
};
use serde_json::{Value, json};
use wasmtime::{Linker, Module};

// ---- fixtures -------------------------------------------------------------------------

/// Denies a call that names `capability`, for `caller` when set and for a context `path`
/// when set; allows everything else by default. Stateless, so `explain` can repeat it.
struct Deny {
    caller: Option<&'static str>,
    capability: &'static str,
    path: Option<&'static str>,
}

#[derive(Default)]
struct Rules {
    deny: Vec<Deny>,
    /// A rule that named the capability but whose filter rejected the call, with this actual.
    near_miss_actual: Option<Value>,
    /// How many times the policy itself was consulted.
    consulted: AtomicU64,
}

impl Rules {
    fn denying(&self, caller: &str, capability: &str, context: &Value) -> Option<usize> {
        self.deny.iter().position(|rule| {
            rule.capability == capability
                && rule.caller.is_none_or(|c| c == caller)
                && rule
                    .path
                    .is_none_or(|p| context.get("path").and_then(Value::as_str) == Some(p))
        })
    }
}

/// The rules plus a capture of every `audit()` call as the pre-recorder fields, which is
/// what the server's audit reads and serializes.
struct Capture {
    rules: Rules,
    audits: Mutex<Vec<Value>>,
}

impl Capture {
    fn new(deny: Vec<Deny>, near_miss_actual: Option<Value>) -> Arc<Self> {
        Arc::new(Self {
            rules: Rules {
                deny,
                near_miss_actual,
                consulted: AtomicU64::new(0),
            },
            audits: Mutex::new(Vec::new()),
        })
    }
}

impl SecurityCheck for Capture {
    fn check(&self, caller: &str, capability: &str, context: &Value) -> CheckOutcome {
        self.rules.consulted.fetch_add(1, Ordering::Relaxed);
        match self.rules.denying(caller, capability, context) {
            Some(rule) => CheckOutcome::Deny {
                rule: Some(rule),
                reason: format!("denied {capability} for {caller}"),
            },
            None => CheckOutcome::Allow { rule: None },
        }
    }

    fn audit(&self, decision: AuditDecision<'_>) {
        self.audits.lock().unwrap().push(json!({
            "caller": decision.caller,
            "capability": decision.capability,
            "context": decision.context,
            "allowed": decision.allowed,
            "source": decision.source,
            "rule": decision.rule,
            "reason": decision.reason,
        }));
    }

    fn explain(
        &self,
        caller: &str,
        capability: &str,
        context: &Value,
        _cwd: &str,
    ) -> Option<DecisionExplanation> {
        let near_misses = self
            .rules
            .near_miss_actual
            .iter()
            .map(|actual| NearMissRecord {
                rule: RuleCitation {
                    caller: caller.to_owned(),
                    index: 9,
                    name: None,
                },
                filter: "host == \"x\"".into(),
                failures: vec![FailureRecord {
                    comparison: "host == \"x\"".into(),
                    actual: Some(actual.clone()),
                    expected: Some("\"x\"".into()),
                    reason: FailureReasonRecord::NotSatisfied,
                    negated: false,
                }],
            })
            .collect::<Vec<_>>();
        let mut explanation = match self.rules.denying(caller, capability, context) {
            Some(index) => DecisionExplanation {
                action: DecisionAction::Deny,
                cause: DecisionCause::Rule(RuleCitation {
                    caller: caller.to_owned(),
                    index,
                    name: None,
                }),
                near_misses: Vec::new(),
            },
            None => DecisionExplanation {
                action: DecisionAction::Allow,
                cause: DecisionCause::Default {
                    caller_block: false,
                },
                near_misses: Vec::new(),
            },
        };
        explanation.near_misses = near_misses;
        Some(explanation)
    }
}

/// Answers every request; when `redirect_to` is set, first walks the request's redirect
/// guard to that URL, as a compliant transport does for each hop it follows.
struct Web {
    redirect_to: Option<&'static str>,
    egress: Egress,
    /// The body every response carries; `ok` when unset.
    body: Option<Vec<u8>>,
}

/// Where the transport refuses the destination at the network layer, as the real one does
/// for a blocked address.
#[derive(Clone, Copy, Default)]
enum Egress {
    #[default]
    Off,
    /// The original request's destination.
    Original,
    /// A redirect hop the guard authorized, refused before its send.
    AuthorizedHop,
    /// A redirect hop refused before the guard saw it.
    UnauthorizedHop,
}

#[async_trait::async_trait]
impl HttpClient for Web {
    async fn send(&self, req: &HttpRequest) -> Result<HttpResponse, HttpError> {
        if req.url.ends_with("/down") {
            return Err(HttpError::Timeout);
        }
        let denied = || HttpError::EgressDenied("blocked address".into());
        if let (Egress::Original, Some(guard)) = (self.egress, req.redirect_guard.as_ref()) {
            let url = url::Url::parse(&req.url).map_err(|e| HttpError::Other(e.to_string()))?;
            let hop = RedirectHop {
                method: &req.method,
                url: &url,
                method_rewritten: false,
                body_len: 0,
            };
            guard.audit_egress_denial(&hop, EgressAt::CurrentHop);
            return Err(denied());
        }
        if let (Some(target), Some(guard)) = (self.redirect_to, req.redirect_guard.as_ref()) {
            let url = url::Url::parse(target).map_err(|e| HttpError::Other(e.to_string()))?;
            let hop = RedirectHop {
                method: "GET",
                url: &url,
                method_rewritten: false,
                body_len: 0,
            };
            if matches!(self.egress, Egress::UnauthorizedHop) {
                guard.audit_egress_denial(&hop, EgressAt::NewHop);
                return Err(denied());
            }
            guard.authorize(&hop).map_err(HttpError::PermissionDenied)?;
            if matches!(self.egress, Egress::AuthorizedHop) {
                guard.audit_egress_denial(&hop, EgressAt::CurrentHop);
                return Err(denied());
            }
        }
        Ok(HttpResponse {
            status: 200,
            status_text: "OK".into(),
            headers: vec![("set-cookie".into(), "session=hidden".into())],
            body: self.body.clone().unwrap_or_else(|| b"ok".to_vec()),
            final_url: req.url.clone(),
        })
    }

    async fn download(
        &self,
        _req: &HttpRequest,
        _writer: &mut (dyn std::io::Write + Send),
    ) -> Result<DownloadMeta, HttpError> {
        Err(HttpError::Other("not used".into()))
    }
}

#[derive(Default)]
struct Setup {
    deny: Vec<Deny>,
    record: Option<DecisionLogConfig>,
    packages: Vec<(&'static str, &'static str)>,
    redirect_to: Option<&'static str>,
    egress: Egress,
    strip_debug_info: bool,
    git: bool,
    session: bool,
    session_limits: Option<SessionKvLimits>,
    /// Mounts a read-only volume at `/ro`.
    read_only_volume: bool,
    /// Installs a fake model provider (`open`, `secret`) with this token ceiling.
    llm: Option<LlmLimits>,
    /// Installs a fake embedding provider (`open-embed`, 4 dimensions).
    embedding: bool,
    near_miss_actual: Option<Value>,
    observer: Option<Arc<dyn RecordObserver>>,
    response_body: Option<Vec<u8>>,
}

struct Outcome {
    result: Result<String, String>,
    fuel: u64,
    audits: Vec<Value>,
    consulted: u64,
    log: Option<DecisionLogOutput>,
    host_attached: u64,
    memory_peak: u64,
}

impl Outcome {
    fn records(&self) -> &[DecisionRecord] {
        &self.log.as_ref().expect("a recorder was installed").records
    }

    fn calls(&self) -> &[CallRecord] {
        &self.log.as_ref().expect("a recorder was installed").calls
    }

    fn call(&self, capability: &str) -> &CallRecord {
        self.calls()
            .iter()
            .find(|call| call.capability == capability)
            .unwrap_or_else(|| panic!("no call for {capability}: {:#?}", self.calls()))
    }

    fn record(&self, capability: &str) -> &DecisionRecord {
        self.records()
            .iter()
            .find(|record| record.capability == capability)
            .unwrap_or_else(|| panic!("no record for {capability}: {:#?}", self.records()))
    }
}

fn recording() -> Option<DecisionLogConfig> {
    Some(DecisionLogConfig::default())
}

/// Drops the DWARF sections, so frames still resolve to a module but not to a line.
fn strip_debug_sections(wasm: &[u8]) -> Vec<u8> {
    fn leb(bytes: &[u8], at: &mut usize) -> usize {
        let (mut value, mut shift) = (0usize, 0);
        loop {
            let byte = bytes[*at];
            *at += 1;
            value |= usize::from(byte & 0x7f) << shift;
            if byte & 0x80 == 0 {
                return value;
            }
            shift += 7;
        }
    }
    let mut out = wasm[..8].to_vec();
    let mut at = 8;
    while at < wasm.len() {
        let start = at;
        let id = wasm[at];
        at += 1;
        let size = leb(wasm, &mut at);
        let payload = at;
        at += size;
        if id == 0 {
            let mut name_at = payload;
            let name_len = leb(wasm, &mut name_at);
            if wasm[name_at..name_at + name_len].starts_with(b".debug") {
                continue;
            }
        }
        out.extend_from_slice(&wasm[start..at]);
    }
    out
}

async fn run(source: &str, setup: Setup) -> Outcome {
    let cfg = RuntimeConfig::default();
    let engine = cfg.engine_async().expect("engine");

    let mut compiled_packages: Vec<CompiledPackage> = Vec::new();
    for (name, lib) in &setup.packages {
        let modules = [PackageSourceModule {
            path: ModulePath::from("lib"),
            source: lib,
        }];
        compiled_packages.push(
            compile_package_with_transitive(name, ModulePath::from("lib"), &modules, &[], &[])
                .unwrap_or_else(|d| panic!("package {name}: {d:#?}")),
        );
    }
    let declarations: Vec<_> = compiled_packages.iter().map(|p| &p.declaration).collect();
    let script = compile_script(source, "main.ts", FileId(0), &declarations, &[])
        .unwrap_or_else(|d| panic!("script: {d:#?}"));

    let policy = Capture::new(setup.deny, setup.near_miss_actual);
    let volume = tempfile::tempdir().expect("volume");
    let mut vfs = Vfs::tempdir().expect("tempdir");
    if setup.read_only_volume {
        vfs = vfs
            .with_mount(MountSpec {
                guest_path: "/ro".into(),
                host: volume.path().to_path_buf(),
                volume: "ro".into(),
                access: Access::ReadOnly,
                quota: None,
            })
            .expect("mount");
    }
    let mut data = StoreData::with_vfs(vfs);
    data.install_type_info(script.type_info.clone());
    data.http_client = Arc::new(Web {
        redirect_to: setup.redirect_to,
        egress: setup.egress,
        body: setup.response_body,
    });
    if let Some(limits) = setup.llm {
        data.llm_provider = Some(Arc::new(FakeLlm));
        data.llm_budget = Some(Arc::new(ExecutionTokenBudget::new(
            limits,
            SharedTokenBudget::new(u64::MAX),
        )));
    }
    if setup.embedding {
        data.embedding_provider = Some(Arc::new(FakeEmbedding));
        data.embedding_budget = Some(Arc::new(EmbeddingTokenBudget::new(
            EmbeddingLimits::default(),
            SharedTokenBudget::new(u64::MAX),
        )));
    }
    data.security_check = policy.clone();
    if setup.git {
        data.git = Some(GitConfig {
            name: "Agent".into(),
            email: "agent@example.com".into(),
            username: None,
        });
    }
    if setup.session {
        data.session_kv = Some(Arc::new(match setup.session_limits {
            Some(limits) => InMemorySessionKv::new(limits),
            None => InMemorySessionKv::default(),
        }));
    }
    let log = setup
        .record
        .map(|config| DecisionLog::install_observed(&mut data, config, setup.observer));

    let mut store = cfg.store_async(&engine, data).expect("store");
    install_tenant_limits(&mut store);
    let mut linker = Linker::<StoreData>::new(&engine);
    install_runtime_host_functions(&mut linker).expect("host functions");
    install_runtime_store_bound(&mut linker, &mut store).expect("store-bound functions");

    let package_modules: Vec<Module> = compiled_packages
        .iter()
        .map(|p| Module::new(&engine, &p.wasm).expect("package module"))
        .collect();
    let linked: Vec<LinkedPackageModule<'_>> = compiled_packages
        .iter()
        .zip(&package_modules)
        .map(|(p, module)| LinkedPackageModule {
            module,
            declaration: &p.declaration,
            type_info: &p.type_info,
        })
        .collect();
    install_package_modules_async(&mut linker, &mut store, &linked)
        .await
        .expect("link packages");

    let wasm = if setup.strip_debug_info {
        strip_debug_sections(&script.wasm)
    } else {
        script.wasm.clone()
    };
    let module = Module::new(&engine, &wasm).expect("module");
    let instance = linker
        .instantiate_async(&mut store, &module)
        .await
        .expect("instantiate");
    let result = dispatch_main_async(&mut store, &instance)
        .await
        .map(Option::unwrap_or_default)
        .map_err(|error| format!("{error:#}"));

    let usage = ExecutionUsage::capture(&store, cfg.fuel).expect("usage");
    let (fuel, memory_peak) = (usage.fuel, usage.memory_peak);
    let host_attached = store.data().tenant_limits.host_attached_bytes();
    let audits = policy.audits.lock().unwrap().clone();
    Outcome {
        result,
        fuel,
        audits,
        consulted: policy.rules.consulted.load(Ordering::Relaxed),
        log: log.map(|log| log.finish()),
        host_attached,
        memory_peak,
    }
}

/// Serves `open` and `secret`, answering every prompt; a prompt `transport down` fails the
/// whole call.
struct FakeLlm;

impl LlmProvider for FakeLlm {
    fn call<'a>(
        &'a self,
        model: &'a str,
        prompts: &'a [String],
        _schema_json: Option<&'a str>,
    ) -> std::pin::Pin<
        Box<dyn std::future::Future<Output = Result<Vec<LlmOutcome>, LlmCallError>> + Send + 'a>,
    > {
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
                if prompt == "refuse me" {
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
        Box::pin(async { Ok(vec![LlmModel::new("open"), LlmModel::new("secret")]) })
    }
}

/// Serves `open-embed`: four dimensions, 7 reported input tokens per batch.
struct FakeEmbedding;

impl EmbeddingProvider for FakeEmbedding {
    fn embed<'a>(
        &'a self,
        alias: &'a str,
        texts: &'a [String],
        _purpose: interpreter::runtime::Purpose,
        budget: &'a EmbeddingTokenBudget,
    ) -> std::pin::Pin<
        Box<dyn std::future::Future<Output = Result<EmbeddingBatch, EmbeddingError>> + Send + 'a>,
    > {
        Box::pin(async move {
            let estimate = self.estimate_tokens(alias, texts);
            budget.mark_sent(alias, estimate)?;
            // A later sub-batch fails after an earlier one was billed (`flaky`)
            // or after none reported usage (`dark`).
            let failed_after = |reported| EmbeddingError::Provider {
                alias: alias.to_string(),
                reason: interpreter::runtime::EmbeddingFailureReason::Transport,
                settlements: vec![
                    interpreter::runtime::SubBatchSettlement {
                        estimate: 4,
                        reported,
                        indeterminate: 4 - reported,
                    },
                    interpreter::runtime::SubBatchSettlement {
                        estimate: 3,
                        reported: 0,
                        indeterminate: 3,
                    },
                ],
            };
            match alias {
                "open-embed-flaky" => return Err(failed_after(4)),
                "open-embed-dark" => return Err(failed_after(0)),
                _ => {}
            }
            let batch = EmbeddingBatch::new(
                vec![0.5; texts.len() * 4],
                texts.len(),
                4,
                "emb1:fake:open-embed:4:01",
                alias,
            )
            .expect("shape")
            .with_input_tokens(Some(7))
            .with_settlements(vec![interpreter::runtime::SubBatchSettlement {
                estimate,
                reported: 7,
                indeterminate: 0,
            }]);
            Ok(batch)
        })
    }

    fn models<'a>(
        &'a self,
    ) -> std::pin::Pin<
        Box<
            dyn std::future::Future<Output = Result<Vec<EmbeddingModel>, EmbeddingError>>
                + Send
                + 'a,
        >,
    > {
        Box::pin(async { Ok(Vec::new()) })
    }

    fn max_input_bytes(&self, alias: &str) -> Option<u64> {
        alias.starts_with("open-embed").then_some(1024)
    }
}

const POST_IN_PACKAGE: &str = r#"
import { post } from "submilli:http";
export function send(): void { post("https://example.test/in", "x"); }
"#;

const GET_IN_PACKAGE: &str = r#"
import { get } from "submilli:http";
export function fetchIt(): void { get("https://example.test/in"); }
"#;

// ---- the guarantee: recording is invisible --------------------------------------------

const MIXED_PROGRAM: &str = r#"
import { get, post } from "submilli:http";
import { fetchIt } from "@acme/core";
function main(): string {
  const a = get("https://example.test/ok");
  let denied = "no";
  try { post("https://example.test/deny", "x"); } catch (e: PermissionDeniedError) { denied = "yes"; }
  fetchIt();
  return a.body + ":" + denied;
}
"#;

fn mixed(record: Option<DecisionLogConfig>) -> Setup {
    Setup {
        deny: vec![Deny {
            caller: None,
            capability: "http.post",
            path: Some("/deny"),
        }],
        record,
        packages: vec![("@acme/core", GET_IN_PACKAGE)],
        ..Setup::default()
    }
}

#[tokio::test]
async fn recording_changes_neither_outcome_nor_audit_nor_fuel() {
    let plain = run(MIXED_PROGRAM, mixed(None)).await;
    let recorded = run(MIXED_PROGRAM, mixed(recording())).await;

    assert_eq!(plain.result, Ok("ok:yes".to_owned()));
    assert_eq!(recorded.result, plain.result, "results and denials");
    assert_eq!(
        recorded.audits, plain.audits,
        "the embedder's audit records"
    );
    assert_eq!(recorded.consulted, plain.consulted, "policy consultations");
    assert_eq!(recorded.fuel, plain.fuel, "fuel use, line lookup included");
    assert_eq!(recorded.memory_peak, plain.memory_peak, "memory peak");
    assert_eq!(recorded.host_attached, plain.host_attached, "host memory");
    assert_eq!(plain.audits.len(), 3);
    assert_eq!(recorded.records().len(), 3);
}

#[tokio::test]
async fn a_failed_line_lookup_leaves_no_line_and_changes_nothing() {
    let reference = run(MIXED_PROGRAM, mixed(None)).await;
    let mut setup = mixed(recording());
    setup.strip_debug_info = true;
    let stripped = run(MIXED_PROGRAM, setup).await;

    assert_eq!(stripped.result, reference.result);
    assert_eq!(stripped.audits, reference.audits);
    assert_eq!(stripped.fuel, reference.fuel);
    assert_eq!(stripped.records().len(), 3);
    assert!(
        stripped
            .records()
            .iter()
            .all(|record| record.line.is_none()),
        "{:#?}",
        stripped.records()
    );
}

// ---- cause, attribution, entry path ----------------------------------------------------

#[tokio::test]
async fn one_allowed_and_one_denied_call_record_default_and_rule_causes() {
    let outcome = run(MIXED_PROGRAM, mixed(recording())).await;
    let get = outcome.record("http.get");
    assert!(get.allowed);
    assert_eq!(get.action, DecisionAction::Allow);
    assert_eq!(
        get.cause,
        DecisionCause::Default {
            caller_block: false
        }
    );
    let post = outcome.record("http.post");
    assert!(!post.allowed);
    assert_eq!(post.action, DecisionAction::Deny);
    assert_eq!(post.rule, Some(0));
    assert_eq!(
        post.cause,
        DecisionCause::Rule(RuleCitation {
            caller: "main".into(),
            index: 0,
            name: None
        })
    );
}

#[tokio::test]
async fn a_dependency_packages_allowed_call_names_the_package_and_gated_op() {
    let source = r#"
import { send } from "@acme/core";
function main(): string { send(); return "done"; }
"#;
    let outcome = run(
        source,
        Setup {
            record: recording(),
            packages: vec![("@acme/core", POST_IN_PACKAGE)],
            ..Setup::default()
        },
    )
    .await;
    assert_eq!(outcome.result, Ok("done".to_owned()));
    let post = outcome.record("http.post");
    assert_eq!(post.caller, "@acme/core");
    assert!(post.allowed);
    assert_eq!(
        post.cause,
        DecisionCause::Default {
            caller_block: false
        }
    );
    assert_eq!(post.entry_path, EntryPath::GatedOp);
}

#[tokio::test]
async fn a_package_check_records_the_consumer_and_package_check_entry() {
    let lib = r#"
import { check } from "submilli:security";
/**
 * Runs the operation.
 * @param id Identifier of the target.
 * @capability test.com/op { id }
 */
export function run(id: string): void { check("test.com/op", { id }); }
"#;
    let source = r#"
import { run } from "@acme/core";
function main(): string { run("7"); return "done"; }
"#;
    let outcome = run(
        source,
        Setup {
            record: recording(),
            packages: vec![("@acme/core", lib)],
            ..Setup::default()
        },
    )
    .await;
    assert_eq!(outcome.result, Ok("done".to_owned()));
    let check = outcome.record("test.com/op");
    assert_eq!(check.caller, "main", "the consumer, not the package");
    assert_eq!(check.entry_path, EntryPath::PackageCheck);
    assert_eq!(check.context, json!({ "id": "7" }));
    assert_eq!(check.line.map(|l| l.line), Some(3));
}

#[tokio::test]
async fn main_calling_a_main_denial_capability_records_an_invariant_without_the_policy() {
    let source = r#"
import { get } from "submilli:secrets";
function main(): string {
  try { get("api-key"); } catch (e: PermissionDeniedError) { return "refused"; }
  return "allowed";
}
"#;
    let outcome = run(
        source,
        Setup {
            record: recording(),
            ..Setup::default()
        },
    )
    .await;
    assert_eq!(outcome.result, Ok("refused".to_owned()));
    assert_eq!(outcome.consulted, 0, "the policy is never asked");
    let record = outcome.record("secrets.get");
    assert!(!record.allowed);
    assert_eq!(record.source, "invariant");
    assert!(
        matches!(&record.cause, DecisionCause::RuntimeInvariant { reason } if !reason.is_empty()),
        "{:?}",
        record.cause
    );
    assert_eq!(
        record.context,
        json!({ "name": "api-key" }),
        "key name only"
    );
}

#[tokio::test]
async fn a_redirect_hop_records_its_parent_its_index_and_the_originating_line() {
    let source = r#"
import { get } from "submilli:http";

function main(): string {
  const r = get("https://example.test/start");
  return r.body;
}
"#;
    let outcome = run(
        source,
        Setup {
            record: recording(),
            redirect_to: Some("https://other.test/landing"),
            ..Setup::default()
        },
    )
    .await;
    assert_eq!(outcome.result, Ok("ok".to_owned()));
    let records = outcome.records();
    assert_eq!(records.len(), 2, "{records:#?}");
    let (call, hop) = (&records[0], &records[1]);
    assert_eq!(call.entry_path, EntryPath::GatedOp);
    assert_eq!(call.line.map(|l| l.line), Some(5));
    assert_eq!(
        hop.entry_path,
        EntryPath::RedirectHop {
            parent_call_index: call.call_index,
            index: 0
        }
    );
    assert_eq!(hop.context["host"], "other.test");
    assert_eq!(hop.line, call.line, "the hop reuses the call's line");
    assert!(hop.call_index > call.call_index);
}

#[tokio::test]
async fn a_git_operation_records_a_git_entry_with_the_originating_line() {
    let source = r#"
import { Repository } from "submilli:git";

function main(): string {
  Repository.init("/repo");
  return "done";
}
"#;
    let outcome = run(
        source,
        Setup {
            record: recording(),
            git: true,
            ..Setup::default()
        },
    )
    .await;
    assert_eq!(outcome.result, Ok("done".to_owned()));
    let init = outcome.record("git.init");
    assert_eq!(init.entry_path, EntryPath::Git);
    assert_eq!(init.caller, "main");
    assert_eq!(init.line.map(|l| l.line), Some(5));
}

// ---- sequence numbers, caps, filters ----------------------------------------------------

#[tokio::test]
async fn sequence_numbers_count_every_call_of_a_pair_denials_included() {
    let source = r#"
import { get } from "submilli:http";
function main(): string {
  get("https://example.test/a");
  try { get("https://example.test/b"); } catch (e: PermissionDeniedError) {}
  get("https://example.test/c");
  return "done";
}
"#;
    let outcome = run(
        source,
        Setup {
            deny: vec![Deny {
                caller: None,
                capability: "http.get",
                path: Some("/b"),
            }],
            record: recording(),
            ..Setup::default()
        },
    )
    .await;
    let gets: Vec<_> = outcome
        .records()
        .iter()
        .filter(|record| record.capability == "http.get")
        .collect();
    assert_eq!(gets.iter().map(|r| r.seq).collect::<Vec<_>>(), [1, 2, 3]);
    assert_eq!(
        gets.iter().map(|r| r.allowed).collect::<Vec<_>>(),
        [true, false, true]
    );
    let indexes: Vec<_> = outcome.records().iter().map(|r| r.call_index).collect();
    assert!(indexes.windows(2).all(|w| w[0] < w[1]), "{indexes:?}");
    let times: Vec<_> = outcome.records().iter().map(|r| r.at_micros).collect();
    assert!(times.windows(2).all(|w| w[0] <= w[1]), "{times:?}");
}

fn caught_denials(count: u32) -> String {
    format!(
        r#"
import {{ get }} from "submilli:http";
function main(): string {{
  let denied = 0;
  for (let i = 0; i < {count}; i++) {{
    try {{ get("https://example.test/deny"); }} catch (e: PermissionDeniedError) {{ denied += 1; }}
  }}
  return denied.toString();
}}
"#
    )
}

#[tokio::test]
async fn a_loop_of_caught_denials_hits_the_cap_and_the_run_completes_truncated() {
    let capped = |count: u32| {
        let source = caught_denials(count);
        async move {
            run(
                &source,
                Setup {
                    deny: vec![Deny {
                        caller: None,
                        capability: "http.get",
                        path: Some("/deny"),
                    }],
                    record: Some(DecisionLogConfig {
                        max_decisions: 20,
                        ..DecisionLogConfig::default()
                    }),
                    ..Setup::default()
                },
            )
            .await
        }
    };
    let few = capped(60).await;
    let many = capped(600).await;

    assert_eq!(
        few.result,
        Ok("60".to_owned()),
        "the run completes normally"
    );
    assert_eq!(many.result, Ok("600".to_owned()));
    let log = many.log.as_ref().unwrap();
    assert!(log.truncated);
    assert_eq!(log.records.len(), 20);
    assert_eq!(log.dropped, 580);
    assert_eq!(few.log.as_ref().unwrap().dropped, 40);
}

fn caught_denial_setup(record: Option<DecisionLogConfig>) -> Setup {
    Setup {
        deny: vec![Deny {
            caller: None,
            capability: "http.get",
            path: Some("/deny"),
        }],
        record,
        ..Setup::default()
    }
}

#[tokio::test]
async fn recorder_buffers_have_their_own_budget_and_never_touch_the_runs_memory() {
    let source = caught_denials(200);
    let unrecorded = run(&source, caught_denial_setup(None)).await;
    let recorded = run(&source, caught_denial_setup(recording())).await;
    assert_eq!(recorded.host_attached, unrecorded.host_attached);
    assert_eq!(recorded.memory_peak, unrecorded.memory_peak);
    assert_eq!(recorded.records().len(), 200);
}

#[tokio::test]
async fn an_exhausted_recorder_budget_truncates_the_log_and_changes_nothing_the_run_sees() {
    let source = caught_denials(200);
    let unrecorded = run(&source, caught_denial_setup(None)).await;
    let starved = run(
        &source,
        // No call records, so the budget goes to decisions alone.
        caught_denial_setup(Some(DecisionLogConfig {
            max_recorder_bytes: 3000,
            max_calls: 0,
            ..DecisionLogConfig::default()
        })),
    )
    .await;

    let log = starved.log.as_ref().unwrap();
    assert!(log.truncated, "the budget ran out");
    assert!(
        log.records.len() < 200 && log.dropped > 0,
        "{} kept",
        log.records.len()
    );
    assert!(
        log.records
            .iter()
            .any(|record| record.payload_dropped && record.context_digest != 0),
        "a record past the payload budget keeps its verdict and digest"
    );
    assert_eq!(starved.result, unrecorded.result);
    assert_eq!(starved.audits, unrecorded.audits);
    assert_eq!(starved.fuel, unrecorded.fuel);
    assert_eq!(starved.memory_peak, unrecorded.memory_peak);
    assert_eq!(starved.host_attached, unrecorded.host_attached);
}

#[tokio::test]
async fn a_byte_budget_payload_drop_does_not_stop_lines_on_later_records() {
    // A large context makes a full record cost several minimal ones, so after the first
    // payload drop the budget still keeps more records, each with its line.
    let source = caught_denials(200).replace("/deny\"", &format!("/{}\"", "q".repeat(900)));
    let starved = run(
        &source,
        Setup {
            deny: vec![Deny {
                caller: None,
                capability: "http.get",
                path: None,
            }],
            // No call records, so the budget goes to decisions alone.
            record: Some(DecisionLogConfig {
                max_recorder_bytes: 10000,
                max_calls: 0,
                ..DecisionLogConfig::default()
            }),
            ..Setup::default()
        },
    )
    .await;
    let log = starved.log.as_ref().unwrap();
    let first_dropped = log
        .records
        .iter()
        .position(|r| r.payload_dropped)
        .expect("a payload was dropped");
    assert!(
        log.records.len() > first_dropped + 1,
        "several records are kept after the first drop: {} kept, first drop at {first_dropped}",
        log.records.len()
    );
    assert!(
        log.records[first_dropped..]
            .iter()
            .all(|r| r.line.is_some()),
        "records kept after the drop still carry their lines"
    );
}

#[tokio::test]
async fn lines_stop_once_the_line_capture_budget_is_spent_and_the_run_is_unchanged() {
    let source = caught_denials(50);
    let unrecorded = run(&source, caught_denial_setup(None)).await;
    let limited = run(
        &source,
        caught_denial_setup(Some(DecisionLogConfig {
            max_line_capture_frames: 6,
            ..DecisionLogConfig::default()
        })),
    )
    .await;
    let unlimited = run(&source, caught_denial_setup(recording())).await;

    let log = limited.log.as_ref().unwrap();
    assert!(log.truncated, "the frame budget ran out");
    assert_eq!(log.records.len(), 50, "records are still kept");
    let with_line = log.records.iter().filter(|r| r.line.is_some()).count();
    let per_capture = unlimited.log.as_ref().unwrap().line_frames / 50;
    assert!(per_capture > 0);
    let expected = (6 / per_capture + 1) as usize;
    assert_eq!(with_line, expected, "{per_capture} frames per capture");
    assert!(
        log.records[..with_line].iter().all(|r| r.line.is_some()),
        "no line returns once capture stops"
    );
    assert_eq!(
        log.line_frames,
        per_capture * with_line as u64,
        "capture itself stopped, not just the lines"
    );
    assert_eq!(limited.result, unrecorded.result);
    assert_eq!(limited.audits, unrecorded.audits);
    assert_eq!(limited.fuel, unrecorded.fuel);
    assert_eq!(limited.memory_peak, unrecorded.memory_peak);
    assert_eq!(limited.host_attached, unrecorded.host_attached);
}

#[tokio::test]
async fn a_zero_frame_budget_allows_one_capture_that_keeps_its_line() {
    let source = caught_denials(10);
    let outcome = run(
        &source,
        caught_denial_setup(Some(DecisionLogConfig {
            max_line_capture_frames: 0,
            ..DecisionLogConfig::default()
        })),
    )
    .await;
    let log = outcome.log.as_ref().unwrap();
    assert!(log.truncated);
    assert_eq!(log.records.len(), 10);
    assert!(
        log.records[0].line.is_some(),
        "the crossing capture keeps its line"
    );
    assert!(log.records[1..].iter().all(|r| r.line.is_none()));
    let one = run(&caught_denials(1), caught_denial_setup(recording())).await;
    assert_eq!(log.line_frames, one.log.as_ref().unwrap().line_frames);
}

#[tokio::test]
async fn records_up_to_the_decision_cap_keep_their_lines_and_the_run_is_unchanged() {
    let source = caught_denials(100);
    let unrecorded = run(&source, caught_denial_setup(None)).await;
    let capped = run(
        &source,
        caught_denial_setup(Some(DecisionLogConfig {
            max_decisions: 5,
            ..DecisionLogConfig::default()
        })),
    )
    .await;
    let log = capped.log.as_ref().unwrap();
    assert_eq!(log.records.len(), 5);
    assert!(log.records.iter().all(|r| r.line.is_some()));
    assert_eq!(log.dropped, 95);
    let five = run(&caught_denials(5), caught_denial_setup(recording())).await;
    assert_eq!(
        log.line_frames,
        five.log.as_ref().unwrap().line_frames,
        "no capture past the cap"
    );
    assert_eq!(capped.result, unrecorded.result);
    assert_eq!(capped.audits, unrecorded.audits);
    assert_eq!(capped.fuel, unrecorded.fuel);
}

#[tokio::test]
async fn a_session_list_denial_is_recorded_as_filtered_and_the_program_succeeds() {
    let source = r#"
import session from "submilli:session";
function main(): string {
  session.set("hidden", 1);
  const page = session.list("", 10);
  return page.entries.length.toString();
}
"#;
    let outcome = run(
        source,
        Setup {
            deny: vec![Deny {
                caller: None,
                capability: "session.read",
                path: None,
            }],
            record: recording(),
            session: true,
            ..Setup::default()
        },
    )
    .await;
    assert_eq!(outcome.result, Ok("0".to_owned()));
    let read = outcome.record("session.read");
    assert!(!read.allowed);
    assert!(read.filtered);
    assert!(!outcome.record("session.list").filtered);
    assert!(!outcome.record("session.write").filtered);
}

#[tokio::test]
async fn a_context_value_over_the_cap_is_recorded_truncated_with_a_marker() {
    let long = "a".repeat(5000);
    let source = format!(
        r#"
import {{ get }} from "submilli:http";
function main(): string {{ get("https://example.test/{long}"); return "done"; }}
"#
    );
    let outcome = run(
        &source,
        Setup {
            record: recording(),
            ..Setup::default()
        },
    )
    .await;
    assert_eq!(outcome.result, Ok("done".to_owned()));
    let get = outcome.record("http.get");
    assert!(get.context_truncated);
    let path = get.context["path"].as_str().unwrap();
    assert!(path.len() < 1200, "{} bytes", path.len());
    assert!(path.contains("truncated"), "{path}");
    // The audit still saw the whole value.
    assert_eq!(
        outcome.audits[0]["context"]["path"].as_str().unwrap().len(),
        5001
    );
}

// ---- the source line -----------------------------------------------------------------------

#[tokio::test]
async fn a_package_call_records_the_users_line_not_a_line_inside_the_package() {
    let source = r#"
import { fetchIt } from "@acme/core";

function main(): string {
  fetchIt();
  return "done";
}
"#;
    // fetchIt() is on line 5 of this source; the package's get is on its line 3.
    let outcome = run(
        source,
        Setup {
            record: recording(),
            packages: vec![("@acme/core", GET_IN_PACKAGE)],
            ..Setup::default()
        },
    )
    .await;
    let get = outcome.record("http.get");
    assert_eq!(get.caller, "@acme/core");
    assert_eq!(get.line.map(|l| l.line), Some(5), "{get:#?}");
}

#[tokio::test]
async fn the_line_of_each_call_is_its_own() {
    let outcome = run(MIXED_PROGRAM, mixed(recording())).await;
    let lines: Vec<_> = outcome
        .records()
        .iter()
        .map(|r| (r.capability.as_str(), r.line.map(|l| l.line)))
        .collect();
    assert_eq!(
        lines,
        [
            ("http.get", Some(5)),
            ("http.post", Some(7)),
            ("http.get", Some(8)),
        ]
    );
}

// ---- one host call, one call index ------------------------------------------------------

const TWO_GETS_REFUSED_AT_EGRESS: &str = r#"
import { get } from "submilli:http";
function main(): string {
  try { get("https://10.0.0.1/a"); } catch (e: Error) {}
  try { get("https://10.0.0.1/b"); } catch (e: Error) {}
  return "done";
}
"#;

#[tokio::test]
async fn an_egress_refusal_of_the_original_request_continues_its_own_call() {
    let outcome = run(
        TWO_GETS_REFUSED_AT_EGRESS,
        Setup {
            record: recording(),
            egress: Egress::Original,
            ..Setup::default()
        },
    )
    .await;
    assert_eq!(outcome.result, Ok("done".to_owned()));
    let records = outcome.records();
    assert_eq!(records.len(), 4, "{records:#?}");
    for (gate, egress, seq) in [(0, 1, 1), (2, 3, 2)] {
        let (gate, egress) = (&records[gate], &records[egress]);
        assert!(gate.allowed && !egress.allowed);
        assert_eq!(egress.source, "egress_guard");
        assert_eq!(egress.call_index, gate.call_index);
        assert_eq!((gate.seq, egress.seq), (seq, seq));
        assert_eq!(egress.entry_path, EntryPath::GatedOp);
        assert_eq!(egress.line, gate.line);
        assert!(matches!(
            egress.cause,
            DecisionCause::RuntimeInvariant { .. }
        ));
    }
    assert!(records[2].call_index > records[0].call_index);
}

#[tokio::test]
async fn an_egress_refusal_of_an_authorized_hop_reuses_that_hops_call() {
    let outcome = run(
        TWO_GETS_REFUSED_AT_EGRESS,
        Setup {
            record: recording(),
            redirect_to: Some("https://10.0.0.2/landing"),
            egress: Egress::AuthorizedHop,
            ..Setup::default()
        },
    )
    .await;
    let records = outcome.records();
    // Per request: the call, its hop (authorized), the hop's egress refusal.
    assert_eq!(records.len(), 6, "{records:#?}");
    let (call, hop, refusal) = (&records[0], &records[1], &records[2]);
    assert_eq!(
        hop.entry_path,
        EntryPath::RedirectHop {
            parent_call_index: call.call_index,
            index: 0
        }
    );
    assert_eq!(refusal.source, "egress_guard");
    assert_eq!(refusal.entry_path, hop.entry_path, "no further hop index");
    assert_eq!((refusal.call_index, refusal.seq), (hop.call_index, hop.seq));
    assert_eq!(refusal.line, call.line);
    // The second request starts again at hop 0, with its own call.
    assert_eq!(
        records[4].entry_path,
        EntryPath::RedirectHop {
            parent_call_index: records[3].call_index,
            index: 0
        }
    );
}

#[tokio::test]
async fn an_egress_refusal_of_an_unauthorized_hop_is_a_new_hop_of_the_request() {
    let outcome = run(
        TWO_GETS_REFUSED_AT_EGRESS,
        Setup {
            record: recording(),
            redirect_to: Some("https://10.0.0.2/landing"),
            egress: Egress::UnauthorizedHop,
            ..Setup::default()
        },
    )
    .await;
    let records = outcome.records();
    assert_eq!(records.len(), 4, "{records:#?}");
    let (call, refusal) = (&records[0], &records[1]);
    assert_eq!(refusal.source, "egress_guard");
    assert_eq!(
        refusal.entry_path,
        EntryPath::RedirectHop {
            parent_call_index: call.call_index,
            index: 0
        }
    );
    assert!(refusal.call_index > call.call_index);
}

#[tokio::test]
async fn a_denial_at_the_entry_of_a_new_check_is_its_own_call_not_an_earlier_ones() {
    // The closure is main's code running inside the package, so its `check` is refused as
    // an invariant at entry. It must not attach to the first, direct `check` of the pair.
    let lib = r#"
export function apply(f: () => void): void { f(); }
"#;
    let source = r#"
import { check } from "submilli:security";
import { apply } from "@acme/core";
function main(): string {
  check("x", { a: 1 });
  try { apply(() => { check("x", { a: 1 }); }); } catch (e: PermissionDeniedError) { return "refused"; }
  return "allowed";
}
"#;
    let outcome = run(
        source,
        Setup {
            record: recording(),
            packages: vec![("@acme/core", lib)],
            ..Setup::default()
        },
    )
    .await;
    assert_eq!(outcome.result, Ok("refused".to_owned()));
    let records = outcome.records();
    assert_eq!(records.len(), 2, "{records:#?}");
    let (first, second) = (&records[0], &records[1]);
    assert!(first.allowed && !second.allowed);
    assert_eq!(second.source, "invariant");
    assert_eq!((first.seq, second.seq), (1, 2));
    assert!(second.call_index > first.call_index);
    assert_eq!(first.line.map(|l| l.line), Some(5));
    assert_eq!(second.line.map(|l| l.line), Some(6));
    assert_eq!(second.entry_path, EntryPath::PackageCheck);
    assert_eq!(first.entry_path, EntryPath::PackageCheck);
}

#[tokio::test]
async fn a_read_only_refusal_continues_the_call_whose_gate_allowed_it() {
    let source = r#"
import { writeText } from "submilli:fs";
function main(): string {
  try { writeText("/ro/a.txt", "x"); } catch (e: PermissionDeniedError) { return "refused"; }
  return "written";
}
"#;
    let outcome = run(
        source,
        Setup {
            record: recording(),
            read_only_volume: true,
            ..Setup::default()
        },
    )
    .await;
    assert_eq!(outcome.result, Ok("refused".to_owned()));
    let records = outcome.records();
    assert_eq!(records.len(), 2, "{records:#?}");
    let (gate, refusal) = (&records[0], &records[1]);
    assert!(gate.allowed && !refusal.allowed);
    assert_eq!(refusal.source, "read_only");
    assert_eq!(
        (refusal.call_index, refusal.seq),
        (gate.call_index, gate.seq)
    );
    assert_eq!(refusal.line, gate.line);
}

#[tokio::test]
async fn a_session_quota_refusal_continues_the_write_that_the_gate_allowed() {
    let source = r#"
import session from "submilli:session";
function main(): string {
  try { session.set("k", "value"); } catch (e: QuotaExceededError) { return "refused"; }
  return "written";
}
"#;
    let outcome = run(
        source,
        Setup {
            record: recording(),
            session: true,
            session_limits: Some(SessionKvLimits {
                max_entries: 0,
                ..SessionKvLimits::default()
            }),
            ..Setup::default()
        },
    )
    .await;
    assert_eq!(outcome.result, Ok("refused".to_owned()));
    let records = outcome.records();
    assert_eq!(records.len(), 2, "{records:#?}");
    let (gate, refusal) = (&records[0], &records[1]);
    assert_eq!(refusal.source, "quota");
    assert_eq!(
        (refusal.call_index, refusal.seq),
        (gate.call_index, gate.seq)
    );
}

#[tokio::test]
async fn a_model_token_quota_refusal_continues_the_call_whose_gate_allowed_it() {
    let source = r#"
import llm from "submilli:llm";
function main(): string {
  try { llm.call("open", "hello"); } catch (e: QuotaExceededError) { return "refused"; }
  return "answered";
}
"#;
    let outcome = run(
        source,
        Setup {
            record: recording(),
            llm: Some(LlmLimits {
                per_execution_tokens: 1,
                ..LlmLimits::default()
            }),
            ..Setup::default()
        },
    )
    .await;
    assert_eq!(outcome.result, Ok("refused".to_owned()));
    let records = outcome.records();
    assert_eq!(records.len(), 2, "{records:#?}");
    let (gate, refusal) = (&records[0], &records[1]);
    assert_eq!(refusal.source, "quota");
    assert_eq!(
        (refusal.call_index, refusal.seq),
        (gate.call_index, gate.seq)
    );
}

#[tokio::test]
async fn a_hidden_model_in_the_listing_is_recorded_as_filtered() {
    let source = r#"
import llm from "submilli:llm";
function main(): string { return llm.models().length.toString(); }
"#;
    let outcome = run(
        source,
        Setup {
            deny: vec![Deny {
                caller: None,
                capability: "llm.call",
                path: None,
            }],
            record: recording(),
            llm: Some(LlmLimits::default()),
            ..Setup::default()
        },
    )
    .await;
    assert_eq!(outcome.result, Ok("0".to_owned()));
    let denied: Vec<_> = outcome
        .records()
        .iter()
        .filter(|record| record.capability == "llm.call")
        .collect();
    assert_eq!(denied.len(), 2, "one gate per candidate");
    assert!(
        denied
            .iter()
            .all(|record| !record.allowed && record.filtered)
    );
    assert_ne!(denied[0].call_index, denied[1].call_index);
}

#[tokio::test]
async fn near_miss_values_are_capped_like_the_context() {
    let huge = json!({ "k".repeat(5000): "v".repeat(5000) });
    let outcome = run(
        r#"
import { get } from "submilli:http";
function main(): string { get("https://example.test/a"); return "done"; }
"#,
        Setup {
            record: recording(),
            near_miss_actual: Some(huge),
            ..Setup::default()
        },
    )
    .await;
    let get = outcome.record("http.get");
    assert!(get.context_truncated, "a capped near miss marks the record");
    let actual = get.near_misses[0].failures[0].actual.as_ref().unwrap();
    let text = serde_json::to_string(actual).unwrap();
    assert!(text.len() < 2600, "{} bytes", text.len());
    assert!(text.contains("truncated"), "{text}");
}

// ---- throwaway cost measurement ---------------------------------------------------------------

/// Run with `cargo test --release -p interpreter --test decision_records -- --ignored
/// --nocapture line_lookup_cost`; prints per-call costs, asserts nothing about time.
#[tokio::test]
#[ignore = "timing measurement, not a check"]
async fn line_lookup_cost() {
    let source = caught_denials(20_000);
    let setup = |record| Setup {
        record,
        deny: vec![Deny {
            caller: None,
            capability: "http.get",
            path: Some("/deny"),
        }],
        ..Setup::default()
    };
    // Unbounded cap so every call pays the whole recording path.
    let config = DecisionLogConfig {
        max_decisions: usize::MAX,
        max_line_capture_frames: u64::MAX,
        ..DecisionLogConfig::default()
    };
    // Best of several runs each: the program's own cost is large next to the recorder's.
    let mut best_plain = std::time::Duration::MAX;
    let mut best_recorded = std::time::Duration::MAX;
    for _ in 0..8 {
        let started = std::time::Instant::now();
        run(&source, setup(None)).await;
        best_plain = best_plain.min(started.elapsed());
        let started = std::time::Instant::now();
        run(&source, setup(Some(config.clone()))).await;
        best_recorded = best_recorded.min(started.elapsed());
    }
    let per_call = best_recorded.saturating_sub(best_plain).as_nanos() / 20_000;
    println!("plain {best_plain:?} recorded {best_recorded:?} => ~{per_call} ns per gated call");
}

// ---- call records ----------------------------------------------------------------------

fn body_text(copy: Option<&BodyCopy>) -> Option<&str> {
    match copy? {
        BodyCopy::Text(text) => Some(text),
        BodyCopy::Base64(_) => None,
    }
}

#[tokio::test]
async fn every_gated_call_records_its_timing_and_outcome() {
    let outcome = run(MIXED_PROGRAM, mixed(recording())).await;
    assert_eq!(outcome.result, Ok("ok:yes".to_owned()));
    let calls = outcome.calls();
    assert_eq!(calls.len(), 3, "{calls:#?}");
    for call in calls {
        let ended = call.ended_micros.expect("every call ended");
        assert!(ended >= call.started_micros, "{call:#?}");
    }
    let outcomes: Vec<_> = calls.iter().map(|call| call.outcome).collect();
    assert_eq!(
        outcomes,
        [
            Some(CallOutcome::Returned),
            Some(CallOutcome::Failed),
            Some(CallOutcome::Returned)
        ],
        "the denied post failed"
    );
    // Each call shares its index with its decision.
    for (call, record) in calls.iter().zip(outcome.records()) {
        assert_eq!(call.call_index, record.call_index);
        assert_eq!(call.capability, record.capability);
    }
    assert_eq!(calls[2].caller, "@acme/core");
}

#[tokio::test]
async fn an_http_call_keeps_its_request_and_response_with_credentials_masked() {
    let source = r#"
import { get } from "submilli:http";
function main(): string {
  const headers = new Map<string, string>([["Authorization", "Bearer token-one"], ["x-trace", "abc"]]);
  const response = get("https://example.test/data", headers);
  return response.body;
}
"#;
    let outcome = run(
        source,
        Setup {
            record: recording(),
            ..Setup::default()
        },
    )
    .await;
    assert_eq!(outcome.result, Ok("ok".to_owned()));
    let call = outcome.call("http.get");
    let request = call.request.as_ref().expect("the request");
    assert_eq!(request.meta["method"], "GET");
    assert_eq!(request.meta["url"], "https://example.test/data");
    assert_eq!(request.masked_headers, ["Authorization"]);
    let response = call.response.as_ref().expect("the response");
    assert_eq!(response.meta["status"], 200);
    assert_eq!(body_text(response.body.as_ref()), Some("ok"));
    assert_eq!(response.bytes, 2);
    assert_eq!(response.masked_headers, ["set-cookie"]);
    let text = serde_json::to_string(call).unwrap();
    assert!(!text.contains("token-one"), "{text}");
    assert!(!text.contains("session=hidden"), "{text}");
    assert!(text.contains("abc"), "an ordinary header is kept");

    // A refreshed credential leaves the request's digest unchanged.
    let refreshed = run(
        &source.replace("token-one", "token-two"),
        Setup {
            record: recording(),
            ..Setup::default()
        },
    )
    .await;
    assert_eq!(
        refreshed.call("http.get").request.as_ref().unwrap().digest,
        request.digest
    );
}

#[tokio::test]
async fn a_response_over_the_payload_cap_keeps_its_full_size_and_digest() {
    let body = vec![b'x'; 4096];
    let setup = |max_payload_bytes| Setup {
        record: Some(DecisionLogConfig {
            max_payload_bytes,
            ..DecisionLogConfig::default()
        }),
        response_body: Some(body.clone()),
        ..Setup::default()
    };
    let source = r#"
import { get } from "submilli:http";
function main(): string { return get("https://example.test/big").body.length.toString(); }
"#;
    let capped = run(source, setup(100)).await;
    let whole = run(source, setup(1 << 20)).await;
    assert_eq!(capped.result, Ok("4096".to_owned()));
    let capped = *capped.call("http.get").response.clone().unwrap();
    let whole = *whole.call("http.get").response.clone().unwrap();
    assert!(capped.truncated && !whole.truncated);
    assert_eq!(body_text(capped.body.as_ref()).map(str::len), Some(100));
    assert_eq!((capped.bytes, whole.bytes), (4096, 4096));
    assert_eq!(capped.digest, whole.digest);
}

#[tokio::test]
async fn file_and_session_reads_keep_what_they_returned() {
    let source = r#"
import * as fs from "submilli:fs";
import session from "submilli:session";
function main(): string {
  fs.writeText("/notes.txt", "hello file");
  session.set("greeting", "hi");
  const text = fs.readText("/notes.txt");
  const stored = session.get<string>("greeting");
  return (text ?? "") + "|" + (stored ?? "");
}
"#;
    let outcome = run(
        source,
        Setup {
            record: recording(),
            session: true,
            ..Setup::default()
        },
    )
    .await;
    assert_eq!(outcome.result, Ok("hello file|hi".to_owned()));
    let read = outcome.call("fs.read");
    let response = read.response.as_ref().expect("the read's contents");
    assert_eq!(body_text(response.body.as_ref()), Some("hello file"));
    assert_eq!(response.bytes, 10);
    let session = outcome.call("session.read");
    let stored = session.response.as_ref().expect("the stored value");
    assert!(
        body_text(stored.body.as_ref()).is_some_and(|text| text.contains("hi")),
        "{stored:?}"
    );
    // A write is a call too, with its timing and no response copy.
    let write = outcome.call("fs.write");
    assert_eq!(write.outcome, Some(CallOutcome::Returned));
    assert!(write.response.is_none());
}

#[tokio::test]
async fn a_model_call_keeps_its_prompts_reply_and_token_usage() {
    let source = r#"
import llm from "submilli:llm";
function main(): string { return llm.call("open", "what is two plus two").text ?? ""; }
"#;
    let outcome = run(
        source,
        Setup {
            record: recording(),
            llm: Some(LlmLimits::default()),
            ..Setup::default()
        },
    )
    .await;
    assert_eq!(outcome.result, Ok("answer".to_owned()));
    let call = outcome.call("llm.call");
    let request = call.request.as_ref().expect("the prompts");
    assert_eq!(request.meta["model"], "open");
    assert!(body_text(request.body.as_ref()).is_some_and(|t| t.contains("two plus two")));
    assert_eq!(request.bytes, "what is two plus two".len() as u64);
    let response = call.response.as_ref().expect("the reply");
    assert!(body_text(response.body.as_ref()).is_some_and(|t| t.contains("answer")));
    assert_eq!(
        call.usage,
        Some(interpreter::runtime::ModelUsage {
            input_tokens: Some(10),
            output_tokens: Some(10),
        })
    );
}

#[tokio::test]
async fn an_embed_call_keeps_its_texts_and_metadata_but_never_the_vectors() {
    let source = r#"
import embedding from "submilli:embedding";
function main(): string {
  return embedding.embed("open-embed", ["alpha", "beta"], "query").count.toString();
}
"#;
    let outcome = run(
        source,
        Setup {
            record: recording(),
            embedding: true,
            ..Setup::default()
        },
    )
    .await;
    assert_eq!(outcome.result, Ok("2".to_owned()));
    let call = outcome.call("embedding.embed");
    assert_eq!(call.outcome, Some(CallOutcome::Returned));
    let request = call.request.as_ref().expect("the texts");
    assert_eq!(
        request.meta,
        json!({ "op": "embed", "model": "open-embed", "purpose": "query", "count": 2 })
    );
    assert_eq!(
        body_text(request.body.as_ref()),
        Some(r#"["alpha","beta"]"#)
    );
    assert_eq!(request.bytes, "alphabeta".len() as u64);
    let response = call.response.as_ref().expect("the metadata");
    assert_eq!(
        response.meta,
        json!({
            "count": 2,
            "dimensions": 4,
            "identity": "emb1:fake:open-embed:4:01",
            "model": "open-embed",
            "inputTokens": 7,
        })
    );
    assert!(response.body.is_none(), "no vectors: {response:?}");
    assert_eq!(response.bytes, 2 * 4 * 4);
    assert_eq!(
        call.usage,
        Some(interpreter::runtime::ModelUsage {
            input_tokens: Some(7),
            output_tokens: None,
        })
    );
}

#[tokio::test]
async fn a_failed_embed_call_keeps_the_usage_its_sent_batches_reported() {
    for (alias, usage) in [
        (
            "open-embed-flaky",
            Some(interpreter::runtime::ModelUsage {
                input_tokens: Some(4),
                output_tokens: None,
            }),
        ),
        ("open-embed-dark", None),
    ] {
        let source = format!(
            r#"
import embedding from "submilli:embedding";
function main(): string {{
  try {{ embedding.embed("{alias}", ["alpha", "beta"], "document"); return "ok"; }}
  catch (e: Error) {{ return "failed"; }}
}}
"#
        );
        let outcome = run(
            &source,
            Setup {
                record: recording(),
                embedding: true,
                ..Setup::default()
            },
        )
        .await;
        assert_eq!(outcome.result, Ok("failed".to_owned()), "{alias}");
        let call = outcome.call("embedding.embed");
        assert_eq!(call.outcome, Some(CallOutcome::Failed), "{alias}");
        assert_eq!(call.usage, usage, "{alias}");
    }
}

#[tokio::test]
async fn a_model_batch_keeps_each_outcomes_own_failure_and_usage() {
    let source = r#"
import llm from "submilli:llm";
function main(): string {
  const done = llm.batch("open", ["fine", "refuse me"]);
  return done.map((c) => c.ok ? "ok" : (c.reason ?? "?")).join(",");
}
"#;
    let outcome = run(
        source,
        Setup {
            record: recording(),
            llm: Some(LlmLimits::default()),
            ..Setup::default()
        },
    )
    .await;
    assert_eq!(outcome.result, Ok("ok,content-filtered".to_owned()));
    let call = outcome.call("llm.call");
    let meta = &call.response.as_ref().expect("the reply").meta;
    assert_eq!(meta["ok"], json!([true, false]));
    assert_eq!(meta["failures"][0], Value::Null);
    assert_eq!(
        meta["failures"][1],
        json!({
            "kind": "content-filtered",
            "message": "filtered",
            "retryable": false,
            "status": 400,
            "finish_reason": "safety",
        })
    );
    assert_eq!(
        meta["usage"],
        json!([
            { "input_tokens": 10, "output_tokens": 10 },
            { "input_tokens": 3, "output_tokens": null },
        ])
    );
    let body = body_text(call.response.as_ref().unwrap().body.as_ref()).unwrap();
    assert_eq!(body, r#"["answer","partial"]"#);
    // The call's own usage stays the sum, absent where any prompt left a count out.
    assert_eq!(
        call.usage,
        Some(interpreter::runtime::ModelUsage {
            input_tokens: Some(13),
            output_tokens: None,
        })
    );
}

#[tokio::test]
async fn a_model_call_that_fails_whole_records_its_kind_and_the_fields_to_raise_it_again() {
    let source = r#"
import llm from "submilli:llm";
function main(): string {
  try { llm.call("open", "transport down"); } catch (e) { return "caught"; }
  return "answered";
}
"#;
    let outcome = run(
        source,
        Setup {
            record: recording(),
            llm: Some(LlmLimits::default()),
            ..Setup::default()
        },
    )
    .await;
    assert_eq!(outcome.result, Ok("caught".to_owned()));
    let call = outcome.call("llm.call");
    assert_eq!(call.outcome, Some(CallOutcome::Failed));
    let response = call.response.as_ref().expect("the failure");
    assert_eq!(
        response.meta,
        json!({ "call_error": { "kind": "transport", "model": "open", "detail": "connection reset" } })
    );
}

#[tokio::test]
async fn a_failed_http_send_records_its_kind_and_message() {
    let source = r#"
import { get } from "submilli:http";
function main(): string {
  try { get("https://example.test/down"); } catch (e) { return "failed"; }
  return "answered";
}
"#;
    let outcome = run(
        source,
        Setup {
            record: recording(),
            ..Setup::default()
        },
    )
    .await;
    assert_eq!(outcome.result, Ok("failed".to_owned()));
    let call = outcome.call("http.get");
    assert_eq!(call.outcome, Some(CallOutcome::Failed));
    let response = call.response.as_ref().expect("the failure");
    assert_eq!(
        response.meta,
        json!({ "kind": "timeout", "error": "request timed out" })
    );
}

#[tokio::test]
async fn a_secret_read_records_its_timing_and_never_its_value() {
    let source = r#"
import { get } from "@acme/keys";
function main(): string { return get().length.toString(); }
"#;
    let keys = r#"
import { get as read } from "submilli:secrets";
export function get(): string { return read("api-key") ?? ""; }
"#;
    let outcome = run(
        source,
        Setup {
            record: recording(),
            packages: vec![("@acme/keys", keys)],
            ..Setup::default()
        },
    )
    .await;
    let call = outcome.call("secrets.get");
    assert!(call.ended_micros.is_some());
    assert!(
        call.request.is_none() && call.response.is_none(),
        "{call:?}"
    );
}

/// Collects what an observer is shown, in order.
#[derive(Default)]
struct Seen(Mutex<Vec<String>>);

impl RecordObserver for Seen {
    fn call_started(&self, call: &CallRecord) {
        self.push(format!("started {} {}", call.call_index, call.capability));
    }
    fn decision(&self, record: &DecisionRecord) {
        self.push(format!("decision {} {}", record.call_index, record.allowed));
    }
    fn call_finished(&self, call: &CallRecord) {
        let size = call.response.as_ref().map(|r| r.bytes);
        self.push(format!("finished {} {:?}", call.call_index, size));
    }
}

impl Seen {
    fn push(&self, entry: String) {
        self.0.lock().unwrap().push(entry);
    }
}

#[tokio::test]
async fn an_observer_sees_each_call_start_decide_and_finish_in_order() {
    let seen = Arc::new(Seen::default());
    let mut setup = mixed(recording());
    setup.observer = Some(seen.clone());
    let outcome = run(MIXED_PROGRAM, setup).await;
    assert_eq!(outcome.result, Ok("ok:yes".to_owned()));
    assert_eq!(
        *seen.0.lock().unwrap(),
        [
            "started 0 http.get",
            "decision 0 true",
            "finished 0 Some(2)",
            "started 1 http.post",
            "decision 1 false",
            "finished 1 None",
            "started 2 http.get",
            "decision 2 true",
            "finished 2 Some(2)",
        ]
    );
}

#[tokio::test]
async fn an_observed_and_recorded_run_matches_an_unrecorded_one() {
    let plain = run(MIXED_PROGRAM, mixed(None)).await;
    let mut setup = mixed(recording());
    setup.observer = Some(Arc::new(Seen::default()));
    let observed = run(MIXED_PROGRAM, setup).await;
    assert_eq!(observed.result, plain.result);
    assert_eq!(observed.audits, plain.audits);
    assert_eq!(observed.fuel, plain.fuel);
    assert_eq!(observed.memory_peak, plain.memory_peak);
    assert_eq!(observed.host_attached, plain.host_attached);
}

#[tokio::test]
async fn calls_past_the_cap_are_counted_and_truncate_the_log() {
    let outcome = run(
        &caught_denials(5),
        Setup {
            record: Some(DecisionLogConfig {
                max_calls: 2,
                ..DecisionLogConfig::default()
            }),
            ..caught_denial_setup(None)
        },
    )
    .await;
    let log = outcome.log.as_ref().unwrap();
    assert_eq!(log.calls.len(), 2);
    assert_eq!(log.calls_dropped, 3);
    assert!(log.truncated);
    assert_eq!(log.records.len(), 5, "decisions are capped separately");
}

#[tokio::test]
async fn large_payloads_never_crowd_out_a_later_denial() {
    let source = r#"
import { get } from "submilli:http";
function main(): string {
  for (let i = 0; i < 6; i++) { get("https://example.test/big"); }
  let denied = "no";
  try { get("https://example.test/deny"); } catch (e: PermissionDeniedError) { denied = "yes"; }
  return denied;
}
"#;
    let outcome = run(
        source,
        Setup {
            deny: vec![Deny {
                caller: None,
                capability: "http.get",
                path: Some("/deny"),
            }],
            // Six 100 KiB bodies fill this budget to within a record's size without the
            // decision reserve, which leaves the denial nowhere to go.
            record: Some(DecisionLogConfig {
                max_recorder_bytes: 6 * 100 * 1024 + 2048,
                ..DecisionLogConfig::default()
            }),
            response_body: Some(vec![b'x'; 100 * 1024]),
            ..Setup::default()
        },
    )
    .await;
    assert_eq!(outcome.result, Ok("yes".to_owned()));
    let log = outcome.log.as_ref().unwrap();
    assert_eq!(log.dropped, 0, "no decision was lost");
    let denial = log
        .records
        .iter()
        .find(|record| !record.allowed)
        .expect("the denial is recorded");
    assert!(!denial.payload_dropped, "{denial:?}");
    assert!(
        outcome
            .calls()
            .iter()
            .filter_map(|call| call.response.as_deref())
            .any(|response| response.body.is_none() && response.truncated),
        "a response past the reserve keeps its digest and size only"
    );
}

#[tokio::test]
async fn every_kind_of_gated_call_ends_and_keeps_the_payloads_its_class_promises() {
    let lib = r#"
import { check } from "submilli:security";
/**
 * Runs the operation.
 * @param id Identifier of the target.
 * @capability test.com/op { id }
 */
export function run(id: string): void { check("test.com/op", { id }); }
"#;
    let source = r#"
import * as fs from "submilli:fs";
import session from "submilli:session";
import { get } from "submilli:http";
import llm from "submilli:llm";
import { read } from "submilli:code";
import { run } from "@acme/core";
function main(): string {
  fs.writeText("/a.txt", "one\ntwo\n");
  session.set("k", "v");
  const text = fs.readText("/a.txt");
  const lines = read("/a.txt").lines.length;
  const stored = session.get<string>("k");
  const body = get("https://example.test/x").body;
  const answer = llm.call("open", "ping").text ?? "";
  run("1");
  return (text ?? "").length.toString() + ":" + lines.toString() + ":" + (stored ?? "") + ":" + body + ":" + answer;
}
"#;
    let outcome = run(
        source,
        Setup {
            record: recording(),
            session: true,
            llm: Some(LlmLimits::default()),
            packages: vec![("@acme/core", lib)],
            ..Setup::default()
        },
    )
    .await;
    assert_eq!(outcome.result, Ok("8:2:v:ok:answer".to_owned()));
    for call in outcome.calls() {
        assert!(
            matches!(call.outcome, Some(CallOutcome::Returned)),
            "every call ends: {call:#?}"
        );
        assert!(call.ended_micros.is_some(), "{call:#?}");
    }
    let reads: Vec<_> = outcome
        .calls()
        .iter()
        .filter(|call| call.capability == "fs.read")
        .collect();
    assert_eq!(reads.len(), 2, "fs.readText and code.read both read");
    for call in reads {
        assert!(
            call.response.is_some(),
            "a read keeps its contents: {call:#?}"
        );
    }
    assert!(outcome.call("session.read").response.is_some());
    // The program makes one request and it does not redirect, so the one `http.get` call
    // is the request's own. A redirect hop would be a further, payload-less call.
    let http_calls = outcome
        .calls()
        .iter()
        .filter(|call| call.capability == "http.get")
        .count();
    assert_eq!(http_calls, 1);
    let http = outcome.call("http.get");
    assert!(http.request.is_some() && http.response.is_some());
    let model = outcome.call("llm.call");
    let prompt = model.request.as_ref().expect("the prompt");
    assert!(body_text(prompt.body.as_ref()).is_some_and(|text| text.contains("ping")));
    let reply = model.response.as_ref().expect("the reply");
    assert!(body_text(reply.body.as_ref()).is_some_and(|text| text.contains("answer")));
    for capability in ["fs.write", "session.write"] {
        let write = outcome.call(capability);
        assert!(
            matches!(write.outcome, Some(CallOutcome::Returned)) && write.request.is_none(),
            "{capability} keeps its timing and outcome: {write:#?}"
        );
    }
    let check = outcome.call("test.com/op");
    assert!(check.request.is_none() && check.response.is_none());
}
