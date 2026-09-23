//! Async compile + run pipeline that captures `console.log` output
//! regardless of whether the program succeeds or traps.
//!
//! `RuntimeConfig::run` drops the console buffer on the failure path; this
//! module replicates the engine/store/linker/instantiate dance so the buffer
//! is readable after `dispatch_main_async` returns in either branch.

use std::io::Write;
use std::sync::{Arc, Mutex};
use std::time::Instant;

use interpreter::diagnostics::{self, Severity};
use interpreter::runtime::{
    AuthProxy, ExecutionTokenBudget, HttpClient, LinkedPackageModule, LlmProvider, McpTransport,
    RuntimeConfig, SecretProvider, SecurityCheck, SessionKvStore, StoreData, Vfs, VfsInfo,
    install_package_modules_async, install_runtime_store_bound, install_tenant_limits,
};
use interpreter::{
    BacktraceMode, Diagnostic, FileId, ParsedScript, ScriptImports, Sources,
    compile_parsed_script_timed, dispatch_main_async, parse_script, render_backtrace,
};
use tracing::debug;
use wasmtime::{Engine, Linker, Module, Trap};

use crate::app::PreparedBlueprintPackages;
use crate::error::{DiagnosticNote, DiagnosticPayload, ErrorKind, ExecuteError};
use crate::mcp::McpCatalog;

pub struct RunOutcome {
    /// Already-JSON-encoded `main` return.
    pub value: Option<String>,
    pub console_raw: String,
    pub error: Option<ExecuteError>,
    /// Operator-facing notices from `@mcp/<server>` discovery — tools dropped or
    /// degraded because their schema wasn't representable. Empty when there's
    /// nothing to report.
    pub discovery_warnings: Vec<String>,
}

const FILENAME: &str = "<execute>";

pub(crate) struct ParsedExecute {
    sources: Sources,
    file: FileId,
    parsed: ParsedScript,
}

impl ParsedExecute {
    pub(crate) fn imports(&self) -> ScriptImports {
        self.parsed.external_imports()
    }
}

pub(crate) fn parse(code: &str) -> ParsedExecute {
    let (sources, file) = Sources::single(FILENAME, code);
    let parsed = parse_script(code, file);
    ParsedExecute {
        sources,
        file,
        parsed,
    }
}

/// The per-request host capabilities a script runs against: outbound-auth
/// injection, the semantic-security policy, and the HTTP client. Grouped so the
/// run signature stays readable as the set grows.
pub struct HostServices {
    pub git: Result<Option<interpreter::stdlib::git::GitConfig>, String>,
    pub auth_proxy: Arc<dyn AuthProxy>,
    pub secret_provider: Arc<dyn SecretProvider>,
    pub security_check: Arc<dyn SecurityCheck>,
    pub http_client: Arc<dyn HttpClient>,
    /// Outbound `@mcp/<server>` dispatch — the JSON-RPC `tools/call` transport.
    pub mcp_transport: Arc<dyn McpTransport>,
    /// `submilli:session` storage for the session this run belongs to.
    pub session_kv: Arc<dyn SessionKvStore>,
    /// Outbound `submilli:llm` dispatch. `None` when no operator dispatch is
    /// installed, which is the catchable configuration error a script sees —
    /// deliberately not an internal failure, because a program running against a
    /// server with no model provider is a configuration state, not a bug.
    pub llm_provider: Option<Arc<dyn LlmProvider>>,
    /// This execution's token budget, reserving against its own ceiling and the
    /// server-wide one together. Released on drop, so a run that traps or times
    /// out returns its reservation.
    pub llm_budget: Option<Arc<ExecutionTokenBudget>>,
}

pub(crate) struct RunnerImports<'a> {
    pub packages: &'a Arc<PreparedBlueprintPackages>,
    pub mcps: &'a Arc<McpCatalog>,
}

pub(crate) struct RunnerRuntime<'a> {
    pub engine: &'a Engine,
    pub base_linker: &'a Linker<StoreData>,
    pub config: &'a RuntimeConfig,
}

/// Run `code` against a caller-provided VFS. Ownership of `vfs` lives outside:
/// `ephemeral` callers pass an owning `Vfs::tempdir()` (wiped when it drops here
/// at return); `per_session` / `persistent` callers pass a non-owning VFS so the
/// directory survives this call.
pub(crate) async fn run(
    code: &str,
    parsed: ParsedExecute,
    runtime: RunnerRuntime<'_>,
    vfs: Vfs,
    vfs_info: VfsInfo,
    services: HostServices,
    imports: RunnerImports<'_>,
) -> RunOutcome {
    let owned_code = code.to_owned();
    let engine = runtime.engine.clone();
    let linker = runtime.base_linker.clone();
    let config = runtime.config.clone();
    let packages = Arc::clone(imports.packages);
    let mcps = Arc::clone(imports.mcps);
    let (request, cancelled) = tokio::sync::oneshot::channel();
    // This task owns the store independently of the request. Dropping the
    // request signals cancellation; the owner drains workers before exiting.
    let owner = tokio::spawn(async move {
        run_inner(
            &owned_code,
            parsed,
            RunnerRuntime {
                engine: &engine,
                base_linker: &linker,
                config: &config,
            },
            (vfs, vfs_info),
            services,
            RunnerImports {
                packages: &packages,
                mcps: &mcps,
            },
            cancelled,
        )
        .await
    });
    let outcome = match owner.await {
        Ok(outcome) => outcome,
        Err(error) => internal_failure(&format!("execution task failed: {error}")),
    };
    drop(request);
    crate::metrics::execution(match &outcome.error {
        None => "success",
        Some(error) => error_kind_tag(error.kind),
    });
    if let Some(error) = &outcome.error {
        report_to_sentry(code, error);
    }
    outcome
}

async fn run_inner(
    code: &str,
    mut parsed: ParsedExecute,
    runtime: RunnerRuntime<'_>,
    (vfs, vfs_info): (Vfs, VfsInfo),
    services: HostServices,
    imports: RunnerImports<'_>,
    mut cancelled: tokio::sync::oneshot::Receiver<()>,
) -> RunOutcome {
    let git = match services.git {
        Ok(git) => git,
        Err(error) => return internal_failure(&error),
    };
    let discovery_warnings: Vec<String> = imports
        .mcps
        .warnings()
        .map(|w| format!("@mcp/{}: {}", w.server, w.message))
        .collect();
    if parsed.parsed.has_errors() {
        return compile_failure(
            &parsed.sources,
            parsed.file,
            parsed.parsed.diagnostics(),
            discovery_warnings,
        );
    }

    let package_refs: Vec<_> = imports.packages.script_declarations.iter().collect();
    let mcp_refs = imports.mcps.defs_refs();
    // The script's `file` id stamps compile diagnostics. Package sources are
    // also registered so package-originated traps render source context.
    register_package_sources(&mut parsed.sources, imports.packages);
    let compiled = match compile_parsed_script_timed(
        code,
        FILENAME,
        &parsed.parsed,
        &imports.packages.stdlib_declarations,
        &package_refs,
        &mcp_refs,
    ) {
        Ok(out) => out,
        Err(diags) => {
            return compile_failure(&parsed.sources, parsed.file, &diags, discovery_warnings);
        }
    };
    crate::metrics::compile_phases(&compiled.timings);

    let buf = Arc::new(Mutex::new(Vec::<u8>::new()));
    let mut data = StoreData::with_vfs_and_cap(vfs, runtime.config.max_store_bytes);
    data.git = git;
    data.vfs_info = vfs_info;
    data.auth_proxy = services.auth_proxy;
    data.secret_provider = services.secret_provider;
    data.security_check = services.security_check;
    data.http_client = services.http_client;
    data.mcp_transport = Some(services.mcp_transport);
    data.session_kv = Some(services.session_kv);
    data.llm_provider = services.llm_provider;
    data.llm_budget = services.llm_budget;
    data.metrics = Arc::new(crate::metrics::SentryMetricsSink);
    data.console = Box::new(Sink(buf.clone()));
    data.install_type_info(compiled.type_info.clone());

    let mut rt = crate::metrics::RuntimePhaseTimings::default();

    let phase_start = Instant::now();
    let mut store = match runtime.config.store_async(runtime.engine, data) {
        Ok(s) => s,
        Err(err) => return internal_failure(&format!("store init failed: {err}")),
    };
    let execution = async {
        // Without this the store has no `ResourceLimiter`, and the engine falls back
        // to its 1 GiB abort-safety cap — which exists to avoid an OOM-abort, not to
        // bound a tenant. Every other embedder installs it; the server is the one
        // that must.
        install_tenant_limits(&mut store);
        rt.store_init = phase_start.elapsed();

        let phase_start = Instant::now();
        let module = match Module::new(runtime.engine, &compiled.wasm) {
            Ok(m) => m,
            Err(err) => return internal_failure(&format!("module load failed: {err}")),
        };
        rt.module_compile = phase_start.elapsed();

        let mut linker = runtime.base_linker.clone();
        let package_modules: Vec<_> = imports
            .packages
            .modules
            .iter()
            .map(|package| LinkedPackageModule {
                module: &package.module,
                declaration: &package.declaration,
                type_info: &package.type_info,
            })
            .collect();
        let phase_start = Instant::now();
        if let Err(err) = install_runtime_store_bound(&mut linker, &mut store) {
            return internal_failure(&format!("install runtime failed: {err}"));
        }
        rt.link_runtime = phase_start.elapsed();

        let phase_start = Instant::now();
        if let Err(err) =
            install_package_modules_async(&mut linker, &mut store, &package_modules).await
        {
            return internal_failure(&format!("install packages failed: {err}"));
        }
        rt.link_packages = phase_start.elapsed();

        let phase_start = Instant::now();
        let instance = match linker.instantiate_async(&mut store, &module).await {
            Ok(i) => i,
            Err(err) => return internal_failure(&format!("instantiate_async failed: {err}")),
        };
        rt.instantiate = phase_start.elapsed();

        let _watchdog = runtime.config.arm_timeout(runtime.engine);
        let phase_start = Instant::now();
        let dispatch = dispatch_main_async(&mut store, &instance).await;
        rt.execute = phase_start.elapsed();
        crate::metrics::runtime_phases(&rt);
        log_phase_breakdown(&compiled.timings, &rt);
        let console_raw = String::from_utf8_lossy(&buf.lock().unwrap()).into_owned();

        match dispatch {
            Ok(value) => RunOutcome {
                value,
                console_raw,
                error: None,
                discovery_warnings,
            },
            Err(err) => RunOutcome {
                value: None,
                error: Some(classify_runtime_error(&err, &parsed.sources, parsed.file)),
                console_raw,
                discovery_warnings,
            },
        }
    };
    let outcome = tokio::select! {
        biased;
        _ = &mut cancelled => internal_failure("execution cancelled"),
        outcome = execution => outcome,
    };
    store.data_mut().blocking_work.finish().await;
    outcome
}

fn compile_failure(
    sources: &Sources,
    file: FileId,
    diags: &[Diagnostic],
    discovery_warnings: Vec<String>,
) -> RunOutcome {
    let line_index = sources
        .get(file)
        .expect("script file is registered")
        .line_index();
    let mut message = String::new();
    let mut diagnostics_out = Vec::with_capacity(diags.len());
    for d in diags {
        message.push_str(&diagnostics::render(d, sources));
        let (line, column) = line_index.line_col(d.span.start);
        diagnostics_out.push(DiagnosticPayload {
            severity: match d.severity {
                Severity::Error => "error",
                Severity::Warning => "warning",
            },
            line,
            column,
            message: d.message.clone(),
            notes: d
                .notes
                .iter()
                .map(|(span, msg)| {
                    let (l, c) = line_index.line_col(span.start);
                    DiagnosticNote {
                        line: l,
                        column: c,
                        message: msg.clone(),
                    }
                })
                .collect(),
        });
    }
    RunOutcome {
        value: None,
        console_raw: String::new(),
        error: Some(ExecuteError {
            kind: ErrorKind::CompileError,
            message,
            diagnostics: diagnostics_out,
        }),
        discovery_warnings,
    }
}

fn classify_runtime_error(err: &wasmtime::Error, sources: &Sources, file: FileId) -> ExecuteError {
    let kind = match err.downcast_ref::<Trap>() {
        Some(Trap::Interrupt) => ErrorKind::Timeout,
        Some(Trap::OutOfFuel) => ErrorKind::FuelExhausted,
        _ => ErrorKind::RuntimeError,
    };
    // drops middle host frames; full trace available from the CLI
    let message = render_backtrace(err, sources, file, BacktraceMode::LlmTrimmed)
        .unwrap_or_else(|| format!("{err:#}"));
    ExecuteError {
        kind,
        message,
        diagnostics: Vec::new(),
    }
}

/// Emit a per-run phase breakdown at `debug` level so local runs show where the
/// wall-clock went without touching the production `info` stream. Enable with
/// `RUST_LOG=submilli_server=debug`. The same numbers reach Sentry as
/// distributions (`compile.phase_ms` + `runtime.phase_ms`); this is the
/// eyeball-it-immediately view.
fn log_phase_breakdown(
    compile: &interpreter::PhaseTimings,
    rt: &crate::metrics::RuntimePhaseTimings,
) {
    let ms = |d: std::time::Duration| d.as_secs_f64() * 1000.0;
    let total = compile.lex
        + compile.parse
        + compile.typecheck
        + compile.capture
        + compile.desugar
        + compile.codegen
        + rt.store_init
        + rt.module_compile
        + rt.link_runtime
        + rt.link_packages
        + rt.instantiate
        + rt.execute;
    debug!(
        "run phases (ms): lex={:.2} parse={:.2} typecheck={:.2} capture={:.2} \
         desugar={:.2} codegen={:.2} | store_init={:.2} module_compile={:.2} \
         link_runtime={:.2} link_packages={:.2} instantiate={:.2} execute={:.2} | total={:.2}",
        ms(compile.lex),
        ms(compile.parse),
        ms(compile.typecheck),
        ms(compile.capture),
        ms(compile.desugar),
        ms(compile.codegen),
        ms(rt.store_init),
        ms(rt.module_compile),
        ms(rt.link_runtime),
        ms(rt.link_packages),
        ms(rt.instantiate),
        ms(rt.execute),
        ms(total),
    );
}

fn register_package_sources(sources: &mut Sources, packages: &PreparedBlueprintPackages) {
    for package in &packages.modules {
        for source in &package.sources {
            sources.add(source.path.clone(), source.text.clone());
        }
    }
}

/// Report a failed execution to Sentry with the source that produced it and the
/// serialized error response attached. A no-op when no Sentry client is bound
/// (tests, or a build without `sentry::init`). The `submilli.error_kind` tag
/// lets operators mute the expected-but-noisy classes (compile errors from
/// LLM-generated code) independently of traps and internal faults.
fn error_kind_tag(kind: ErrorKind) -> &'static str {
    match kind {
        ErrorKind::CompileError => "compile_error",
        ErrorKind::Timeout => "timeout",
        ErrorKind::FuelExhausted => "fuel_exhausted",
        ErrorKind::RuntimeError => "runtime_error",
        ErrorKind::BlueprintNotFound => "blueprint_not_found",
        ErrorKind::PackageResolution => "package_resolution",
        ErrorKind::InvalidRequest => "invalid_request",
    }
}

fn report_to_sentry(code: &str, error: &ExecuteError) {
    let kind = error_kind_tag(error.kind);
    sentry::with_scope(
        |scope| {
            scope.set_tag("submilli.error_kind", kind);
            scope.set_extra("code", code.into());
            if let Ok(response) = serde_json::to_value(error) {
                scope.set_extra("response", response);
            }
        },
        || {
            let summary = error.message.lines().next().unwrap_or_default();
            sentry::capture_message(&format!("[{kind}] {summary}"), sentry::Level::Error);
        },
    );
}

fn internal_failure(msg: &str) -> RunOutcome {
    RunOutcome {
        value: None,
        console_raw: String::new(),
        error: Some(ExecuteError {
            kind: ErrorKind::RuntimeError,
            message: format!("internal: {msg}"),
            diagnostics: Vec::new(),
        }),
        discovery_warnings: Vec::new(),
    }
}

struct Sink(Arc<Mutex<Vec<u8>>>);

impl Write for Sink {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        self.0.lock().unwrap().write(buf)
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use interpreter::runtime::security::CheckOutcome;
    use interpreter::runtime::{InMemorySessionKv, install_runtime_host_functions};
    use std::time::Duration;

    struct PausedGit {
        started: tokio::sync::Notify,
        resumed: std::sync::atomic::AtomicBool,
        finish: Mutex<std::sync::mpsc::Receiver<()>>,
    }

    impl SecurityCheck for PausedGit {
        fn check(&self, _: &str, capability: &str, _: &serde_json::Value) -> CheckOutcome {
            if capability == "fs.write" {
                self.resumed
                    .store(true, std::sync::atomic::Ordering::Relaxed);
            }
            if capability == "git.init" {
                self.started.notify_one();
                self.finish
                    .lock()
                    .unwrap()
                    .recv_timeout(Duration::from_secs(5))
                    .unwrap();
            }
            CheckOutcome::Allow
        }
    }

    struct UnusedMcp;

    impl McpTransport for UnusedMcp {
        fn call<'a>(
            &'a self,
            _: &'a str,
            _: &'a str,
            _: &'a str,
        ) -> std::pin::Pin<
            Box<
                dyn std::future::Future<
                        Output = Result<serde_json::Value, interpreter::runtime::mcp::McpCallError>,
                    > + Send
                    + 'a,
            >,
        > {
            Box::pin(async { panic!("test must not call MCP") })
        }
    }

    #[tokio::test]
    async fn cancelled_request_waits_for_git_before_releasing_store_and_vfs() {
        let vfs = Vfs::tempdir().unwrap();
        let root = vfs.root().to_owned();
        let defaults = StoreData::with_vfs(Vfs::none());
        let (finish, cleanup) = std::sync::mpsc::channel();
        let security = Arc::new(PausedGit {
            started: tokio::sync::Notify::new(),
            resumed: std::sync::atomic::AtomicBool::new(false),
            finish: Mutex::new(cleanup),
        });
        let services = HostServices {
            git: Ok(Some(interpreter::stdlib::git::GitConfig {
                name: "Agent".into(),
                email: "agent@example.com".into(),
                username: None,
            })),
            auth_proxy: defaults.auth_proxy,
            secret_provider: defaults.secret_provider,
            security_check: security.clone(),
            http_client: defaults.http_client,
            mcp_transport: Arc::new(UnusedMcp),
            session_kv: Arc::new(InMemorySessionKv::default()),
            llm_provider: None,
            llm_budget: None,
        };
        let config = RuntimeConfig::default();
        let engine = config.engine().unwrap();
        let mut linker = Linker::new(&engine);
        install_runtime_host_functions(&mut linker).unwrap();
        let packages = Arc::new(PreparedBlueprintPackages {
            stdlib_declarations: interpreter::runtime::stdlib_package_declarations(),
            ..Default::default()
        });
        let mcps = Arc::new(McpCatalog::empty());
        let code = r#"
            import { Repository } from "submilli:git";
            import * as fs from "submilli:fs";
            function main(): void {
                try { Repository.init("/repo"); } catch (error) {}
                fs.writeText("/continued", "must not run");
            }
        "#;
        let mut request = Box::pin(run(
            code,
            parse(code),
            RunnerRuntime {
                engine: &engine,
                base_linker: &linker,
                config: &config,
            },
            vfs,
            defaults.vfs_info,
            services,
            RunnerImports {
                packages: &packages,
                mcps: &mcps,
            },
        ));
        tokio::select! {
            _ = security.started.notified() => {},
            outcome = &mut request => panic!("worker never started: {:?}", outcome.error),
            _ = tokio::time::sleep(Duration::from_secs(5)) => panic!("worker never started"),
        }
        drop(request);
        tokio::task::yield_now().await;
        assert!(root.exists(), "VFS must survive while the worker is active");
        assert!(!root.join("continued").exists());
        assert_eq!(
            Arc::strong_count(&security),
            3,
            "test, store, and worker must retain the policy until cleanup completes"
        );
        finish.send(()).unwrap();
        tokio::time::timeout(Duration::from_secs(5), async {
            while root.exists() {
                assert!(!root.join("continued").exists(), "cancelled guest resumed");
                tokio::time::sleep(Duration::from_millis(1)).await;
            }
        })
        .await
        .expect("execution owner must finish cleanup");
        // The owner's store and worker both release their policy references.
        assert_eq!(Arc::strong_count(&security), 1);
        assert!(!security.resumed.load(std::sync::atomic::Ordering::Relaxed));
    }
}
