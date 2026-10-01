//! Async compile + run pipeline that captures `console.log` output
//! regardless of whether the program succeeds or traps.
//!
//! `RuntimeConfig::run` drops the console buffer on the failure path; this
//! module replicates the engine/store/linker/instantiate dance so the buffer
//! is readable after `dispatch_main_async` returns in either branch.

use std::io::Write;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Instant;

use interpreter::diagnostics::{self, Severity};
use interpreter::runtime::{
    AuthProxy, ExecutionTokenBudget, HttpClient, LinkedPackageModule, LlmProvider, McpTransport,
    RuntimeConfig, SecretProvider, SecurityCheck, SessionKvStore, StoreData, Vfs, VfsInfo,
    install_package_modules_async, install_runtime_store_bound, install_tenant_limits,
    is_memory_exhausted,
};
use interpreter::{
    BacktraceMode, Diagnostic, FileId, ParsedScript, ScriptImports, Sources,
    compile_parsed_script_timed, dispatch_main_async, failure_message, instantiate_program_async,
    parse_script, render_backtrace,
};
use tracing::debug;
use wasmtime::{Engine, Linker, Module, Trap};

use crate::app::PreparedBlueprintPackages;
use crate::compiler_thread;
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
    pub(crate) fn imports(&self) -> Result<ScriptImports, String> {
        self.parsed.external_imports().map_err(|error| {
            error
                .into_diagnostics(self.file)
                .iter()
                .map(|diagnostic| diagnostics::render(diagnostic, &self.sources))
                .collect()
        })
    }
}

pub(crate) fn parse(code: &str) -> Result<ParsedExecute, interpreter::source::SourceError> {
    let (sources, file) = Sources::single(FILENAME, code)?;
    let parsed = parse_script(code, file);
    Ok(ParsedExecute {
        sources,
        file,
        parsed,
    })
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
    if let Err(error) = register_package_sources(&mut parsed.sources, imports.packages) {
        return internal_failure(&error.to_string());
    }
    let compiled = compiler_thread::run(|| {
        compile_parsed_script_timed(
            code,
            FILENAME,
            &parsed.parsed,
            &imports.packages.stdlib_declarations,
            &package_refs,
            &mcp_refs,
        )
    });
    let compiled = match compiled {
        Ok(Ok(out)) => out,
        Ok(Err(diags)) => {
            return compile_failure(&parsed.sources, parsed.file, &diags, discovery_warnings);
        }
        Err(error) => return internal_failure(&error.to_string()),
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
    // The shared ticker must not interrupt setup using RuntimeConfig's
    // single-watchdog deadline. Arm this store only when the program begins.
    store.set_epoch_deadline(u64::MAX);
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

        // Instantiation runs the program's top-level statements, so the deadline
        // covers it and its failure is reported as one in `main` is.
        crate::execution_timeout::arm(&mut store, runtime.config.timeout);
        let phase_start = Instant::now();
        let instantiated = instantiate_program_async(&linker, &mut store, &module).await;
        rt.instantiate = phase_start.elapsed();

        let phase_start = Instant::now();
        let dispatch = match instantiated {
            Ok(instance) => dispatch_main_async(&mut store, &instance).await,
            Err(err) if raised_by_top_level_statements(&err) => Err(err),
            Err(err) => return internal_failure(&format!("instantiate_async failed: {err}")),
        };
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
    _file: FileId,
    diags: &[Diagnostic],
    discovery_warnings: Vec<String>,
) -> RunOutcome {
    let mut message = String::new();
    for diagnostic in diags {
        message.push_str(&diagnostics::render(diagnostic, sources));
    }
    let payloads = diags
        .iter()
        .map(|diagnostic| {
            let (line, column) = diagnostic_position(sources, diagnostic.span)?;
            let notes = diagnostic
                .notes
                .iter()
                .map(|(span, message)| {
                    let (line, column) = diagnostic_position(sources, *span)?;
                    Ok(DiagnosticNote {
                        line,
                        column,
                        message: message.clone(),
                    })
                })
                .collect::<Result<Vec<_>, interpreter::source::SourceError>>()?;
            Ok(DiagnosticPayload {
                severity: match diagnostic.severity {
                    Severity::Error => "error",
                    Severity::Warning => "warning",
                },
                line,
                column,
                message: diagnostic.message.clone(),
                notes,
            })
        })
        .collect::<Result<Vec<_>, interpreter::source::SourceError>>();
    let diagnostics_out = match payloads {
        Ok(payloads) => payloads,
        Err(error) => {
            let mut outcome =
                internal_failure(&format!("{message}invalid diagnostic metadata: {error}"));
            outcome.discovery_warnings = discovery_warnings;
            return outcome;
        }
    };
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

fn diagnostic_position(
    sources: &Sources,
    span: interpreter::Span,
) -> Result<(u32, u32), interpreter::source::SourceError> {
    interpreter::Span::new(span.file, span.start, span.end)?;
    if span.file.reserved_path().is_some() {
        return Ok((0, 0));
    }
    let source = sources
        .get(span.file)
        .ok_or(interpreter::source::SourceError::UnknownFile { file: span.file })?;
    source.span_text(span)?;
    source.line_index().line_col(span.start)
}

fn classify_runtime_error(err: &wasmtime::Error, sources: &Sources, file: FileId) -> ExecuteError {
    let kind = match err.downcast_ref::<Trap>() {
        Some(Trap::Interrupt) => ErrorKind::Timeout,
        Some(Trap::OutOfFuel) => ErrorKind::FuelExhausted,
        _ if is_memory_exhausted(err) => ErrorKind::MemoryExhausted,
        _ => ErrorKind::RuntimeError,
    };
    // drops middle host frames; full trace available from the CLI
    let message = render_backtrace(err, sources, file, BacktraceMode::LlmTrimmed)
        .unwrap_or_else(|| unframed_message(err));
    ExecuteError {
        kind,
        message,
        diagnostics: Vec::new(),
    }
}

/// Whether an instantiation error was raised once the program's top-level
/// statements were running, as opposed to the module failing to link. These are
/// the shapes `instantiate_program_async` yields from the start function: a
/// trap, a shaped throw, a fatal host error, and memory exhaustion.
fn raised_by_top_level_statements(err: &wasmtime::Error) -> bool {
    err.is::<Trap>()
        || err.is::<interpreter::backtrace::ThrownError>()
        || err.is::<interpreter::runtime::host::FatalHostError>()
        || is_memory_exhausted(err)
}

/// Top-level statements have no frames to render, so a trap or an uncaught
/// throw raised there gets only the header a rendered failure has. Any other
/// failure keeps its whole cause chain, which is what reaches the caller for a
/// host or setup failure.
fn unframed_message(err: &wasmtime::Error) -> String {
    if err.is::<Trap>() || err.is::<interpreter::backtrace::ThrownError>() {
        return format!("error: {}", failure_message(err));
    }
    format!("{err:#}")
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

fn register_package_sources(
    sources: &mut Sources,
    packages: &PreparedBlueprintPackages,
) -> Result<(), interpreter::source::SourceError> {
    for package in &packages.modules {
        for source in &package.sources {
            sources.add(source.path.clone(), &source.text)?;
        }
    }
    Ok(())
}

/// Whether [`report_to_sentry`] attaches the failed program's source and the
/// rendered error, which quotes the failing lines. Process-wide, like the Sentry
/// hub it feeds: set once at boot from the resolved config, and off until then,
/// so a report can never carry source the operator didn't opt into.
static TELEMETRY_INCLUDE_SOURCE: AtomicBool = AtomicBool::new(false);

pub fn set_telemetry_include_source(include: bool) {
    TELEMETRY_INCLUDE_SOURCE.store(include, Ordering::Relaxed);
}

/// Report a failed execution to Sentry. Without the operator's opt-in
/// ([`set_telemetry_include_source`]) the report is the error kind and the
/// message's first line; with it, the program's source and the full error
/// response — a backtrace or compile diagnostics, both of which quote source
/// lines — are attached as extras. A no-op when no Sentry client is bound
/// (tests, or a build without `sentry::init`). The `submilli.error_kind` tag
/// lets operators mute the expected-but-noisy classes (compile errors from
/// LLM-generated code) independently of traps and internal faults.
fn error_kind_tag(kind: ErrorKind) -> &'static str {
    match kind {
        ErrorKind::CompileError => "compile_error",
        ErrorKind::Timeout => "timeout",
        ErrorKind::FuelExhausted => "fuel_exhausted",
        ErrorKind::MemoryExhausted => "memory_exhausted",
        ErrorKind::RuntimeError => "runtime_error",
        ErrorKind::BlueprintNotFound => "blueprint_not_found",
        ErrorKind::PackageResolution => "package_resolution",
        ErrorKind::InvalidRequest => "invalid_request",
    }
}

fn report_to_sentry(code: &str, error: &ExecuteError) {
    let kind = error_kind_tag(error.kind);
    let extras = sentry_extras(
        code,
        error,
        TELEMETRY_INCLUDE_SOURCE.load(Ordering::Relaxed),
    );
    sentry::with_scope(
        |scope| {
            scope.set_tag("submilli.error_kind", kind);
            for (key, value) in extras {
                scope.set_extra(key, value);
            }
        },
        || {
            let summary = error.message.lines().next().unwrap_or_default();
            sentry::capture_message(&format!("[{kind}] {summary}"), sentry::Level::Error);
        },
    );
}

/// The extras a failure report carries. Both quote the program: the source
/// itself, and the error response, whose backtrace or diagnostics reproduce the
/// failing lines. So both wait for `include_source`; without it the report is
/// only the event message.
fn sentry_extras(
    code: &str,
    error: &ExecuteError,
    include_source: bool,
) -> Vec<(&'static str, serde_json::Value)> {
    if !include_source {
        return Vec::new();
    }
    let mut extras = vec![("code", serde_json::Value::from(code))];
    if let Ok(response) = serde_json::to_value(error) {
        extras.push(("response", response));
    }
    extras
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

    #[test]
    fn compile_metadata_failure_preserves_error_and_discovery_warnings() {
        let (sources, file) = Sources::single("test.ts", "é").unwrap();
        let diagnostic = Diagnostic {
            severity: Severity::Error,
            span: interpreter::Span {
                file,
                start: 1,
                end: 2,
            },
            message: "original compile error".into(),
            help: vec![],
            notes: vec![],
        };
        let outcome = compile_failure(
            &sources,
            file,
            &[diagnostic],
            vec!["discovery warning".into()],
        );
        let error = outcome.error.unwrap();
        assert!(matches!(error.kind, ErrorKind::RuntimeError));
        assert!(error.message.contains("original compile error"));
        assert!(error.message.contains("invalid diagnostic metadata"));
        assert_eq!(outcome.discovery_warnings, ["discovery warning"]);
        assert_eq!(
            diagnostic_position(&sources, interpreter::Span::at(file)).unwrap(),
            (1, 1)
        );
    }

    #[test]
    fn diagnostic_positions_use_each_file_and_no_position_for_compiler_errors() {
        let mut sources = Sources::new();
        sources.add("root.ts", "x").unwrap();
        let dependency = sources.add("dep.ts", "first\nsecond").unwrap();
        assert_eq!(
            diagnostic_position(&sources, interpreter::Span::new(dependency, 6, 12).unwrap())
                .unwrap(),
            (2, 1)
        );
        assert_eq!(
            diagnostic_position(&sources, interpreter::Span::at(FileId::COMPILER)).unwrap(),
            (0, 0)
        );
        assert!(diagnostic_position(&sources, interpreter::Span::at(FileId(999))).is_err());
    }

    #[test]
    fn fatal_host_failure_is_reported_as_a_runtime_error() {
        let (sources, file) = Sources::single("test.ts", "function main(): void {}").unwrap();
        let cause = interpreter::runtime::host::fatal_host_error("host ABI: invalid result buffer");
        let err = cause.context("executing host call");
        let payload = classify_runtime_error(&err, &sources, file);
        assert!(matches!(payload.kind, ErrorKind::RuntimeError));
        assert!(payload.message.contains("internal host error: host ABI"));
        assert!(payload.message.contains("invalid result buffer"));
        assert!(payload.diagnostics.is_empty());
    }

    /// Neither the program's source nor the rendered error, which quotes its
    /// lines, reaches a telemetry report without the explicit opt-in.
    #[test]
    fn source_is_attached_to_telemetry_only_on_opt_in() {
        let error = ExecuteError {
            kind: ErrorKind::RuntimeError,
            message: "boom".into(),
            diagnostics: Vec::new(),
        };
        let keys = |include: bool| -> Vec<&'static str> {
            sentry_extras("function main(): number { return 1; }", &error, include)
                .into_iter()
                .map(|(key, _)| key)
                .collect()
        };
        assert!(keys(false).is_empty());
        assert_eq!(keys(true), vec!["code", "response"]);
    }

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
            parse(code).unwrap(),
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
