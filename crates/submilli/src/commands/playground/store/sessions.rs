//! The started-session log: `sessions.jsonl`, one line when a session is started
//! through the playground and one when it is ended.
//!
//! The run index already names the session each run executed in; this log adds the
//! sessions the playground started with fixed variables, so one is listed before it
//! has runs, and says whether it is still open.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use super::{FORMAT, Result, Store, append_line, json_line, now_micros, read_lines};

/// One line of the started-session log.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub(crate) struct SessionLine {
    pub(crate) format: u32,
    /// Microseconds since the Unix epoch when the line was written.
    pub(crate) at_micros: u64,
    #[serde(flatten)]
    pub(crate) entry: SessionEntry,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case", tag = "kind")]
pub(crate) enum SessionEntry {
    /// A session started with its variables fixed.
    Started {
        session_id: String,
        variables: BTreeMap<String, String>,
        /// Who started it: the playground's label for the runs it holds (`assistant`,
        /// `example`, ...).
        label: String,
    },
    /// The session was closed.
    Ended { session_id: String },
}

impl SessionEntry {
    pub(crate) fn session_id(&self) -> &str {
        match self {
            Self::Started { session_id, .. } | Self::Ended { session_id } => session_id,
        }
    }
}

impl Store {
    pub(crate) fn sessions_path(&self) -> std::path::PathBuf {
        self.root.join("sessions.jsonl")
    }

    /// Appends one line to the started-session log.
    pub(crate) fn append_session(&self, entry: SessionEntry) -> Result<()> {
        self.writer()?;
        let _inner = self.lock();
        append_line(
            &self.sessions_path(),
            &json_line(&SessionLine {
                format: FORMAT,
                at_micros: now_micros(),
                entry,
            }),
        )
    }

    /// The started-session log in the order it was written, without lines that do not
    /// parse; a line in a newer format is refused.
    pub(crate) fn session_log(&self) -> Result<Vec<SessionLine>> {
        read_lines(&self.sessions_path()).map(|lines| lines.records)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn started_and_ended_lines_read_back_in_order_around_a_torn_line() {
        let dir = tempfile::tempdir().unwrap();
        let store = Store::open(&dir.path().join("store")).unwrap();
        store
            .append_session(SessionEntry::Started {
                session_id: "s-1".into(),
                variables: BTreeMap::from([("customerId".into(), "cus_northwind".into())]),
                label: "assistant".into(),
            })
            .unwrap();
        // A write cut short by a crash.
        let path = store.sessions_path();
        let mut bytes = std::fs::read(&path).unwrap();
        bytes.extend_from_slice(b"{\"format\":1,\"at_mic");
        std::fs::write(&path, bytes).unwrap();
        store
            .append_session(SessionEntry::Ended {
                session_id: "s-1".into(),
            })
            .unwrap();

        let read = Store::open_read_only(store.root()).unwrap();
        let log = read.session_log().unwrap();
        let ids: Vec<(&str, bool)> = log
            .iter()
            .map(|line| {
                (
                    line.entry.session_id(),
                    matches!(line.entry, SessionEntry::Started { .. }),
                )
            })
            .collect();
        assert_eq!(ids, [("s-1", true), ("s-1", false)]);
        assert!(
            read.append_session(SessionEntry::Ended {
                session_id: "s-1".into()
            })
            .is_err()
        );
    }
}
