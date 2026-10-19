//! The host's session database: one SQLite file under `<state>/host/`, owned by
//! this process alone.
//!
//! The connection settings and the close follow `submilli-server`'s
//! `database.rs`; the key-value mapping follows its `adapters/session/kv.rs`,
//! and last runs its `adapters/session/last_run.rs`. One project and one
//! process: no idempotency ledger, no recovery generations, no cross-process
//! lock beyond the playground's instance lock.

use std::path::Path;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use anyhow::{Context, Result, anyhow};
use sqlx::sqlite::{SqliteConnectOptions, SqliteJournalMode, SqlitePoolOptions, SqliteSynchronous};
use sqlx::{SqliteConnection, SqlitePool};
use submilli_durable::codec::Sealer;

static MIGRATOR: sqlx::migrate::Migrator =
    sqlx::migrate!("src/commands/playground/host/sessions/migrations");

/// How long an operation waits for the connection before the busy database counts
/// as a failure, as the server's session values wait for admission.
const ADMISSION_WAIT: Duration = Duration::from_secs(30);

/// One open session as stored.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct SessionRow {
    pub(crate) id: String,
    pub(crate) blueprint_name: String,
    pub(crate) idle_timeout: Duration,
    pub(crate) last_activity: SystemTime,
    pub(crate) variables: Vec<(String, String)>,
}

/// A session's last run as stored.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum StoredLastRun {
    Kept {
        run_id: String,
        completed_at: SystemTime,
        json: Vec<u8>,
    },
    /// Over the limit: only its size is kept.
    TooLarge {
        run_id: String,
        completed_at: SystemTime,
        bytes: u64,
        limit: u64,
    },
}

/// `run_id, completed_at_unix_ms, encoded_bytes, payload, limit_bytes`.
type LastRunRow = (String, i64, i64, Option<Vec<u8>>, Option<i64>);

#[derive(Clone)]
pub(crate) struct Database {
    pool: SqlitePool,
}

impl Database {
    /// Open, or create, the database at `path` and bring its schema up to date.
    pub(crate) async fn open(path: &Path) -> Result<Self> {
        let options = SqliteConnectOptions::new()
            .filename(path)
            .create_if_missing(true)
            .journal_mode(SqliteJournalMode::Wal)
            .synchronous(SqliteSynchronous::Full)
            .foreign_keys(true)
            .busy_timeout(Duration::from_secs(5))
            .pragma("wal_autocheckpoint", "1000")
            .optimize_on_close(false, None);
        // One connection: every write is serialized, as the server's single
        // database worker serializes them.
        let pool = SqlitePoolOptions::new()
            .max_connections(1)
            .acquire_timeout(ADMISSION_WAIT)
            .connect_with(options)
            .await
            .with_context(|| format!("opening {}", path.display()))?;
        MIGRATOR
            .run(&pool)
            .await
            .with_context(|| format!("migrating {}", path.display()))?;
        Ok(Self { pool })
    }

    /// Fold the write-ahead log into the database and close the connection, so
    /// nothing is left for the next open to recover.
    pub(crate) async fn close(&self) -> Result<()> {
        let checkpoint = sqlx::query("PRAGMA wal_checkpoint(TRUNCATE)")
            .execute(&self.pool)
            .await;
        self.pool.close().await;
        checkpoint.context("checkpointing the session database")?;
        Ok(())
    }

    pub(crate) async fn insert_session(&self, row: &SessionRow) -> Result<()> {
        let mut tx = self.pool.begin().await?;
        sqlx::query(
            "INSERT INTO sessions(session_id,blueprint_name,idle_timeout_ms,last_activity_unix_ms) \
             VALUES (?,?,?,?)",
        )
        .bind(&row.id)
        .bind(&row.blueprint_name)
        .bind(millis(row.idle_timeout)?)
        .bind(unix_ms(row.last_activity)?)
        .execute(&mut *tx)
        .await?;
        for (name, value) in &row.variables {
            sqlx::query("INSERT INTO session_variables(session_id,name,value) VALUES (?,?,?)")
                .bind(&row.id)
                .bind(name)
                .bind(value)
                .execute(&mut *tx)
                .await?;
        }
        tx.commit().await?;
        Ok(())
    }

    pub(crate) async fn session(&self, id: &str) -> Result<Option<SessionRow>> {
        let mut connection = self.pool.acquire().await?;
        let row: Option<(String, String, i64, i64)> = sqlx::query_as(
            "SELECT session_id,blueprint_name,idle_timeout_ms,last_activity_unix_ms \
             FROM sessions WHERE session_id=? AND closing=0",
        )
        .bind(id)
        .fetch_optional(&mut *connection)
        .await?;
        match row {
            None => Ok(None),
            Some(row) => session_row(&mut connection, row).await.map(Some),
        }
    }

    pub(crate) async fn sessions(&self) -> Result<Vec<SessionRow>> {
        let mut connection = self.pool.acquire().await?;
        let rows: Vec<(String, String, i64, i64)> = sqlx::query_as(
            "SELECT session_id,blueprint_name,idle_timeout_ms,last_activity_unix_ms \
             FROM sessions WHERE closing=0 ORDER BY session_id",
        )
        .fetch_all(&mut *connection)
        .await?;
        let mut sessions = Vec::with_capacity(rows.len());
        for row in rows {
            sessions.push(session_row(&mut connection, row).await?);
        }
        Ok(sessions)
    }

    /// Mark the session active at `now`. Returns whether it is open.
    pub(crate) async fn touch(&self, id: &str, now: SystemTime) -> Result<bool> {
        let result = sqlx::query(
            "UPDATE sessions SET last_activity_unix_ms=? WHERE session_id=? AND closing=0",
        )
        .bind(unix_ms(now)?)
        .bind(id)
        .execute(&self.pool)
        .await?;
        Ok(result.rows_affected() > 0)
    }

    /// A session that is closing: no longer open, its cleanup not yet done.
    pub(crate) async fn closing_session(&self, id: &str) -> Result<Option<SessionRow>> {
        let mut connection = self.pool.acquire().await?;
        let row: Option<(String, String, i64, i64)> = sqlx::query_as(
            "SELECT session_id,blueprint_name,idle_timeout_ms,last_activity_unix_ms \
             FROM sessions WHERE session_id=? AND closing=1",
        )
        .bind(id)
        .fetch_optional(&mut *connection)
        .await?;
        match row {
            None => Ok(None),
            Some(row) => session_row(&mut connection, row).await.map(Some),
        }
    }

    /// The ids of every closing session.
    pub(crate) async fn closing_ids(&self) -> Result<Vec<String>> {
        Ok(sqlx::query_scalar(
            "SELECT session_id FROM sessions WHERE closing=1 ORDER BY session_id",
        )
        .fetch_all(&self.pool)
        .await?)
    }

    /// Delete the session with its variables, values and last run.
    pub(crate) async fn delete_session(&self, id: &str) -> Result<()> {
        sqlx::query("DELETE FROM sessions WHERE session_id=?")
            .bind(id)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    /// Store the session's MCP `initialize` parameters. Returns whether the
    /// session is open; a closed one gets none.
    pub(crate) async fn put_mcp_state(&self, id: &str, json: &str) -> Result<bool> {
        let result = sqlx::query(
            "INSERT INTO mcp_states(session_id,initialize_params) \
             SELECT session_id,? FROM sessions WHERE session_id=? AND closing=0 \
             ON CONFLICT(session_id) DO UPDATE SET initialize_params=excluded.initialize_params",
        )
        .bind(json)
        .bind(id)
        .execute(&self.pool)
        .await?;
        Ok(result.rows_affected() > 0)
    }

    pub(crate) async fn mcp_state(&self, id: &str) -> Result<Option<String>> {
        Ok(
            sqlx::query_scalar("SELECT initialize_params FROM mcp_states WHERE session_id=?")
                .bind(id)
                .fetch_optional(&self.pool)
                .await?,
        )
    }

    /// Store `json` as the session's last run, or only its size when it is over
    /// `limit`. A closed session gets none. Returns whether it was written.
    pub(crate) async fn put_last_run(
        &self,
        sealer: &Sealer,
        session_id: &str,
        run_id: &str,
        completed_at: SystemTime,
        json: &[u8],
        limit: u64,
    ) -> Result<bool> {
        let encoded = json.len() as u64;
        let (payload, limit_bytes) = if encoded > limit {
            (None, Some(size_to_sql(limit)?))
        } else {
            let sealed = sealer
                .seal(&last_run_owner(session_id, run_id), json)
                .map_err(|error| anyhow!("{error}"))?;
            (Some(sealed), None)
        };
        let mut tx = self.pool.begin().await?;
        if !is_open(&mut tx, session_id).await? {
            return Ok(false);
        }
        sqlx::query(
            "INSERT INTO last_runs(session_id,run_id,completed_at_unix_ms,encoded_bytes,payload,\
             limit_bytes) VALUES (?,?,?,?,?,?) ON CONFLICT(session_id) DO UPDATE SET \
             run_id=excluded.run_id,completed_at_unix_ms=excluded.completed_at_unix_ms,\
             encoded_bytes=excluded.encoded_bytes,payload=excluded.payload,\
             limit_bytes=excluded.limit_bytes",
        )
        .bind(session_id)
        .bind(run_id)
        .bind(unix_ms(completed_at)?)
        .bind(size_to_sql(encoded)?)
        .bind(payload)
        .bind(limit_bytes)
        .execute(&mut *tx)
        .await?;
        tx.commit().await?;
        Ok(true)
    }

    pub(crate) async fn remove_last_run(&self, session_id: &str) -> Result<()> {
        sqlx::query("DELETE FROM last_runs WHERE session_id=?")
            .bind(session_id)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    pub(crate) async fn last_run(
        &self,
        sealer: &Sealer,
        session_id: &str,
    ) -> Result<Option<StoredLastRun>> {
        let row: Option<LastRunRow> = sqlx::query_as(
            "SELECT run_id,completed_at_unix_ms,encoded_bytes,payload,limit_bytes \
             FROM last_runs WHERE session_id=?",
        )
        .bind(session_id)
        .fetch_optional(&self.pool)
        .await?;
        let Some((run_id, completed_at, bytes, payload, limit)) = row else {
            return Ok(None);
        };
        let completed_at = from_unix_ms(completed_at)?;
        Ok(Some(match (payload, limit) {
            (Some(payload), _) => {
                let json = sealer
                    .open(
                        &last_run_owner(session_id, &run_id),
                        &payload,
                        "the session's last run",
                    )
                    .map_err(|error| anyhow!("{error}"))?;
                StoredLastRun::Kept {
                    run_id,
                    completed_at,
                    json,
                }
            }
            (None, limit) => StoredLastRun::TooLarge {
                run_id,
                completed_at,
                bytes: size_from_sql(bytes)?,
                limit: size_from_sql(limit.unwrap_or_default())?,
            },
        }))
    }

    pub(crate) async fn begin(&self) -> Result<sqlx::Transaction<'static, sqlx::Sqlite>> {
        Ok(self.pool.begin().await?)
    }

    pub(crate) async fn acquire(&self) -> Result<sqlx::pool::PoolConnection<sqlx::Sqlite>> {
        Ok(self.pool.acquire().await?)
    }
}

async fn session_row(
    connection: &mut SqliteConnection,
    (id, blueprint_name, idle_timeout_ms, last_activity_ms): (String, String, i64, i64),
) -> Result<SessionRow> {
    let variables: Vec<(String, String)> =
        sqlx::query_as("SELECT name,value FROM session_variables WHERE session_id=? ORDER BY name")
            .bind(&id)
            .fetch_all(connection)
            .await?;
    Ok(SessionRow {
        id,
        blueprint_name,
        idle_timeout: Duration::from_millis(size_from_sql(idle_timeout_ms)?),
        last_activity: from_unix_ms(last_activity_ms)?,
        variables,
    })
}

/// Whether the session is open. A closing or closed one's state is going or gone,
/// and a write must not bring it back.
pub(crate) async fn is_open(connection: &mut SqliteConnection, session_id: &str) -> Result<bool> {
    Ok(
        sqlx::query_scalar(
            "SELECT EXISTS(SELECT 1 FROM sessions WHERE session_id=? AND closing=0)",
        )
        .bind(session_id)
        .fetch_one(connection)
        .await?,
    )
}

pub(crate) mod kv {
    //! Key-value rows. Transactions belong to the caller.
    //!
    //! Keys and payloads are UTF-16 code units, stored big-endian: exact for lone
    //! surrogates, and the byte order SQLite compares blobs in is then code-unit
    //! order, which is the order `list` pages in. A payload is sealed bound to its
    //! session and key.

    use anyhow::{Result, anyhow, bail};
    use interpreter::runtime::SessionKvUsage;
    use sqlx::SqliteConnection;
    use submilli_durable::codec::Sealer;

    use super::{size_from_sql, size_to_sql};

    pub(crate) async fn get(
        connection: &mut SqliteConnection,
        sealer: &Sealer,
        session_id: &str,
        key: &[u16],
    ) -> Result<Option<Vec<u16>>> {
        let stored_key = units_to_bytes(key);
        let payload: Option<Vec<u8>> =
            sqlx::query_scalar("SELECT payload FROM session_kv WHERE session_id=? AND key=?")
                .bind(session_id)
                .bind(&stored_key)
                .fetch_optional(connection)
                .await?;
        let Some(payload) = payload else {
            return Ok(None);
        };
        let plain = sealer
            .open(&owner(session_id, &stored_key), &payload, &entry_label(key))
            .map_err(|error| anyhow!("{error}"))?;
        bytes_to_units(&plain, &entry_label(key)).map(Some)
    }

    pub(crate) async fn has(
        connection: &mut SqliteConnection,
        session_id: &str,
        key: &[u16],
    ) -> Result<bool> {
        Ok(sqlx::query_scalar(
            "SELECT EXISTS(SELECT 1 FROM session_kv WHERE session_id=? AND key=?)",
        )
        .bind(session_id)
        .bind(units_to_bytes(key))
        .fetch_one(connection)
        .await?)
    }

    pub(crate) async fn usage(
        connection: &mut SqliteConnection,
        session_id: &str,
        key: &[u16],
    ) -> Result<SessionKvUsage> {
        let (entries, bytes): (i64, i64) = sqlx::query_as(
            "SELECT COUNT(*), COALESCE(SUM(retained_bytes), 0) FROM session_kv WHERE session_id=?",
        )
        .bind(session_id)
        .fetch_one(&mut *connection)
        .await?;
        let replaced: Option<i64> = sqlx::query_scalar(
            "SELECT retained_bytes FROM session_kv WHERE session_id=? AND key=?",
        )
        .bind(session_id)
        .bind(units_to_bytes(key))
        .fetch_optional(connection)
        .await?;
        Ok(SessionKvUsage {
            entries: size_from_sql(entries)?,
            bytes: size_from_sql(bytes)?,
            replaced: replaced.map(size_from_sql).transpose()?,
        })
    }

    pub(crate) async fn put(
        connection: &mut SqliteConnection,
        sealer: &Sealer,
        session_id: &str,
        key: &[u16],
        payload: &[u16],
        (value_bytes, retained_bytes): (u64, u64),
    ) -> Result<()> {
        let stored_key = units_to_bytes(key);
        let sealed = sealer
            .seal(&owner(session_id, &stored_key), &units_to_bytes(payload))
            .map_err(|error| anyhow!("{error}"))?;
        sqlx::query(
            "INSERT INTO session_kv(session_id,key,payload,value_bytes,retained_bytes) \
             VALUES (?,?,?,?,?) ON CONFLICT(session_id,key) DO UPDATE SET \
             payload=excluded.payload,value_bytes=excluded.value_bytes,\
             retained_bytes=excluded.retained_bytes",
        )
        .bind(session_id)
        .bind(stored_key)
        .bind(sealed)
        .bind(size_to_sql(value_bytes)?)
        .bind(size_to_sql(retained_bytes)?)
        .execute(connection)
        .await?;
        Ok(())
    }

    pub(crate) async fn remove(
        connection: &mut SqliteConnection,
        session_id: &str,
        key: &[u16],
    ) -> Result<bool> {
        let result = sqlx::query("DELETE FROM session_kv WHERE session_id=? AND key=?")
            .bind(session_id)
            .bind(units_to_bytes(key))
            .execute(connection)
            .await?;
        Ok(result.rows_affected() > 0)
    }

    /// Up to `max_keys` keys after `after`, in code-unit order, with each value's
    /// size, and whether more remain. Never opens a value.
    pub(crate) async fn keys(
        connection: &mut SqliteConnection,
        session_id: &str,
        after: Option<&[u16]>,
        max_keys: usize,
    ) -> Result<(Vec<(Vec<u16>, u64)>, bool)> {
        // One row past the bound says whether keys remain after it.
        let limit = i64::try_from(max_keys.saturating_add(1))?;
        let rows: Vec<(Vec<u8>, i64)> = sqlx::query_as(
            "SELECT key, value_bytes FROM session_kv WHERE session_id=? AND key>? \
             ORDER BY key LIMIT ?",
        )
        .bind(session_id)
        // An empty blob sorts before every key, and the table never stores an empty one.
        .bind(after.map(units_to_bytes).unwrap_or_default())
        .bind(limit)
        .fetch_all(connection)
        .await?;
        let more = rows.len() > max_keys;
        let keys = rows
            .into_iter()
            .take(max_keys)
            .map(|(key, size)| Ok((bytes_to_units(&key, "session key")?, size_from_sql(size)?)))
            .collect::<Result<_>>()?;
        Ok((keys, more))
    }

    fn owner(session_id: &str, key: &[u8]) -> String {
        format!(
            "submilli:session-kv:v1:{session_id}:{}",
            crate::commands::playground::state::hex(key)
        )
    }

    fn entry_label(key: &[u16]) -> String {
        format!("session value '{}'", String::from_utf16_lossy(key))
    }

    fn units_to_bytes(units: &[u16]) -> Vec<u8> {
        units.iter().flat_map(|unit| unit.to_be_bytes()).collect()
    }

    fn bytes_to_units(bytes: &[u8], what: &str) -> Result<Vec<u16>> {
        let (pairs, rest) = bytes.as_chunks::<2>();
        if !rest.is_empty() {
            bail!("{what} is not whole UTF-16 code units");
        }
        Ok(pairs.iter().map(|pair| u16::from_be_bytes(*pair)).collect())
    }
}

fn last_run_owner(session_id: &str, run_id: &str) -> String {
    format!("submilli:last-run:v1:{session_id}:{run_id}")
}

fn millis(duration: Duration) -> Result<i64> {
    Ok(i64::try_from(duration.as_millis())?)
}

fn unix_ms(time: SystemTime) -> Result<i64> {
    millis(time.duration_since(UNIX_EPOCH).unwrap_or_default())
}

fn from_unix_ms(ms: i64) -> Result<SystemTime> {
    Ok(UNIX_EPOCH + Duration::from_millis(size_from_sql(ms)?))
}

fn size_to_sql(bytes: u64) -> Result<i64> {
    i64::try_from(bytes).map_err(|_| anyhow!("session size overflow"))
}

fn size_from_sql(bytes: i64) -> Result<u64> {
    u64::try_from(bytes).map_err(|_| anyhow!("negative session size"))
}
