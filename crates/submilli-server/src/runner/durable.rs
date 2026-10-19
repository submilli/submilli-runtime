//! Internal recorded execution. Transport enablement and restart scheduling are
//! separate from this path; the caller retains mounts and exclusively owns the run.
use super::{HostServices, RunOutcome, RunnerImports, RunnerRuntime};
use crate::application::sessions::ports::StagedLastRun;
use crate::application::sessions::runs::{ResolveRun, RunFailure, RunResolution};
use crate::application::unit_of_work::UnitOfWorkFactory;
use crate::domain::run::Run;
use crate::domain::run_program::{EmbeddingLimits, ModelLimits, RunLimits, RunProgram};
use interpreter::runtime::effects::EffectHistory;
use interpreter::runtime::{RuntimeConfig, Vfs, VfsInfo};
use std::sync::Arc;
use submilli_durable::history::{ExecutionAttempt, History};

/// Bump this contract when compiler, host ABI, codecs or deterministic semantics
/// change incompatibly. Package content retention is a separate prerequisite for
/// replay across package changes; this version assumes unchanged package contents.
pub(crate) const COMPATIBILITY: &str = concat!("durable-v2/wasmgc/", env!("CARGO_PKG_VERSION"));

pub(crate) fn capture_program(
    source: String,
    blueprint: submilli_blueprint::Blueprint,
    services: &HostServices,
    config: &RuntimeConfig,
) -> RunProgram {
    RunProgram {
        source,
        blueprint,
        variables: (*services.variables).clone(),
        limits: limits(config, services),
        compatibility: COMPATIBILITY.into(),
    }
}

/// Return a fresh replay cursor for this claimed attempt. An old attempt cannot
/// append or publish after `reopen_run` increments the persistent attempt number.
pub(crate) fn history(run: &Run, units: submilli_durable::UnitFactory) -> Arc<History> {
    Arc::new(History::new(
        ExecutionAttempt {
            run_id: run.reservation_id().into(),
            owner_generation: run.owner_generation().into(),
            attempt: run.attempt(),
        },
        units,
    ))
}

/// Host services must be constructed from the retained blueprint and current
/// credentials. Completed calls use history; live calls still check authority.
/// Callers must drain the previous attempt before opening its mounts here.
#[allow(clippy::too_many_arguments)] // Same explicit runtime/import/mount inputs as the ordinary runner.
pub(crate) async fn run(
    run: &Run,
    history: Arc<History>,
    units: submilli_durable::UnitFactory,
    runtime: RunnerRuntime<'_>,
    vfs: Vfs,
    vfs_info: VfsInfo,
    mut services: HostServices,
    imports: RunnerImports<'_>,
) -> Result<RunOutcome, String> {
    let program = run.program().ok_or("run has no retained program")?;
    if program.compatibility != COMPATIBILITY || program.limits != limits(runtime.config, &services)
    {
        return Err("recorded runtime or limits do not match this execution environment".into());
    }
    if runtime.session != run.session_id() || runtime.blueprint != program.blueprint.name {
        return Err("recorded execution identity does not match the requested environment".into());
    }
    let filesystem =
        submilli_durable::DurableFilesystem::open(vfs.clone(), &run.reservation_id().into(), units)
            .await
            .map_err(|error| error.to_string())?;
    services.effects = Some(history.clone());
    services.filesystem = Some(Arc::new(filesystem));
    services.variables = Arc::new(program.variables.clone());
    let parsed = super::parse(&program.source).map_err(|error| error.to_string())?;
    let outcome = super::run(
        &program.source,
        parsed,
        runtime,
        vfs,
        vfs_info,
        services,
        imports,
    )
    .await;
    // A fatal interruption may leave a missing outcome. It must remain resumable,
    // not be turned into a definitive completion by a transport error formatter.
    if !outcome.durable_completion {
        return Err("recorded execution was interrupted; retain its history and resources".into());
    }
    history.finish().await.map_err(|error| error.to_string())?;
    Ok(outcome)
}

/// Publish only after the history prefix is consumed. The existing unit of work
/// commits the exact response and session last-result reference together.
pub(crate) async fn publish(
    units: Arc<dyn UnitOfWorkFactory>,
    run: &Run,
    history: &History,
    resolution: &RunResolution,
    last_run: Option<&StagedLastRun>,
) -> Result<Option<String>, RunFailure> {
    history.finish().await.map_err(|error| {
        RunFailure::Store(crate::application::error::StoreError::Io(error.to_string()))
    })?;
    ResolveRun::new(units)
        .execute_recorded(run, resolution, last_run, history.position().await)
        .await
}

fn limits(config: &RuntimeConfig, services: &HostServices) -> RunLimits {
    RunLimits {
        fuel: config.fuel,
        max_wasm_stack: config.max_wasm_stack,
        memory_reservation: config.memory_reservation,
        memory_guard_size: config.memory_guard_size,
        memory_reservation_for_growth: config.memory_reservation_for_growth,
        max_store_bytes: config.max_store_bytes,
        timeout: config.timeout,
        async_yield_fuel: config.async_yield_fuel,
        llm: services.llm_budget.as_ref().map(|budget| {
            let limits = budget.limits();
            ModelLimits {
                per_execution_tokens: limits.per_execution_tokens,
                max_held_tokens: limits.max_held_tokens,
                default_output_cap: limits.default_output_cap,
                max_prompt_count: limits.max_prompt_count,
                max_prompt_bytes: limits.max_prompt_bytes,
            }
        }),
        embedding: services.embedding_budget.as_ref().map(|budget| {
            let limits = budget.limits();
            EmbeddingLimits {
                per_execution_tokens: limits.per_execution_tokens,
                max_held_tokens: limits.max_held_tokens,
                max_requests: limits.max_requests,
                max_texts_per_call: limits.max_texts_per_call,
                max_bytes_per_call: limits.max_bytes_per_call,
            }
        }),
    }
}
