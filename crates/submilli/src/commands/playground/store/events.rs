//! Each session's event log: `events/<session>.jsonl`, one event per line, appended and
//! written out as each arrives (KTD24).
//!
//! The server numbers events across all sessions; the store numbers each session's
//! events again from 1 with no gaps, and that number is the reader's resume cursor. Each
//! event also carries its causal position (when it happened on the playground's clock,
//! then its run and run-wide call index), which is the order to show it in: an event
//! recovered from a run's record after the server dropped it sorts where it happened,
//! not where it was appended.
//!
//! When the server's event buffer overflowed during a run, the log holds a gap entry for
//! it, and the events the run's record still has are appended after it, marked
//! backfilled.

use std::collections::HashMap;
use std::io::Write as _;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, PoisonError};

use serde::{Deserialize, Serialize};
use submilli_server::record::SessionEvent;

use super::{Result, Store, StoreError, io_error, open_for_append, parse_record, read_lines};

/// One line of a session's event log.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct StoredEvent {
    pub(crate) format: u32,
    /// The session's own sequence: from 1, no gaps. The resume cursor.
    pub(crate) session_seq: u64,
    /// Stable across reads: the server's id, or one derived from the run for an event
    /// recovered from its record.
    pub(crate) event_id: String,
    pub(crate) position: Position,
    /// The playground's run id, for an event of a run it recorded.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) run: Option<u64>,
    /// Recovered from the run's record after the server dropped it.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub(crate) backfilled: bool,
    /// For a decision, its number in its run (`<run>.<n>`, from 1), the one `show` and
    /// `explain` give it; `None` when the store could not tell it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) decision: Option<u64>,
    pub(crate) body: EventBody,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub(crate) enum EventBody {
    Event(Box<SessionEvent>),
    /// Events of a run were dropped under load.
    Gap(Gap),
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct Gap {
    /// The server's execution id of the run that lost events.
    pub(crate) run_id: String,
    /// How many of its events the server dropped; `None` when even its end was lost.
    pub(crate) dropped: Option<u64>,
    /// How many were recovered from the run's record and appended after this entry.
    pub(crate) recovered: u64,
    /// The run's record was itself cut at its cap, so some decisions or calls are gone
    /// for good.
    pub(crate) lost: bool,
}

/// Where an event happened, the order to read a session in: a timestamp from the
/// playground's one clock (microseconds since the Unix epoch), then for a run's event its
/// run id and run-wide call index, then within one call: its start, its decisions, and
/// its end.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub(crate) struct Position {
    pub(crate) at_micros: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) run: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) call_index: Option<u64>,
    pub(crate) rank: u8,
}

/// A session's events as read.
#[derive(Debug, Clone, Default)]
pub(crate) struct EventLog {
    /// In the order they were appended (`session_seq`).
    pub(crate) events: Vec<StoredEvent>,
    /// Lines that did not parse (a write cut short), left out of `events`.
    pub(crate) skipped: usize,
}

impl EventLog {
    /// Whether events were dropped from this log: the log says so, and how, in its gap
    /// entries.
    pub(crate) fn incomplete(&self) -> bool {
        self.gaps().next().is_some()
    }

    /// Whether some dropped events could not be recovered from their run's record.
    pub(crate) fn lost(&self) -> bool {
        self.gaps().any(|gap| gap.lost || gap.dropped.is_none())
    }

    pub(crate) fn gaps(&self) -> impl Iterator<Item = &Gap> {
        self.events.iter().filter_map(|event| match &event.body {
            EventBody::Gap(gap) => Some(gap),
            EventBody::Event(_) => None,
        })
    }

    /// The events in causal order: where each happened, not when it was appended.
    pub(crate) fn causal(&self) -> Vec<&StoredEvent> {
        let mut events: Vec<&StoredEvent> = self.events.iter().collect();
        events.sort_by_key(|event| (event.position, event.session_seq));
        events
    }
}

/// The file a session's events go to. Session ids the server makes are lowercase UUIDs
/// and are used as they are; anything else (an MCP client's own id, or any id with an
/// uppercase letter) is hashed, so it cannot name a path, and two ids that differ only
/// in case never share a file on a case-insensitive filesystem. A hashed name starts with
/// `_`, which a plain id cannot. Events outside any session share one log.
pub(crate) fn session_file_name(session: Option<&str>) -> String {
    match session {
        None => "_sessionless.jsonl".to_owned(),
        Some(id)
            if !id.is_empty()
                && id.len() <= 128
                && id.bytes().all(|byte| {
                    byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-'
                }) =>
        {
            format!("{id}.jsonl")
        }
        Some(id) => format!(
            "_h-{}.jsonl",
            crate::commands::playground::state::hex(&sha256(id.as_bytes()))
        ),
    }
}

fn sha256(bytes: &[u8]) -> [u8; 32] {
    use sha2::Digest as _;
    sha2::Sha256::digest(bytes).into()
}

/// Appends events, numbering each session's from 1.
pub(crate) struct Appender {
    dir: PathBuf,
    /// Opened by the store's writer; a read-only store's appender refuses to append.
    writable: bool,
    /// The last sequence number per session log file, read from the file the first time
    /// the session is appended to in this process.
    last: Mutex<HashMap<String, u64>>,
    /// Counts the lines appended to any log, so a reader following one wakes when it may
    /// have grown instead of polling it.
    appended: tokio::sync::watch::Sender<u64>,
}

impl Appender {
    pub(super) fn new(dir: PathBuf, writable: bool) -> Self {
        Self {
            dir,
            writable,
            last: Mutex::new(HashMap::new()),
            appended: tokio::sync::watch::Sender::new(0),
        }
    }

    /// Changes after every line appended to any session's log, in this process.
    pub(crate) fn subscribe(&self) -> tokio::sync::watch::Receiver<u64> {
        self.appended.subscribe()
    }

    /// Appends `event` to its session's log, numbered next in that session, and returns
    /// the number. `write` turns the numbered event into the line written, so the caller
    /// can redact it, or into `None` to leave it out; the number is then not used and
    /// `None` returned.
    pub(crate) fn append(
        &self,
        session: Option<&str>,
        mut event: StoredEvent,
        write: impl FnOnce(&StoredEvent) -> Option<Vec<u8>>,
    ) -> Result<Option<u64>> {
        if !self.writable {
            return Err(StoreError::ReadOnly {
                path: self.dir.clone(),
            });
        }
        let name = session_file_name(session);
        let path = self.dir.join(&name);
        let mut last = self.last.lock().unwrap_or_else(PoisonError::into_inner);
        let (mut file, repaired) = open_for_append(&path)?;
        // A repaired file may now end in a line this process numbered but saw fail (a
        // write cut between the line and its newline), so the number is read again.
        let previous = match last.get(&name) {
            Some(seq) if !repaired => *seq,
            _ => last_seq(&path)?,
        };
        event.session_seq = previous.saturating_add(1);
        let Some(line) = write(&event) else {
            return Ok(None);
        };
        file.write_all(&line).map_err(io_error(&path))?;
        last.insert(name, event.session_seq);
        drop(last);
        self.appended
            .send_modify(|count| *count = count.wrapping_add(1));
        Ok(Some(event.session_seq))
    }

    /// Forgets the numbering, after the logs were removed.
    pub(crate) fn reset(&self) {
        self.last
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clear();
    }
}

/// The highest sequence number among a log's complete lines that parse, or 0 for no
/// log. A line a crash cut short is skipped, so numbering carries on past it.
fn last_seq(path: &Path) -> Result<u64> {
    #[derive(Deserialize)]
    struct Seq {
        session_seq: u64,
    }
    Ok(read_lines::<Seq>(path)?
        .records
        .iter()
        .map(|seq| seq.session_seq)
        .max()
        .unwrap_or(0))
}

impl Store {
    /// A session's whole event log, without lines that do not parse (counted in
    /// `skipped`). Refuses a line in a newer format.
    pub(crate) fn read_events(&self, session: Option<&str>) -> Result<EventLog> {
        let lines = read_lines(&self.events_path(session))?;
        Ok(EventLog {
            events: lines.records,
            skipped: lines.skipped,
        })
    }

    /// The events in the complete lines of a session's log from byte `offset` on, and
    /// the offset just past the last of them: a reader following the log passes it back
    /// to read each line once, and a line still being written waits for its newline. A
    /// line that does not parse is skipped; one in a newer format is refused. An offset
    /// past the end, after the logs were cleared, reads from the start.
    pub(crate) fn read_events_from(
        &self,
        session: Option<&str>,
        offset: u64,
    ) -> Result<(Vec<StoredEvent>, u64)> {
        use std::io::{Read as _, Seek as _};
        let path = self.events_path(session);
        let mut file = match std::fs::File::open(&path) {
            Ok(file) => file,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                return Ok((Vec::new(), 0));
            }
            Err(error) => return Err(io_error(&path)(error)),
        };
        let len = file.metadata().map_err(io_error(&path))?.len();
        let start = if offset > len { 0 } else { offset };
        file.seek(std::io::SeekFrom::Start(start))
            .map_err(io_error(&path))?;
        let mut bytes = Vec::new();
        file.read_to_end(&mut bytes).map_err(io_error(&path))?;
        let Some(last_newline) = bytes.iter().rposition(|byte| *byte == b'\n') else {
            return Ok((Vec::new(), start));
        };
        let complete = bytes.get(..last_newline).unwrap_or_default();
        let mut events = Vec::new();
        for line in complete
            .split(|byte| *byte == b'\n')
            .filter(|line| !line.is_empty())
        {
            match parse_record::<StoredEvent>(&path, line) {
                Ok(event) => events.push(event),
                Err(StoreError::Corrupt { .. }) => {}
                Err(error) => return Err(error),
            }
        }
        let read = u64::try_from(last_newline)
            .unwrap_or(u64::MAX)
            .saturating_add(1);
        Ok((events, start.saturating_add(read)))
    }

    pub(crate) fn events_path(&self, session: Option<&str>) -> PathBuf {
        self.root.join("events").join(session_file_name(session))
    }
}
