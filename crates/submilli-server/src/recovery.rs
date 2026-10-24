//! The single-node execution owner. SQLite grants attempts; wakeups only reduce
//! polling latency. Dropping a transport waiter never drops accepted work.
use crate::app::AppState;
use crate::application::sessions::recovery::{self, RunQuery, RunStatus, StuckReason};
use crate::application::sessions::runs::RunResolution;
use crate::domain::run::{Run, RunIdentity};
use crate::handlers::execute::{self, ExecuteInputs, ExecuteResponse};
use crate::runner;
use interpreter::runtime::durable::RunId;
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, SystemTime};
use tokio::sync::{Notify, oneshot};
use tokio::task::{Id, JoinSet};
use tokio_util::sync::CancellationToken;

/// Prepare the complete replay environment before atomically accepting the run.
pub(crate) async fn admit(
    state: &AppState,
    mut inputs: ExecuteInputs<'_>,
) -> Result<RunId, String> {
    if !state.durable_execution() {
        return Err("durable execution is disabled".into());
    }
    if inputs.tools.is_some() {
        return Err("durable application-tool waits are not supported yet".into());
    }
    let vfs = inputs
        .blueprint
        .vfs
        .resolve(&inputs.variables)
        .map_err(|e| e.to_string())?;
    if matches!(vfs, submilli_blueprint::VfsConfig::Ephemeral { .. }) {
        return Err(
            "durable runs require per_session, named, or disabled filesystem storage".into(),
        );
    }
    let audit = inputs.audit.clone().unwrap_or_else(|| {
        crate::audit::ExecutionAudit::new(
            state.audit().clone(),
            "unauthenticated",
            "durable",
            Some(inputs.session_id),
        )
    });
    let mut run = Run::prepare(
        inputs.session_id.into(),
        inputs.idempotency_key.map(str::to_owned),
        format!("{:x}", Sha256::digest(inputs.code.as_bytes())),
        audit.id.clone(),
        state.idempotency().generation().into(),
        RunIdentity {
            execution_id: Some(audit.id.clone()),
            principal: Some(audit.principal.clone()),
            entry: "durable".into(),
            admission_order: 0,
        },
        SystemTime::now(),
    )
    .map_err(|e| e.to_string())?;
    // A retry should observe its original run even if discovery or credentials
    // are now unavailable. Admission checks the key again in its transaction.
    if let Some(id) = recovery::existing(state.units().as_ref(), &run)
        .await
        .map_err(|e| e.to_string())?
    {
        audit.replay();
        audit.finish(true);
        return Ok(id);
    }
    let (vfs, vfs_info) = state
        .session_manager()
        .reopen_vfs(inputs.session_id, &inputs.blueprint, &inputs.variables)
        .await
        .map_err(|e| e.to_string())?;
    if vfs.dir().is_some()
        && vfs.access() == interpreter::runtime::vfs::Access::ReadOnly
        && vfs
            .mounts()
            .iter()
            .all(|mount| mount.access() == interpreter::runtime::vfs::Access::ReadOnly)
    {
        return Err("durable filesystem handles require at least one writable root or mount for private recovery files".into());
    }
    inputs.vfs_source = execute::VfsSource::Opened { vfs, vfs_info };
    let source = inputs.code.to_owned();
    let blueprint = (*inputs.blueprint).clone();
    let prepared = execute::prepare_execution(state, inputs, None, None)
        .await
        .map_err(|outcome| {
            outcome.response.error.map_or_else(
                || "execution preparation failed".into(),
                |error| error.message,
            )
        })?;
    let mut program =
        runner::durable::capture_program(source, blueprint, &prepared.services, state.runtime());
    program.mcp_catalog = prepared.mcp_catalog;
    program.filesystem_config_fingerprint = if prepared.vfs.dir().is_some() {
        Some(
            submilli_durable::filesystem_config_fingerprint(&prepared.vfs)
                .map_err(|e| e.to_string())?,
        )
    } else {
        None
    };
    run.enable_durability().map_err(|e| e.to_string())?;
    let id = recovery::admit(state.units().as_ref(), run, program)
        .await
        .map_err(|e| e.to_string())?;
    audit.accepted();
    state.wake_recovery();
    Ok(id)
}

struct ActiveRun {
    run: Run,
    cancel: oneshot::Sender<()>,
}

pub(crate) async fn supervise(
    state: impl Fn() -> Option<AppState>,
    wakeup: Arc<Notify>,
    capacity: usize,
    draining: CancellationToken,
) {
    let mut tasks = JoinSet::new();
    let mut active: HashMap<Id, ActiveRun> = HashMap::new();
    let mut after = None;
    loop {
        if draining.is_cancelled() {
            break;
        }
        let Some(app) = state() else {
            break;
        };
        if active.len() < capacity {
            match schedule(&app, &mut tasks, &mut active, after.clone(), capacity).await {
                Ok(next) => after = next,
                Err(error) => tracing::warn!(%error, "run discovery unavailable"),
            }
        }
        drop(app);
        tokio::select! {
            biased;
            () = draining.cancelled() => break,
            finished = tasks.join_next_with_id(), if !tasks.is_empty() => {
                let (id, result) = match finished {
                    Some(Ok(result)) => result,
                    Some(Err(error)) => (error.id(), Err(StuckReason::Interrupted)),
                    None => continue,
                };
                if let Some(active_run) = active.remove(&id)
                    && let Err(reason) = result
                    && let Some(app) = state()
                {
                    loop {
                        match recovery::stop_attempt(app.units().as_ref(), &active_run.run, reason).await {
                            Ok(()) => break,
                            Err(error) => tracing::error!(%error, run = %active_run.run.reservation_id(), "retrying stuck-state commit"),
                        }
                        tokio::select! {
                            () = draining.cancelled() => break,
                            () = tokio::time::sleep(Duration::from_millis(250)) => {}
                        }
                    }
                }
                after = None;
            }
            () = wakeup.notified() => { after = None; }
            () = tokio::time::sleep(Duration::from_millis(250)) => {}
        }
    }
    // Cancel the guest, then let the runner drain owned native work. Keep the
    // persisted attempt running so the next process can recover it.
    for (_, active_run) in active {
        let _ = active_run.cancel.send(());
    }
    while tasks.join_next().await.is_some() {}
}

async fn schedule(
    state: &AppState,
    tasks: &mut JoinSet<Result<(), StuckReason>>,
    active: &mut HashMap<Id, ActiveRun>,
    after: Option<RunId>,
    capacity: usize,
) -> Result<Option<RunId>, crate::application::error::StoreError> {
    let page = candidates(state, after).await?;
    let next = if page.len() == 100 {
        page.last().map(|run| run.reservation_id().clone())
    } else {
        None
    };
    for candidate in page {
        if active.len() == capacity {
            return Ok(None);
        }
        if active
            .values()
            .any(|running| running.run.session_id() == candidate.session_id())
        {
            continue;
        }
        if matches!(
            candidate.status(),
            RunStatus::Completed | RunStatus::Cancelled | RunStatus::Aborted | RunStatus::Expired
        ) {
            if let Err(error) = retire(
                state.session_manager(),
                state.units().as_ref(),
                candidate.reservation_id(),
            )
            .await
            {
                tracing::warn!(run = %candidate.reservation_id(), %error, "run resources retained; cleanup unavailable");
            }
            continue;
        }
        if !matches!(candidate.status(), RunStatus::Queued | RunStatus::Running) {
            continue;
        }
        if !state.durable_execution() {
            recovery::mark_stuck(
                state.units().as_ref(),
                candidate.reservation_id(),
                candidate.revision(),
                StuckReason::Disabled,
            )
            .await?;
            continue;
        }
        // A committed claim whose acknowledgement/reconciliation both failed
        // has no live task. Never leave it running forever or launch it twice.
        if candidate.status() == RunStatus::Running
            && candidate.owner_generation() == state.idempotency().generation()
        {
            recovery::mark_stuck(
                state.units().as_ref(),
                candidate.reservation_id(),
                candidate.revision(),
                StuckReason::Interrupted,
            )
            .await?;
            continue;
        }
        let Some(run) = claim_candidate(state, &candidate).await? else {
            continue;
        };
        #[cfg(test)]
        tests::process::checkpoint("claimed");
        let (cancel, cancelled) = oneshot::channel();
        let executing = run.clone();
        let worker = state.clone();
        let handle = tasks.spawn(async move { run_attempt(&worker, &executing, cancelled).await });
        active.insert(handle.id(), ActiveRun { run, cancel });
    }
    Ok(next)
}

async fn claim_candidate(
    state: &AppState,
    candidate: &Run,
) -> Result<Option<Run>, crate::application::error::StoreError> {
    match recovery::claim(
        state.units().as_ref(),
        candidate.reservation_id(),
        state.idempotency().generation(),
    )
    .await
    {
        Ok(run) => Ok(run),
        Err(error) => {
            tracing::warn!(run = %candidate.reservation_id(), %error, "cannot recover run");
            // No task was launched. Read again: the claim might have committed
            // even if both its acknowledgement and reconciliation read failed.
            let mut unit = state.units().begin().await?;
            if let Some(mut current) = unit.get_run(candidate.reservation_id()).await?
                && matches!(current.status(), RunStatus::Queued | RunStatus::Running)
            {
                current
                    .mark_stuck(StuckReason::History)
                    .map_err(|_| crate::application::error::StoreError::Conflict)?;
                unit.save_run(current).await?;
                unit.commit().await?;
            }
            Ok(None)
        }
    }
}

async fn candidates(
    state: &AppState,
    after: Option<RunId>,
) -> Result<Vec<Run>, crate::application::error::StoreError> {
    let mut unit = state.units().begin().await?;
    unit.list_runs(RunQuery {
        after,
        limit: 100,
        recovery: true,
        ..Default::default()
    })
    .await
}

async fn run_attempt(
    state: &AppState,
    run: &Run,
    cancelled: oneshot::Receiver<()>,
) -> Result<(), StuckReason> {
    let program = {
        let mut unit = state
            .units()
            .begin()
            .await
            .map_err(|_| StuckReason::History)?;
        unit.get_run_program(run.reservation_id())
            .await
            .map_err(|_| StuckReason::History)?
            .ok_or(StuckReason::History)?
    };
    let saved = &program.limits;
    let current = state.runtime();
    if program.compatibility != runner::durable::COMPATIBILITY
        || saved.max_wasm_stack != current.max_wasm_stack
        || saved.memory_reservation != current.memory_reservation
        || saved.memory_guard_size != current.memory_guard_size
        || saved.memory_reservation_for_growth != current.memory_reservation_for_growth
    {
        return Err(StuckReason::Compatibility);
    }
    let (_, credentials) = state
        .session_manager()
        .execution_bindings(run.session_id())
        .await
        .map_err(|_| StuckReason::Credentials)?;
    let harness_secrets = credentials.unwrap_or_default();
    submilli_blueprint::resolve_harness_secrets(&program.blueprint.secrets, &harness_secrets)
        .map_err(|_| StuckReason::Credentials)?;
    let audit = crate::audit::ExecutionAudit::for_run(
        state.audit().clone(),
        run.reservation_id().clone(),
        run.identity()
            .principal
            .as_deref()
            .unwrap_or("unauthenticated"),
        "recovery",
        Some(run.session_id()),
    );
    audit.annotate(
        &program.source,
        &program.blueprint.name,
        Some(&program.blueprint),
        &program.variables,
    );
    audit.begin();
    let (vfs, vfs_info) = state
        .session_manager()
        .reopen_vfs(run.session_id(), &program.blueprint, &program.variables)
        .await
        .map_err(|_| StuckReason::Storage)?;
    let inputs = ExecuteInputs {
        session_id: run.session_id(),
        code: &program.source,
        blueprint_name: &program.blueprint.name,
        blueprint: Arc::new(program.blueprint.clone()),
        variables: Arc::new(program.variables.clone()),
        harness_secrets,
        audit: Some(audit.clone()),
        vfs_source: execute::VfsSource::Opened { vfs, vfs_info },
        run_entry: "session",
        idempotency_key: run.optional_key(),
        tools: None,
    };
    let mut prepared = execute::prepare_execution(state, inputs, Some(cancelled), Some(program.mcp_catalog.clone())).await.map_err(|outcome| {
        tracing::warn!(run = %run.reservation_id(), error = ?outcome.response.error, "recovery preparation failed");
        StuckReason::Storage
    })?;
    let config = runner::durable::restore_limits(
        state.runtime(),
        &program.limits,
        state.session_manager(),
        &mut prepared.services,
    );
    prepared.services.session_manager = None;
    let units = crate::adapters::unit_of_work::durable::units(
        state.database().ok_or(StuckReason::Storage)?,
        state.secret_cipher().ok_or(StuckReason::Credentials)?,
    );
    let history = runner::durable::history(run, units.clone());
    let outcome = runner::durable::run(
        run,
        &program,
        history.clone(),
        units,
        runner::RunnerRuntime {
            run_id: run.reservation_id(),
            blueprint: &program.blueprint.name,
            session: run.session_id(),
            engine: state.engine(),
            base_linker: state.base_linker(),
            config: &config,
        },
        prepared.vfs,
        prepared.vfs_info,
        prepared.services,
        runner::RunnerImports {
            packages: &prepared.packages,
            mcps: &prepared.mcp_catalog,
            tools: None,
        },
    )
    .await
    .map_err(|error| {
        tracing::warn!(run = %run.reservation_id(), %error, "replay failed");
        StuckReason::History
    })?;
    if !outcome.durable_completion {
        return Err(match outcome.error.as_ref().map(|e| e.kind) {
            Some(crate::error::ErrorKind::FuelExhausted) => StuckReason::Fuel,
            Some(crate::error::ErrorKind::Timeout) => StuckReason::Timeout,
            _ => StuckReason::Interrupted,
        });
    }
    #[cfg(test)]
    tests::process::checkpoint("before-terminal");
    let console = execute::split_console(&outcome.console_raw);
    let response: ExecuteResponse =
        execute::into_response(Some(&audit), run.session_id(), &outcome, &console);
    let last = crate::session::LastRun {
        result: response.result.clone(),
        console,
        error: response.error.clone(),
    };
    let json = serde_json::to_vec(&last).map_err(|_| StuckReason::Storage)?;
    let result = crate::handlers::runs::finished_body(&response);
    if !result.recordable {
        return Err(StuckReason::Storage);
    }
    let mut files =
        crate::idempotency::LastRunFiles::before(state.last_runs().as_ref(), run.session_id())
            .await;
    let Some(staged) = files.stage(json).await else {
        return Err(StuckReason::Storage);
    };
    let published = runner::durable::publish(
        state.units(),
        run,
        &history,
        &RunResolution::Completed(crate::domain::idempotent_request::RecordedOutcome {
            status: 200,
            body: result.body,
        }),
        Some(&staged),
    )
    .await;
    files
        .release(published.as_ref().ok().and_then(Clone::clone))
        .await;
    published.map_err(|_| StuckReason::Storage)?;
    Ok(())
}

#[cfg(test)]
mod tests;

/// Also called on startup for a crash between terminal commit and retirement.
pub(crate) async fn retire(
    manager: &crate::session_manager::SessionManager,
    units: &dyn crate::application::unit_of_work::UnitOfWorkFactory,
    id: &RunId,
) -> Result<(), String> {
    let run = {
        let mut unit = units.begin().await.map_err(|e| e.to_string())?;
        unit.get_run(id)
            .await
            .map_err(|e| e.to_string())?
            .ok_or("run missing")?
    };
    let program = {
        let mut unit = units.begin().await.map_err(|e| e.to_string())?;
        unit.get_run_program(id)
            .await
            .map_err(|e| e.to_string())?
            .ok_or("retained program missing")?
    };
    if let Some(expected) = program.filesystem_config_fingerprint {
        let (vfs, _) = manager
            .reopen_vfs(run.session_id(), &program.blueprint, &program.variables)
            .await
            .map_err(|e| e.to_string())?;
        let id = id.clone();
        tokio::task::spawn_blocking(move || {
            if submilli_durable::filesystem_config_fingerprint(&vfs).map_err(|e| e.to_string())?
                != expected
            {
                return Err("filesystem roots changed".to_owned());
            }
            submilli_durable::retire_filesystem(&vfs, &id).map_err(|e| e.to_string())
        })
        .await
        .map_err(|e| e.to_string())??;
    }
    let mut unit = units.begin().await.map_err(|e| e.to_string())?;
    unit.retire_run_resources(id)
        .await
        .map_err(|e| e.to_string())?;
    unit.commit().await.map_err(|e| e.to_string())
}
