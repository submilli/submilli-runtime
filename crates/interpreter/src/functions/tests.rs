use std::collections::{BTreeMap, VecDeque};
use std::pin::Pin;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

use wasmtime::Linker;

use super::*;
use crate::compile::parse_script;
use crate::compiler_limits::{COMPILER_STACK_BYTES, MAX_TYPE_DEPTH};
use crate::docs_view::{Availability, DocAvailability};
use crate::packages::DocAudience;
use crate::runtime::decision::{DecisionLog, DecisionLogConfig};
use crate::runtime::durable::EffectId;
use crate::runtime::effects::{EffectCall, EffectHistory, HistoryError, RecordedOutcome};
use crate::runtime::secrets::SecretProvider;
use crate::runtime::security::{CheckOutcome, SecurityCheck};
use crate::runtime::{
    LinkedPackageModule, RuntimeConfig, StoreData, Vfs, dispatch_main_async,
    install_package_modules_async, install_runtime_async, instantiate_program_async,
};
use crate::stdlib::OptionalPackage;
use crate::stdlib::http::transport::{
    DownloadMeta, HttpClient, HttpError, HttpRequest, HttpResponse,
};
use crate::{
    DocCapability, FileId, ModulePath, PackageSourceModule, compile_package, compile_script,
};

struct OpenAvailability;

impl DocAvailability for OpenAvailability {
    fn tag(&self, _tag: &DocCapability) -> Availability {
        Availability::OPEN
    }

    fn capability(&self, _name: &str) -> Availability {
        Availability::OPEN
    }
}

struct AllowAllSecurityCheck;

impl SecurityCheck for AllowAllSecurityCheck {
    fn check(
        &self,
        _caller: &str,
        _capability: &str,
        _context: &serde_json::Value,
    ) -> CheckOutcome {
        CheckOutcome::Allow { rule: None }
    }
}

fn run_on_stack<F, T>(f: F) -> T
where
    F: FnOnce() -> T + Send + 'static,
    T: Send + 'static,
{
    let stack_bytes = COMPILER_STACK_BYTES.max(RuntimeConfig::default().native_stack_size());
    std::thread::Builder::new()
        .name("functions-test".into())
        .stack_size(stack_bytes)
        .spawn(f)
        .expect("spawn functions-test thread")
        .join()
        .expect("functions-test thread panicked")
}

fn run_async_on_stack<F, Fut, T>(f: F) -> T
where
    F: FnOnce() -> Fut + Send + 'static,
    Fut: std::future::Future<Output = T> + 'static,
    T: Send + 'static,
{
    run_on_stack(move || {
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("tokio current_thread runtime");
        rt.block_on(f())
    })
}

fn def(name: &str, description: &str, source: &str) -> FunctionDefinition {
    FunctionDefinition {
        name: name.to_string(),
        description: description.to_string(),
        source: source.to_string(),
    }
}

type SecurityPredicate = dyn Fn(&str, &str, &serde_json::Value) -> bool + Send + Sync;

struct RecordingSecurity {
    seen: Mutex<Vec<(String, String, serde_json::Value)>>,
    allow: Box<SecurityPredicate>,
}

impl RecordingSecurity {
    fn allow_matching(
        allow: impl Fn(&str, &str, &serde_json::Value) -> bool + Send + Sync + 'static,
    ) -> Arc<Self> {
        Arc::new(Self {
            seen: Mutex::new(Vec::new()),
            allow: Box::new(allow),
        })
    }

    fn calls(&self) -> Vec<(String, String, serde_json::Value)> {
        self.seen.lock().expect("seen mutex").clone()
    }
}

impl SecurityCheck for RecordingSecurity {
    fn check(&self, caller: &str, capability: &str, context: &serde_json::Value) -> CheckOutcome {
        self.seen.lock().expect("seen mutex").push((
            caller.to_string(),
            capability.to_string(),
            context.clone(),
        ));
        if (self.allow)(caller, capability, context) {
            CheckOutcome::Allow { rule: None }
        } else {
            CheckOutcome::Deny {
                reason: "denied by test policy".into(),
                rule: None,
            }
        }
    }
}

struct MapSecretProvider {
    secrets: BTreeMap<String, String>,
}

impl MapSecretProvider {
    fn with(entries: &[(&str, &str)]) -> Arc<Self> {
        Arc::new(Self {
            secrets: entries
                .iter()
                .map(|(k, v)| ((*k).to_string(), (*v).to_string()))
                .collect(),
        })
    }
}

impl SecretProvider for MapSecretProvider {
    fn get<'a>(
        &'a self,
        name: &'a str,
    ) -> Pin<Box<dyn std::future::Future<Output = Result<Option<String>, String>> + Send + 'a>>
    {
        let found = self.secrets.get(name).cloned();
        Box::pin(async move { Ok(found) })
    }
}

struct MockHttp {
    calls: AtomicUsize,
    responses: Mutex<VecDeque<Result<HttpResponse, HttpError>>>,
}

impl MockHttp {
    fn scripted(responses: Vec<Result<HttpResponse, HttpError>>) -> Arc<Self> {
        Arc::new(Self {
            calls: AtomicUsize::new(0),
            responses: Mutex::new(responses.into()),
        })
    }

    fn ok_json(url: &str, body: &str) -> Result<HttpResponse, HttpError> {
        Ok(HttpResponse {
            status: 200,
            status_text: "OK".to_string(),
            headers: vec![("content-type".to_string(), "application/json".to_string())],
            body: body.as_bytes().to_vec(),
            final_url: url.to_string(),
        })
    }

    fn call_count(&self) -> usize {
        self.calls.load(Ordering::SeqCst)
    }
}

#[async_trait::async_trait]
impl HttpClient for MockHttp {
    async fn send(&self, _req: &HttpRequest) -> Result<HttpResponse, HttpError> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        self.responses
            .lock()
            .expect("mock http responses")
            .pop_front()
            .unwrap_or_else(|| Err(HttpError::Other("no scripted HTTP response left".into())))
    }

    async fn download(
        &self,
        _req: &HttpRequest,
        _writer: &mut (dyn std::io::Write + Send),
    ) -> Result<DownloadMeta, HttpError> {
        Err(HttpError::Other("download not scripted".into()))
    }
}

struct MemoryHistory {
    state: Mutex<ReplayState>,
}

struct ReplayState {
    outcomes: Vec<RecordedOutcome>,
    cursor: usize,
    replay: bool,
}

impl MemoryHistory {
    fn new() -> Arc<Self> {
        Arc::new(Self {
            state: Mutex::new(ReplayState {
                outcomes: Vec::new(),
                cursor: 0,
                replay: false,
            }),
        })
    }

    fn switch_to_replay(&self) {
        let mut guard = self.state.lock().expect("history mutex");
        guard.replay = true;
        guard.cursor = 0;
    }

    fn recorded_len(&self) -> usize {
        self.state.lock().expect("history mutex").outcomes.len()
    }
}

#[async_trait::async_trait]
impl EffectHistory for MemoryHistory {
    async fn begin(
        &self,
        _operation: &str,
        _fingerprint: [u8; 32],
    ) -> Result<EffectCall, HistoryError> {
        let mut guard = self.state.lock().expect("history mutex");
        let id = EffectId::stub().map_err(|err| HistoryError(err.to_string()))?;
        if guard.replay {
            if let Some(recorded) = guard.outcomes.get(guard.cursor).cloned() {
                let sequence = guard.cursor as u64;
                guard.cursor += 1;
                return Ok(EffectCall {
                    id,
                    sequence,
                    recorded: Some(recorded),
                });
            }
            // Past recorded prefix during continuation: execute live from here!
            let sequence = guard.outcomes.len() as u64;
            return Ok(EffectCall {
                id,
                sequence,
                recorded: None,
            });
        }
        Ok(EffectCall {
            id,
            sequence: guard.outcomes.len() as u64,
            recorded: None,
        })
    }

    async fn complete(
        &self,
        call: &EffectCall,
        outcome: RecordedOutcome,
    ) -> Result<(), HistoryError> {
        let mut guard = self.state.lock().expect("history mutex");
        if guard.replay && (call.sequence as usize) < guard.outcomes.len() {
            return Ok(());
        }
        if call.sequence as usize != guard.outcomes.len() {
            return Err(HistoryError("effect completed out of order".into()));
        }
        guard.outcomes.push(outcome);
        if guard.replay {
            guard.cursor = guard.outcomes.len();
        }
        Ok(())
    }

    async fn ensure_history_consumed(&self) -> Result<(), HistoryError> {
        Ok(())
    }
}

#[test]
fn two_function_set_compiles_caches_and_runs_from_main_without_json_boundaries() {
    run_async_on_stack(|| async {
        let cfg = RuntimeConfig::default();
        let engine = cfg.engine().expect("engine");
        let cache = FunctionCache::new();

        // Both functions export an identically named `Params` interface AND one
        // accepts a closure `(n: number) => number` (non-JSON!) to prove `main`
        // calls `@functions` directly in WasmGC with zero JSON marshalling.
        let set = FunctionSet::new(vec![
            def(
                "lead-scan",
                "Scan leads and transform scores in memory.",
                r#"
                export interface Params {
                    scores: number[];
                }
                function helper(): number {
                    return 999;
                }
                export function run(params: Params, boost: (n: number) => number): number[] {
                    return params.scores.map(boost);
                }
                "#,
            ),
            def(
                "enrich-contact",
                "Format a contact record with normalized domain.",
                r#"
                export interface Params {
                    email: string;
                    score: number;
                }
                export function run(params: Params): string {
                    const parts = params.email.split("@");
                    return `${parts[0]}@${parts[1].toLowerCase()}:${params.score}`;
                }
                "#,
            ),
        ]);

        let compiled_set =
            compile_function_set(&engine, &set, &[], &[], Some(&cache)).expect("compile set");
        assert_eq!(cache.len(), 2);
        assert_eq!(cache.misses(), 2);
        assert_eq!(cache.hits(), 0);

        // Compiling the identical set hits the content-addressed cache for both functions.
        let _recompiled =
            compile_function_set(&engine, &set, &[], &[], Some(&cache)).expect("recompile set");
        assert_eq!(cache.misses(), 2);
        assert_eq!(cache.hits(), 2);

        let main_source = r#"
            import { leadScan, enrichContact } from "@functions";

            function main(): string[] {
                const boosted = leadScan({ scores: [10, 25] }, (x: number): number => x * 2 + 1);
                return boosted.map((score: number): string =>
                    enrichContact({ email: "Ada@EXAMPLE.COM", score })
                );
            }
        "#;

        let parsed = parse_script(main_source, FileId(0));
        let imports = parsed.external_imports().expect("external imports");
        assert!(
            imports.functions,
            "parse_script must mark @functions import"
        );
        assert!(imports.registry_packages.is_empty());

        let compiled_main = compile_script(
            main_source,
            "main.ts",
            FileId(0),
            &[compiled_set.package_declaration()],
            &[],
        )
        .expect("compile main against @functions");

        let main_module = Module::new(&engine, &compiled_main.wasm).expect("main module");
        let mut data = StoreData::with_vfs("test-run".into(), Vfs::tempdir().expect("tempdir"));
        data.security_check = Arc::new(AllowAllSecurityCheck);
        let mut store = cfg.store(&engine, data).expect("store");
        let mut linker = Linker::<StoreData>::new(&engine);
        install_runtime_async(&mut linker, &mut store)
            .await
            .expect("install runtime");
        install_functions_async(&mut linker, &mut store, &compiled_set)
            .await
            .expect("install functions");
        store
            .data_mut()
            .install_type_info(compiled_main.type_info.clone());

        let instance = instantiate_program_async(&linker, &mut store, &main_module)
            .await
            .expect("instantiate main");
        let output = dispatch_main_async(&mut store, &instance)
            .await
            .expect("run main");
        assert_eq!(
            output.as_deref(),
            Some(r#"["Ada@example.com:21","Ada@example.com:51"]"#)
        );

        // Colliding `Params` interfaces are disambiguated in `@functions` docs and
        // scoped cleanly in per-function docs.
        let view = compiled_set.view(&OpenAvailability);
        let all_docs = view.docs("@functions").expect("@functions docs");
        assert!(all_docs.declarations.contains("interface LeadScanParams"));
        assert!(
            all_docs
                .declarations
                .contains("interface EnrichContactParams")
        );

        let single_docs = view
            .docs("@functions/lead-scan")
            .expect("single function docs (kebab)");
        assert_eq!(single_docs.name, "@functions/lead-scan");
        assert!(
            single_docs
                .declarations
                .contains("function leadScan(params: LeadScanParams,"),
            "expected disambiguated signature in per-function docs, got:\n{}",
            single_docs.declarations
        );
        assert!(
            single_docs
                .declarations
                .contains("interface LeadScanParams")
        );
        assert!(!single_docs.declarations.contains("enrichContact"));
        assert!(!single_docs.declarations.contains("EnrichContactParams"));

        let camel_docs = view
            .docs("@functions/leadScan")
            .expect("single function docs (camel)");
        assert_eq!(camel_docs.declarations, single_docs.declarations);
    });
}

#[test]
fn naming_contract_and_compile_errors_report_function_name_and_line() {
    run_on_stack(|| {
        let engine = RuntimeConfig::default().engine().expect("engine");

        // Valid kebab-case names map to camelCase bindings.
        assert_eq!(validate_function_name("lead-scan").unwrap(), "leadScan");
        assert_eq!(validate_function_name("v2-sync-1").unwrap(), "v2Sync1");

        // Invalid spellings and reserved identifiers are rejected.
        assert_eq!(validate_function_name(""), Err(FunctionNameError::Empty));
        let long_name = "a".repeat(MAX_FUNCTION_NAME_LEN + 1);
        assert!(matches!(
            validate_function_name(&long_name),
            Err(FunctionNameError::TooLong { .. })
        ));
        for bad in [
            "LeadScan",
            "lead_scan",
            "1lead",
            "-lead",
            "lead-",
            "lead--scan",
        ] {
            assert_eq!(
                validate_function_name(bad),
                Err(FunctionNameError::InvalidFormat),
                "expected InvalidFormat for `{bad}`"
            );
        }
        for reserved in ["return", "for", "eval", "arguments", "undefined"] {
            assert!(
                matches!(
                    validate_function_name(reserved),
                    Err(FunctionNameError::ReservedExportName { .. })
                ),
                "expected ReservedExportName for `{reserved}`"
            );
        }

        // Duplicate kebab-case names fail naming the duplicate.
        let dup_err = compile_function_set(
            &engine,
            &FunctionSet::new(vec![
                def(
                    "dup-fn",
                    "first",
                    "export function run(): number { return 1; }",
                ),
                def(
                    "dup-fn",
                    "second",
                    "export function run(): number { return 2; }",
                ),
            ]),
            &[],
            &[],
            None,
        )
        .expect_err("duplicate function names must fail");
        assert_eq!(dup_err.function_name(), Some("dup-fn"));
        assert!(matches!(
            dup_err,
            FunctionCompileFailure::DuplicateName { .. }
        ));

        // Distinct kebab-case names that collapse to the same camelCase export fail.
        let collision_err = compile_function_set(
            &engine,
            &FunctionSet::new(vec![
                def(
                    "foo-bar1",
                    "first",
                    "export function run(): number { return 1; }",
                ),
                def(
                    "foo-bar-1",
                    "second",
                    "export function run(): number { return 2; }",
                ),
            ]),
            &[],
            &[],
            None,
        )
        .expect_err("camelCase export collision must fail");
        assert!(matches!(
            collision_err,
            FunctionCompileFailure::ExportNameCollision { ref export_name, .. }
            if export_name == "fooBar1"
        ));

        // Missing `export function run` reports the function name.
        let missing_run = compile_function_set(
            &engine,
            &FunctionSet::new(vec![def(
                "no-run",
                "missing run",
                "export function helper(): number { return 1; }",
            )]),
            &[],
            &[],
            None,
        )
        .expect_err("missing run export must fail");
        assert_eq!(missing_run.function_name(), Some("no-run"));
        assert_eq!(missing_run.line(), Some(1));

        // `export const run = ...` on line 3 is rejected with exact 1-based line 3.
        let const_run = compile_function_set(
            &engine,
            &FunctionSet::new(vec![def(
                "const-run",
                "arrow run",
                "// line 1\nconst x = 1;\nexport const run = (): number => x;\n",
            )]),
            &[],
            &[],
            None,
        )
        .expect_err("const run export must fail");
        assert_eq!(const_run.function_name(), Some("const-run"));
        assert_eq!(const_run.line(), Some(3));

        // Type error on line 4 reports the offending function name and line 4.
        let type_err = compile_function_set(
            &engine,
            &FunctionSet::new(vec![
                def("good-one", "ok", "export function run(): number { return 42; }"),
                def(
                    "broken-fn",
                    "bad type",
                    "export function run(): number {\n  const a = 1;\n  const b = 2;\n  return \"not-a-number\";\n}\n",
                ),
            ]),
            &[],
            &[],
            None,
        )
        .expect_err("type error in function must fail");
        assert_eq!(type_err.function_name(), Some("broken-fn"));
        assert_eq!(type_err.line(), Some(4));

        // Functions cannot import `@functions` (no inter-function calls).
        let self_import = compile_function_set(
            &engine,
            &FunctionSet::new(vec![def(
                "calls-sibling",
                "tries to import @functions",
                "// line 1\nimport { other } from \"@functions\";\nexport function run(): void { other(); }\n",
            )]),
            &[],
            &[],
            None,
        )
        .expect_err("importing @functions inside a function must fail");
        assert_eq!(self_import.function_name(), Some("calls-sibling"));
        assert_eq!(self_import.line(), Some(2));

        // `submilli:functions` is rejected in `main` and in ordinary packages.
        let main_escalate = compile_script(
            "import { escalate } from \"submilli:functions\";\nfunction main(): void { escalate(\"no\"); }\n",
            "main.ts",
            FileId(0),
            &[],
            &[],
        )
        .expect_err("main must not import submilli:functions");
        let main_rendered = format!("{main_escalate:?}");
        assert!(
            main_rendered.contains("@functions"),
            "expected actionable help mentioning @functions, got: {main_rendered}"
        );

        let pkg_escalate = compile_package(
            "@acme/pkg",
            ModulePath::from("index"),
            &[PackageSourceModule {
                path: ModulePath::from("index"),
                source: "import { escalate } from \"submilli:functions\";\nexport function f(): void { escalate(\"no\"); }\n",
            }],
            &[],
        )
        .expect_err("ordinary packages must not import submilli:functions");
        let pkg_rendered = format!("{pkg_escalate:?}");
        assert!(pkg_rendered.contains("@functions"));

        // `Stdlib` discovery omits `submilli:functions`, while `stdlib::functions::docs()`
        // renders its authoring reference (including the pre-side-effect guidance).
        assert!(Stdlib::core().docs("submilli:functions").is_none());
        let authoring_doc = crate::stdlib::functions::docs();
        assert_eq!(authoring_doc.name, "submilli:functions");
        assert!(
            authoring_doc
                .declarations
                .contains("function escalate(reason: string): never;")
        );
        assert!(
            authoring_doc.declarations.contains("side effects")
                && authoring_doc.declarations.contains("not be resumed"),
            "expected pre-side-effect / non-resumable guidance in docs, got:\n{}",
            authoring_doc.declarations
        );
    });
}

#[test]
fn capability_derivation_captures_core_and_optional_stdlib_requirements() {
    run_on_stack(|| {
        let engine = RuntimeConfig::default().engine().expect("engine");
        let stdlib = Stdlib::core().with(OptionalPackage::Agents);

        let set = FunctionSet::new(vec![
            def(
                "crm-triage",
                "Fetch leads from CRM and classify with fast model.",
                r#"
                import * as http from "submilli:http";
                import * as llm from "submilli:llm";
                import { escalate } from "submilli:functions";

                export interface Triage {
                    urgent: boolean;
                }

                export function run(prompt: string): Triage {
                    const resp = http.get("https://api.crm.example/v1/leads");
                    if (resp.status === 403) {
                        escalate("CRM returned 403");
                    }
                    const completion = llm.call<Triage>("fast-triage", prompt);
                    if (!completion.ok) {
                        return { urgent: false };
                    }
                    return completion.value;
                }
                "#,
            ),
            def(
                "delegate-research",
                "Delegate open-ended research to a specialist agent.",
                r#"
                import * as agents from "submilli:agents";

                export function run(question: string): string {
                    return agents.runText("researcher", question);
                }
                "#,
            ),
        ]);

        let compiled = compile_function_set_for(stdlib, &engine, &set, &[], &[], None)
            .expect("compile functions with optional agents stdlib");

        let triage = compiled.get("crm-triage").expect("crm-triage");
        let triage_caps: BTreeMap<&str, &DerivedCapability> = triage
            .requires()
            .iter()
            .map(|cap| (cap.capability.as_str(), cap))
            .collect();
        let http_cap = triage_caps.get("http.get").expect("derives http.get");
        assert_eq!(
            http_cap.known_bindings.get("host").map(String::as_str),
            Some("\"api.crm.example\"")
        );
        let llm_cap = triage_caps.get("llm.call").expect("derives llm.call");
        assert_eq!(
            llm_cap.known_bindings.get("model").map(String::as_str),
            Some("\"fast-triage\"")
        );

        let delegate = compiled
            .get("delegate-research")
            .expect("delegate-research");
        assert!(
            delegate
                .requires()
                .iter()
                .any(|cap| cap.capability == "agent.run"),
            "expected agent.run capability on delegate-research, got: {:?}",
            delegate.requires()
        );
    });
}

#[test]
fn caller_attribution_runs_functions_as_main_and_preserves_package_boundaries() {
    run_async_on_stack(|| async {
        let cfg = RuntimeConfig::default();
        let engine = cfg.engine().expect("engine");

        // Compile a linked `@acme/crm` package that:
        // 1. Reads its own `CRM_TOKEN` secret via `submilli:secrets`,
        // 2. Calls `submilli:security.check("acme.com/crm.query", { org })` to
        //    gate its caller (`"main"`!),
        // 3. Performs an outbound `http.get` attributed to `@acme/crm`, and
        // 4. Exposes `withCallback(fn)` so we can prove a closure created inside a
        //    function still runs as `"main"` even with `@acme/crm` on the stack!
        let crm_pkg = compile_package(
            "@acme/crm",
            ModulePath::from("index"),
            &[PackageSourceModule {
                path: ModulePath::from("index"),
                source: r#"
                import * as secrets from "submilli:secrets";
                import * as http from "submilli:http";
                import { check } from "submilli:security";

                /**
                 * Query CRM using package-scoped credentials.
                 * @param org Organization slug.
                 * @capability acme.com/crm.query { org: $org }
                 */
                export function fetchOrg(org: string): string {
                    check("acme.com/crm.query", { org });
                    const token = secrets.get("CRM_TOKEN") ?? "missing";
                    const headers = new Map<string, string>();
                    headers.set("authorization", "Bearer " + token);
                    const resp = http.get("https://crm.acme.example/orgs/" + org, headers);
                    return resp.body;
                }

                export function invokeCallback<T>(cb: () => T): T {
                    return cb();
                }
                "#,
            }],
            &[],
        )
        .expect("compile @acme/crm");
        let crm_module = Module::new(&engine, &crm_pkg.wasm).expect("crm module");

        let set = FunctionSet::new(vec![
            def(
                "direct-http",
                "Calls http.get directly from the function.",
                r#"
                import * as http from "submilli:http";
                export function run(): string {
                    return http.get("https://direct.example/ping").body;
                }
                "#,
            ),
            def(
                "steal-secret",
                "Tries to read secrets.get directly inside a function.",
                r#"
                import * as secrets from "submilli:secrets";
                export function run(): string {
                    return secrets.get("CRM_TOKEN") ?? "none";
                }
                "#,
            ),
            def(
                "callback-steal-secret",
                "Passes a closure into @acme/crm to try reading secrets under @acme/crm's stack.",
                r#"
                import * as secrets from "submilli:secrets";
                import { invokeCallback } from "@acme/crm";
                export function run(): string {
                    return invokeCallback((): string => secrets.get("CRM_TOKEN") ?? "none");
                }
                "#,
            ),
            def(
                "via-crm-package",
                "Calls @acme/crm which uses its own secret and checks its caller.",
                r#"
                import { fetchOrg } from "@acme/crm";
                export function run(org: string): string {
                    return fetchOrg(org);
                }
                "#,
            ),
        ]);

        let compiled_set = compile_function_set(&engine, &set, &[&crm_pkg.declaration], &[], None)
            .expect("compile function set with @acme/crm");

        let linked_crm = [LinkedPackageModule {
            module: &crm_module,
            declaration: &crm_pkg.declaration,
            type_info: &crm_pkg.type_info,
        }];

        // 1. Denied direct `http.get` inside a function is attributed to `"main"`!
        {
            let security = RecordingSecurity::allow_matching(|_caller, _cap, _ctx| false);
            let mut data =
                StoreData::with_vfs("test-denied".into(), Vfs::tempdir().expect("tempdir"));
            data.security_check = security.clone();
            let mut store = cfg.store(&engine, data).expect("store");
            let err = invoke_function_async::<&str>(
                &mut store,
                &compiled_set,
                &linked_crm,
                "direct-http",
                &[],
            )
            .await
            .expect_err("denied http.get must fail");
            match err {
                FunctionError::Denied(denial) => {
                    assert_eq!(denial.caller, "main");
                    assert_eq!(denial.capability, "http.get");
                }
                other => panic!("expected FunctionError::Denied, got {other:?}"),
            }
            let calls = security.calls();
            assert_eq!(calls.len(), 1);
            assert_eq!(calls[0].0, "main");
            assert_eq!(calls[0].1, "http.get");
        }

        // 2. `secrets.get` inside a function is refused as `"main"` even under AllowAllSecurityCheck,
        //    both directly AND inside a closure invoked by `@acme/crm`!
        for fn_name in ["steal-secret", "callback-steal-secret"] {
            let mut data =
                StoreData::with_vfs("test-secret".into(), Vfs::tempdir().expect("tempdir"));
            data.security_check = Arc::new(AllowAllSecurityCheck);
            data.secret_provider = MapSecretProvider::with(&[("CRM_TOKEN", "s3cr3t")]);
            let mut store = cfg.store(&engine, data).expect("store");
            let err =
                invoke_function_async::<&str>(&mut store, &compiled_set, &linked_crm, fn_name, &[])
                    .await
                    .expect_err("secrets.get in function must be refused as main");
            match err {
                FunctionError::Denied(denial) => {
                    assert_eq!(denial.caller, "main");
                    assert_eq!(denial.capability, "secrets.get");
                }
                other => panic!("expected Denied for `{fn_name}`, got {other:?}"),
            }
        }

        // 3. Calling `@acme/crm` from a function:
        //    - `@acme/crm`'s `submilli:security.check("acme.com/crm.query")` sees `"main"` as caller!
        //    - `@acme/crm`'s `secrets.get` and `http.get` run as `"@acme/crm"`!
        {
            let security = RecordingSecurity::allow_matching(|caller, cap, _ctx| {
                matches!(
                    (caller, cap),
                    ("main", "acme.com/crm.query") | ("@acme/crm", "secrets.get" | "http.get")
                )
            });
            let http = MockHttp::scripted(vec![MockHttp::ok_json(
                "https://crm.acme.example/orgs/globex",
                r#"{"ok":true}"#,
            )]);
            let mut data = StoreData::with_vfs("test-crm".into(), Vfs::tempdir().expect("tempdir"));
            data.security_check = security.clone();
            data.secret_provider = MapSecretProvider::with(&[("CRM_TOKEN", "s3cr3t")]);
            data.http_client = http.clone();
            let mut store = cfg.store(&engine, data).expect("store");

            let out = invoke_function_async(
                &mut store,
                &compiled_set,
                &linked_crm,
                "via-crm-package",
                &[r#""globex""#],
            )
            .await
            .expect("via-crm-package succeeds");
            assert_eq!(out.as_deref(), Some(r#""{\"ok\":true}""#));

            let principals_and_caps: Vec<(String, String)> = security
                .calls()
                .into_iter()
                .map(|(caller, cap, _)| (caller, cap))
                .collect();
            assert_eq!(
                principals_and_caps,
                vec![
                    ("main".to_string(), "acme.com/crm.query".to_string()),
                    ("@acme/crm".to_string(), "secrets.get".to_string()),
                    ("@acme/crm".to_string(), "http.get".to_string()),
                ]
            );
        }

        // 4. `install_package_modules_async` refuses reserved `@functions*` and `main` names.
        for bad_pkg_name in ["@functions", "@functions/spoof", "main"] {
            let spoof = compile_package(
                bad_pkg_name,
                ModulePath::from("index"),
                &[PackageSourceModule {
                    path: ModulePath::from("index"),
                    source: "export function f(): number { return 1; }",
                }],
                &[],
            )
            .expect("compile spoof package");
            let spoof_module = Module::new(&engine, &spoof.wasm).expect("spoof module");
            let data = StoreData::with_vfs("test-spoof".into(), Vfs::tempdir().expect("tempdir"));
            let mut store = cfg.store(&engine, data).expect("store");
            let mut linker = Linker::<StoreData>::new(&engine);
            install_runtime_async(&mut linker, &mut store)
                .await
                .expect("install runtime");
            let err = install_package_modules_async(
                &mut linker,
                &mut store,
                &[LinkedPackageModule {
                    module: &spoof_module,
                    declaration: &spoof.declaration,
                    type_info: &spoof.type_info,
                }],
            )
            .await
            .expect_err("reserved principal name must be refused");
            assert!(
                err.to_string().contains("reserved"),
                "expected reserved principal error for `{bad_pkg_name}`, got: {err}"
            );
        }
    });
}

#[test]
fn discovery_projects_docs_and_search_by_caller_capability_availability() {
    struct HostFilteredAvailability;

    impl DocAvailability for HostFilteredAvailability {
        fn tag(&self, tag: &DocCapability) -> Availability {
            if tag.capability == "http.get" {
                let ctx = crate::docs_view::known_context(tag);
                if ctx.get("host").and_then(|v| v.as_str()) == Some("allowed.example") {
                    return Availability::OPEN;
                }
                return Availability::Denied;
            }
            Availability::OPEN
        }

        fn capability(&self, name: &str) -> Availability {
            if name == "http.get" {
                Availability::OPEN
            } else {
                Availability::Denied
            }
        }
    }

    run_on_stack(|| {
        let engine = RuntimeConfig::default().engine().expect("engine");
        let set = FunctionSet::new(vec![
            def(
                "pure-math",
                "Compute deterministic score weights.",
                "export function run(a: number, b: number): number { return a + b; }",
            ),
            def(
                "allowed-crm",
                "Read allowed CRM endpoint.",
                r#"
                import * as http from "submilli:http";
                export interface AllowedLead { id: string; }
                export interface AllowedStandalone { ok: boolean; }
                export function run(): AllowedLead {
                    const _r = http.get("https://allowed.example/leads");
                    return { id: "1" };
                }
                "#,
            ),
            def(
                "blocked-erp",
                "Read blocked ERP endpoint.",
                r#"
                import * as http from "submilli:http";
                export interface BlockedOrder { orderId: string; }
                export interface BlockedStandalone { secret: number; }
                export function run(): BlockedOrder {
                    const _r = http.get("https://blocked.example/orders");
                    return { orderId: "9" };
                }
                "#,
            ),
        ]);

        let compiled = compile_function_set(&engine, &set, &[], &[], None).expect("compile set");

        let filtered = compiled.view(&HostFilteredAvailability);
        assert!(filtered.is_usable());
        assert!(
            !filtered
                .declaration()
                .types
                .values()
                .any(|t| t.name.contains("BlockedStandalone")),
            "standalone types owned by hidden functions must be pruned from view"
        );
        assert!(filtered.search("BlockedStandalone").is_empty());

        let docs = filtered
            .docs_for("@functions", DocAudience::Agent)
            .expect("@functions docs");
        assert!(docs.declarations.contains("function pureMath("));
        assert!(docs.declarations.contains("function allowedCrm("));
        assert!(docs.declarations.contains("interface AllowedLead"));
        assert!(docs.declarations.contains("interface AllowedStandalone"));
        assert!(!docs.declarations.contains("blockedErp"));
        assert!(!docs.declarations.contains("BlockedOrder"));
        assert!(!docs.declarations.contains("BlockedStandalone"));

        // Per-function docs return Some for visible functions and None for denied/unknown ones,
        // prune sibling functions' standalone types, and honor `DocAudience`.
        let pure_docs = filtered
            .docs("@functions/pure-math")
            .expect("pure-math docs");
        assert!(!pure_docs.declarations.contains("AllowedStandalone"));
        let agent_crm_docs = filtered
            .docs_for("@functions/allowed-crm", DocAudience::Agent)
            .expect("agent crm docs");
        assert!(!agent_crm_docs.declarations.contains("@capability"));
        let author_crm_docs = filtered
            .docs_for("@functions/allowed-crm", DocAudience::Author)
            .expect("author crm docs");
        assert!(
            author_crm_docs
                .declarations
                .contains("@capability http.get"),
            "Author docs must preserve @capability tags, got:\n{}",
            author_crm_docs.declarations
        );
        assert!(filtered.docs("@functions/allowedCrm").is_some());
        assert!(filtered.docs("@functions/blocked-erp").is_none());
        assert!(filtered.docs("@functions/blockedErp").is_none());
        assert!(filtered.docs("@functions/does-not-exist").is_none());

        // Search finds visible functions and hides denied ones.
        let visible_names: Vec<String> = filtered
            .search_functions("")
            .into_iter()
            .map(|s| s.name)
            .collect();
        assert_eq!(visible_names, vec!["pure-math", "allowed-crm"]);
        assert!(filtered.search("allowed").len() == 1);
        assert!(filtered.search("blocked").is_empty());

        // When every function in a set requires a denied capability, `@functions` hides completely.
        let gated_only = compile_function_set(
            &engine,
            &FunctionSet::new(vec![def(
                "blocked-only",
                "Blocked function.",
                r#"
                import * as http from "submilli:http";
                export function run(): number {
                    return http.get("https://blocked.example/x").status;
                }
                "#,
            )]),
            &[],
            &[],
            None,
        )
        .expect("compile gated_only");
        let empty_view = gated_only.view(&HostFilteredAvailability);
        assert!(!empty_view.is_usable());
        assert!(empty_view.docs("@functions").is_none());
        assert!(empty_view.search("").is_empty());
        assert!(empty_view.search_functions("").is_empty());
    });
}

#[test]
fn invoke_validates_arguments_before_side_effects_and_classifies_escalate_and_budget() {
    run_async_on_stack(|| async {
        let cfg = RuntimeConfig::default();
        let engine = cfg.engine().expect("engine");

        let set = FunctionSet::new(vec![
            def(
                "compute-order",
                "Positional, defaulted, and rest parameters.",
                r#"
                import * as http from "submilli:http";

                export interface Customer {
                    id: string;
                    tier: "free" | "pro";
                }

                export function run(
                    customer: Customer,
                    discount: number = 5,
                    ...tags: string[]
                ): { summary: string; count: number } {
                    const resp = http.get("https://orders.example/check");
                    return {
                        summary: `${customer.id}:${customer.tier}:${discount}:${resp.status}`,
                        count: tags.length,
                    };
                }
                "#,
            ),
            def(
                "void-fn",
                "Returns nothing.",
                "export function run(x: number): void { const _y = x + 1; }",
            ),
            def(
                "needs-callback",
                "Has a closure parameter that only main can pass.",
                "export function run(cb: (n: number) => number): number { return cb(3); }",
            ),
            def(
                "generic-fn",
                "Generic run<T>.",
                "export function run<T>(val: T): T { return val; }",
            ),
            def(
                "escalating-fn",
                "Escalates even inside a guest try/catch block.",
                r#"
                import { escalate } from "submilli:functions";

                export function run(reason: string): string {
                    try {
                        escalate(reason);
                    } catch (_err) {
                        return "guest-swallowed-escalation";
                    }
                    return "unreachable";
                }
                "#,
            ),
            def(
                "busy-loop",
                "Burns CPU until store fuel runs out.",
                r#"
                export function run(iterations: number): number {
                    let acc = 0;
                    for (let i = 0; i < iterations; i = i + 1) {
                        acc = acc + i;
                    }
                    return acc;
                }
                "#,
            ),
        ]);

        let compiled = compile_function_set(&engine, &set, &[], &[], None).expect("compile set");

        let make_store = |http: Arc<MockHttp>| {
            let mut data = StoreData::with_vfs("test-invoke".into(), Vfs::tempdir().expect("vfs"));
            data.security_check = Arc::new(AllowAllSecurityCheck);
            data.http_client = http;
            cfg.store(&engine, data).expect("store")
        };

        // 1. Unknown function -> Unavailable.
        {
            let http = MockHttp::scripted(Vec::new());
            let mut store = make_store(http);
            let err = invoke_function_async::<&str>(&mut store, &compiled, &[], "missing", &[])
                .await
                .expect_err("unknown function");
            assert!(matches!(err, FunctionError::Unavailable(_)));
        }

        // 2. Arity error, malformed JSON, uncastable param, generic `run<T>`, and runtime
        //    shape mismatches all return `InvalidArguments` BEFORE `run` performs `http.get`!
        let http = MockHttp::scripted(vec![MockHttp::ok_json(
            "https://orders.example/check",
            "{}",
        )]);

        // Too few arguments (0 < min_args 1)
        {
            let mut store = make_store(http.clone());
            let err =
                invoke_function_async::<&str>(&mut store, &compiled, &[], "compute-order", &[])
                    .await
                    .expect_err("missing required arg");
            assert!(matches!(err, FunctionError::InvalidArguments(_)));
        }
        // Malformed JSON
        {
            let mut store = make_store(http.clone());
            let err =
                invoke_function_async(&mut store, &compiled, &[], "compute-order", &["{not-json"])
                    .await
                    .expect_err("malformed json");
            assert!(matches!(err, FunctionError::InvalidArguments(_)));
        }
        // Runtime type mismatch on `Customer.tier` ("enterprise" not in `"free" | "pro"`)
        {
            let mut store = make_store(http.clone());
            let err = invoke_function_async(
                &mut store,
                &compiled,
                &[],
                "compute-order",
                &[r#"{"id":"c1","tier":"enterprise"}"#],
            )
            .await
            .expect_err("invalid union variant");
            assert!(matches!(err, FunctionError::InvalidArguments(_)));
        }
        // Runtime type mismatch on rest argument (`123` instead of `string`)
        {
            let mut store = make_store(http.clone());
            let err = invoke_function_async(
                &mut store,
                &compiled,
                &[],
                "compute-order",
                &[r#"{"id":"c1","tier":"pro"}"#, "10", r#""ok""#, "123"],
            )
            .await
            .expect_err("invalid rest item");
            assert!(matches!(err, FunctionError::InvalidArguments(_)));
        }
        // Uncastable closure parameter & generic `run<T>`
        {
            let mut store = make_store(http.clone());
            let err =
                invoke_function_async(&mut store, &compiled, &[], "needs-callback", &["null"])
                    .await
                    .expect_err("closure param");
            assert!(matches!(err, FunctionError::InvalidArguments(_)));

            let err = invoke_function_async(&mut store, &compiled, &[], "generic-fn", &["1"])
                .await
                .expect_err("generic run");
            assert!(matches!(err, FunctionError::InvalidArguments(_)));
        }
        // Zero side effects occurred during all rejected invocations!
        assert_eq!(http.call_count(), 0);

        // 3. Valid invocations: omitted defaulted param + rest args, and `void` -> `Ok(None)`.
        {
            let mut store = make_store(http.clone());
            let out = invoke_function_async(
                &mut store,
                &compiled,
                &[],
                "compute-order",
                &[r#"{"id":"cust-7","tier":"pro"}"#],
            )
            .await
            .expect("valid invoke with omitted default");
            assert_eq!(
                out.as_deref(),
                Some(r#"{"count":0,"summary":"cust-7:pro:5:200"}"#)
            );
            assert_eq!(http.call_count(), 1);
        }
        {
            let http2 = MockHttp::scripted(vec![MockHttp::ok_json(
                "https://orders.example/check",
                "{}",
            )]);
            let mut store = make_store(http2);
            let out = invoke_function_async(
                &mut store,
                &compiled,
                &[],
                "computeOrder",
                &[
                    r#"{"id":"cust-8","tier":"free"}"#,
                    "15",
                    r#""vip""#,
                    r#""annual""#,
                ],
            )
            .await
            .expect("valid invoke with rest args via camelCase name");
            assert_eq!(
                out.as_deref(),
                Some(r#"{"count":2,"summary":"cust-8:free:15:200"}"#)
            );
        }
        {
            let mut store = make_store(MockHttp::scripted(Vec::new()));
            let out = invoke_function_async(&mut store, &compiled, &[], "void-fn", &["41"])
                .await
                .expect("void function succeeds");
            assert_eq!(out, None);
        }

        // 4. `escalate(reason)` surfaces as `FunctionError::Escalated` and cannot be caught by guest `try/catch`!
        {
            let mut store = make_store(MockHttp::scripted(Vec::new()));
            let err = invoke_function_async(
                &mut store,
                &compiled,
                &[],
                "escalating-fn",
                &[r#""customer requested refund > $500""#],
            )
            .await
            .expect_err("escalate must terminate the invocation");
            match err {
                FunctionError::Escalated { reason } => {
                    assert_eq!(reason, "customer requested refund > $500");
                }
                other => panic!("expected FunctionError::Escalated, got {other:?}"),
            }

            // Calling `escalatingFn` from `main` inside a guest `try/catch` in `main`
            // also terminates the run with `Escalated` rather than letting `main` catch it.
            let main_src = r#"
                import { escalatingFn } from "@functions";
                function main(): string {
                    try {
                        return escalatingFn("from-main-caller");
                    } catch (_e) {
                        return "caught-in-main";
                    }
                }
            "#;
            let compiled_main = compile_script(
                main_src,
                "main.ts",
                FileId(0),
                &[compiled.package_declaration()],
                &[],
            )
            .expect("compile main calling escalatingFn");
            let main_mod = Module::new(&engine, &compiled_main.wasm).expect("main module");
            let mut store = make_store(MockHttp::scripted(Vec::new()));
            let mut linker = Linker::<StoreData>::new(&engine);
            install_runtime_async(&mut linker, &mut store)
                .await
                .expect("install runtime");
            install_functions_async(&mut linker, &mut store, &compiled)
                .await
                .expect("install functions");
            store
                .data_mut()
                .install_type_info(compiled_main.type_info.clone());
            let inst = instantiate_program_async(&linker, &mut store, &main_mod)
                .await
                .expect("instantiate main");
            let main_err = dispatch_main_async(&mut store, &inst)
                .await
                .expect_err("escalate inside @functions must terminate main");
            let escalated = main_err
                .downcast_ref::<Escalated>()
                .expect("main error must downcast to Escalated");
            assert_eq!(escalated.reason, "from-main-caller");
        }

        // 5. Fuel exhaustion surfaces as `FunctionError::BudgetExhausted`.
        {
            let tight_cfg = RuntimeConfig {
                fuel: 5_000,
                ..Default::default()
            };
            let tight_engine = tight_cfg.engine().expect("tight engine");
            let tight_compiled =
                compile_function_set(&tight_engine, &set, &[], &[], None).expect("compile");
            let mut data = StoreData::with_vfs("test-fuel".into(), Vfs::tempdir().expect("vfs"));
            data.security_check = Arc::new(AllowAllSecurityCheck);
            let mut store = tight_cfg.store(&tight_engine, data).expect("tight store");
            let err =
                invoke_function_async(&mut store, &tight_compiled, &[], "busy-loop", &["1000000"])
                    .await
                    .expect_err("tight fuel must exhaust");
            assert!(
                matches!(err, FunctionError::BudgetExhausted(_)),
                "expected BudgetExhausted, got {err:?}"
            );
        }
    });
}

#[test]
fn invoke_participates_in_effect_history_journal_and_replay() {
    run_async_on_stack(|| async {
        let cfg = RuntimeConfig::default();
        let engine = cfg.engine().expect("engine");

        let set = FunctionSet::new(vec![def(
            "two-step-sync",
            "Fetches step 1 then step 2 so replay can resume after step 1.",
            r#"
            import * as http from "submilli:http";

            export function run(itemId: string): string {
                const first = http.get("https://sync.example/step1/" + itemId).body;
                const second = http.get("https://sync.example/step2/" + itemId).body;
                return `${first}+${second}`;
            }
            "#,
        )]);
        let compiled = compile_function_set(&engine, &set, &[], &[], None).expect("compile");

        let history = MemoryHistory::new();
        let single_step_set = FunctionSet::new(vec![def(
            "fetch-once",
            "Fetches an item once.",
            r#"
            import * as http from "submilli:http";
            export function run(itemId: string): string {
                return http.get("https://sync.example/step1/" + itemId).body;
            }
            "#,
        )]);
        let single_compiled =
            compile_function_set(&engine, &single_step_set, &[], &[], None).expect("compile");

        let replay_http = MockHttp::scripted(vec![MockHttp::ok_json(
            "https://sync.example/step1/42",
            "alpha",
        )]);
        {
            let mut data = StoreData::with_vfs("run-1".into(), Vfs::tempdir().expect("vfs"));
            data.security_check = Arc::new(AllowAllSecurityCheck);
            data.http_client = replay_http.clone();
            data.effects = Some(history.clone());
            let mut store = cfg.store(&engine, data).expect("store");
            let first = invoke_function_async(
                &mut store,
                &single_compiled,
                &[],
                "fetch-once",
                &[r#""42""#],
            )
            .await
            .expect("first run");
            assert_eq!(first.as_deref(), Some(r#""alpha""#));
        }
        assert_eq!(replay_http.call_count(), 1);
        assert_eq!(history.recorded_len(), 1);

        // Switch history to replay: `two-step-sync` replays `step1/42` ("alpha") from the
        // journal WITHOUT calling `http` a second time for step 1, and executes `step2/42`
        // live against `continuation_http`!
        history.switch_to_replay();
        let continuation_http = MockHttp::scripted(vec![MockHttp::ok_json(
            "https://sync.example/step2/42",
            "beta",
        )]);
        {
            let mut data = StoreData::with_vfs("run-2".into(), Vfs::tempdir().expect("vfs"));
            data.security_check = Arc::new(AllowAllSecurityCheck);
            data.http_client = continuation_http.clone();
            data.effects = Some(history.clone());
            let mut store = cfg.store(&engine, data).expect("store");
            let resumed =
                invoke_function_async(&mut store, &compiled, &[], "two-step-sync", &[r#""42""#])
                    .await
                    .expect("resumed run");
            assert_eq!(resumed.as_deref(), Some(r#""alpha+beta""#));
        }
        // Only 1 live HTTP call happened in run 2 (`step2/42`), proving `step1/42` replayed!
        assert_eq!(continuation_http.call_count(), 1);
        assert_eq!(history.recorded_len(), 2);
    });
}

#[test]
fn round_one_regressions_config_name_narrowing_classes_closure_return_surrogates_limits_and_decision_log()
 {
    run_async_on_stack(|| async {
        let cfg = RuntimeConfig::default();
        let engine = cfg.engine().expect("engine");

        // 1. Over-deep `PackageDeclaration` in `dependencies` is rejected with `InvalidDependency`
        //    even when the `FunctionSet` itself is empty (before digesting or caching).
        {
            let mut deep_ty = crate::Type::Number;
            for _ in 0..=(MAX_TYPE_DEPTH + 2) {
                deep_ty = crate::Type::Array(Box::new(deep_ty));
            }
            let mut bad_dep = PackageDeclaration {
                package_name: "@acme/deep".to_string(),
                ..Default::default()
            };
            bad_dep
                .runtime_globals
                .insert(crate::mangle::package_symbol("@acme/deep", "g"), deep_ty);
            let cache = FunctionCache::new();
            let err = compile_function_set(
                &engine,
                &FunctionSet::default(),
                &[&bad_dep],
                &[],
                Some(&cache),
            )
            .expect_err("over-deep dependency must be rejected");
            assert!(
                matches!(err, FunctionCompileFailure::InvalidDependency { .. }),
                "expected InvalidDependency, got {err:?}"
            );
            assert_eq!(err.function_name(), None);
        }

        // Compile a `@acme/crm` package for `DecisionLog` `package_line` verification, plus a
        // function set covering `"config"` naming, unexported narrowing classes, `object` closure
        // return rejection, lone-surrogate round-trip, and empty-description JSDoc fallback.
        let crm_pkg = compile_package(
            "@acme/crm",
            ModulePath::from("index"),
            &[PackageSourceModule {
                path: ModulePath::from("index"),
                source: r#"
                import * as http from "submilli:http";
                export function pingCrm(): string {
                    return http.get("https://crm.acme.example/ping").body;
                }
                "#,
            }],
            &[],
        )
        .expect("compile @acme/crm");
        let crm_module = Module::new(&engine, &crm_pkg.wasm).expect("crm module");
        let linked_crm = [LinkedPackageModule {
            module: &crm_module,
            declaration: &crm_pkg.declaration,
            type_info: &crm_pkg.type_info,
        }];

        let set = FunctionSet::new(vec![
            // Function named `"config"` must parse as an ordinary function module, not config.ts!
            def(
                "config",
                "Read runtime configuration summary.",
                r#"
                const prefix = "cfg";
                export function run(env: string): string {
                    return `${prefix}:${env}`;
                }
                "#,
            ),
            // Unexported class hierarchy emitting `mod:@functions/narrow-class#...` narrowing checks
            // must link cleanly onto `@functions` when called from `main`.
            def(
                "narrow-class",
                "Exercise internal class narrowing export forwarding.",
                r#"
                class Animal {
                    label(): string { return "animal"; }
                }
                class Dog extends Animal {
                    bark(): string { return "woof"; }
                }
                export function run(flag: boolean): string {
                    const pet: Animal = flag ? new Dog() : new Animal();
                    if (pet instanceof Dog) {
                        return pet.bark();
                    }
                    return pet.label();
                }
                "#,
            ),
            // `run(): object` returning a closure must fail with `FunctionError::Failed` (`TypeError`)
            // rather than serializing `"null"`.
            def(
                "returns-closure-as-object",
                "Returns a function disguised behind `object`.",
                r#"
                export function run(): object {
                    return (): void => {};
                }
                "#,
            ),
            // Echoes UTF-16 strings (including lone surrogates) and exercises metered JSON arg parsing.
            def(
                "echo-text",
                "",
                r#"
                /** Echoes input text verbatim for UTF-16 fidelity checks. */
                export function run(text: string): string {
                    return text;
                }
                "#,
            ),
            // Direct and package-mediated `http.get` for `DecisionLog` `line` / `package_line` checks.
            def(
                "audit-lines",
                "Calls http.get directly then via @acme/crm.",
                r#"
                import * as http from "submilli:http";
                import { pingCrm } from "@acme/crm";
                export function run(): string {
                    const direct = http.get("https://direct.example/one").body;
                    const pkg = pingCrm();
                    return `${direct}:${pkg}`;
                }
                "#,
            ),
        ]);

        let compiled = compile_function_set(&engine, &set, &[&crm_pkg.declaration], &[], None)
            .expect("compile regression function set");

        // 2. Empty `FunctionDefinition.description` falls back to `run`'s JSDoc summary, and
        //    non-empty `FunctionDefinition.description` suppresses missing-doc warnings on `run`!
        let echo_fn = compiled.get("echo-text").expect("echo-text");
        assert_eq!(
            echo_fn.description(),
            "Echoes input text verbatim for UTF-16 fidelity checks."
        );
        let config_fn = compiled.get("config").expect("config");
        assert!(
            !config_fn.warnings().iter().any(|w| w
                .message
                .contains("exported symbol `run` has no doc comment")),
            "non-empty FunctionDefinition.description must suppress missing-doc warning on `run`, got: {:?}",
            config_fn.warnings()
        );
        let view = compiled.view(&OpenAvailability);
        assert_eq!(
            view.search_functions("UTF-16")
                .iter()
                .map(|s| s.name.as_str())
                .collect::<Vec<_>>(),
            vec!["echo-text"]
        );

        // 3. Both `"config"` and `"narrow-class"` link and execute from `main` via `@functions`.
        {
            let main_source = r#"
                import { config, narrowClass } from "@functions";
                function main(): string {
                    return `${config("prod")}|${narrowClass(true)}|${narrowClass(false)}`;
                }
            "#;
            let compiled_main = compile_script(
                main_source,
                "main.ts",
                FileId(0),
                &[compiled.package_declaration()],
                &[],
            )
            .expect("compile main calling config and narrowClass");
            let main_mod = Module::new(&engine, &compiled_main.wasm).expect("main mod");
            let mut data =
                StoreData::with_vfs("test-main-link".into(), Vfs::tempdir().expect("vfs"));
            data.security_check = Arc::new(AllowAllSecurityCheck);
            let mut store = cfg.store(&engine, data).expect("store");
            let mut linker = Linker::<StoreData>::new(&engine);
            install_runtime_async(&mut linker, &mut store)
                .await
                .expect("install runtime");
            install_package_modules_async(&mut linker, &mut store, &linked_crm)
                .await
                .expect("install crm");
            install_functions_async(&mut linker, &mut store, &compiled)
                .await
                .expect("install functions");
            store
                .data_mut()
                .install_type_info(compiled_main.type_info.clone());
            let inst = instantiate_program_async(&linker, &mut store, &main_mod)
                .await
                .expect("instantiate main");
            let out = dispatch_main_async(&mut store, &inst)
                .await
                .expect("run main");
            assert_eq!(out.as_deref(), Some("cfg:prod|woof|animal"));
        }

        // 4. `invoke_function_async` runs `"config"`, rejects `run(): object` returning a closure,
        //    and round-trips lone UTF-16 surrogates losslessly.
        {
            let mut data =
                StoreData::with_vfs("test-invoke-reg".into(), Vfs::tempdir().expect("vfs"));
            data.security_check = Arc::new(AllowAllSecurityCheck);
            let mut store = cfg.store(&engine, data).expect("store");
            let cfg_out =
                invoke_function_async(&mut store, &compiled, &linked_crm, "config", &[r#""dev""#])
                    .await
                    .expect("invoke config");
            assert_eq!(cfg_out.as_deref(), Some(r#""cfg:dev""#));

            let mut data2 =
                StoreData::with_vfs("test-closure-ret".into(), Vfs::tempdir().expect("vfs"));
            data2.security_check = Arc::new(AllowAllSecurityCheck);
            let mut store2 = cfg.store(&engine, data2).expect("store2");
            let closure_err = invoke_function_async::<&str>(
                &mut store2,
                &compiled,
                &linked_crm,
                "returns-closure-as-object",
                &[],
            )
            .await
            .expect_err("returning closure as `object` must fail");
            assert!(
                matches!(
                    closure_err,
                    FunctionError::Failed(ref err)
                        if err.to_string().contains("cannot be serialized to JSON")
                ),
                "expected TypeError Failed for closure return, got {closure_err:?}"
            );

            let mut data3 =
                StoreData::with_vfs("test-surrogate".into(), Vfs::tempdir().expect("vfs"));
            data3.security_check = Arc::new(AllowAllSecurityCheck);
            let mut store3 = cfg.store(&engine, data3).expect("store3");
            let surr_out = invoke_function_async(
                &mut store3,
                &compiled,
                &linked_crm,
                "echo-text",
                &[r#""\ud800""#],
            )
            .await
            .expect("lone surrogate echo");
            assert_eq!(surr_out.as_deref(), Some(r#""\ud800""#));
        }

        // 5. `DecisionLog` under `invoke_function_async` attributes the function's line as `line`
        //    with `package_line: None` for direct calls, and populates `package_line` for `@acme/crm`!
        {
            let http = MockHttp::scripted(vec![
                MockHttp::ok_json("https://direct.example/one", "d1"),
                MockHttp::ok_json("https://crm.acme.example/ping", "p2"),
            ]);
            let mut data = StoreData::with_vfs("test-dlog".into(), Vfs::tempdir().expect("vfs"));
            data.security_check = Arc::new(AllowAllSecurityCheck);
            data.http_client = http;
            let log = DecisionLog::install(&mut data, DecisionLogConfig::default());
            let mut store = cfg.store(&engine, data).expect("store");
            let out = invoke_function_async::<&str>(
                &mut store,
                &compiled,
                &linked_crm,
                "audit-lines",
                &[],
            )
            .await
            .expect("audit-lines succeeds");
            assert_eq!(out.as_deref(), Some(r#""d1:p2""#));

            let finished = log.finish();
            assert_eq!(finished.records.len(), 2);
            assert_eq!(finished.calls.len(), 2);
            assert!(finished.records[0].line.is_some());
            assert!(finished.records[1].line.is_some());

            let direct_call = &finished.calls[0];
            assert_eq!(direct_call.caller, "main");
            assert!(
                direct_call.line.is_some(),
                "direct call inside @functions/audit-lines must record `line`"
            );
            assert_eq!(
                direct_call.package_line, None,
                "direct call inside @functions/audit-lines must not set `package_line`"
            );

            let pkg_call = &finished.calls[1];
            assert_eq!(pkg_call.caller, "@acme/crm");
            assert!(
                pkg_call.line.is_some(),
                "package call reached from @functions/audit-lines must record caller `line`"
            );
            assert_eq!(
                pkg_call.package_line.as_ref().map(|pl| pl.package.as_str()),
                Some("@acme/crm")
            );
        }

        // 6. Metered JSON argument parsing enforces fuel before allocating guest strings.
        {
            let tiny_fuel_cfg = RuntimeConfig {
                fuel: 50,
                ..Default::default()
            };
            let tiny_engine = tiny_fuel_cfg.engine().expect("tiny engine");
            let tiny_compiled =
                compile_function_set(&tiny_engine, &set, &[&crm_pkg.declaration], &[], None)
                    .expect("compile tiny");
            let tiny_crm_mod = Module::new(&tiny_engine, &crm_pkg.wasm).expect("tiny crm mod");
            let tiny_linked = [LinkedPackageModule {
                module: &tiny_crm_mod,
                declaration: &crm_pkg.declaration,
                type_info: &crm_pkg.type_info,
            }];
            let mut data =
                StoreData::with_vfs("test-arg-fuel".into(), Vfs::tempdir().expect("vfs"));
            data.security_check = Arc::new(AllowAllSecurityCheck);
            let mut store = tiny_fuel_cfg.store(&tiny_engine, data).expect("tiny store");
            let big_json_arg = format!("\"{}\"", "x".repeat(10_000));
            let err = invoke_function_async(
                &mut store,
                &tiny_compiled,
                &tiny_linked,
                "echo-text",
                &[big_json_arg.as_str()],
            )
            .await
            .expect_err("large JSON arg under tiny fuel must exhaust budget");
            assert!(
                matches!(err, FunctionError::BudgetExhausted(_)),
                "expected BudgetExhausted for large JSON arg, got {err:?}"
            );
        }

        // 7. Cross-namespace collision: an exported class/type named `leadScan` in a sibling
        //    function (`other-fn`) is disambiguated (`OtherFnLeadScan`) against `lead-scan`'s
        //    camelCase value export (`leadScan`) so `new OtherFnLeadScan()`, `.d.ts` rendering,
        //    and single-function `docs("@functions/...")` all stay collision-free.
        {
            let collision_set = FunctionSet::new(vec![
                def(
                    "lead-scan",
                    "Increments a score.",
                    "export function run(x: number): number { return x + 1; }",
                ),
                def(
                    "other-fn",
                    "Exports a class whose authored name matches lead-scan's camelCase export.",
                    r#"
                    export class leadScan {
                        v: number = 7;
                    }
                    export function run(): leadScan {
                        return new leadScan();
                    }
                    "#,
                ),
            ]);
            let collision_compiled = compile_function_set(&engine, &collision_set, &[], &[], None)
                .expect("compile cross-namespace collision set");
            let collision_view = collision_compiled.view(&OpenAvailability);

            let full_docs = collision_view.docs("@functions").expect("@functions docs");
            assert!(full_docs.declarations.contains("function leadScan("));
            assert!(full_docs.declarations.contains("class OtherFnLeadScan"));
            assert!(
                !full_docs.declarations.contains("class leadScan"),
                "colliding class name must be disambiguated, got:\n{}",
                full_docs.declarations
            );

            let lead_docs = collision_view
                .docs("@functions/lead-scan")
                .expect("@functions/lead-scan docs");
            assert!(lead_docs.declarations.contains("function leadScan("));
            assert!(!lead_docs.declarations.contains("OtherFnLeadScan"));
            assert!(!lead_docs.declarations.contains("otherFn"));

            let other_docs = collision_view
                .docs("@functions/other-fn")
                .expect("@functions/other-fn docs");
            assert!(other_docs.declarations.contains("function otherFn("));
            assert!(other_docs.declarations.contains("class OtherFnLeadScan"));
            assert!(
                !other_docs.declarations.contains("function leadScan("),
                "sibling function must not leak into single-function docs:\n{}",
                other_docs.declarations
            );

            let main_src = r#"
                import { leadScan, OtherFnLeadScan } from "@functions";
                function main(): string {
                    const inst = new OtherFnLeadScan();
                    return `${leadScan(inst.v)}`;
                }
            "#;
            let compiled_main = compile_script(
                main_src,
                "main.ts",
                FileId(0),
                &[collision_compiled.package_declaration()],
                &[],
            )
            .expect("compile main importing both leadScan and OtherFnLeadScan");
            let main_mod = Module::new(&engine, &compiled_main.wasm).expect("main mod");
            let mut data =
                StoreData::with_vfs("test-collision".into(), Vfs::tempdir().expect("vfs"));
            data.security_check = Arc::new(AllowAllSecurityCheck);
            let mut store = cfg.store(&engine, data).expect("store");
            let mut linker = Linker::<StoreData>::new(&engine);
            install_runtime_async(&mut linker, &mut store)
                .await
                .expect("install runtime");
            install_functions_async(&mut linker, &mut store, &collision_compiled)
                .await
                .expect("install functions");
            store
                .data_mut()
                .install_type_info(compiled_main.type_info.clone());
            let inst = instantiate_program_async(&linker, &mut store, &main_mod)
                .await
                .expect("instantiate main");
            let out = dispatch_main_async(&mut store, &inst)
                .await
                .expect("run main");
            assert_eq!(out.as_deref(), Some("8"));
        }
    });
}

#[test]
fn a_function_set_compiles_from_a_worker_sized_thread() {
    // The set compiles on one compiler thread of its own, so a caller with a
    // runtime worker's stack is enough.
    let compiled = std::thread::Builder::new()
        .stack_size(2 * 1024 * 1024)
        .spawn(|| {
            let engine = RuntimeConfig::default().engine().expect("engine");
            let set = FunctionSet::new(vec![
                FunctionDefinition {
                    name: "first".into(),
                    description: "The first.".into(),
                    source: "export function run(n: number): number { return n + 1; }".into(),
                },
                FunctionDefinition {
                    name: "second".into(),
                    description: "The second.".into(),
                    source: "export function run(n: number): number { return n * 2; }".into(),
                },
            ]);
            compile_function_set(&engine, &set, &[], &[], None)
                .expect("compile set")
                .functions
                .len()
        })
        .expect("spawn a worker-sized thread")
        .join()
        .expect("the worker-sized thread does not panic");
    assert_eq!(compiled, 2);
}
