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
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, PoisonError};

use serde::{Deserialize, Serialize};
use submilli_server::record::SessionEvent;

use super::{
    FORMAT, Result, Store, append_line, complete_lines, io_error, parse_record, read_lines,
};

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

/// The file a session's events go to. Session ids the server makes are UUIDs and are
/// used as they are; anything else (an MCP client's own id) is hashed so it cannot name
/// a path. Events outside any session share one log.
pub(crate) fn session_file_name(session: Option<&str>) -> String {
    match session {
        None => "_sessionless.jsonl".to_owned(),
        Some(id)
            if !id.is_empty()
                && id.len() <= 128
                && id
                    .bytes()
                    .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-') =>
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
    /// The last sequence number per session log file, read from the file the first time
    /// the session is appended to in this process.
    last: Mutex<HashMap<String, u64>>,
}

impl Appender {
    pub(super) fn new(dir: PathBuf) -> Self {
        Self {
            dir,
            last: Mutex::new(HashMap::new()),
        }
    }

    /// Appends `event` to its session's log, numbered next in that session, and returns
    /// the number. `write` turns the numbered event into the line written, so the caller
    /// can redact it.
    pub(crate) fn append(
        &self,
        session: Option<&str>,
        mut event: StoredEvent,
        write: impl FnOnce(&StoredEvent) -> Vec<u8>,
    ) -> Result<u64> {
        let name = session_file_name(session);
        let path = self.dir.join(&name);
        let mut last = self.last.lock().unwrap_or_else(PoisonError::into_inner);
        let previous = match last.get(&name) {
            Some(seq) => *seq,
            None => last_seq(&path)?,
        };
        event.session_seq = previous.saturating_add(1);
        append_line(&path, &write(&event))?;
        last.insert(name, event.session_seq);
        Ok(event.session_seq)
    }

    /// Forgets the numbering, after the logs were removed.
    pub(crate) fn reset(&self) {
        self.last
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clear();
    }
}

/// The last complete line's sequence number in a log, or 0 for no log.
fn last_seq(path: &Path) -> Result<u64> {
    #[derive(Deserialize)]
    struct Seq {
        format: u32,
        session_seq: u64,
    }
    let bytes = match fs::read(path) {
        Ok(bytes) => bytes,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(0),
        Err(error) => return Err(io_error(path)(error)),
    };
    let Some(line) = complete_lines(&bytes)
        .filter(|line| !line.is_empty())
        .last()
    else {
        return Ok(0);
    };
    let seq: Seq = parse_record(path, line)?;
    debug_assert!(seq.format <= FORMAT, "parse_record refuses newer formats");
    Ok(seq.session_seq)
}

impl Store {
    /// A session's whole event log. Refuses a line in a newer format.
    pub(crate) fn read_events(&self, session: Option<&str>) -> Result<EventLog> {
        let path = self.events_path(session);
        Ok(EventLog {
            events: read_lines(&path)?,
        })
    }

    /// The events appended after `after` (a `session_seq`), for a reader following the
    /// log: each complete line once, and nothing still being written.
    pub(crate) fn read_events_after(
        &self,
        session: Option<&str>,
        after: u64,
    ) -> Result<Vec<StoredEvent>> {
        let mut events = self.read_events(session)?.events;
        events.retain(|event| event.session_seq > after);
        Ok(events)
    }

    pub(crate) fn events_path(&self, session: Option<&str>) -> PathBuf {
        self.root.join("events").join(session_file_name(session))
    }
}
