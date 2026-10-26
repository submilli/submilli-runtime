//! The single-node execution owner. SQLite grants attempts; wakeups only reduce
//! polling latency. Dropping a transport waiter never drops accepted work.
use crate::app::AppState;
use crate::application::sessions::recovery::{
    self, RecoveryFilter, RunQuery, RunStatus, StuckReason,
};
use crate::application::sessions::runs::RunResolution;
use crate::domain::run::{Run, RunIdentity};
use crate::handlers::execute::{self, ExecuteInputs, ExecuteResponse};
use crate::runner;
use interpreter::runtime::durable::RunId;
use sha2::{Digest, Sha256};
use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use std::time::{Duration, SystemTime};
use tokio::sync::{Notify, oneshot};
use tokio::task::{Id, JoinSet};
use tokio_util::sync::CancellationToken;

mod volumes;

/// Prepare the complete replay environment before atomically accepting the run.
pub(crate) async fn admit(
    state: &AppState,
    mut inputs: ExecuteInputs<'_>,
) -> Result<RunId, String> {
    if !state.durable_execution() {
        return Err("durable execution is disabled".into());
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
    // The session catalog is immutable. Retain it with the accepted program,
    // independently of later blueprint availability and session hydration.
    program.tools =
        if submilli_shared::docs_scope::tools_availability(&program.blueprint).is_available() {
            state
                .session_manager()
                .tools(run.session_id())
                .await
                .map_err(|e| e.to_string())?
        } else {
            None
        };
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
    entry: Arc<crate::runs::RunEntry>,
    writable_volumes: HashSet<String>,
}

const DISCOVERY_INTERVAL: Duration = Duration::from_secs(1);
pub(crate) const SCHEDULER_TICK: Duration = Duration::from_millis(250);

pub(crate) async fn supervise(
    state: impl Fn() -> Option<AppState>,
    wakeup: Arc<Notify>,
    capacity: usize,
    draining: CancellationToken,
) {
    let mut tasks = JoinSet::new();
    let mut active: HashMap<Id, ActiveRun> = HashMap::new();
    let mut after = None;
    let mut retirements = JoinSet::new();
    let mut retirement_after = None;
    let mut next_retirement = tokio::time::Instant::now();
    loop {
        if draining.is_cancelled() {
            break;
        }
        let Some(app) = state() else {
            break;
        };
        let mut continue_scan = false;
        if live_work(&active) < capacity {
            match schedule(&app, &mut tasks, &mut active, after.clone(), capacity).await {
                Ok(next) => {
                    after = next;
                    continue_scan = after.is_some();
                }
                Err(error) => tracing::warn!(%error, "run discovery unavailable"),
            }
        }
        if retirements.is_empty() && tokio::time::Instant::now() >= next_retirement {
            match retirement_candidates(&app, retirement_after.clone()).await {
                Ok(page) => {
                    retirement_after = next_page(&page);
                    // Publication may precede task completion. Select terminal
                    // rows first, then exclude owners not yet joined. No new
                    // attempt can start for these terminal runs.
                    let ready: Vec<_> = page
                        .into_iter()
                        .filter(|candidate| {
                            !active.values().any(|running| {
                                running.run.reservation_id() == candidate.reservation_id()
                            })
                        })
                        .collect();
                    if !ready.is_empty() {
                        retirements.spawn(retire_runs(app.clone(), ready, draining.clone()));
                    }
                }
                Err(error) => tracing::warn!(%error, "retirement discovery unavailable"),
            }
            next_retirement = tokio::time::Instant::now() + DISCOVERY_INTERVAL;
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
            }
            finished = retirements.join_next(), if !retirements.is_empty() => {
                if let Some(Err(error)) = finished {
                    tracing::warn!(%error, "retirement worker failed");
                }
            }
            // At full capacity a stream of ready notifications must still let
            // workers run to completion and release their execution slots.
            () = wakeup.notified() => tokio::task::yield_now().await,
            // Keep the cursor on errors, but only successful pages continue
            // immediately. Failed discovery waits for the next poll or wakeup.
            () = tokio::task::yield_now(), if continue_scan && live_work(&active) < capacity => {}
            () = tokio::time::sleep_until(next_retirement), if retirements.is_empty() => {}
            () = tokio::time::sleep(SCHEDULER_TICK) => {}
        }
    }
    // Cancel the guest, then let the runner drain owned native work. Keep the
    // persisted attempt running so the next process can recover it.
    for (_, active_run) in active {
        active_run.entry.cancel();
    }
    while tasks.join_next().await.is_some() {}
    while retirements.join_next().await.is_some() {}
}

/// Waiting tools retain their instances, session order and volume reservations,
/// but do not prevent the scheduler from admitting unrelated execution work.
fn live_work(active: &HashMap<Id, ActiveRun>) -> usize {
    active
        .values()
        .filter(|run| !matches!(run.entry.current(), crate::runs::RunState::Suspended(_)))
        .count()
}

async fn schedule(
    state: &AppState,
    tasks: &mut JoinSet<Result<(), StuckReason>>,
    active: &mut HashMap<Id, ActiveRun>,
    mut after: Option<RunId>,
    capacity: usize,
) -> Result<Option<RunId>, crate::application::error::StoreError> {
    let page = candidates(state, after.clone()).await?;
    let next = next_page(&page);
    for candidate in page {
        if live_work(active) >= capacity {
            return Ok(after);
        }
        after = Some(candidate.reservation_id().clone());
        if active
            .values()
            .any(|running| running.run.session_id() == candidate.session_id())
        {
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
        let writable_volumes = match volumes::writable_volumes(state, candidate.reservation_id())
            .await
        {
            Ok(volumes) => volumes,
            Err(error) => {
                tracing::warn!(run = %candidate.reservation_id(), %error, "run volume configuration unavailable");
                recovery::mark_stuck(
                    state.units().as_ref(),
                    candidate.reservation_id(),
                    candidate.revision(),
                    StuckReason::History,
                )
                .await?;
                continue;
            }
        };
        if active
            .values()
            .any(|running| !running.writable_volumes.is_disjoint(&writable_volumes))
            || volumes::has_prior_recovering_writer(state, &candidate, &writable_volumes).await?
        {
            continue;
        }
        let Some(run) = claim_candidate(state, &candidate).await else {
            continue;
        };
        #[cfg(test)]
        tests::process::checkpoint("claimed");
        let (cancel, cancelled) = oneshot::channel();
        let entry = state.session_manager().runs().insert_durable(
            run.reservation_id(),
            run.session_id(),
            cancel,
        );
        let executing = run.clone();
        let worker = state.clone();
        let running_entry = entry.clone();
        let handle = tasks.spawn(async move {
            let _attempt = ToolAttempt {
                registry: worker.session_manager().runs().clone(),
                entry: running_entry,
            };
            run_attempt(&worker, &executing, cancelled).await
        });
        active.insert(
            handle.id(),
            ActiveRun {
                run,
                entry,
                writable_volumes,
            },
        );
    }
    Ok(next)
}

async fn claim_candidate(state: &AppState, candidate: &Run) -> Option<Run> {
    recovery::claim(
        state.units().as_ref(),
        candidate.reservation_id(),
        state.idempotency().generation(),
    )
    .await
    .unwrap_or_else(|error| {
        // No task was launched. A transient claim failure must not turn a
        // queued run stuck: the next scan reclaims a still-queued run, and
        // the rule above marks a committed but unacknowledged claim stuck.
        tracing::warn!(run = %candidate.reservation_id(), %error, "claim not confirmed; retrying on the next scan");
        None
    })
}

async fn candidates(
    state: &AppState,
    after: Option<RunId>,
) -> Result<Vec<Run>, crate::application::error::StoreError> {
    let mut unit = state.units().begin().await?;
    unit.list_runs(RunQuery {
        after,
        limit: 100,
        recovery: Some(RecoveryFilter::Execution),
        ..Default::default()
    })
    .await
}

async fn retirement_candidates(
    state: &AppState,
    after: Option<RunId>,
) -> Result<Vec<Run>, crate::application::error::StoreError> {
    let mut unit = state.units().begin().await?;
    unit.list_runs(RunQuery {
        after,
        limit: 100,
        recovery: Some(RecoveryFilter::Retirement),
        ..Default::default()
    })
    .await
}

async fn retire_runs(state: AppState, runs: Vec<Run>, draining: CancellationToken) {
    for run in runs {
        if draining.is_cancelled() {
            break;
        }
        state
            .session_manager()
            .retire_run(run.reservation_id())
            .await;
    }
}

fn next_page(page: &[Run]) -> Option<RunId> {
    if page.len() == 100 {
        page.last().map(|run| run.reservation_id().clone())
    } else {
        None
    }
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
    let entry = state.session_manager().runs().get(run.reservation_id());
    let tools = match (&program.tools, &entry) {
        (Some(tools), Some(entry)) => Some(run_tools(state, run, &program, tools, entry)),
        (Some(_), None) => return Err(StuckReason::Interrupted),
        _ => None,
    };
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
        tools,
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
    let history = runner::durable::history(
        run,
        state
            .history_objects()
            .ok_or(StuckReason::Storage)?
            .history(),
    );
    if let Some(entry) = entry {
        prepared.services.effects = Some(Arc::new(crate::runs::durable::ToolHistory::new(
            history.clone(),
            entry,
        )));
    }
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
            tools: prepared.tools.as_ref().map(|tools| &tools.package),
        },
    )
    .await
    .map_err(|error| {
        tracing::warn!(run = %run.reservation_id(), %error, "replay failed");
        match error {
            runner::durable::ReplayError::FilesystemChanged => StuckReason::FilesystemChanged,
            _ => StuckReason::History,
        }
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
        &RunResolution::Completed(response.output()),
        Some(&staged),
    )
    .await;
    files
        .release(published.as_ref().ok().and_then(Clone::clone))
        .await;
    published.map_err(|_| StuckReason::Storage)?;
    Ok(())
}

#[cfg(all(test, not(windows)))]
mod tests;

/// Also called on startup for a crash between terminal commit and retirement.
pub(crate) async fn retire(
    manager: &crate::session_manager::SessionManager,
    units: &dyn crate::application::unit_of_work::UnitOfWorkFactory,
    id: &RunId,
) -> Result<(), String> {
    let run = {
        let mut unit = units.begin().await.map_err(|e| e.to_string())?;
        // Another cleanup owner may have completed a previously listed run and
        // removed its session root. Do not reopen storage for a stale candidate.
        if unit
            .run_resources_retired(id)
            .await
            .map_err(|e| e.to_string())?
        {
            return Ok(());
        }
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
    if !matches!(
        run.status(),
        RunStatus::Completed | RunStatus::Cancelled | RunStatus::Aborted | RunStatus::Expired
    ) {
        return Err("run is not terminal".into());
    }
    let filesystem_changed = if let Some(expected) = program.filesystem_config_fingerprint {
        let (vfs, _) = manager
            .reopen_vfs(run.session_id(), &program.blueprint, &program.variables)
            .await
            .map_err(|e| e.to_string())?;
        let id = id.clone();
        tokio::task::spawn_blocking(move || {
            // Only private artifacts under current writable roots are removed.
            // A replay-compatibility check must not prevent terminal cleanup.
            let changed = submilli_durable::filesystem_config_fingerprint(&vfs)
                .is_ok_and(|actual| actual != expected);
            submilli_durable::retire_filesystem(&vfs, &id).map_err(|e| e.to_string())?;
            Ok::<_, String>(changed)
        })
        .await
        .map_err(|e| e.to_string())??
    } else {
        false
    };
    if let Some(objects) = &manager.history_objects {
        objects
            .remove_run(run.session_id(), id)
            .await
            .map_err(|e| e.to_string())?;
    }
    let mut unit = units.begin().await.map_err(|e| e.to_string())?;
    unit.retire_run_resources(id)
        .await
        .map_err(|e| e.to_string())?;
    unit.commit().await.map_err(|e| e.to_string())?;
    if filesystem_changed {
        tracing::warn!(run = %id, "filesystem roots or limits changed; retired private artifacts under current roots");
    }
    Ok(())
}

/// Dropping an attempt (including a failed task) wakes its observers and releases
/// registry resources. The supervisor still owns durable status transitions.
struct ToolAttempt {
    registry: Arc<crate::runs::RunRegistry>,
    entry: Arc<crate::runs::RunEntry>,
}
impl Drop for ToolAttempt {
    fn drop(&mut self) {
        self.entry.unavailable();
        self.registry.forget(&self.entry);
    }
}

fn run_tools(
    state: &AppState,
    run: &Run,
    program: &crate::domain::run_program::RunProgram,
    tools: &crate::domain::tools::SessionTools,
    entry: &Arc<crate::runs::RunEntry>,
) -> execute::RunTools {
    let (declaration, warnings) = crate::app_tools::declaration(tools);
    execute::RunTools {
        package: Arc::new(runner::ToolsPackage {
            declaration,
            warnings,
        }),
        provider: Arc::new(crate::runs::SuspendingToolProvider {
            entry: entry.clone(),
            timeout: program.blueprint.tool_call_timeout,
            touch: crate::handlers::sessions::session_touch(state, run.session_id()),
        }),
        cancel: None,
    }
}

#[cfg(all(test, not(windows)))]
pub(crate) fn tool_checkpoint(point: &str) {
    tests::process::checkpoint(point);
}
