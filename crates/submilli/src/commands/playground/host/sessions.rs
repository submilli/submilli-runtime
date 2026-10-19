//! The host's sessions: open, run, idle out, close, and survive a restart.
//!
//! Lifecycle rules follow `submilli-server`'s `session_manager.rs` and
//! `domain/session.rs`: variables are bound once, when the session opens, with
//! the blueprint's defaults filled in; the blueprint's idle timeout closes a
//! session nobody used, but never one with a run in flight; closing deletes the
//! session's values, last run and directory. Session values follow
//! `application/sessions/kv.rs`, and last runs the record, forget and keep rules
//! of `application/sessions/runs.rs`. Everything lives in [`store::Database`],
//! so an open session is still open after the playground restarts.

use std::collections::{BTreeMap, HashMap};
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, SystemTime};

use anyhow::{Context, Result};
use interpreter::runtime::{
    SessionKvEntry, SessionKvError, SessionKvFuture, SessionKvLimits, SessionKvPage,
    SessionKvStore, entry_bytes, value_bytes,
};
use submilli_blueprint::{Blueprint, VariableError, VfsConfig, resolve_variables};
use submilli_durable::codec::Sealer;
use tokio::sync::Mutex;

use super::super::log::{note, warn};
use super::super::state::StateDir;
use super::volumes::create_owned_dir;

mod store;

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

/// What [`Sessions::boot`] found.
pub(crate) struct Booted {
    pub(crate) sessions: Sessions,
    /// The sessions still open from the last start.
    pub(crate) open: Vec<Session>,
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
}

impl Sessions {
    /// Prepare `state`, open the host's database in it, and restore the sessions
    /// open when the playground last stopped. A session directory with no open
    /// session is a leftover and is removed. The first start after the embedded
    /// server kept sessions here prints, and returns, a one-line notice that they
    /// were left behind.
    pub(crate) async fn boot(state: &StateDir) -> Result<Booted> {
        let notice = state.create_noting_upgrade()?;
        if let Some(notice) = notice {
            note(notice);
        }
        let sealer = Sealer::new(Some(Arc::new(state.session_cipher()?)));
        let database = Database::open(&state.host_database_path()).await?;
        let sessions = Self {
            database,
            sealer,
            sessions_dir: state.host_sessions_dir(),
            limits: SessionKvLimits::default(),
            max_last_run_bytes: MAX_LAST_RUN_BYTES,
            in_flight: Mutex::new(HashMap::new()),
        };
        let open: Vec<Session> = sessions
            .database
            .sessions()
            .await?
            .into_iter()
            .map(Session::from)
            .collect();
        sessions.remove_leftover_dirs(&open);
        Ok(Booted {
            sessions,
            open,
            notice,
        })
    }

    #[cfg(test)]
    fn with_max_last_run_bytes(mut self, limit: u64) -> Self {
        self.max_last_run_bytes = limit;
        self
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
        let variables = resolve_variables(&blueprint.variables, supplied)?;
        let session = Session {
            id: uuid::Uuid::new_v4().to_string(),
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

    pub(crate) async fn list(&self) -> Result<Vec<Session>> {
        Ok(self
            .database
            .sessions()
            .await?
            .into_iter()
            .map(Session::from)
            .collect())
    }

    /// Where a `per_session` session's files live.
    pub(crate) fn session_dir(&self, id: &str) -> PathBuf {
        self.sessions_dir.join(id)
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

    /// Close the session: its values, last run and directory go with it. Returns
    /// whether it was open.
    pub(crate) async fn close(&self, id: &str) -> Result<bool> {
        let closed = self.database.delete_session(id).await?;
        self.remove_dir(id);
        Ok(closed)
    }

    /// Close every session.
    pub(crate) async fn wipe(&self) -> Result<()> {
        for session in self.database.sessions().await? {
            self.close(&session.id).await?;
        }
        Ok(())
    }

    /// Close every session idle past its timeout at `now` with no run in flight.
    /// Returns the ids it closed.
    pub(crate) async fn reap(&self, now: SystemTime) -> Result<Vec<String>> {
        let in_flight = self.in_flight.lock().await;
        let mut closed = Vec::new();
        for session in self.database.sessions().await? {
            let idle_until = session.last_activity + session.idle_timeout;
            if idle_until > now || in_flight.contains_key(&session.id) {
                continue;
            }
            if self.close(&session.id).await? {
                closed.push(session.id);
            }
        }
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

    fn remove_leftover_dirs(&self, open: &[Session]) {
        let Ok(entries) = std::fs::read_dir(&self.sessions_dir) else {
            return;
        };
        for entry in entries.flatten() {
            let name = entry.file_name();
            let Some(name) = name.to_str() else { continue };
            if !open.iter().any(|session| session.id == name) {
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
