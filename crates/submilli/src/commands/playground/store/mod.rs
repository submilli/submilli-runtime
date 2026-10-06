//! The run store: every run the playground serves, its sessions' events, and the
//! blueprint change log, under `<project>/.submilli/playground/store/` (KTD20).
//!
//! ```text
//! store/
//!   store.json          the store's format version
//!   sequence.json       the last run id handed out, so ids are never reused
//!   index.jsonl         one summary line per stored run, for listing without parsing runs
//!   runs/<id>.json      one file per run, written to a temporary file and renamed
//!   events/<session>.jsonl  each session's events, appended as they arrive
//!   changes.jsonl       the change log: blueprint versions and clear markers
//!   retries.jsonl       idempotent retries answered without running
//! ```
//!
//! The directory is 0700 and every file 0600. Each file carries the format version it
//! was written in, and a reader refuses one newer than it knows with an upgrade
//! message. Nothing reaches disk before KTD19's redaction (see [`redact`]).

// The readers and the change log's writers are the store's API for the controls and the
// blueprint watcher; the running playground itself only records.
#![allow(
    dead_code,
    reason = "read by the controls (U10, U17) and the watcher (U8)"
)]

use std::fs::{self, OpenOptions};
use std::io::{self, Write};
use std::os::unix::fs::{DirBuilderExt, OpenOptionsExt, PermissionsExt};
use std::path::{Path, PathBuf};
use std::sync::{Mutex, MutexGuard, PoisonError};

use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};

pub(crate) mod changes;
pub(crate) mod events;
pub(crate) mod recorder;
pub(crate) mod redact;
pub(crate) mod run;

#[cfg(test)]
mod tests;

use changes::ChangeEntry;
pub(crate) use recorder::Recorder;
pub(crate) use redact::KnownSecrets;
use run::{RunSummary, StoredRun};

/// The format this build writes, and the newest it reads.
pub(crate) const FORMAT: u32 = 1;

/// Why the store could not be read or written.
#[derive(Debug)]
pub(crate) enum StoreError {
    /// A file written by a newer submilli.
    NewerFormat {
        path: PathBuf,
        found: u32,
    },
    Io {
        path: PathBuf,
        source: io::Error,
    },
    /// A file that does not parse as what it should hold.
    Corrupt {
        path: PathBuf,
        message: String,
    },
}

impl std::fmt::Display for StoreError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NewerFormat { path, found } => write!(
                f,
                "{} was written by a newer submilli (store format {found}; this one reads \
                 format {FORMAT}); upgrade submilli to read it",
                path.display()
            ),
            Self::Io { path, source } => write!(f, "{}: {source}", path.display()),
            Self::Corrupt { path, message } => {
                write!(f, "{} is not a valid store file: {message}", path.display())
            }
        }
    }
}

impl std::error::Error for StoreError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Io { source, .. } => Some(source),
            Self::NewerFormat { .. } | Self::Corrupt { .. } => None,
        }
    }
}

pub(crate) type Result<T, E = StoreError> = std::result::Result<T, E>;

fn io_error(path: &Path) -> impl FnOnce(io::Error) -> StoreError + '_ {
    move |source| StoreError::Io {
        path: path.to_path_buf(),
        source,
    }
}

/// Just the format a file or line declares.
#[derive(Deserialize)]
struct FormatProbe {
    format: u32,
}

/// Parses one store record, refusing a newer format before reading the rest of it.
fn parse_record<T: DeserializeOwned>(path: &Path, bytes: &[u8]) -> Result<T> {
    let corrupt = |error: serde_json::Error| StoreError::Corrupt {
        path: path.to_path_buf(),
        message: error.to_string(),
    };
    let probe: FormatProbe = serde_json::from_slice(bytes).map_err(corrupt)?;
    if probe.format > FORMAT {
        return Err(StoreError::NewerFormat {
            path: path.to_path_buf(),
            found: probe.format,
        });
    }
    serde_json::from_slice(bytes).map_err(corrupt)
}

#[derive(Serialize, Deserialize)]
struct StoreMarker {
    format: u32,
}

#[derive(Serialize, Deserialize)]
struct Sequence {
    format: u32,
    /// The last run id handed out.
    last_run: u64,
}

/// The store. One per playground process writes it; any number of readers, in that
/// process or another, may read it at the same time.
pub(crate) struct Store {
    root: PathBuf,
    /// Serializes run id assignment, index appends, and `clear`.
    inner: Mutex<Inner>,
    /// Serializes change-log appends, which assign version numbers.
    changes: Mutex<()>,
    pub(crate) events: events::Appender,
}

struct Inner {
    last_run: u64,
}

impl Store {
    /// Opens the store at `root`, creating it when absent. Refuses a store written in a
    /// newer format.
    pub(crate) fn open(root: &Path) -> Result<Self> {
        private_dir(root)?;
        private_dir(&root.join("runs"))?;
        private_dir(&root.join("events"))?;
        let marker = root.join("store.json");
        match fs::read(&marker) {
            Ok(bytes) => {
                let _: StoreMarker = parse_record(&marker, &bytes)?;
            }
            Err(error) if error.kind() == io::ErrorKind::NotFound => {
                write_private(&marker, &json_bytes(&StoreMarker { format: FORMAT }))?;
            }
            Err(error) => return Err(io_error(&marker)(error)),
        }
        let store = Self {
            root: root.to_path_buf(),
            inner: Mutex::new(Inner { last_run: 0 }),
            changes: Mutex::new(()),
            events: events::Appender::new(root.join("events")),
        };
        store.remove_staged()?;
        let last_run = store.recover_last_run()?;
        store.lock().last_run = last_run;
        store.reconcile_index()?;
        Ok(store)
    }

    pub(crate) fn root(&self) -> &Path {
        &self.root
    }

    fn lock(&self) -> MutexGuard<'_, Inner> {
        self.inner.lock().unwrap_or_else(PoisonError::into_inner)
    }

    fn runs_dir(&self) -> PathBuf {
        self.root.join("runs")
    }

    pub(crate) fn run_path(&self, id: u64) -> PathBuf {
        self.runs_dir().join(format!("{id}.json"))
    }

    fn index_path(&self) -> PathBuf {
        self.root.join("index.jsonl")
    }

    fn sequence_path(&self) -> PathBuf {
        self.root.join("sequence.json")
    }

    pub(crate) fn changes_path(&self) -> PathBuf {
        self.root.join("changes.jsonl")
    }

    fn retries_path(&self) -> PathBuf {
        self.root.join("retries.jsonl")
    }

    /// The last run id handed out: the highest of the persisted sequence, any run file,
    /// and the latest clear marker's high-water mark, so an id is never reused after a
    /// clear, a restart, or a lost sequence file.
    fn recover_last_run(&self) -> Result<u64> {
        let path = self.sequence_path();
        let from_sequence = match fs::read(&path) {
            Ok(bytes) => parse_record::<Sequence>(&path, &bytes)?.last_run,
            Err(error) if error.kind() == io::ErrorKind::NotFound => 0,
            Err(error) => return Err(io_error(&path)(error)),
        };
        let from_files = self.run_ids()?.into_iter().max().unwrap_or(0);
        let from_clears = self.changes()?.high_water();
        Ok(from_sequence.max(from_files).max(from_clears))
    }

    /// Hands out the next run id and persists it before returning, so a restart never
    /// hands it out again.
    pub(crate) fn next_run_id(&self) -> Result<u64> {
        let mut inner = self.lock();
        let id = inner.last_run.saturating_add(1);
        write_private(
            &self.sequence_path(),
            &json_bytes(&Sequence {
                format: FORMAT,
                last_run: id,
            }),
        )?;
        inner.last_run = id;
        Ok(id)
    }

    pub(crate) fn last_run_id(&self) -> u64 {
        self.lock().last_run
    }

    /// Writes a run: its file, by temporary file and rename, then its index line. The
    /// run must already be redacted.
    pub(crate) fn write_run(&self, run: &StoredRun) -> Result<()> {
        let bytes = json_bytes(run);
        let summary = json_line(&RunSummary::of(run));
        let path = self.run_path(run.id);
        let staged = stage(&path, &bytes)?;
        let _inner = self.lock();
        staged.persist(&path).map_err(|error| StoreError::Io {
            path: path.clone(),
            source: error.error,
        })?;
        append_line(&self.index_path(), &summary)
    }

    /// A stored run, or `None` when there is none with that id.
    pub(crate) fn load_run(&self, id: u64) -> Result<Option<StoredRun>> {
        let path = self.run_path(id);
        match fs::read(&path) {
            Ok(bytes) => parse_record(&path, &bytes).map(Some),
            Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(None),
            Err(error) => Err(io_error(&path)(error)),
        }
    }

    /// Every stored run's summary, by id. Reads the index only, so it stays fast however
    /// large the runs are; a run whose file is gone is left out.
    pub(crate) fn list_runs(&self) -> Result<Vec<RunSummary>> {
        let present: std::collections::BTreeSet<u64> = self.run_ids()?.into_iter().collect();
        let mut summaries: std::collections::BTreeMap<u64, RunSummary> =
            std::collections::BTreeMap::new();
        for summary in read_lines::<RunSummary>(&self.index_path())? {
            if present.contains(&summary.id) {
                summaries.insert(summary.id, summary);
            }
        }
        Ok(summaries.into_values().collect())
    }

    /// The store's run id for a server `execution_id`, when that run is stored.
    pub(crate) fn run_id_of(&self, execution_id: &str) -> Result<Option<u64>> {
        Ok(self
            .list_runs()?
            .into_iter()
            .find(|summary| summary.execution_id == execution_id)
            .map(|summary| summary.id))
    }

    /// Ids of the run files present, in no particular order.
    fn run_ids(&self) -> Result<Vec<u64>> {
        let dir = self.runs_dir();
        let entries = fs::read_dir(&dir).map_err(io_error(&dir))?;
        let mut ids = Vec::new();
        for entry in entries {
            let entry = entry.map_err(io_error(&dir))?;
            let name = entry.file_name();
            let Some(id) = name
                .to_str()
                .and_then(|name| name.strip_suffix(".json"))
                .and_then(|stem| stem.parse::<u64>().ok())
            else {
                continue;
            };
            ids.push(id);
        }
        Ok(ids)
    }

    /// Removes every run file and the index, and appends a clear marker naming the last
    /// id handed out, which also starts a new audit window. Ids keep counting from there,
    /// and a run already in progress is stored when it finishes. Returns how many runs
    /// were removed.
    pub(crate) fn clear(&self) -> Result<usize> {
        let inner = self.lock();
        let ids = self.run_ids()?;
        for id in &ids {
            let path = self.run_path(*id);
            match fs::remove_file(&path) {
                Ok(()) => {}
                Err(error) if error.kind() == io::ErrorKind::NotFound => {}
                Err(error) => return Err(io_error(&path)(error)),
            }
        }
        write_private(&self.index_path(), b"")?;
        self.append_change(ChangeEntry::Clear {
            high_water: inner.last_run,
        })?;
        Ok(ids.len())
    }

    /// An idempotent retry answered from the ledger: kept as a link to the run it
    /// repeated, not as a run.
    pub(crate) fn record_retry(&self, retry: &run::RetryRecord) -> Result<()> {
        append_line(&self.retries_path(), &json_line(retry))
    }

    pub(crate) fn retries(&self) -> Result<Vec<run::RetryRecord>> {
        read_lines(&self.retries_path())
    }

    /// Temporary files a crash left behind.
    fn remove_staged(&self) -> Result<()> {
        let dir = self.runs_dir();
        for entry in fs::read_dir(&dir).map_err(io_error(&dir))? {
            let entry = entry.map_err(io_error(&dir))?;
            if entry
                .file_name()
                .to_str()
                .is_some_and(|name| name.starts_with(STAGED_PREFIX))
            {
                let _ = fs::remove_file(entry.path());
            }
        }
        Ok(())
    }

    /// Adds index lines for run files that have none: a crash between a run's rename and
    /// its index append.
    fn reconcile_index(&self) -> Result<()> {
        let indexed: std::collections::BTreeSet<u64> =
            read_lines::<RunSummary>(&self.index_path())?
                .into_iter()
                .map(|summary| summary.id)
                .collect();
        let mut missing: Vec<u64> = self
            .run_ids()?
            .into_iter()
            .filter(|id| !indexed.contains(id))
            .collect();
        missing.sort_unstable();
        for id in missing {
            if let Some(run) = self.load_run(id)? {
                append_line(&self.index_path(), &json_line(&RunSummary::of(&run)))?;
            }
        }
        Ok(())
    }
}

const STAGED_PREFIX: &str = ".staged-";

fn json_bytes<T: Serialize>(value: &T) -> Vec<u8> {
    // Every type the store writes serializes: plain fields, string-keyed maps, and JSON
    // values, none of which can fail.
    serde_json::to_vec(value).expect("store records serialize")
}

fn json_line<T: Serialize>(value: &T) -> Vec<u8> {
    let mut line = json_bytes(value);
    line.push(b'\n');
    line
}

/// Creates `path` as a 0700 directory, or tightens an existing one to 0700.
fn private_dir(path: &Path) -> Result<()> {
    match fs::DirBuilder::new()
        .mode(0o700)
        .recursive(true)
        .create(path)
    {
        Ok(()) => {
            fs::set_permissions(path, fs::Permissions::from_mode(0o700)).map_err(io_error(path))
        }
        Err(error) => Err(io_error(path)(error)),
    }
}

/// `bytes` in a 0600 temporary file beside `path`, synced, ready to be renamed over it.
fn stage(path: &Path, bytes: &[u8]) -> Result<tempfile::NamedTempFile> {
    let dir = path.parent().unwrap_or_else(|| Path::new("."));
    let mut staged = tempfile::Builder::new()
        .prefix(STAGED_PREFIX)
        .permissions(fs::Permissions::from_mode(0o600))
        .tempfile_in(dir)
        .map_err(io_error(path))?;
    staged.write_all(bytes).map_err(io_error(path))?;
    staged.as_file().sync_all().map_err(io_error(path))?;
    Ok(staged)
}

/// Replaces `path` with `bytes` in one rename, so a reader never sees a partial file.
fn write_private(path: &Path, bytes: &[u8]) -> Result<()> {
    stage(path, bytes)?
        .persist(path)
        .map(drop)
        .map_err(|error| StoreError::Io {
            path: path.to_path_buf(),
            source: error.error,
        })
}

/// Appends one whole line in a single write to a 0600 file opened for appending.
fn append_line(path: &Path, line: &[u8]) -> Result<()> {
    let mut file = OpenOptions::new()
        .create(true)
        .append(true)
        .mode(0o600)
        .open(path)
        .map_err(io_error(path))?;
    file.write_all(line).map_err(io_error(path))
}

/// The complete lines of an append-only file, parsed. A last line without its newline
/// is still being written and is left for the next read.
fn read_lines<T: DeserializeOwned>(path: &Path) -> Result<Vec<T>> {
    let bytes = match fs::read(path) {
        Ok(bytes) => bytes,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(error) => return Err(io_error(path)(error)),
    };
    complete_lines(&bytes)
        .filter(|line| !line.is_empty())
        .map(|line| parse_record(path, line))
        .collect()
}

/// The newline-terminated lines of `bytes`, without their newlines.
fn complete_lines(bytes: &[u8]) -> impl Iterator<Item = &[u8]> {
    let complete = bytes
        .iter()
        .rposition(|byte| *byte == b'\n')
        .map_or(&[][..], |end| bytes.get(..end).unwrap_or_default());
    complete.split(|byte| *byte == b'\n')
}

fn now_micros() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |elapsed| {
            u64::try_from(elapsed.as_micros()).unwrap_or(u64::MAX)
        })
}
