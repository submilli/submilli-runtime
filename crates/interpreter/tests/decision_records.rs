//! Decision records: what the per-run recorder sees at the policy and invariant seams, and
//! that installing it changes nothing a program, its embedder's audit, or its fuel can see.

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

use interpreter::runtime::security::AuditDecision;
use interpreter::runtime::{
    CheckOutcome, DecisionAction, DecisionCause, DecisionExplanation, DecisionLog,
    DecisionLogConfig, DecisionLogOutput, DecisionRecord, EntryPath, InMemorySessionKv,
    LinkedPackageModule, RuleCitation, SecurityCheck, StoreData, Vfs,
    install_package_modules_async, install_runtime_host_functions, install_runtime_store_bound,
    install_tenant_limits, limits::ExecutionUsage,
};
use interpreter::stdlib::git::GitConfig;
use interpreter::stdlib::http::transport::{
    DownloadMeta, HttpClient, HttpError, HttpRequest, HttpResponse, RedirectHop,
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
    fn new(deny: Vec<Deny>) -> Arc<Self> {
        Arc::new(Self {
            rules: Rules {
                deny,
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
        Some(match self.rules.denying(caller, capability, context) {
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
        })
    }
}

/// Answers every request; when `redirect_to` is set, first walks the request's redirect
/// guard to that URL, as a compliant transport does for each hop it follows.
struct Web {
    redirect_to: Option<&'static str>,
}

#[async_trait::async_trait]
impl HttpClient for Web {
    async fn send(&self, req: &HttpRequest) -> Result<HttpResponse, HttpError> {
        if let (Some(target), Some(guard)) = (self.redirect_to, req.redirect_guard.as_ref()) {
            let url = url::Url::parse(target).map_err(|e| HttpError::Other(e.to_string()))?;
            guard
                .authorize(&RedirectHop {
                    method: "GET",
                    url: &url,
                    method_rewritten: false,
                    body_len: 0,
                })
                .map_err(HttpError::PermissionDenied)?;
        }
        Ok(HttpResponse {
            status: 200,
            status_text: "OK".into(),
            headers: Vec::new(),
            body: b"ok".to_vec(),
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
    strip_debug_info: bool,
    git: bool,
    session: bool,
}

struct Outcome {
    result: Result<String, String>,
    fuel: u64,
    audits: Vec<Value>,
    consulted: u64,
    log: Option<DecisionLogOutput>,
    host_attached: u64,
}

impl Outcome {
    fn records(&self) -> &[DecisionRecord] {
        &self.log.as_ref().expect("a recorder was installed").records
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

    let policy = Capture::new(setup.deny);
    let mut data = StoreData::with_vfs(Vfs::tempdir().expect("tempdir"));
    data.install_type_info(script.type_info.clone());
    data.http_client = Arc::new(Web {
        redirect_to: setup.redirect_to,
    });
    data.security_check = policy.clone();
    if setup.git {
        data.git = Some(GitConfig {
            name: "Agent".into(),
            email: "agent@example.com".into(),
            username: None,
        });
    }
    if setup.session {
        data.session_kv = Some(Arc::new(InMemorySessionKv::default()));
    }
    let log = setup
        .record
        .map(|config| DecisionLog::install(&mut data, config));

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

    let fuel = ExecutionUsage::capture(&store, cfg.fuel)
        .expect("usage")
        .fuel;
    let host_attached = store.data().tenant_limits.host_attached_bytes();
    let audits = policy.audits.lock().unwrap().clone();
    Outcome {
        result,
        fuel,
        audits,
        consulted: policy.rules.consulted.load(Ordering::Relaxed),
        log: log.map(|log| log.finish()),
        host_attached,
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

#[tokio::test]
async fn recorder_buffers_are_charged_to_host_memory_and_refunded() {
    let source = caught_denials(50);
    let setup = || Setup {
        deny: vec![Deny {
            caller: None,
            capability: "http.get",
            path: Some("/deny"),
        }],
        ..Setup::default()
    };
    let unrecorded = run(&source, setup()).await;
    // `run` reads host memory before `finish` releases the log's charge.
    let recorded = run(
        &source,
        Setup {
            record: recording(),
            ..setup()
        },
    )
    .await;
    assert!(
        recorded.host_attached > unrecorded.host_attached,
        "the recorder's buffers are charged: {} vs {}",
        recorded.host_attached,
        unrecorded.host_attached
    );

    // A cap of 10 decisions bounds the charge however long the loop runs.
    let bounded = |count: u32| {
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
                        max_decisions: 10,
                        ..DecisionLogConfig::default()
                    }),
                    ..Setup::default()
                },
            )
            .await
        }
    };
    assert_eq!(
        bounded(100).await.host_attached,
        bounded(1000).await.host_attached,
        "host memory does not grow past the cap"
    );
}

#[tokio::test]
async fn a_session_list_denial_is_recorded_as_filtered_and_the_program_succeeds() {
    let source = r#"
import session from "submilli:session";
function main(): string {
  session.set("hidden", 1);
  const page = session.list("", 10, null);
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
