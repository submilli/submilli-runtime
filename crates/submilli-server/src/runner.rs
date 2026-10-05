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
use interpreter::runtime::limits::ExecutionUsage;
use interpreter::runtime::{
    AuthProxy, DecisionLog, ExecutionTokenBudget, HttpClient, LinkedPackageModule, LlmProvider,
    McpTransport, RuntimeConfig, SecretProvider, SecurityCheck, SessionKvStore, StoreData, Vfs,
    VfsInfo, install_package_modules_async, install_runtime_store_bound, install_tenant_limits,
    is_memory_exhausted,
};
use interpreter::{
    BacktraceMode, Diagnostic, FileId, ParsedScript, ScriptImports, Sources,
    compile_parsed_script_timed, dispatch_main_async, failure_message, instantiate_program_async,
    parse_script,
};
use tracing::debug;
use wasmtime::{Engine, Linker, Module, Trap};

use crate::app::PreparedBlueprintPackages;
use crate::compiler_thread;
use crate::error::{DenialDetails, DiagnosticNote, DiagnosticPayload, ErrorKind, ExecuteError};
use crate::mcp::McpCatalog;
use crate::record::FinishedRun;

pub struct RunOutcome {
    pub usage: ExecutionUsage,
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
            let diagnostics = error.into_diagnostics(self.file);
            diagnostics::render_collection(&diagnostics, &self.sources).map_or_else(
                |failure| {
                    let primary = diagnostics
                        .first()
                        .map_or("compilation failed", |diag| diag.message.as_str());
                    interpreter::rendering::failure_text(primary, &failure)
                },
                |rendered| rendered.text,
            )
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
    pub(crate) audit: Option<Arc<crate::audit::ExecutionAudit>>,
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
    /// The run's recorder, finished from the owner task; `None` records nothing.
    pub(crate) recording: Option<crate::record::Recording>,
    /// Fires when someone other than the caller cancels the run.
    pub(crate) cancel: Option<tokio::sync::oneshot::Receiver<()>>,
}

pub(crate) struct RunnerImports<'a> {
    pub packages: &'a Arc<PreparedBlueprintPackages>,
    pub mcps: &'a Arc<McpCatalog>,
}

pub(crate) struct RunnerRuntime<'a> {
    pub blueprint: &'a str,
    pub session: &'a str,
    pub engine: &'a Engine,
    pub base_linker: &'a Linker<StoreData>,
    pub config: &'a RuntimeConfig,
}

/// Run `code` against a caller-provided VFS. Ownership of `vfs` lives outside:
/// `ephemeral` callers pass an owning `Vfs::tempdir()` (wiped when it drops here
/// at return); `per_session` / `named` callers pass a non-owning VFS so the
/// directory survives this call. Mounted volumes are never owned.
pub(crate) async fn run(
    code: &str,
    parsed: ParsedExecute,
    runtime: RunnerRuntime<'_>,
    vfs: Vfs,
    vfs_info: VfsInfo,
    services: HostServices,
    imports: RunnerImports<'_>,
) -> RunOutcome {
    let started = Instant::now();
    let blueprint = runtime.blueprint.to_owned();
    let session = runtime.session.to_owned();
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
        let audit = services.audit.clone();
        let budget = services.llm_budget.clone();
        let recording = services.recording.clone();
        let log = recording.as_ref().map(crate::record::Recording::log);
        let outcome = run_inner(
            &owned_code,
            parsed,
            RunnerRuntime {
                blueprint: &blueprint,
                session: &session,
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
            (cancelled, log.clone()),
        )
        .await;
        if let Some(audit) = audit {
            audit.result(&outcome, budget.as_ref().map_or(0, |b| b.used()));
            audit.finish(outcome.error.is_none());
        }
        if let (Some(recording), Some(log)) = (recording, log) {
            recording.finish(FinishedRun {
                dispatched: true,
                error: outcome.error.clone(),
                result: outcome.value.clone(),
                console: outcome.console_raw.clone(),
                usage: outcome.usage,
                log: log.finish(),
                wall: recording.started.elapsed(),
            });
        }
        log_execution(&blueprint, &session, started, &outcome);
        outcome
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
    mut services: HostServices,
    imports: RunnerImports<'_>,
    (mut cancelled, log): (tokio::sync::oneshot::Receiver<()>, Option<Arc<DecisionLog>>),
) -> RunOutcome {
    let external = services.cancel.take();
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
    // Outermost, so the host functions see the recorder; installed before packages run
    // their top-level statements, so those decisions are recorded too.
    if let Some(log) = &log {
        data.security_check = log.wrap(data.security_check.clone());
    }
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
    // single-watchdog deadline. Arm this store only when guest code begins
    // (the packages' top-level statements, then the program's).
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

        // Installing a package runs its top-level statements, and instantiation
        // runs the program's, so the deadline covers both and their failure is
        // reported as one in `main` is.
        crate::execution_timeout::arm(&mut store, runtime.config.timeout);
        let phase_start = Instant::now();
        let installed =
            install_package_modules_async(&mut linker, &mut store, &package_modules).await;
        rt.link_packages = phase_start.elapsed();

        let dispatch = 'program: {
            match installed {
                Ok(()) => {}
                Err(err) if raised_by_top_level_statements(&err) => break 'program Err(err),
                Err(err) => return internal_failure(&format!("install packages failed: {err}")),
            }

            let phase_start = Instant::now();
            let instantiated = instantiate_program_async(&linker, &mut store, &module).await;
            rt.instantiate = phase_start.elapsed();
            let instance = match instantiated {
                Ok(instance) => instance,
                Err(err) if raised_by_top_level_statements(&err) => break 'program Err(err),
                Err(err) => return internal_failure(&format!("instantiate_async failed: {err}")),
            };

            let phase_start = Instant::now();
            let returned = dispatch_main_async(&mut store, &instance).await;
            rt.execute = phase_start.elapsed();
            returned
        };
        crate::metrics::runtime_phases(&rt);
        log_phase_breakdown(&compiled.timings, &rt);
        let console_raw = captured_console(&buf);

        match dispatch {
            Ok(value) => RunOutcome {
                usage: ExecutionUsage::default(),
                value,
                console_raw,
                error: None,
                discovery_warnings,
            },
            Err(err) => RunOutcome {
                usage: ExecutionUsage::default(),
                value: None,
                error: Some(classify_runtime_error(&err, &parsed.sources, parsed.file)),
                console_raw,
                discovery_warnings,
            },
        }
    };
    // Only a sent cancel counts: the canceller is dropped, unsent, when the run ends.
    let cancelled_elsewhere = async {
        if let Some(external) = external
            && external.await.is_ok()
        {
            return;
        }
        std::future::pending::<()>().await;
    };
    let mut outcome = tokio::select! {
        biased;
        _ = &mut cancelled => cancelled_outcome(),
        () = cancelled_elsewhere => cancelled_outcome(),
        outcome = execution => outcome,
    };
    if let Err(error) = store.data_mut().blocking_work.finish().await {
        record_worker_cleanup_failure(&mut outcome, &error);
    }
    match ExecutionUsage::capture(&store, runtime.config.fuel) {
        Ok(usage) => outcome.usage = usage,
        Err(error) => return internal_failure(&format!("usage capture failed: {error}")),
    }
    outcome
}

fn cancelled_outcome() -> RunOutcome {
    let mut outcome = internal_failure("execution cancelled");
    if let Some(error) = &mut outcome.error {
        error.kind = ErrorKind::Cancelled;
    }
    outcome
}

fn record_worker_cleanup_failure(
    outcome: &mut RunOutcome,
    error: &interpreter::runtime::blocking::BlockingWorkDrainError,
) {
    let secondary = format!("worker cleanup failure: {error}");
    if let Some(primary) = outcome.error.as_mut() {
        primary.message.push('\n');
        primary.message.push_str(&secondary);
        return;
    }
    outcome.value = None;
    outcome.error = internal_failure(&secondary).error;
}

fn log_execution(blueprint: &str, session: &str, started: Instant, outcome: &RunOutcome) {
    let status = match outcome.error.as_ref().map(|error| error.kind) {
        None => "ok",
        Some(ErrorKind::FuelExhausted) => "fuel_exhausted",
        Some(ErrorKind::MemoryExhausted) => "memory_exhausted",
        Some(ErrorKind::Timeout) => "timeout",
        Some(_) => "error",
    };
    tracing::info!(
        target: "submilli_server::execute",
        blueprint, session,
        fuel = outcome.usage.fuel,
        wasm_fuel = outcome.usage.wasm_fuel,
        host_fuel = outcome.usage.host_fuel,
        memory_peak = outcome.usage.memory_peak,
        wall_ms = started.elapsed().as_millis(),
        outcome = status,
        "execution finished",
    );
}

fn compile_failure(
    sources: &Sources,
    _file: FileId,
    diags: &[Diagnostic],
    discovery_warnings: Vec<String>,
) -> RunOutcome {
    let rendered = diagnostics::render_collection_with_limits(
        diags,
        sources,
        interpreter::rendering::RenderLimits {
            bytes: 512 * 1024 - 64,
            ..Default::default()
        },
    );
    let payloads = diagnostic_payloads(sources, diags);
    let (mut message, diagnostics_out, omitted) = match (rendered, payloads) {
        (Ok(rendered), Ok((payloads, omitted))) => (rendered.text, payloads, omitted),
        (Err(error), _) | (_, Err(error)) => {
            let primary = diags.first().map_or("compilation failed", |diagnostic| {
                diagnostic.message.as_str()
            });
            let mut outcome =
                internal_failure(&interpreter::rendering::failure_text(primary, &error));
            outcome.discovery_warnings = discovery_warnings;
            return outcome;
        }
    };
    if omitted {
        message.push_str(interpreter::rendering::TRUNCATED);
    }
    RunOutcome {
        usage: ExecutionUsage::default(),
        value: None,
        console_raw: String::new(),
        error: Some(ExecuteError {
            kind: ErrorKind::CompileError,
            message,
            diagnostics: diagnostics_out,
            denial: None,
        }),
        discovery_warnings,
    }
}

fn diagnostic_payloads(
    sources: &Sources,
    diagnostics: &[Diagnostic],
) -> Result<(Vec<DiagnosticPayload>, bool), interpreter::rendering::RenderError> {
    use interpreter::rendering::{RenderError, RenderLimits, bounded_text};
    let mut remaining = 512 * 1024usize;
    let mut result = Vec::new();
    let mut omitted = false;
    for diagnostic in diagnostics.iter().take(RenderLimits::default().steps) {
        if remaining < 128 {
            omitted = true;
            break;
        }
        let (line, column) = diagnostic_position(sources, diagnostic.span)?;
        let message = bounded_text(
            &diagnostic.message,
            remaining.min(RenderLimits::default().bytes),
        )?;
        remaining = remaining
            .saturating_sub(message.text.len())
            .saturating_sub(64);
        omitted |= message.truncated;
        let mut notes = Vec::new();
        for (span, text) in diagnostic.notes.iter().take(RenderLimits::default().steps) {
            if remaining < 128 {
                omitted = true;
                break;
            }
            let (line, column) = diagnostic_position(sources, *span)?;
            let message = bounded_text(text, remaining.min(RenderLimits::default().bytes))?;
            remaining = remaining
                .saturating_sub(message.text.len())
                .saturating_sub(64);
            omitted |= message.truncated;
            notes.try_reserve(1).map_err(|_| RenderError::Allocation)?;
            notes.push(DiagnosticNote {
                line,
                column,
                message: message.text,
            });
        }
        omitted |= notes.len() != diagnostic.notes.len();
        result.try_reserve(1).map_err(|_| RenderError::Allocation)?;
        result.push(DiagnosticPayload {
            severity: match diagnostic.severity {
                Severity::Error => "error",
                Severity::Warning => "warning",
            },
            line,
            column,
            message: message.text,
            notes,
        });
    }
    omitted |= result.len() != diagnostics.len();
    Ok((result, omitted))
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
    let denial = err
        .downcast_ref::<interpreter::backtrace::ThrownError>()
        .and_then(|thrown| thrown.denial.as_ref());
    let kind = if denial.is_some() {
        ErrorKind::PermissionDenied
    } else {
        match err.downcast_ref::<Trap>() {
            Some(Trap::Interrupt) => ErrorKind::Timeout,
            Some(Trap::OutOfFuel) => ErrorKind::FuelExhausted,
            Some(Trap::StackOverflow) => ErrorKind::StackExhausted,
            _ if is_memory_exhausted(err) => ErrorKind::MemoryExhausted,
            _ => ErrorKind::RuntimeError,
        }
    };
    // drops middle host frames; full trace available from the CLI
    let message =
        match interpreter::backtrace::render_checked(err, sources, file, BacktraceMode::LlmTrimmed)
        {
            Ok(Some(rendered)) => rendered.text,
            Ok(None) => unframed_message(err),
            Err(failure) => interpreter::rendering::failure_text(&unframed_message(err), &failure),
        };
    ExecuteError {
        kind,
        message,
        diagnostics: Vec::new(),
        denial: denial.map(|denial| DenialDetails {
            caller: denial.caller.clone(),
            capability: denial.capability.clone(),
            source: denial.source.as_str(),
        }),
    }
}

/// Whether an instantiation error was raised once top-level statements (the
/// program's or a package's) were running, as opposed to a module failing to
/// link. These are the shapes `instantiate_program_async` and
/// `install_package_modules_async` yield from a start function: a trap, a shaped
/// throw, a fatal host error, and memory exhaustion.
fn raised_by_top_level_statements(err: &wasmtime::Error) -> bool {
    err.is::<Trap>()
        || err.is::<interpreter::backtrace::ThrownError>()
        || err.is::<interpreter::runtime::host::FatalHostError>()
        || is_memory_exhausted(err)
}

/// A trap or an uncaught throw with no frame to render gets only the header a
/// rendered failure has. Any other failure keeps its whole cause chain, which is
/// what reaches the caller for a host or setup failure.
fn unframed_message(err: &wasmtime::Error) -> String {
    if err.is::<Trap>() || err.is::<interpreter::backtrace::ThrownError>() {
        return format!("error: {}", failure_message(err));
    }
    interpreter::backtrace::failure_chain_checked(err).map_or_else(
        |failure| interpreter::rendering::failure_text("runtime execution failed", &failure),
        |rendered| rendered.text,
    )
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

/// Registers every package module so its frames render with their source.
fn register_package_sources(
    sources: &mut Sources,
    packages: &PreparedBlueprintPackages,
) -> Result<(), interpreter::source::SourceError> {
    for package in &packages.modules {
        let name = &package.declaration.package_name;
        for source in &package.sources {
            sources.add_package_module(name, source.path.as_str(), &source.text)?;
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
        ErrorKind::StackExhausted => "stack_exhausted",
        ErrorKind::Cancelled => "cancelled",
        ErrorKind::PermissionDenied => "permission_denied",
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
        usage: ExecutionUsage::default(),
        value: None,
        console_raw: String::new(),
        error: Some(ExecuteError {
            kind: ErrorKind::RuntimeError,
            message: format!("internal: {msg}"),
            diagnostics: Vec::new(),
            denial: None,
        }),
        discovery_warnings: Vec::new(),
    }
}

// Poison means a panic may have interrupted a console write. AGENTS.md permits
// poisoned-lock panics instead of treating potentially partial output as intact;
// it does not permit the panic that caused poisoning. This also applies on readback.
fn captured_console(buf: &Mutex<Vec<u8>>) -> String {
    String::from_utf8_lossy(&buf.lock().expect("console buffer lock poisoned")).into_owned()
}

// A panic during writing may leave partial console output. AGENTS.md permits
// panicking on poisoned access instead of recovering it; the panic that caused
// poisoning is still subject to the no-panic policy.
struct Sink(Arc<Mutex<Vec<u8>>>);

const MAX_CONSOLE_BYTES: usize = 1024 * 1024;

impl Write for Sink {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        let mut output = self.0.lock().expect("console buffer lock poisoned");
        if buf.len() > MAX_CONSOLE_BYTES.saturating_sub(output.len()) {
            return Err(std::io::Error::new(
                std::io::ErrorKind::FileTooLarge,
                "console output exceeds 1 MiB; print less output",
            ));
        }
        let needed = output.len() + buf.len();
        if needed > output.capacity() {
            let capacity = needed
                .max(output.capacity().saturating_mul(2))
                .min(MAX_CONSOLE_BYTES);
            let additional = capacity - output.len();
            output
                .try_reserve_exact(additional)
                .map_err(|error| std::io::Error::new(std::io::ErrorKind::OutOfMemory, error))?;
        }
        output.extend_from_slice(buf);
        Ok(buf.len())
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
    fn console_sink_caps_retained_output_without_losing_prior_bytes() {
        let buffer = Arc::new(Mutex::new(Vec::new()));
        let mut sink = Sink(buffer.clone());
        let chunk = vec![b'x'; MAX_CONSOLE_BYTES / 2];
        sink.write_all(&chunk).unwrap();
        sink.write_all(&chunk).unwrap();
        let error = sink.write_all(b"extra").unwrap_err();
        assert_eq!(error.kind(), std::io::ErrorKind::FileTooLarge);
        let output = buffer.lock().unwrap();
        assert_eq!(output.len(), MAX_CONSOLE_BYTES);
        assert!(output.capacity() <= MAX_CONSOLE_BYTES);
        assert!(output.iter().all(|byte| *byte == b'x'));
    }

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
            denial: None,
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
        panic_after_resume: bool,
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
                if self.panic_after_resume {
                    panic!("injected cancelled worker failure");
                }
            }
            CheckOutcome::Allow { rule: None }
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
            Box<dyn std::future::Future<Output = interpreter::runtime::McpOutcome> + Send + 'a>,
        > {
            Box::pin(async { panic!("test must not call MCP") })
        }
    }

    #[tokio::test]
    async fn cancelled_request_waits_for_git_before_releasing_store_and_vfs() {
        cancelled_git_owner_retains_resources(false).await;
    }

    #[tokio::test]
    async fn cancelled_request_drains_panicking_git_worker() {
        cancelled_git_owner_retains_resources(true).await;
    }

    async fn cancelled_git_owner_retains_resources(panic_after_resume: bool) {
        let vfs = Vfs::tempdir().unwrap();
        let root = vfs.root().to_owned();
        let defaults = StoreData::with_vfs(Vfs::none());
        let (finish, cleanup) = std::sync::mpsc::channel();
        let security = Arc::new(PausedGit {
            panic_after_resume,
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
            audit: None,
            security_check: security.clone(),
            http_client: defaults.http_client,
            mcp_transport: Arc::new(UnusedMcp),
            session_kv: Arc::new(InMemorySessionKv::default()),
            llm_provider: None,
            llm_budget: None,
            recording: None,
            cancel: None,
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
                blueprint: "test",
                session: "test-session",
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

#[cfg(test)]
mod worker_cleanup_tests {
    use super::*;

    async fn injected_drain_failure() -> interpreter::runtime::blocking::BlockingWorkDrainError {
        let mut workers = interpreter::runtime::blocking::BlockingWork::default();
        let (started, ready) = tokio::sync::oneshot::channel();
        let (release, gate) = std::sync::mpsc::channel();
        let mut waiter = Box::pin(workers.spawn(move || {
            started.send(()).unwrap();
            gate.recv_timeout(std::time::Duration::from_secs(5))
                .unwrap();
            panic!("injected abandoned worker failure");
        }));
        tokio::select! {
            _ = &mut waiter => panic!("worker ended early"),
            result = ready => result.unwrap(),
        }
        drop(waiter);
        release.send(()).unwrap();
        workers.finish().await.unwrap_err()
    }

    #[tokio::test]
    async fn cleanup_failure_preserves_primary_error_and_captured_output() {
        let cleanup = injected_drain_failure().await;
        for message in ["execution failed", "execution cancelled"] {
            let mut outcome = internal_failure(message);
            outcome.error.as_mut().unwrap().kind = ErrorKind::Timeout;
            outcome.console_raw = "before\n".into();
            outcome.discovery_warnings = vec!["discovery warning".into()];
            let primary = outcome.error.as_ref().unwrap().message.clone();
            record_worker_cleanup_failure(&mut outcome, &cleanup);
            let error = outcome.error.unwrap();
            assert!(matches!(error.kind, ErrorKind::Timeout));
            assert_eq!(
                error.message,
                format!("{primary}\nworker cleanup failure: {cleanup}")
            );
            assert_eq!(outcome.console_raw, "before\n");
            assert_eq!(outcome.discovery_warnings, ["discovery warning"]);
        }
    }

    #[tokio::test]
    async fn cleanup_failure_suppresses_success_and_preserves_reporting_context() {
        let cleanup = injected_drain_failure().await;
        let mut outcome = RunOutcome {
            usage: ExecutionUsage::default(),
            value: Some("42".into()),
            console_raw: "before\n".into(),
            error: None,
            discovery_warnings: vec!["discovery warning".into()],
        };
        record_worker_cleanup_failure(&mut outcome, &cleanup);
        assert!(outcome.value.is_none());
        let error = outcome.error.unwrap();
        assert!(matches!(error.kind, ErrorKind::RuntimeError));
        assert!(error.message.contains("blocking worker panicked"));
        assert_eq!(outcome.console_raw, "before\n");
        assert_eq!(outcome.discovery_warnings, ["discovery warning"]);
    }
}
