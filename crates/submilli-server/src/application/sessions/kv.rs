//! A session's key-value state, `submilli:session`.
//!
//! [`SessionKeyValues`] runs each operation as its own transaction. The functions
//! below it run inside a caller's unit of work, so a later caller can commit a
//! mutation together with whatever records it (a run's history). Each first
//! checks the session is still active in that transaction: a closed session's
//! state is gone, and a run still holding its store can neither read it nor
//! write it back. Their callers check keys and values against the limits first
//! (`check_key`, `check_write`), as [`SessionKeyValues`] does.
//!
//! Waiting for a busy database is not bounded by the run's execution time,
//! which only interrupts guest code: a call can wait up to [`ADMISSION_WAIT`],
//! as an HTTP call can wait for its own timeout.
use std::time::Duration;

use interpreter::runtime::{
    SessionKvEntry, SessionKvError, SessionKvLimits, SessionKvPage, SessionKvUsage, entry_bytes,
    value_bytes,
};

use crate::application::error::StoreError;
use crate::application::unit_of_work::{SessionStateSize, UnitOfWork, UnitOfWorkFactory};

/// How long an operation waits for the database to take it before the busy
/// database counts as a failure. Until then nothing has happened, so waiting
/// is safe where failing would end the run.
const ADMISSION_WAIT: Duration = Duration::from_secs(30);
const FIRST_PAUSE: Duration = Duration::from_millis(5);
const LONGEST_PAUSE: Duration = Duration::from_millis(200);

pub(crate) struct SessionKeyValues<'a> {
    units: &'a dyn UnitOfWorkFactory,
    limits: SessionKvLimits,
}

impl<'a> SessionKeyValues<'a> {
    pub fn new(units: &'a dyn UnitOfWorkFactory, limits: SessionKvLimits) -> Self {
        Self { units, limits }
    }

    pub async fn get(
        &self,
        session_id: &str,
        key: &[u16],
    ) -> Result<Option<Vec<u16>>, SessionKvError> {
        self.limits.check_key("get", key)?;
        let mut unit = self.begin("get").await?;
        let found = get(unit.as_mut(), session_id, key).await;
        commit(unit, "get", found).await
    }

    pub async fn has(&self, session_id: &str, key: &[u16]) -> Result<bool, SessionKvError> {
        self.limits.check_key("has", key)?;
        let mut unit = self.begin("has").await?;
        let found = has(unit.as_mut(), session_id, key).await;
        commit(unit, "has", found).await
    }

    pub async fn set(
        &self,
        session_id: &str,
        key: &[u16],
        payload: &[u16],
    ) -> Result<(), SessionKvError> {
        self.limits.check_write(key, payload)?;
        let mut unit = self.begin("set").await?;
        let written = set(unit.as_mut(), &self.limits, session_id, key, payload).await;
        commit(unit, "set", written).await
    }

    pub async fn remove(&self, session_id: &str, key: &[u16]) -> Result<bool, SessionKvError> {
        self.limits.check_key("remove", key)?;
        let mut unit = self.begin("remove").await?;
        let removed = remove(unit.as_mut(), session_id, key).await;
        commit(unit, "remove", removed).await
    }

    pub async fn scan(
        &self,
        session_id: &str,
        after: Option<&[u16]>,
        prefix: &[u16],
        max_scan: usize,
    ) -> Result<SessionKvPage, SessionKvError> {
        let mut unit = self.begin("list").await?;
        let page = scan(unit.as_mut(), session_id, after, prefix, max_scan).await;
        commit(unit, "list", page).await
    }

    /// Begin a unit, waiting while the database has too much work queued.
    async fn begin(&self, operation: &'static str) -> Result<Box<dyn UnitOfWork>, SessionKvError> {
        let deadline = tokio::time::Instant::now() + ADMISSION_WAIT;
        let mut pause = FIRST_PAUSE;
        loop {
            match self.units.begin().await {
                Err(StoreError::Busy) if tokio::time::Instant::now() < deadline => {
                    tokio::time::sleep(pause).await;
                    pause = (pause * 2).min(LONGEST_PAUSE);
                }
                begun => return begun.map_err(|error| backend(operation, error)),
            }
        }
    }
}

/// Commit, or leave the transaction to roll back when the operation failed:
/// dropping an uncommitted unit discards it.
async fn commit<T>(
    unit: Box<dyn UnitOfWork>,
    operation: &'static str,
    outcome: Result<T, SessionKvError>,
) -> Result<T, SessionKvError> {
    let value = outcome?;
    unit.commit()
        .await
        .map_err(|error| backend(operation, error))?;
    Ok(value)
}

/// A storage failure, named by the operation it failed. The value is never in
/// it: a [`StoreError`] carries none.
fn backend(operation: &'static str, error: StoreError) -> SessionKvError {
    SessionKvError::Backend {
        operation,
        message: error.to_string(),
    }
}

async fn require_active(
    unit: &mut dyn UnitOfWork,
    session_id: &str,
    operation: &'static str,
) -> Result<(), SessionKvError> {
    match unit.session_is_active(session_id).await {
        Ok(true) => Ok(()),
        Ok(false) => Err(SessionKvError::SessionClosed { operation }),
        Err(error) => Err(backend(operation, error)),
    }
}

pub(crate) async fn get(
    unit: &mut dyn UnitOfWork,
    session_id: &str,
    key: &[u16],
) -> Result<Option<Vec<u16>>, SessionKvError> {
    require_active(unit, session_id, "get").await?;
    unit.session_state_get(session_id, key)
        .await
        .map_err(|error| backend("get", error))
}

pub(crate) async fn has(
    unit: &mut dyn UnitOfWork,
    session_id: &str,
    key: &[u16],
) -> Result<bool, SessionKvError> {
    require_active(unit, session_id, "has").await?;
    unit.session_state_has(session_id, key)
        .await
        .map_err(|error| backend("has", error))
}

/// Checks the write against what the session holds and writes it, in one
/// transaction: two writers cannot both spend the same headroom.
pub(crate) async fn set(
    unit: &mut dyn UnitOfWork,
    limits: &SessionKvLimits,
    session_id: &str,
    key: &[u16],
    payload: &[u16],
) -> Result<(), SessionKvError> {
    require_active(unit, session_id, "set").await?;
    let held = unit
        .session_state_usage(session_id, key)
        .await
        .map_err(|error| backend("set", error))?;
    limits.check_capacity(
        key,
        payload,
        SessionKvUsage {
            entries: held.entries,
            bytes: held.bytes,
            replaced: held.replaced,
        },
    )?;
    let size = SessionStateSize {
        value_bytes: value_bytes(payload),
        retained_bytes: entry_bytes(key, payload),
    };
    unit.session_state_put(session_id, key, payload, size)
        .await
        .map_err(|error| backend("set", error))
}

pub(crate) async fn remove(
    unit: &mut dyn UnitOfWork,
    session_id: &str,
    key: &[u16],
) -> Result<bool, SessionKvError> {
    require_active(unit, session_id, "remove").await?;
    unit.session_state_remove(session_id, key)
        .await
        .map_err(|error| backend("remove", error))
}

/// Up to `max_scan` keys after `after`, keeping those that start with
/// `prefix`: the page [`interpreter::runtime::SessionKvStore::scan`] describes.
pub(crate) async fn scan(
    unit: &mut dyn UnitOfWork,
    session_id: &str,
    after: Option<&[u16]>,
    prefix: &[u16],
    max_scan: usize,
) -> Result<SessionKvPage, SessionKvError> {
    require_active(unit, session_id, "list").await?;
    let scanned = unit
        .session_state_keys(session_id, after, max_scan)
        .await
        .map_err(|error| backend("list", error))?;
    let last_scanned = scanned.keys.last().map(|(key, _)| key.clone());
    let entries = scanned
        .keys
        .into_iter()
        .filter(|(key, _)| key.starts_with(prefix))
        .map(|(key, size_bytes)| SessionKvEntry { key, size_bytes })
        .collect();
    Ok(SessionKvPage {
        entries,
        // A walk that considered no key has stopped on nothing, however many remain.
        scanned_all: !scanned.more || last_scanned.is_none(),
        last_scanned,
    })
}
