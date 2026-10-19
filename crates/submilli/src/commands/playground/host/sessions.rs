//! The host's sessions: open, run, idle out, close, and survive a restart.
//!
//! Lifecycle rules follow `submilli-server`'s `session_manager.rs` and
//! `domain/session.rs`: variables are bound once, when the session opens, with
//! the blueprint's defaults filled in; the blueprint's idle timeout closes a
//! session nobody used, but never one with a run in flight; closing deletes the
//! session's values, last run and directory. Session values follow
//! `application/sessions/kv.rs`, and last runs the record, forget and keep rules
//! of `application/sessions/runs.rs`. Closing follows `session_manager.rs`
//! (`cleanup`, `close_owned`): every close takes one path, in turn, and runs the
//! session's package finalizers before its files and values go. Everything lives
//! in [`store::Database`], so an open session is still open after the playground
//! restarts, and a close cut short finishes after it.

use std::collections::{BTreeMap, HashMap};
use std::path::PathBuf;
use std::sync::{Arc, OnceLock};
use std::time::{Duration, SystemTime};

use anyhow::{Context, Result};
use interpreter::runtime::{
    SessionKvEntry, SessionKvError, SessionKvFuture, SessionKvLimits, SessionKvPage,
    SessionKvStore, entry_bytes, value_bytes,
};
use submilli_blueprint::{
    Blueprint, HarnessSecretBindings, VariableError, VfsConfig, resolve_variables,
};
use submilli_durable::codec::Sealer;
use tokio::sync::Mutex;

use super::super::log::{note, warn};
use super::super::state::StateDir;
use super::finalizers::{ClosingSession, FinalizerExecutor, Registrations, finalize_session};
use super::volumes::create_owned_dir;

pub(super) mod store;

use store::{Database, SessionRow, StoredLastRun};

/// The largest last run kept; a larger one is reported by its size alone. The
/// server's default, `DEFAULT_MAX_LAST_RUN_BYTES`.
pub(crate) const MAX_LAST_RUN_BYTES: u64 = 64 * 1024 * 1024;

/// One open session.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Session {
    pub(crate) id: String,
    pub(crate) blueprint_name: String,
    pub(crate) idle_timeout: Duration,
    pub(crate) last_activity: SystemTime,
    /// Bound when the session opened, defaults filled in.
    pub(crate) variables: BTreeMap<String, String>,
}

impl From<SessionRow> for Session {
    fn from(row: SessionRow) -> Self {
        Self {
            id: row.id,
            blueprint_name: row.blueprint_name,
            idle_timeout: row.idle_timeout,
            last_activity: row.last_activity,
            variables: row.variables.into_iter().collect(),
        }
    }
}

/// How a run ended, for its session's last run.
#[derive(Clone, Copy, Debug)]
pub(crate) enum RunEnd<'a> {
    /// The run ended on its own: its output, `{result, console, error}` JSON, is
    /// the latest.
    Finished(&'a [u8]),
    /// What the run did is unknown, so no earlier run is the latest.
    Unknown,
    /// The run never started: the last run before is still the latest.
    NeverStarted,
}

/// A session's last run.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum LastRun {
    Kept {
        run_id: String,
        completed_at: SystemTime,
        json: Vec<u8>,
    },
    /// The run's output was over the limit, so only its size was kept.
    TooLarge {
        run_id: String,
        completed_at: SystemTime,
        bytes: u64,
        limit: u64,
    },
}

#[derive(Debug)]
pub(crate) enum SessionsError {
    UnknownSession,
    Variables(VariableError),
    Storage(anyhow::Error),
}

impl std::fmt::Display for SessionsError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::UnknownSession => f.write_str("unknown or expired session"),
            Self::Variables(error) => error.fmt(f),
            Self::Storage(error) => write!(f, "session storage unavailable: {error:#}"),
        }
    }
}

impl std::error::Error for SessionsError {}

impl From<VariableError> for SessionsError {
    fn from(error: VariableError) -> Self {
        Self::Variables(error)
    }
}

impl From<anyhow::Error> for SessionsError {
    fn from(error: anyhow::Error) -> Self {
        Self::Storage(error)
    }
}

/// What [`Sessions::boot_reporting`] found.
#[cfg(test)]
pub(crate) struct Booted {
    pub(crate) sessions: Sessions,
    /// The sessions still open from the last start.
    pub(crate) open: Vec<Session>,
    /// The notice printed, if any.
    pub(crate) notice: Option<&'static str>,
}

pub(crate) struct Sessions {
    database: Database,
    sealer: Sealer,
    sessions_dir: PathBuf,
    limits: SessionKvLimits,
    max_last_run_bytes: u64,
    /// Runs in flight per session. The reaper takes this lock for its whole pass,
    /// so a run that starts while it decides is either counted or finds the
    /// session closed.
    in_flight: Mutex<HashMap<String, usize>>,
    /// Each session's resource scope while anything holds it: closing a session
    /// cancels its scope and waits for the work admitted under it to drain.
    resource_scopes: std::sync::Mutex<HashMap<String, std::sync::Weak<ResourceScope>>>,
    /// Each open session's secret bindings, as its caller bound them. Kept in
    /// memory only: a closing session with finalizers keeps a sealed copy.
    secrets: std::sync::Mutex<HashMap<String, Arc<HarnessSecretBindings>>>,
    registrations: Registrations,
    finalizer_executor: OnceLock<Arc<dyn FinalizerExecutor>>,
    /// Held across one session's cleanup, so closes arriving together wait in
    /// turn and a session's finalizers run once.
    cleanup_turn: Mutex<()>,
}

/// What a session's admitted work shares: the lock closing takes for writing once
/// that work has drained, and the token that tells the work to stop.
#[derive(Default)]
struct ResourceScope {
    work: Arc<tokio::sync::RwLock<()>>,
    cancel: tokio_util::sync::CancellationToken,
}

/// Work admitted to run against an open session, from
/// [`Sessions::admit_resource_work`]. Closing the session cancels
/// [`Self::cancel`], then waits until every one of these has dropped before it
/// deletes the session's files and values. Mirrors `submilli-server`'s
/// `session_manager.rs` (`ResourceUse`).
pub(crate) struct ResourceUse {
    _scope: Arc<ResourceScope>,
    _permit: tokio::sync::OwnedRwLockReadGuard<()>,
    pub(crate) cancel: tokio_util::sync::CancellationToken,
}

impl Sessions {
    /// Prepare `state`, open the host's database in it, and restore the sessions
    /// open when the playground last stopped. A session directory with no open
    /// session is a leftover and is removed. The first start after the embedded
    /// server kept sessions here prints, and returns, a one-line notice that they
    /// were left behind.
    pub(crate) async fn boot(state: &StateDir) -> Result<Self> {
        Ok(Self::boot_inner(state).await?.0)
    }

    /// [`Sessions::boot`], with the sessions it restored and the notice it printed.
    #[cfg(test)]
    pub(crate) async fn boot_reporting(state: &StateDir) -> Result<Booted> {
        let (sessions, open, notice) = Self::boot_inner(state).await?;
        Ok(Booted {
            sessions,
            open,
            notice,
        })
    }

    async fn boot_inner(state: &StateDir) -> Result<(Self, Vec<Session>, Option<&'static str>)> {
        let notice = state.create_noting_upgrade()?;
        if let Some(notice) = notice {
            note(notice);
        }
        let sealer = Sealer::new(Some(Arc::new(state.session_cipher()?)));
        let database = Database::open(&state.host_database_path()).await?;
        let limits = SessionKvLimits::default();
        let registrations = Registrations {
            database: database.clone(),
            sealer: sealer.clone(),
            limits,
        };
        // Nothing runs yet: an attempt still running was cut short.
        registrations.recover().await?;
        let sessions = Self {
            database,
            sealer,
            sessions_dir: state.host_sessions_dir(),
            limits,
            max_last_run_bytes: MAX_LAST_RUN_BYTES,
            in_flight: Mutex::new(HashMap::new()),
            resource_scopes: std::sync::Mutex::new(HashMap::new()),
            secrets: std::sync::Mutex::new(HashMap::new()),
            registrations,
            finalizer_executor: OnceLock::new(),
            cleanup_turn: Mutex::new(()),
        };
        let open: Vec<Session> = sessions
            .database
            .sessions()
            .await?
            .into_iter()
            .map(Session::from)
            .collect();
        let mut kept: Vec<String> = open.iter().map(|session| session.id.clone()).collect();
        kept.extend(sessions.database.closing_ids().await?);
        sessions.remove_leftover_dirs(&kept);
        Ok((sessions, open, notice))
    }

    #[cfg(test)]
    fn with_max_last_run_bytes(mut self, limit: u64) -> Self {
        self.max_last_run_bytes = limit;
        self
    }

    #[cfg(test)]
    pub(crate) fn with_limits(mut self, limits: SessionKvLimits) -> Self {
        self.limits = limits;
        self.registrations.limits = limits;
        self
    }

    /// Where the sessions' runs register finalizers, and what they registered.
    pub(crate) fn finalizers(&self) -> &Registrations {
        &self.registrations
    }

    /// Install what runs finalizers, once, before any close needs it.
    pub(crate) fn set_finalizer_executor(&self, executor: Arc<dyn FinalizerExecutor>) {
        let _ = self.finalizer_executor.set(executor);
    }

    /// Bind the open session's secrets, for its finalizers should it close with
    /// any registered.
    pub(crate) fn bind_secrets(&self, id: &str, secrets: Arc<HarnessSecretBindings>) {
        self.lock_secrets().insert(id.to_owned(), secrets);
    }

    /// The open session's bound secrets, if any were bound.
    pub(crate) fn secrets(&self, id: &str) -> Option<Arc<HarnessSecretBindings>> {
        self.lock_secrets().get(id).cloned()
    }

    /// Drop the session's bound secrets.
    pub(crate) fn forget_secrets(&self, id: &str) {
        self.lock_secrets().remove(id);
    }

    fn lock_secrets(
        &self,
    ) -> std::sync::MutexGuard<'_, HashMap<String, Arc<HarnessSecretBindings>>> {
        // Poison means a panic interrupted an insert or a removal. AGENTS.md
        // permits the poisoned-lock panic rather than reading the map.
        self.secrets.lock().expect("session secrets poisoned")
    }

    /// Close the database. Nothing may use these sessions after.
    pub(crate) async fn close_database(&self) -> Result<()> {
        self.database.close().await
    }

    /// Open a session for `blueprint`, binding `supplied` against its variables.
    pub(crate) async fn open(
        &self,
        blueprint: &Blueprint,
        supplied: &BTreeMap<String, String>,
    ) -> Result<Session, SessionsError> {
        self.open_as(uuid::Uuid::new_v4().to_string(), blueprint, supplied)
            .await
    }

    /// Like [`Self::open`], under an id the caller chose, such as the MCP session
    /// id an agent already holds.
    pub(crate) async fn open_as(
        &self,
        id: String,
        blueprint: &Blueprint,
        supplied: &BTreeMap<String, String>,
    ) -> Result<Session, SessionsError> {
        let variables = resolve_variables(&blueprint.variables, supplied)?;
        let session = Session {
            id,
            blueprint_name: blueprint.name.clone(),
            idle_timeout: blueprint.idle_timeout,
            last_activity: SystemTime::now(),
            variables,
        };
        if matches!(blueprint.vfs, VfsConfig::PerSession { .. }) {
            create_owned_dir(&self.session_dir(&session.id))
                .context("creating the session's directory")?;
        }
        self.database
            .insert_session(&SessionRow {
                id: session.id.clone(),
                blueprint_name: session.blueprint_name.clone(),
                idle_timeout: session.idle_timeout,
                last_activity: session.last_activity,
                variables: session
                    .variables
                    .iter()
                    .map(|(name, value)| (name.clone(), value.clone()))
                    .collect(),
            })
            .await?;
        Ok(session)
    }

    pub(crate) async fn get(&self, id: &str) -> Result<Option<Session>> {
        Ok(self.database.session(id).await?.map(Session::from))
    }

    /// Where a `per_session` session's files live.
    pub(crate) fn session_dir(&self, id: &str) -> PathBuf {
        self.sessions_dir.join(id)
    }

    /// The session is active now. Returns whether it is open.
    pub(crate) async fn touch(&self, id: &str) -> Result<bool> {
        self.database.touch(id, SystemTime::now()).await
    }

    /// Store the session's MCP `initialize` parameters, as JSON, for it to
    /// resume under after a restart. Returns whether the session is open.
    pub(crate) async fn put_mcp_state(&self, id: &str, json: &str) -> Result<bool> {
        self.database.put_mcp_state(id, json).await
    }

    /// The session's stored MCP `initialize` parameters.
    pub(crate) async fn mcp_state(&self, id: &str) -> Result<Option<String>> {
        self.database.mcp_state(id).await
    }

    /// A run in the session starts: it is active now, and the reaper leaves it
    /// open until [`Self::end_run`].
    pub(crate) async fn begin_run(&self, id: &str) -> Result<(), SessionsError> {
        let mut in_flight = self.in_flight.lock().await;
        if !self.database.touch(id, SystemTime::now()).await? {
            return Err(SessionsError::UnknownSession);
        }
        *in_flight.entry(id.to_owned()).or_default() += 1;
        Ok(())
    }

    /// The run [`Self::begin_run`] counted ended as `end`: the session is active
    /// now, and its last run follows how the run ended.
    pub(crate) async fn end_run(&self, id: &str, run_id: &str, end: RunEnd<'_>) -> Result<()> {
        {
            let mut in_flight = self.in_flight.lock().await;
            if let Some(count) = in_flight.get_mut(id) {
                *count -= 1;
                if *count == 0 {
                    in_flight.remove(id);
                }
            }
        }
        let now = SystemTime::now();
        self.database.touch(id, now).await?;
        match end {
            RunEnd::Finished(json) => {
                self.database
                    .put_last_run(&self.sealer, id, run_id, now, json, self.max_last_run_bytes)
                    .await?;
            }
            RunEnd::Unknown => self.database.remove_last_run(id).await?,
            RunEnd::NeverStarted => {}
        }
        Ok(())
    }

    pub(crate) async fn last_run(&self, id: &str) -> Result<Option<LastRun>> {
        Ok(self
            .database
            .last_run(&self.sealer, id)
            .await?
            .map(|stored| match stored {
                StoredLastRun::Kept {
                    run_id,
                    completed_at,
                    json,
                } => LastRun::Kept {
                    run_id,
                    completed_at,
                    json,
                },
                StoredLastRun::TooLarge {
                    run_id,
                    completed_at,
                    bytes,
                    limit,
                } => LastRun::TooLarge {
                    run_id,
                    completed_at,
                    bytes,
                    limit,
                },
            }))
    }

    /// Admit work against the open session `id`: a run holds the returned use
    /// until its blocking host work has drained.
    pub(crate) async fn admit_resource_work(&self, id: &str) -> Result<ResourceUse, SessionsError> {
        let scope = self.resource_scope(id);
        let permit = scope.work.clone().read_owned().await;
        if self.database.session(id).await?.is_none() || scope.cancel.is_cancelled() {
            return Err(SessionsError::UnknownSession);
        }
        Ok(ResourceUse {
            _permit: permit,
            cancel: scope.cancel.clone(),
            _scope: scope,
        })
    }

    fn resource_scope(&self, id: &str) -> Arc<ResourceScope> {
        // Poison means a panic interrupted an insert or a prune of this map.
        // AGENTS.md permits the poisoned-lock panic rather than reading it.
        let mut scopes = self
            .resource_scopes
            .lock()
            .expect("session resource scopes poisoned");
        scopes.retain(|_, scope| scope.strong_count() > 0);
        if let Some(scope) = scopes.get(id).and_then(std::sync::Weak::upgrade) {
            return scope;
        }
        let scope = Arc::new(ResourceScope::default());
        scopes.insert(id.to_owned(), Arc::downgrade(&scope));
        scope
    }

    /// Close the session: it stops being open, its admitted work is told to stop
    /// and drains, its finalizers run, and then its values, last run and
    /// directory go. Returns whether it was open. Closing a session already
    /// closing waits for that cleanup, and retries it if it failed.
    pub(crate) async fn close(&self, id: &str) -> Result<bool> {
        let closed = self.begin_close(id).await?;
        self.cleanup(id).await?;
        Ok(closed)
    }

    /// Mark the open session closing, in one transaction with its finalizers
    /// becoming pending and its secret bindings kept for them. Returns whether it
    /// was open.
    async fn begin_close(&self, id: &str) -> Result<bool> {
        let secrets = self.lock_secrets().get(id).cloned().unwrap_or_default();
        let mut tx = self.database.begin().await?;
        if !store::is_open(&mut tx, id).await? {
            return Ok(false);
        }
        super::finalizers::terminate(&mut tx, &self.sealer, id, &secrets).await?;
        sqlx::query("UPDATE sessions SET closing=1 WHERE session_id=?")
            .bind(id)
            .execute(&mut *tx)
            .await?;
        tx.commit().await?;
        self.lock_secrets().remove(id);
        Ok(true)
    }

    /// Finish closing `id`, if it is closing. Nothing goes until every finalizer
    /// has finished and that is recorded; until then the session stays closing,
    /// to be retried.
    async fn cleanup(&self, id: &str) -> Result<()> {
        let _turn = self.cleanup_turn.lock().await;
        let Some(row) = self.database.closing_session(id).await? else {
            return Ok(());
        };
        let scope = self.resource_scope(id);
        scope.cancel.cancel();
        // A run holds its read permit until its blocking host work has drained.
        let _drained = scope.work.write().await;
        if let Some(executor) = self.finalizer_executor.get() {
            let session = self
                .registrations
                .secrets(id)
                .await
                .map(|secrets| ClosingSession {
                    id: row.id.clone(),
                    blueprint_name: row.blueprint_name.clone(),
                    variables: Arc::new(row.variables.iter().cloned().collect()),
                    secrets: Arc::new(secrets),
                });
            finalize_session(&self.registrations, executor.as_ref(), id, session).await?;
        } else if self.registrations.has_unfinished(id).await? {
            anyhow::bail!("finalizer executor is unavailable");
        }
        if self.registrations.has_unfinished(id).await? {
            anyhow::bail!("finalization completion could not be recorded; recovery is required");
        }
        self.remove_dir(id);
        // The kept secret bindings go with the session row.
        self.database.delete_session(id).await?;
        Ok(())
    }

    /// Finish every close cut short, by a stop or by a cleanup that failed.
    pub(crate) async fn finish_closing(&self) -> Result<()> {
        for id in self.database.closing_ids().await? {
            self.cleanup(&id).await?;
        }
        Ok(())
    }

    /// Close every session idle past its timeout at `now` with no run in flight,
    /// then finish every close still pending. Returns the ids it closed.
    pub(crate) async fn reap(&self, now: SystemTime) -> Result<Vec<String>> {
        let mut closed = Vec::new();
        {
            let in_flight = self.in_flight.lock().await;
            for session in self.database.sessions().await? {
                let idle_until = session.last_activity + session.idle_timeout;
                if idle_until > now || in_flight.contains_key(&session.id) {
                    continue;
                }
                if self.begin_close(&session.id).await? {
                    closed.push(session.id);
                }
            }
        }
        self.finish_closing().await?;
        Ok(closed)
    }

    /// Reap idle sessions every `every` until the task is dropped.
    pub(crate) fn spawn_reaper(self: &Arc<Self>, every: Duration) -> tokio::task::JoinHandle<()> {
        let sessions = Arc::clone(self);
        tokio::spawn(async move {
            let mut ticks = tokio::time::interval(every);
            loop {
                ticks.tick().await;
                if let Err(error) = sessions.reap(SystemTime::now()).await {
                    warn(&format!("closing idle sessions failed: {error:#}"));
                }
            }
        })
    }

    /// The session's values, for its runs.
    pub(crate) fn values(&self, id: &str) -> Arc<dyn SessionKvStore> {
        Arc::new(SessionValues {
            database: self.database.clone(),
            sealer: self.sealer.clone(),
            session_id: id.to_owned(),
            limits: self.limits,
        })
    }

    fn remove_dir(&self, id: &str) {
        let dir = self.session_dir(id);
        match std::fs::remove_dir_all(&dir) {
            Ok(()) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => warn(&format!(
                "removing the closed session's directory {} failed: {error}",
                dir.display()
            )),
        }
    }

    fn remove_leftover_dirs(&self, kept: &[String]) {
        let Ok(entries) = std::fs::read_dir(&self.sessions_dir) else {
            return;
        };
        for entry in entries.flatten() {
            let name = entry.file_name();
            let Some(name) = name.to_str() else { continue };
            if !kept.iter().any(|id| id == name) {
                self.remove_dir(name);
            }
        }
    }
}

/// One session's values, `submilli:session`. Each operation is its own
/// transaction, and first checks the session is still open in it: a closed
/// session's values are gone, and a run still holding them can neither read
/// them nor write them back.
struct SessionValues {
    database: Database,
    sealer: Sealer,
    session_id: String,
    limits: SessionKvLimits,
}

impl SessionValues {
    async fn transaction<T>(
        &self,
        operation: &'static str,
        body: impl AsyncFnOnce(&mut sqlx::SqliteConnection) -> Result<T, SessionKvError>,
    ) -> Result<T, SessionKvError> {
        let backend = backend(operation);
        let mut tx = self.database.begin().await.map_err(backend)?;
        if !store::is_open(&mut tx, &self.session_id)
            .await
            .map_err(backend)?
        {
            return Err(SessionKvError::SessionClosed { operation });
        }
        let value = body(&mut tx).await?;
        tx.commit().await.map_err(|error| backend(error.into()))?;
        Ok(value)
    }
}

fn backend(operation: &'static str) -> impl Fn(anyhow::Error) -> SessionKvError + Copy {
    move |error| SessionKvError::Backend {
        operation,
        message: format!("{error:#}"),
    }
}

impl SessionKvStore for SessionValues {
    fn get<'a>(&'a self, key: &'a [u16]) -> SessionKvFuture<'a, Option<Vec<u16>>> {
        Box::pin(async move {
            self.limits.check_key("get", key)?;
            self.transaction("get", async |connection| {
                store::kv::get(connection, &self.sealer, &self.session_id, key)
                    .await
                    .map_err(backend("get"))
            })
            .await
        })
    }

    fn has<'a>(&'a self, key: &'a [u16]) -> SessionKvFuture<'a, bool> {
        Box::pin(async move {
            self.limits.check_key("has", key)?;
            self.transaction("has", async |connection| {
                store::kv::has(connection, &self.session_id, key)
                    .await
                    .map_err(backend("has"))
            })
            .await
        })
    }

    fn set<'a>(&'a self, key: &'a [u16], payload: &'a [u16]) -> SessionKvFuture<'a, ()> {
        Box::pin(async move {
            self.limits.check_write(key, payload)?;
            // The check and the write share one transaction: two writers cannot
            // both spend the same headroom.
            self.transaction("set", async |connection| {
                let held = store::kv::usage(connection, &self.session_id, key)
                    .await
                    .map_err(backend("set"))?;
                self.limits.check_capacity(key, payload, held)?;
                let size = (value_bytes(payload), entry_bytes(key, payload));
                store::kv::put(
                    connection,
                    &self.sealer,
                    &self.session_id,
                    key,
                    payload,
                    size,
                )
                .await
                .map_err(backend("set"))
            })
            .await
        })
    }

    fn remove<'a>(&'a self, key: &'a [u16]) -> SessionKvFuture<'a, bool> {
        Box::pin(async move {
            self.limits.check_key("remove", key)?;
            self.transaction("remove", async |connection| {
                store::kv::remove(connection, &self.session_id, key)
                    .await
                    .map_err(backend("remove"))
            })
            .await
        })
    }

    fn scan<'a>(
        &'a self,
        after: Option<&'a [u16]>,
        prefix: &'a [u16],
        max_scan: usize,
    ) -> SessionKvFuture<'a, SessionKvPage> {
        Box::pin(async move {
            self.transaction("list", async |connection| {
                let (keys, more) = store::kv::keys(connection, &self.session_id, after, max_scan)
                    .await
                    .map_err(backend("list"))?;
                let last_scanned = keys.last().map(|(key, _)| key.clone());
                let entries = keys
                    .into_iter()
                    .filter(|(key, _)| key.starts_with(prefix))
                    .map(|(key, size_bytes)| SessionKvEntry { key, size_bytes })
                    .collect();
                Ok(SessionKvPage {
                    entries,
                    // A walk that considered no key has stopped on nothing,
                    // however many remain.
                    scanned_all: !more || last_scanned.is_none(),
                    last_scanned,
                })
            })
            .await
        })
    }

    fn limits(&self) -> SessionKvLimits {
        self.limits
    }
}

#[cfg(test)]
mod tests;
