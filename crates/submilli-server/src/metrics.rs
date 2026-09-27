//! Sentry counters for server operations, kept in one place so the metric
//! taxonomy is discoverable. Every function is a no-op unless a Sentry client is
//! bound and built with the `metrics` feature — safe to call from tests.

use std::time::Duration;

use interpreter::PhaseTimings;
use interpreter::runtime::{HttpMetric, MetricsSink};
use sentry::metrics::{counter, distribution};
use sentry::protocol::Unit;

/// A new session was registered (explicit `POST /v1/sessions` or first execute
/// under a fresh id). `vfs_mode` is the bounded sandbox kind.
pub fn session_init(vfs_mode: &'static str) {
    counter("submilli.server.session.init", 1)
        .attribute("vfs_mode", vfs_mode)
        .capture();
}

/// One script execution finished. `outcome` is the bounded result class
/// (`success`, `compile_error`, `timeout`, `fuel_exhausted`, `runtime_error`).
pub fn execution(outcome: &'static str) {
    counter("submilli.server.execution", 1)
        .attribute("outcome", outcome)
        .capture();
}

/// A package-docs lookup (`GET /v1/packages/docs`). `found` is false for an
/// unknown name.
pub fn docs(found: bool) {
    counter("submilli.server.docs", 1)
        .attribute("found", found)
        .capture();
}

/// A package search (`GET /v1/packages/search`).
pub fn search() {
    counter("submilli.server.search", 1).capture();
}

/// A built-in discovery call. `kind` is `list` (`GET /v1/builtins`) or `docs`
/// (`GET /v1/builtins/docs`).
pub fn builtins(kind: &'static str) {
    counter("submilli.server.builtins", 1)
        .attribute("kind", kind)
        .capture();
}

/// A capability-catalog read (`GET /v1/capabilities`).
pub fn capabilities() {
    counter("submilli.server.capabilities", 1).capture();
}

/// A blueprint create-or-replace (`PUT /v1/blueprints/{name}`) succeeded.
/// `created` distinguishes a new registration from a replacement.
pub fn blueprint_apply(created: bool) {
    counter("submilli.server.blueprint.apply", 1)
        .attribute("created", created)
        .capture();
}

/// The server bound its listener and began serving.
pub fn server_start() {
    counter("submilli.server.start", 1).capture();
}

/// Per-phase compile latency for one successful `compile_script`. Emitted as a
/// distribution per phase so each (`lex`, `parse`, `typecheck`, `capture`,
/// `desugar`, `codegen`) is independently filterable by the `phase` attribute —
/// the shape of LLM-generated programs flowing through `/v1/execute`.
pub fn compile_phases(timings: &PhaseTimings) {
    let phases = [
        ("lex", timings.lex),
        ("parse", timings.parse),
        ("typecheck", timings.typecheck),
        ("capture", timings.capture),
        ("desugar", timings.desugar),
        ("codegen", timings.codegen),
    ];
    for (phase, duration) in phases {
        distribution(
            "submilli.server.compile.phase_ms",
            duration.as_secs_f64() * 1000.0,
        )
        .unit(Unit::Millisecond)
        .attribute("phase", phase)
        .capture();
    }
}

/// Per-phase latency for the wasmtime side of one execution — the part
/// [`compile_phases`] doesn't cover. Populated by the runner from `store_init`
/// through `main` dispatch.
#[derive(Default)]
pub struct RuntimePhaseTimings {
    /// `Store` creation (`RuntimeConfig::store_async`).
    pub store_init: Duration,
    /// Cranelift compile of the user script (`Module::new`). Never AOT-cached —
    /// the script is LLM-generated and changes every request, so this is a
    /// fresh compile on the hot path.
    pub module_compile: Duration,
    /// Instantiating the prelude + stdlib shims into the fresh store
    /// (`install_runtime_store_bound`). Per-request because instances are
    /// store-tied; the modules themselves are AOT-cached.
    pub link_runtime: Duration,
    /// Instantiating blueprint package modules (`install_package_modules_async`).
    pub link_packages: Duration,
    /// Instantiating the user module (`linker.instantiate_async`).
    pub instantiate: Duration,
    /// Running `main` (`dispatch_main_async`).
    pub execute: Duration,
}

/// Per-phase runtime latency for one execution. Emitted as a distribution per
/// phase — same shape as [`compile_phases`] — so each (`store_init`,
/// `module_compile`, `link_runtime`, `link_packages`, `instantiate`, `execute`)
/// is independently filterable by the `phase` attribute. Together with
/// `submilli.server.compile.phase_ms` this makes the whole source-to-output
/// pass observable.
pub fn runtime_phases(timings: &RuntimePhaseTimings) {
    let phases = [
        ("store_init", timings.store_init),
        ("module_compile", timings.module_compile),
        ("link_runtime", timings.link_runtime),
        ("link_packages", timings.link_packages),
        ("instantiate", timings.instantiate),
        ("execute", timings.execute),
    ];
    for (phase, duration) in phases {
        distribution(
            "submilli.server.runtime.phase_ms",
            duration.as_secs_f64() * 1000.0,
        )
        .unit(Unit::Millisecond)
        .attribute("phase", phase)
        .capture();
    }
}

/// One HTTP host operation finished: transport latency (tagged by `capability`
/// and `outcome`) and response size. The destination host is deliberately not
/// an attribute: which hosts a program talks to is the operator's business.
/// Backs [`SentryMetricsSink`].
pub fn http_operation(metric: &HttpMetric) {
    distribution(
        "submilli.server.http.duration_ms",
        metric.duration_ms as f64,
    )
    .unit(Unit::Millisecond)
    .attribute("capability", metric.capability.clone())
    .attribute("outcome", metric.outcome)
    .capture();
    distribution("submilli.server.http.response_bytes", metric.bytes as f64)
        .unit(Unit::Byte)
        .attribute("capability", metric.capability.clone())
        .attribute("outcome", metric.outcome)
        .capture();
}

/// Forwards interpreter runtime metrics to Sentry. Installed on every
/// per-request `StoreData` so HTTP host operations are observable in production.
pub struct SentryMetricsSink;

impl MetricsSink for SentryMetricsSink {
    fn http_operation(&self, metric: HttpMetric) {
        http_operation(&metric);
    }
}
