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
    AuthProxy, HttpClient, LinkedPackageModule, McpTransport, RuntimeConfig, SecretProvider,
    SecurityCheck, SessionKvStore, StoreData, Vfs, VfsInfo, install_package_modules_async,
    install_runtime_store_bound, install_tenant_limits,
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
    pub auth_proxy: Arc<dyn AuthProxy>,
    pub secret_provider: Arc<dyn SecretProvider>,
    pub security_check: Arc<dyn SecurityCheck>,
    pub http_client: Arc<dyn HttpClient>,
    /// Outbound `@mcp/<server>` dispatch — the JSON-RPC `tools/call` transport.
    pub mcp_transport: Arc<dyn McpTransport>,
    /// `submilli:session` storage for the session this run belongs to.
    pub session_kv: Arc<dyn SessionKvStore>,
}

pub(crate) struct RunnerImports<'a> {
    pub packages: &'a PreparedBlueprintPackages,
    pub mcps: &'a McpCatalog,
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
    let outcome = run_inner(code, parsed, runtime, vfs, vfs_info, services, imports).await;
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
    vfs: Vfs,
    vfs_info: VfsInfo,
    services: HostServices,
    imports: RunnerImports<'_>,
) -> RunOutcome {
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
    data.vfs_info = vfs_info;
    data.auth_proxy = services.auth_proxy;
    data.secret_provider = services.secret_provider;
    data.security_check = services.security_check;
    data.http_client = services.http_client;
    data.mcp_transport = Some(services.mcp_transport);
    data.session_kv = Some(services.session_kv);
    data.metrics = Arc::new(crate::metrics::SentryMetricsSink);
    data.console = Box::new(Sink(buf.clone()));
    data.install_type_info(compiled.type_info.clone());

    let mut rt = crate::metrics::RuntimePhaseTimings::default();

    let phase_start = Instant::now();
    let mut store = match runtime.config.store_async(runtime.engine, data) {
        Ok(s) => s,
        Err(err) => return internal_failure(&format!("store init failed: {err}")),
    };
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
    if let Err(err) = install_package_modules_async(&mut linker, &mut store, &package_modules).await
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
