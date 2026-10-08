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
//!   sessions.jsonl      sessions the playground started, and when each ended
//!   links.jsonl         runs the playground ran again live, linked to their source
//!   running/<id>.json   runs in flight, removed as each finishes
//! ```
//!
//! The directory is 0700 and every file 0600. Each file carries the format version it
//! was written in, and a reader refuses one newer than it knows with an upgrade
//! message. Nothing reaches disk before KTD19's redaction (see [`redact`]).
//!
//! One process writes the store: the running playground, which opens it with
//! [`Store::open`]. That open also repairs what a crash left: temporary files, a line cut
//! short at the end of an append-only file, and an index that disagrees with the run
//! files. Any other process (a control reading runs or events) opens it with
//! [`Store::open_read_only`], which never changes a file, and reads around the same
//! damage instead.
//!
//! Every line of an append-only file is read on its own: a line that does not parse
//! (a write cut short by a crash or a full disk) is skipped and counted, never fatal, and
//! the next append starts on a line of its own. A line in a newer format is still
//! refused.

// The readers and the change log's writers are the store's API for the controls and the
// blueprint watcher; the running playground itself only records.
#![allow(
    dead_code,
    reason = "read by the controls (U10, U17) and the watcher (U8)"
)]

use std::collections::{BTreeMap, BTreeSet};
use std::fs::{self, OpenOptions};
use std::io::{self, Write};
use std::os::unix::fs::{FileExt, OpenOptionsExt};
use std::path::{Path, PathBuf};
use std::sync::{Mutex, MutexGuard, PoisonError};

use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};

use super::fsx::{self, STAGED_PREFIX};
use super::log::warn;

pub(crate) mod changes;
pub(crate) mod events;
pub(crate) mod links;
pub(crate) mod recorder;
pub(crate) mod redact;
pub(crate) mod run;
pub(crate) mod running;
pub(crate) mod sessions;

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
    /// A write through a store opened with [`Store::open_read_only`].
    ReadOnly {
        path: PathBuf,
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
            Self::ReadOnly { path } => write!(
                f,
                "{} was opened read-only; only the running playground writes it",
                path.display()
            ),
        }
    }
}

impl std::error::Error for StoreError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Io { source, .. } => Some(source),
            Self::NewerFormat { .. } | Self::Corrupt { .. } | Self::ReadOnly { .. } => None,
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
    /// Opened by the writer ([`Store::open`]) rather than [`Store::open_read_only`].
    writable: bool,
    /// Serializes run id assignment, index and retry appends, and `clear`.
    inner: Mutex<Inner>,
    /// Serializes change-log appends, which assign version numbers.
    changes: Mutex<()>,
    pub(crate) events: events::Appender,
}

struct Inner {
    last_run: u64,
}

impl Store {
    /// Opens the store at `root` for writing, creating it when absent. Refuses a store
    /// written in a newer format.
    ///
    /// Only the running playground, the store's one writer, opens it this way: the open
    /// repairs what a crash left (temporary files, a cut-short last line of an
    /// append-only file, an index that disagrees with the run files), which would race a
    /// writer still running. Every other process uses [`Store::open_read_only`].
    pub(crate) fn open(root: &Path) -> Result<Self> {
        private_dir_all(root)?;
        private_dir_all(&root.join("runs"))?;
        private_dir_all(&root.join("events"))?;
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
        let store = Self::unopened(root, true);
        store.remove_staged()?;
        store.clear_running()?;
        store.repair_tails()?;
        let last_run = store.recover_last_run()?;
        store.lock().last_run = last_run;
        store.reconcile_index()?;
        Ok(store)
    }

    /// Opens an existing store for reading only, from any process, while the playground
    /// may be writing it. Changes no file: a crash's leftovers are read around, not
    /// repaired, and a write through it fails with [`StoreError::ReadOnly`]. Refuses a
    /// store written in a newer format; a missing store is an I/O error (not found).
    pub(crate) fn open_read_only(root: &Path) -> Result<Self> {
        let marker = root.join("store.json");
        let bytes = fs::read(&marker).map_err(io_error(&marker))?;
        let _: StoreMarker = parse_record(&marker, &bytes)?;
        let store = Self::unopened(root, false);
        let last_run = store.recover_last_run()?;
        store.lock().last_run = last_run;
        Ok(store)
    }

    fn unopened(root: &Path, writable: bool) -> Self {
        Self {
            root: root.to_path_buf(),
            writable,
            inner: Mutex::new(Inner { last_run: 0 }),
            changes: Mutex::new(()),
            events: events::Appender::new(root.join("events"), writable),
        }
    }

    /// Fails a write through a read-only store.
    fn writer(&self) -> Result<()> {
        if self.writable {
            Ok(())
        } else {
            Err(StoreError::ReadOnly {
                path: self.root.clone(),
            })
        }
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
        self.writer()?;
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
        self.writer()?;
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

    /// Replaces a stored run's file in one rename and leaves its index line as it is: for
    /// what is kept with a run after it was recorded (a test run's report), which changes
    /// nothing its summary holds. The run must already be redacted. A run removed since
    /// it was loaded (by [`Store::clear`]) stays removed.
    pub(crate) fn rewrite_run(&self, run: &StoredRun) -> Result<()> {
        self.writer()?;
        let path = self.run_path(run.id);
        let staged = stage(&path, &json_bytes(run))?;
        // Checked under the lock `clear` removes runs under, so it cannot remove the
        // file between this check and the rename.
        let _inner = self.lock();
        match fs::symlink_metadata(&path) {
            Ok(_) => {}
            Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(()),
            Err(error) => return Err(io_error(&path)(error)),
        }
        staged
            .persist(&path)
            .map(drop)
            .map_err(|error| StoreError::Io {
                path: path.clone(),
                source: error.error,
            })
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

    /// Every stored run's summary, by id. Reads the index, so it stays fast however
    /// large the runs are; a run whose file is gone is left out, and one the index lacks
    /// (a crash between a run's rename and its index line, or an index line that did not
    /// parse) is read from its file. A run file that does not parse is left out.
    pub(crate) fn list_runs(&self) -> Result<Vec<RunSummary>> {
        let present: BTreeSet<u64> = self.run_ids()?.into_iter().collect();
        let mut summaries: BTreeMap<u64, RunSummary> = BTreeMap::new();
        let index = match read_lines::<RunSummary>(&self.index_path()) {
            Ok(index) => index.records,
            Err(error @ StoreError::NewerFormat { .. }) => return Err(error),
            // Derived from the run files, so they answer instead.
            Err(_) => Vec::new(),
        };
        for summary in index {
            if present.contains(&summary.id) {
                summaries.insert(summary.id, summary);
            }
        }
        for id in present {
            if summaries.contains_key(&id) {
                continue;
            }
            if let Some(run) = self.load_run_tolerant(id)? {
                summaries.insert(id, RunSummary::of(&run));
            }
        }
        Ok(summaries.into_values().collect())
    }

    /// A stored run, or `None` when there is none or its file does not parse.
    fn load_run_tolerant(&self, id: u64) -> Result<Option<StoredRun>> {
        match self.load_run(id) {
            Err(StoreError::Corrupt { path, message }) => {
                if self.writable {
                    warn(&format!(
                        "{} is not a valid run file, so it is not listed: {message}",
                        path.display()
                    ));
                }
                Ok(None)
            }
            other => other,
        }
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

    /// Removes every run file, the index, and every session's event log, and appends a
    /// clear marker naming the last id handed out, which also starts a new audit window.
    /// Ids and each session's event numbers keep counting from there, a run already in
    /// progress is stored when it finishes, and the started-session log stays, so open
    /// sessions are still listed. Returns how many runs were removed.
    pub(crate) fn clear(&self) -> Result<usize> {
        self.writer()?;
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
        self.events.remove_logs()?;
        Ok(ids.len())
    }

    /// An idempotent retry answered from the ledger: kept as a link to the run it
    /// repeated, not as a run.
    pub(crate) fn record_retry(&self, retry: &run::RetryRecord) -> Result<()> {
        self.writer()?;
        let _inner = self.lock();
        append_line(&self.retries_path(), &json_line(retry))
    }

    /// The retries recorded, without any line that does not parse.
    pub(crate) fn retries(&self) -> Result<Vec<run::RetryRecord>> {
        read_lines(&self.retries_path()).map(|lines| lines.records)
    }

    /// Temporary files a crash left behind: a run's in `runs/`, and the sequence's,
    /// index's, or marker's in the store's root.
    fn remove_staged(&self) -> Result<()> {
        for dir in [self.root.clone(), self.runs_dir()] {
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
        }
        Ok(())
    }

    /// Cuts the unfinished last line, which a crash left, off every append-only file.
    fn repair_tails(&self) -> Result<()> {
        let mut files = vec![
            self.index_path(),
            self.changes_path(),
            self.retries_path(),
            self.sessions_path(),
            self.links_path(),
        ];
        let events = self.root.join("events");
        for entry in fs::read_dir(&events).map_err(io_error(&events))? {
            let path = entry.map_err(io_error(&events))?.path();
            if path
                .extension()
                .is_some_and(|extension| extension == "jsonl")
            {
                files.push(path);
            }
        }
        for path in files {
            cut_unfinished_line(&path)?;
        }
        Ok(())
    }

    /// Makes the index agree with the run files. A crash between a run's rename and its
    /// index line leaves lines missing, which are appended. An index that does not parse,
    /// or names a run whose file is gone, is rebuilt from the run files: it is derived
    /// from them, so it never keeps the store from opening.
    fn reconcile_index(&self) -> Result<()> {
        let present: BTreeSet<u64> = self.run_ids()?.into_iter().collect();
        let indexed = match read_lines::<RunSummary>(&self.index_path()) {
            Ok(lines) if lines.skipped == 0 => {
                let ids: Vec<u64> = lines.records.iter().map(|summary| summary.id).collect();
                let unique: BTreeSet<u64> = ids.iter().copied().collect();
                (unique.len() == ids.len() && unique.is_subset(&present)).then_some(unique)
            }
            Ok(_) => None,
            Err(error @ StoreError::NewerFormat { .. }) => return Err(error),
            Err(_) => None,
        };
        if let Some(indexed) = indexed {
            for id in present.difference(&indexed) {
                if let Some(run) = self.load_run_tolerant(*id)? {
                    append_line(&self.index_path(), &json_line(&RunSummary::of(&run)))?;
                }
            }
        } else {
            let mut index = Vec::new();
            for id in &present {
                if let Some(run) = self.load_run_tolerant(*id)? {
                    index.extend(json_line(&RunSummary::of(&run)));
                }
            }
            write_private(&self.index_path(), &index)?;
        }
        Ok(())
    }
}

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

/// Creates `path` (and its parents) as a 0700 directory, or tightens an existing one to
/// 0700.
fn private_dir_all(path: &Path) -> Result<()> {
    fsx::create_private_dir_all(path).map_err(io_error(path))
}

/// `bytes` in a 0600 temporary file beside `path`, synced, ready to be renamed over it.
fn stage(path: &Path, bytes: &[u8]) -> Result<tempfile::NamedTempFile> {
    let dir = path.parent().unwrap_or_else(|| Path::new("."));
    fsx::stage(dir, bytes).map_err(io_error(path))
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

/// Appends one whole line in a single write to a 0600 file opened for appending; see
/// [`open_for_append`].
fn append_line(path: &Path, line: &[u8]) -> Result<()> {
    let (mut file, _) = open_for_append(path)?;
    file.write_all(line).map_err(io_error(path))
}

/// `path` opened for appending, as a 0600 file, and whether it was repaired. When the file
/// does not end in a newline (an earlier write was cut short), one is written first, so
/// the cut line stays a line of its own and the next one still reads. A cut line that is
/// whole but for its newline then reads as a complete line too.
fn open_for_append(path: &Path) -> Result<(fs::File, bool)> {
    let mut file = OpenOptions::new()
        .create(true)
        .read(true)
        .append(true)
        .mode(0o600)
        .open(path)
        .map_err(io_error(path))?;
    let len = file.metadata().map_err(io_error(path))?.len();
    let unfinished = match len.checked_sub(1) {
        Some(last) => {
            let mut byte = [0_u8];
            file.read_exact_at(&mut byte, last)
                .map_err(io_error(path))?;
            byte != *b"\n"
        }
        None => false,
    };
    if unfinished {
        file.write_all(b"\n").map_err(io_error(path))?;
    }
    Ok((file, unfinished))
}

/// Truncates `path` after its last newline, when it has bytes after it. Only the writer
/// calls it, on opening, when no write can be in progress.
fn cut_unfinished_line(path: &Path) -> Result<()> {
    let bytes = match fs::read(path) {
        Ok(bytes) => bytes,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(()),
        Err(error) => return Err(io_error(path)(error)),
    };
    if bytes.is_empty() || bytes.ends_with(b"\n") {
        return Ok(());
    }
    let keep = bytes
        .iter()
        .rposition(|byte| *byte == b'\n')
        .map_or(0, |end| end.saturating_add(1));
    let file = OpenOptions::new()
        .write(true)
        .open(path)
        .map_err(io_error(path))?;
    file.set_len(keep as u64).map_err(io_error(path))?;
    file.sync_all().map_err(io_error(path))
}

/// The parsed lines of an append-only file, and how many lines did not parse.
pub(crate) struct Lines<T> {
    pub(crate) records: Vec<T>,
    pub(crate) skipped: usize,
}

/// The complete lines of an append-only file, parsed. A last line without its newline
/// is still being written and is left for the next read. A line that does not parse is
/// skipped and counted; a line in a newer format is refused.
fn read_lines<T: DeserializeOwned>(path: &Path) -> Result<Lines<T>> {
    let bytes = match fs::read(path) {
        Ok(bytes) => bytes,
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            return Ok(Lines {
                records: Vec::new(),
                skipped: 0,
            });
        }
        Err(error) => return Err(io_error(path)(error)),
    };
    let mut lines = Lines {
        records: Vec::new(),
        skipped: 0,
    };
    for line in complete_lines(&bytes).filter(|line| !line.is_empty()) {
        match parse_record(path, line) {
            Ok(record) => lines.records.push(record),
            Err(StoreError::Corrupt { .. }) => lines.skipped = lines.skipped.saturating_add(1),
            Err(error) => return Err(error),
        }
    }
    Ok(lines)
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
